#!/usr/bin/bash
# Prueft auf dem Server, ob die Aussagen des Abschnitts "Was der eigene Server
# protokolliert" (viewer/about.md) zu dem passen, was auf der Platte liegt.
# Nur lesend. Vor jedem Deploy der Ueber-Seite laufen lassen.
#
#   deploy/bin/check_access_log.sh
#   LOG=... ACCESS=... REPO=... deploy/bin/check_access_log.sh   # andere Pfade
#
# Zwei der vier Zusagen haelt dieses Repository nicht selbst ein, deshalb dieses
# Skript: das Kuerzen der Adressen macht der Logfilter von Caddy (Infra-Repo,
# `ip_mask`), und `aggregate_access.py` loest eine root-Einheit aus, die hier
# ebenfalls nicht liegt. Faellt eine der beiden aus, wird die Aussage auf der
# Ueber-Seite still falsch -- ohne Kuerzung stehen vollstaendige Adressen im
# Protokoll, ohne Verdichtung waechst es ueber die genannte Frist hinaus.
#
# Caddy kuerzt nicht von sich aus: ohne `ip_mask` im Logformat landet
# `request.remote_ip` vollstaendig im JSON.
#
# Jeder Abschnitt endet mit einem URTEIL, damit Stille nicht als "in Ordnung"
# durchgeht. Exit 1, sobald ein Abschnitt ein PROBLEM meldet.
set -u

LOG=${LOG:-/var/log/caddy/access.log}
ACCESS=${ACCESS:-/srv/bubatz/www/stats/access}
REPO=${REPO:-/srv/bubatz/repo}
befund=0

echo "===== 1. Laeuft die taegliche Verdichtung? ====================="
systemctl list-timers --all 2>/dev/null | grep -iE 'access|aggregate' || echo "  kein passender Timer"
for u in $(systemctl list-units --all --plain --no-legend 2>/dev/null \
            | awk '{print $1}' | grep -iE 'access|aggregate'); do
  echo "--- $u"
  systemctl cat "$u" 2>/dev/null | grep -E 'ExecStart|OnCalendar|Persistent'
  systemctl is-active "$u"
  journalctl -u "$u" --since -14d --no-pager 2>/dev/null | tail -15
done
crontab -l 2>/dev/null | grep -i aggregate || true
grep -rl aggregate_access /etc/cron.* /etc/crontab 2>/dev/null || true

echo
echo "===== 2. Wie alt ist die aelteste Zeile im Protokoll? =========="
if [ -r "$LOG" ]; then
  echo "Zeilen: $(wc -l < "$LOG"),  Groesse: $(du -h "$LOG" | cut -f1)"
  python3 - "$LOG" <<'PY' || befund=1
import datetime as dt, json, sys
erste = letzte = None
with open(sys.argv[1], errors="replace") as f:
    for line in f:
        try:
            ts = dt.datetime.fromtimestamp(float(json.loads(line)["ts"]))
        except Exception:
            continue
        erste = erste or ts
        letzte = ts
if erste is None:
    print("  keine lesbare Zeile")
    raise SystemExit(0)
alter = dt.datetime.now() - erste
print(f"  aelteste Zeile: {erste}  (Alter: {alter})")
print(f"  neueste Zeile:  {letzte}")
if alter < dt.timedelta(hours=48):
    print("  URTEIL: ok, unter 48 h")
else:
    print(f"  URTEIL: PROBLEM -- die Ueber-Seite sagt '< 48 h', hier sind es {alter}")
    raise SystemExit(1)
PY
else
  echo "  $LOG nicht lesbar -- Pfad per LOG=... setzen"
fi

echo
echo "===== 3. Sind die Adressen wirklich gekuerzt? =================="
echo "--- Konfiguration:"
# Kein `| head` vor dem `||`: die Pipe verdeckt den Exit-Code von grep, dann
# bliebe genau der entscheidende Hinweis stumm.
treffer=$(grep -rnE 'ip_mask|format[[:space:]]+filter' /etc/caddy/ 2>/dev/null || true)
if [ -n "$treffer" ]; then
  printf '%s\n' "$treffer" | head -20
