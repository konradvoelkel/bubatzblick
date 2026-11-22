#!/usr/bin/bash
# Prueft von aussen, ob die Kartendaten erreichbar und frisch sind: das, was der
# caddy-watchdog (fragt bubatzblick.de auf dem Server selbst) und der Zyklus-Healthcheck
# (meldet nur gescheiterte Laeufe) nicht sehen -- DNS, Zertifikat und Cloudflare R2
# hinter tiles.bubatzblick.de. Faellt tiles. aus, steht die Seite ohne Karte da.
#
#   deploy/bin/check_tiles.sh
#   TILES_BASE_URL=... MAX_ALTER_H=36 HEALTHCHECK_URL=https://hc-ping.com/... deploy/bin/check_tiles.sh
#
# Je Lauf drei kleine Anfragen (zwei Range-Anfragen auf 7 Bytes, stand.json): bei
# einem Lauf alle 30 Minuten rund 4 300 Lesungen im Monat, bei 10 Mio. freien.
# Die Zonenregel von tiles. laesst nur GET/HEAD/OPTIONS ohne Query-String zu.
#
# Ausgang wie kv-access-age im Infra-Repo: 0 alles da, 1 Befund, 2 Pruefung kaputt
# (Werkzeug fehlt). Mit HEALTHCHECK_URL geht bei 0 ein Ping an die URL, sonst an
# URL/fail -- Stille heisst dann "Pruefung lief nicht".
set -u

B=${TILES_BASE_URL:-https://tiles.bubatzblick.de}
B=${B%/}
MAX=${MAX_ALTER_H:-36}
befund=0
say() { echo "check_tiles: $*"; }

ende() {
  if [ -n "${HEALTHCHECK_URL:-}" ]; then
    if [ "$1" -eq 0 ]; then ziel=$HEALTHCHECK_URL; else ziel=$HEALTHCHECK_URL/fail; fi
    curl -fsS -m 10 --retry 3 -o /dev/null "$ziel" || say "Healthcheck nicht erreichbar"
  fi
  exit "$1"
}

for w in curl python3 openssl; do
  command -v "$w" >/dev/null || { say "PRUEFUNG KAPUTT: $w fehlt"; ende 2; }
done
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# 1. Die Archive antworten auf eine Range-Anfrage mit 206 und der PMTiles-Kennung.
for a in zonen.pmtiles basemap.pmtiles; do
  code=$(curl -sS -m 20 -r 0-6 -o "$tmp" -w '%{http_code}' "$B/$a" 2>&1)
  kennung=$(head -c 7 "$tmp" 2>/dev/null)
  if [ "$code" = 206 ] && [ "$kennung" = PMTiles ]; then
    say "OK   $a: 206, PMTiles"
  else
    say "FEHLER $a: HTTP $code, Kennung '${kennung//[^[:print:]]/?}'"
    befund=1
  fi
done

# 2. stand.json ist frisch: der Tageszyklus veroeffentlicht zweimal am Tag, aber nur
#    bei neuer OSM-Folge; 36 h lassen einen Tag ohne neue Folge zu.
if curl -fsS -m 20 -o "$tmp" "$B/stand.json"; then
  alter=$(python3 -I - "$tmp" <<'PY'
import datetime as dt, json, sys
try:
    p = dt.datetime.fromisoformat(json.load(open(sys.argv[1]))["published"].replace("Z", "+00:00"))
    print(int((dt.datetime.now(dt.timezone.utc) - p).total_seconds() // 3600))
except Exception:
    print("x")
PY
)
  if [ "$alter" = x ]; then
    say "FEHLER stand.json: kein lesbares published"
    befund=1
  elif [ "$alter" -le "$MAX" ]; then
    say "OK   stand.json: veroeffentlicht vor $alter h (hoechstens $MAX)"
  else
    say "FEHLER stand.json: veroeffentlicht vor $alter h (hoechstens $MAX)"
    befund=1
  fi
else
  say "FEHLER stand.json: nicht abrufbar"
  befund=1
fi

# 3. Das Zertifikat von tiles. gilt noch mindestens 14 Tage (Cloudflare erneuert
#    selbst; das hier faellt auf, wenn es das einmal nicht tut).
host=${B#https://}
host=${host%%/*}
if echo | openssl s_client -connect "$host:443" -servername "$host" 2>/dev/null \
    | openssl x509 -noout -checkend $((14 * 86400)) >/dev/null 2>&1; then
  say "OK   Zertifikat $host: gilt noch mindestens 14 Tage"
else
  say "FEHLER Zertifikat $host: laeuft in unter 14 Tagen ab oder nicht lesbar"
  befund=1
fi

[ "$befund" -eq 0 ] && say "alles da" || say "mindestens ein Befund"
ende "$befund"
