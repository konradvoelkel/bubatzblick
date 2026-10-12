#!/usr/bin/bash
# Veroeffentlichung des Bundeslaufs NUR nach bestandener Pruefkette
# (Aufruf: `make publish`).
#
# Prueft den fertigen Lauf (Anker, Kachelaufblaehung, Plausibilitaet gegen den
# zuletzt veroeffentlichten Stand, `pmtiles verify`) und ersetzt erst dann das
# oeffentliche Archiv atomar (`rename`); der Vorgaenger bleibt als
# `zonen.pmtiles.prev` fuer einen Rueckgriff liegen. Scheitert eine Pruefung,
# bleibt das veroeffentlichte Archiv unangetastet und der Exit-Code ist != 0
# (fuer `OnFailure=` der Timer-Einheit).
#
# Umgebung (Voreinstellungen wie im Makefile):
#   BUBATZ        Binary                              rust/target/release/bubatz
#   OUT_DIR       Ausgabe des Laufs (…/germany/zonen.pmtiles)   data/out
#   INTERIM_DIR   Zwischendaten (…/germany/zones, protected)    data/interim
#   PUBLISH_DIR   Webwurzel, in die das Archiv kommt  (Pflicht)
#   ANCHOR_ID / ANCHOR_M2 / ANCHOR_TOL_PCT   Ankerobjekt 29359108 (Spielplatz Duesseldorf-Stadtmitte), Soll 8461 m2, ±2 %
#   TILES_TOL_PCT  hoechstens erlaubte Kachelaufblaehung am Anker (3 %; Ist -1,3 %: im z14-Band sind Grundrisse abgezogen),
#                  Pruefung nur wenn `uv` vorhanden ist (Python-Orakel)
#   MIN_RATIO      Baender und Archivgroesse muessen >= MIN_RATIO x des zuletzt
#                  veroeffentlichten Stands sein (0.9) -- faengt leere/abgebrochene
#                  Eingaben ab, die formal durchlaufen
#   MAX_RATIO      ... und <= MAX_RATIO x (1.1): Gegenstueck nach oben -- OSM ist von
#                  jedem editierbar, und ein Tag-Vandalismus, der eine Stadt rot
#                  faerbt, darf nicht automatisch live gehen
#   CAT_TOL_PCT    je Kategorie darf die Objektzahl gegen den letzten Stand um hoechstens
#   CAT_TOL_ABS    max(CAT_TOL_PCT %, CAT_TOL_ABS Objekte) abweichen (5 % / 50)
#   HARD_FACTOR    kein Objekt darf groesser sein als HARD_FACTOR x die
#                  Plausibilitaetsgrenze seiner Kategorie (tags.json; 10 -- auf einem
#                  Bundeslauf gemessen: 0 Treffer bei 10, 1 bei 5, 4 bei 2)
#   OVERSIZE_OK    Komma-Liste von osm_ids, die trotzdem durchduerfen (nach Pruefung)
#   Scheitert eine dieser Pruefungen, bleibt der alte Stand online und die Einheit
#   ist `failed` (Healthcheck /fail) -- die Karte zeigt ab 3 Tagen "nicht aktualisiert".
#   PUBLISH_DRY=1  alles pruefen, nichts veroeffentlichen
#   R2_REMOTE      optional: rclone-Ziel "<remote>:<bucket>" (Cloudflare R2, S3-API);
#                  nach dem lokalen Tausch werden zonen.pmtiles (Vorgaenger zuvor
#                  serverseitig nach zonen.prev.pmtiles kopiert), stand.json und
#                  zonen_tiles_summary.json hochgeladen. Ein S3-Upload ersetzt das
#                  Objekt atomar; die Seite liest per Range.
set -u
BUBATZ=${BUBATZ:-rust/target/release/bubatz}
OUT_DIR=${OUT_DIR:-data/out}
INTERIM_DIR=${INTERIM_DIR:-data/interim}
PUBLISH_DIR=${PUBLISH_DIR:?PUBLISH_DIR (Webwurzel) fehlt}
ANCHOR_ID=${ANCHOR_ID:-29359108}
ANCHOR_M2=${ANCHOR_M2:-8461}
ANCHOR_TOL_PCT=${ANCHOR_TOL_PCT:-2}
TILES_TOL_PCT=${TILES_TOL_PCT:-3}
MIN_RATIO=${MIN_RATIO:-0.9}
MAX_RATIO=${MAX_RATIO:-1.1}
CAT_TOL_PCT=${CAT_TOL_PCT:-5}
CAT_TOL_ABS=${CAT_TOL_ABS:-50}
HARD_FACTOR=${HARD_FACTOR:-10}
OVERSIZE_OK=${OVERSIZE_OK:-}
PUBLISH_DRY=${PUBLISH_DRY:-0}

