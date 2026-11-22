#!/usr/bin/bash
# Grundkarte erneuern: Deutschland plus 10 km aus dem neuesten taeglichen
# Protomaps-Build schneiden, pruefen und nach R2 laden. Gedacht fuer einen
# Monats-Timer in der Rolle bubatz des Infra-Repos; laeuft auch zu Hause.
# Bisher von Hand.
#
#   deploy/bin/basemap-refresh.sh                 # schneiden, pruefen, hochladen
#   DRY_RUN=1 deploy/bin/basemap-refresh.sh       # schneiden und pruefen, nichts hochladen
#
# Umgebung (alle mit Vorgabe):
#   WORK_DIR       Arbeitsverzeichnis, braucht rund 6 GB frei  (mktemp unter /var/tmp)
#   REGION         Schnittflaeche (GeoJSON)                    (deploy/basemap/germany-region.geojson)
#   BUILD_BASE     Protomaps-Builds                             (https://build.protomaps.com)
#   MIN_GB/MAX_GB  erwartete Groesse in GB                      (5 / 7; bisher rund 5,7 GB)
#   R2_REMOTE      rclone-Ziel                                  (bubatz:bubatz)
#   KEEP_PREV      Vorgaenger als basemap.prev.pmtiles behalten (1; kostet rund 5 Cent im Monat,
#                  weil der Speicher dann ueber die freien 10 GB steigt)
#   RCLONE_CONFIG  wie bei rclone selbst; auf dem Server die Datei aus LoadCredential
#   HEALTHCHECK_URL  optional: Ping bei Erfolg, URL/fail bei Fehler
#
# Ausgang: 0 erneuert (oder Trockenlauf bestanden), 1 eine Pruefung schlug fehl --
# dann wird nichts hochgeladen und die alte Grundkarte bleibt --, 2 Werkzeug fehlt.
set -euo pipefail
cd "$(dirname "$0")/../.."