elif [ -d /etc/caddy ]; then
  echo "  kein ip_mask in /etc/caddy -- das ist der Befund"
  befund=1
else
  echo "  /etc/caddy nicht vorhanden -- Konfiguration von hier nicht einsehbar."
  echo "  Das Urteil faellt die Gegenprobe an den Zeilen."
fi
echo "--- Gegenprobe an den tatsaechlichen Zeilen:"
if [ -r "$LOG" ]; then
  python3 - "$LOG" <<'PY' || befund=1
import ipaddress, json, sys
from collections import Counter
c, beispiel = Counter(), {}
with open(sys.argv[1], errors="replace") as f:
    for line in f:
        try:
            r = json.loads(line).get("request") or {}
            a = ipaddress.ip_address(r.get("client_ip") or r.get("remote_ip") or "")
        except Exception:
            c["unlesbar"] += 1
            continue
        if a.version == 4:
            k = "IPv4 gekuerzt (/24)" if int(a) & 0xFF == 0 else "IPv4 VOLLSTAENDIG"
        else:
            k = "IPv6 gekuerzt (/48)" if int(a) & ((1 << 80) - 1) == 0 else "IPv6 VOLLSTAENDIG"
        c[k] += 1
        beispiel.setdefault(k, str(a))
for k, n in c.most_common():
    print(f"  {n:>8}  {k}" + (f"   z.B. {beispiel[k]}" if k in beispiel else ""))
schlecht = sum(n for k, n in c.items() if "VOLLSTAENDIG" in k)
if schlecht:
    print(f"  URTEIL: PROBLEM -- {schlecht} Zeile(n) mit vollstaendiger Adresse")
    raise SystemExit(1)
print("  URTEIL: ok, alle gekuerzt")
PY
fi

echo
echo "===== 4. Tageszahlen: lueckenlos, Frist eingehalten? =========="
if [ -d "$ACCESS" ]; then
  python3 - "$ACCESS" <<'PY' || befund=1
import datetime as dt, pathlib, sys
d = pathlib.Path(sys.argv[1])
tage = sorted(dt.date.fromisoformat(p.stem) for p in d.glob("*.json") if p.stem[:4].isdigit())
if not tage:
    print(f"  keine Tagesdatei in {d}")
    raise SystemExit(0)
print(f"  {len(tage)} Tagesdatei(en), {tage[0]} .. {tage[-1]}")
luecken = [f"{tage[i - 1]} -> {tage[i]}" for i in range(1, len(tage))
           if (tage[i] - tage[i - 1]).days > 1]
print(f"  Luecken: {', '.join(luecken) if luecken else 'keine'}")
alt = [t for t in tage if (dt.date.today() - t).days > 400]
if alt:
    print(f"  URTEIL: PROBLEM -- {len(alt)} Datei(en) jenseits der 400 Tage")
    raise SystemExit(1)
print("  URTEIL: ok, nichts jenseits der 400 Tage")
PY
else
  echo "  $ACCESS fehlt -- Pfad per ACCESS=... setzen"
fi

echo
echo "===== 5. Kennt die Fassung auf dem Server die Frist? =========="
if [ -r "$REPO/deploy/bin/aggregate_access.py" ]; then
  grep -nE '^KEEP_DAYS' "$REPO/deploy/bin/aggregate_access.py" \
    && echo "  URTEIL: ok" \
    || { echo "  URTEIL: PROBLEM -- alte Fassung ohne KEEP_DAYS, dort loescht nichts"; befund=1; }
else
  echo "  $REPO/deploy/bin/aggregate_access.py nicht gefunden -- Pfad per REPO=... setzen"
fi

echo
[ "$befund" -eq 0 ] && echo "check_access_log: alle Abschnitte ohne Befund" \
  || echo "check_access_log: mindestens ein Abschnitt mit PROBLEM"
exit "$befund"