ARCHIVE=$OUT_DIR/germany/zonen.pmtiles
ZONES=$INTERIM_DIR/germany/zones
PROTECTED=$INTERIM_DIR/germany/protected.parquet
TARGET=$PUBLISH_DIR/zonen.pmtiles
STAMP=$PUBLISH_DIR/stand.json
fail=0
say() { echo "publish: $*"; }
check() { # check <name> <ok:0|1> <text>
  if [ "$2" -eq 0 ]; then say "OK   $1: $3"; else say "FEHLER $1: $3"; fail=1; fi
}

for f in "$ARCHIVE" "$ZONES/.stamp" "$ZONES/zones_summary.json" "$PROTECTED"; do
  [ -e "$f" ] || { say "FEHLER: $f fehlt -- Lauf nicht vollstaendig"; exit 2; }
done
[ -x "$BUBATZ" ] || { say "FEHLER: Binary $BUBATZ fehlt"; exit 2; }

# 1. Anker: Flaeche der Zone bis 100 m des freigegebenen Objekts aus den Zonendaten.
anchor_out=$("$BUBATZ" verify anchor --zones-dir "$ZONES" --protected-path "$PROTECTED" --osm-id "$ANCHOR_ID" 2>/dev/null)
anchor_m2=$(echo "$anchor_out" | sed -n 's/.*Zone <= [0-9]* m: \([0-9]*\) m2.*/\1/p')
if [ -z "$anchor_m2" ]; then
  check anchor 1 "keine Flaeche fuer osm_id $ANCHOR_ID ($anchor_out)"
else
  dev=$(python3 -c "print(abs($anchor_m2-$ANCHOR_M2)*100/$ANCHOR_M2)")
  ok=$(python3 -c "print(0 if $dev <= $ANCHOR_TOL_PCT else 1)")
  check anchor "$ok" "$anchor_m2 m2 (Soll $ANCHOR_M2 ±$ANCHOR_TOL_PCT %, Abweichung $dev %)"
fi

# 2. Kachelaufblaehung am Anker (Python-Orakel, nur wo `uv` und das Repo vorhanden sind).
#    --no-sync: die Einheit haelt das Repo samt .venv read-only (ReadOnlyPaths); ohne den
#    Schalter versucht `uv run`, dev-Pakete nachzuinstallieren, und die Pruefung scheitert
#    `uv sync --frozen --no-dev` richtet die Umgebung beim Aufsetzen ein.
if command -v uv >/dev/null && [ -f scripts/rollout_check_tiles.py ]; then
  tiles_out=$(uv run --no-sync python scripts/rollout_check_tiles.py "$OUT_DIR/germany" "$ANCHOR_ID" "$ZONES" 2>&1)
  infl=$(echo "$tiles_out" | sed -n 's/^Aufblaehung: *\([-+0-9.]*\) %.*/\1/p')
  if [ -z "$infl" ]; then
    check tiles 1 "keine Aufblaehung messbar: $(echo "$tiles_out" | tail -1)"
  else
    ok=$(python3 -c "print(0 if $infl <= $TILES_TOL_PCT else 1)")
    check tiles "$ok" "Kachel gegen Zonendaten $infl % (hoechstens $TILES_TOL_PCT %)"
  fi
else
  say "tiles: uebersprungen (kein uv/Repo)"
fi