REGION=${REGION:-deploy/basemap/germany-region.geojson}
BUILD_BASE=${BUILD_BASE:-https://build.protomaps.com}
MIN_GB=${MIN_GB:-5}
MAX_GB=${MAX_GB:-7}
R2_REMOTE=${R2_REMOTE:-bubatz:bubatz}
KEEP_PREV=${KEEP_PREV:-1}
say() { echo "basemap-refresh: $*"; }

ende() {
  if [ -n "${HEALTHCHECK_URL:-}" ]; then
    if [ "$1" -eq 0 ]; then ziel=$HEALTHCHECK_URL; else ziel=$HEALTHCHECK_URL/fail; fi
    curl -fsS -m 10 --retry 3 -o /dev/null "$ziel" || say "Healthcheck nicht erreichbar"
  fi
  exit "$1"
}
fehler() { say "FEHLER: $*"; ende 1; }

for w in curl pmtiles python3; do
  command -v "$w" >/dev/null || { say "PRUEFUNG KAPUTT: $w fehlt"; ende 2; }
done
[ -n "${DRY_RUN:-}" ] || command -v rclone >/dev/null || { say "PRUEFUNG KAPUTT: rclone fehlt"; ende 2; }
[ -f "$REGION" ] || { say "PRUEFUNG KAPUTT: $REGION fehlt"; ende 2; }

WORK_DIR=${WORK_DIR:-$(mktemp -d /var/tmp/basemap-refresh.XXXXXX)}
mkdir -p "$WORK_DIR"
trap 'rm -rf "$WORK_DIR"' EXIT
frei_gb=$(df -P --block-size=1G "$WORK_DIR" | awk 'NR == 2 {print $4}')
[ "$frei_gb" -ge $((MAX_GB + 1)) ] || fehler "$WORK_DIR hat $frei_gb GB frei, gebraucht $((MAX_GB + 1))"

# 1. Neuesten Build finden: die Builds heissen nach ihrem Tag; heute ist er oft
#    noch nicht fertig, also bis zu acht Tage zurueck.
quelle=
for tage in 0 1 2 3 4 5 6 7; do
  tag=$(date -u -d "-$tage day" +%Y%m%d)
  if curl -fsSI -m 20 -o /dev/null "$BUILD_BASE/$tag.pmtiles"; then
    quelle=$BUILD_BASE/$tag.pmtiles
    break
  fi
done
[ -n "$quelle" ] || fehler "kein Build der letzten acht Tage unter $BUILD_BASE"
say "Quelle $quelle"

# 2. Schneiden, ohne den ganzen Planeten zu laden (~4 min, Range-Anfragen).
ziel=$WORK_DIR/basemap.pmtiles
pmtiles extract "$quelle" "$ziel" --region="$REGION" --download-threads=8 --quiet \
  || fehler "pmtiles extract"

# 3. Pruefen, bevor irgendetwas hochgeladen wird.
groesse=$(stat -c %s "$ziel")
python3 -I -c "import sys; g=$groesse/1e9; sys.exit(0 if $MIN_GB <= g <= $MAX_GB else 1)" \
  || fehler "Groesse $groesse Bytes ausserhalb $MIN_GB..$MAX_GB GB"
pmtiles verify "$ziel" >/dev/null 2>&1 || fehler "pmtiles verify"
kopf=$(pmtiles show --header-json "$ziel")
meta=$(pmtiles show --metadata "$ziel")
python3 -I - "$REGION" "$kopf" "$meta" <<'PY' || fehler "Kopf, Fassung oder Grenzen passen nicht"
import json, sys
region, kopf, meta = json.load(open(sys.argv[1])), json.loads(sys.argv[2]), json.loads(sys.argv[3])
# Der Stil @protomaps/basemaps in viewer/vendor/basemaps/ erwartet Kacheln der Fassung 4.x.
fassung = str(meta.get("version", ""))
assert kopf["tile_type"] == "mvt", kopf["tile_type"]
assert fassung.startswith("4."), f"Kachelfassung {fassung!r}, erwartet 4.x"
# Die Grenzen des Archivs muessen die Schnittflaeche abdecken (bis auf 0,05 Grad).
def punkte(g):
    if g["type"] == "FeatureCollection":
        for f in g["features"]: yield from punkte(f["geometry"])
    elif g["type"] == "Feature": yield from punkte(g["geometry"])
    else:
        def tief(c):
            if isinstance(c[0], (int, float)): yield c
            else:
                for x in c: yield from tief(x)
        yield from tief(g["coordinates"])
p = list(punkte(region))
w, s, e, n = min(x for x, _ in p), min(y for _, y in p), max(x for x, _ in p), max(y for _, y in p)
b = kopf["bounds"]
eps = 0.05
assert b[0] <= w + eps and b[1] <= s + eps and b[2] >= e - eps and b[3] >= n - eps, (b, (w, s, e, n))
print(f"basemap-refresh: OK   Fassung {fassung}, Grenzen {b}")
PY
say "OK   $groesse Bytes, pmtiles verify bestanden"

if [ -n "${DRY_RUN:-}" ]; then
  say "Trockenlauf: nichts hochgeladen"
  ende 0
fi

# 4. Hochladen. Erst die bisherige Grundkarte serverseitig als Vorgaenger kopieren
#    (die Zonenregel laesst basemap.prev.pmtiles nicht nach aussen), dann die neue.
if [ "$KEEP_PREV" = 1 ]; then
  rclone copyto "$R2_REMOTE/basemap.pmtiles" "$R2_REMOTE/basemap.prev.pmtiles" \
    || fehler "Vorgaenger nicht gesichert, nichts hochgeladen"
fi
rclone copyto "$ziel" "$R2_REMOTE/basemap.pmtiles" --s3-upload-concurrency 8 --s3-chunk-size 64M \
  || fehler "Hochladen gescheitert"
oben=$(rclone lsjson "$R2_REMOTE/basemap.pmtiles" | python3 -I -c 'import json,sys; print(json.load(sys.stdin)[0]["Size"])')
[ "$oben" = "$groesse" ] || fehler "im Bucket $oben Bytes, lokal $groesse"
say "erneuert: $R2_REMOTE/basemap.pmtiles ($groesse Bytes, aus $quelle)"
ende 0