# 3. Plausibilitaet gegen den zuletzt veroeffentlichten Stand (nach unten UND oben).
bands=$(python3 -c "import json;print(json.load(open('$ZONES/zones_summary.json'))['total_zones'])")
size=$(stat -c %s "$ARCHIVE")
if [ -f "$STAMP" ]; then
  read -r prev_bands prev_size < <(python3 -c "import json;d=json.load(open('$STAMP'));print(d.get('bands',0),d.get('archive_bytes',0))")
  ok=$(python3 -c "print(0 if $MIN_RATIO*$prev_bands <= $bands <= $MAX_RATIO*$prev_bands and $MIN_RATIO*$prev_size <= $size <= $MAX_RATIO*$prev_size else 1)")
  check plausibel "$ok" "$bands Baender, $size Bytes (zuletzt $prev_bands / $prev_size, erlaubt x$MIN_RATIO..x$MAX_RATIO)"
else
  say "plausibel: erste Veroeffentlichung ($bands Baender, $size Bytes), kein Vergleich"
fi

# 3b. Verbotsobjekte: kein Riesenobjekt, keine Kategorie springt (OSM-Vandalismus,
#     kaputter Diff, falsche Tag-Datei). Kennzahlen aus dem Binary (50 ms).
objects_json=$("$BUBATZ" verify protected --protected-path "$PROTECTED" --hard-factor "$HARD_FACTOR" 2>/dev/null) \
  || { objects_json='{}'; }
objects_out=$(OBJ_JSON=$objects_json python3 - "$STAMP" "$CAT_TOL_PCT" "$CAT_TOL_ABS" "$OVERSIZE_OK" <<'EOF'
import json, os, sys
stamp, tol_pct, tol_abs, ok_ids = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
d = json.loads(os.environ["OBJ_JSON"] or "{}")
if not d:
    print(1); print("bubatz verify protected fehlgeschlagen"); sys.exit()
allowed = {int(x) for x in ok_ids.split(",") if x.strip()}
problems = [f"osm_id {o['osm_id']} ({o['category']}) {o['area_m2']:.0f} m2 > {o['limit_m2']:.0f} m2"
            for o in d["oversize"] if o["osm_id"] not in allowed]
prev = json.load(open(stamp)).get("categories") if os.path.exists(stamp) else None
if prev:
    for cat in sorted(set(prev) | set(d["categories"])):
        a, b = prev.get(cat, 0), d["categories"].get(cat, 0)
        if abs(b - a) > max(a * tol_pct / 100, tol_abs):
            problems.append(f"{cat}: {a} -> {b}")
print(1 if problems else 0)
print("; ".join(problems) if problems else
      f"{d['objects']} Objekte, {d['warnings']} Plausibilitaetswarnungen, "
      + ", ".join(f"{k} {v}" for k, v in d["categories"].items())
      + ("" if prev else " (erster Stand, kein Kategorienvergleich)"))
EOF
)
check objekte "$(printf '%s\n' "$objects_out" | head -1)" "$(printf '%s\n' "$objects_out" | tail -n +2)"

# 4. Archiv lesbar.
if command -v pmtiles >/dev/null; then
  if pmtiles verify "$ARCHIVE" >/dev/null 2>&1; then check pmtiles 0 "verify bestanden"; else check pmtiles 1 "pmtiles verify"; fi
else
  say "pmtiles: uebersprungen (CLI fehlt)"
fi

if [ "$fail" -ne 0 ]; then
  say "NICHT veroeffentlicht, $TARGET bleibt."
  exit 1
fi
if [ "$PUBLISH_DRY" = 1 ]; then
  say "Probelauf: alle Pruefungen bestanden, nichts veroeffentlicht."
  exit 0
fi

# Veroeffentlichen: Kopie in die Webwurzel (gleiches Dateisystem), Vorgaenger
# als Hardlink sichern, dann atomar ersetzen -- es gibt keinen Moment ohne Archiv.
mkdir -p "$PUBLISH_DIR"
cp "$ARCHIVE" "$TARGET.new" || { say "FEHLER: Kopie nach $TARGET.new"; exit 1; }
[ -f "$TARGET" ] && ln -f "$TARGET" "$TARGET.prev"
mv -f "$TARGET.new" "$TARGET"
[ -f "$OUT_DIR/germany/zonen_tiles_summary.json" ] && cp "$OUT_DIR/germany/zonen_tiles_summary.json" "$PUBLISH_DIR/zonen_tiles_summary.json"
# Fussgaengerzonen: eigene Ebene, eigener Tausch. Sie haengt nicht an den
# Zonenpruefungen oben (andere Daten, keine Sichtweite), darf deren Ergebnis
# aber auch nicht aufhalten -- fehlt die Datei, bleibt der alte Stand liegen.
FUSS=$OUT_DIR/germany/fussgaengerzonen.pmtiles
if [ -f "$FUSS" ]; then
  cp "$FUSS" "$PUBLISH_DIR/fussgaengerzonen.pmtiles.new" && mv -f "$PUBLISH_DIR/fussgaengerzonen.pmtiles.new" "$PUBLISH_DIR/fussgaengerzonen.pmtiles"
fi
pbf_seq=$(osmium fileinfo -e -g header.option.osmosis_replication_sequence_number "${GERMANY_PBF:-data/raw/germany-latest.osm.pbf}" 2>/dev/null || echo null)
pbf_ts=$(osmium fileinfo -e -g header.option.osmosis_replication_timestamp "${GERMANY_PBF:-data/raw/germany-latest.osm.pbf}" 2>/dev/null || echo "")
# Welche Software den Stand gerechnet hat (Versionsnummer aus rust/Cargo.toml).
pipeline_version=$("$BUBATZ" --version 2>/dev/null | awk '{print $2}')
OBJ_JSON=$objects_json python3 - "$STAMP" "$bands" "$size" "$anchor_m2" "${pbf_seq:-null}" "$pbf_ts" "$pipeline_version" <<'EOF'
import json, sys, datetime, os
stamp, bands, size, anchor, seq, ts, ver = sys.argv[1:]
o = json.loads(os.environ["OBJ_JSON"] or "{}")
d = {"published": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
     "bands": int(bands), "archive_bytes": int(size), "anchor_m2": int(anchor),
     "pbf_sequence": int(seq) if seq.isdigit() else None, "pbf_timestamp": ts or None,
     "objects": o.get("objects"), "warnings": o.get("warnings"), "categories": o.get("categories"),
     "pipeline_version": ver or None}
tmp = stamp + ".tmp"
json.dump(d, open(tmp, "w"), indent=1); os.replace(tmp, stamp)
EOF
echo "$(date -u +%FT%TZ) $bands Baender $size Bytes Anker $anchor_m2 m2 PBF ${pbf_seq:-?}" >> "$PUBLISH_DIR/publish.log"
say "veroeffentlicht: $TARGET ($size Bytes), Vorgaenger $TARGET.prev"

# 5. Optional: Objektspeicher (Cloudflare R2). Reihenfolge: Vorgaenger sichern,
#    Archiv, dann erst stand.json -- wer stand.json sieht, findet das passende Archiv.
if [ -n "${R2_REMOTE:-}" ]; then
  command -v rclone >/dev/null || { say "FEHLER: rclone fehlt (R2_REMOTE gesetzt)"; exit 1; }
  rclone copyto "$R2_REMOTE/zonen.pmtiles" "$R2_REMOTE/zonen.prev.pmtiles" 2>/dev/null || say "R2: kein Vorgaenger (erste Veroeffentlichung)"
  rclone copyto "$TARGET" "$R2_REMOTE/zonen.pmtiles" --s3-upload-concurrency 8 --s3-chunk-size 64M \
    || { say "FEHLER: Upload nach $R2_REMOTE/zonen.pmtiles (lokal ist veroeffentlicht)"; exit 1; }
  [ -f "$PUBLISH_DIR/zonen_tiles_summary.json" ] && rclone copyto "$PUBLISH_DIR/zonen_tiles_summary.json" "$R2_REMOTE/zonen_tiles_summary.json"
  if [ -f "$PUBLISH_DIR/fussgaengerzonen.pmtiles" ]; then
    rclone copyto "$PUBLISH_DIR/fussgaengerzonen.pmtiles" "$R2_REMOTE/fussgaengerzonen.pmtiles" \
      || say "WARNUNG: Fussgaengerzonen nicht hochgeladen (Zonen sind veroeffentlicht)"
  fi
  rclone copyto "$STAMP" "$R2_REMOTE/stand.json" || { say "FEHLER: stand.json nach R2"; exit 1; }
  say "R2: $R2_REMOTE aktualisiert (zonen.pmtiles, fussgaengerzonen.pmtiles, stand.json)"
fi
