#!/usr/bin/bash
# Tageszyklus auf dem Server (Einheit bubatz-daily.service, Umgebung aus
# /srv/bubatz/env): PBF per Geofabrik-Tagesdiff fortschreiben, `make germany`
# (inkrementell), `make publish` (Pruefkette, atomarer Tausch in die Webwurzel).
# Ohne neue Folge endet der Zyklus nach update-pbf (~1,5 min); der Timer
# startet alle sechs Stunden (die Tagesfolge kommt meist ~02:40 Ortszeit).
set -u
: "${BUBATZ_REPO:?}" "${RAW_DIR:?}" "${INTERIM_DIR:?}" "${OUT_DIR:?}" "${PUBLISH_DIR:?}" "${LOG_DIR:?}" "${N_JOBS:?}"
cd "$BUBATZ_REPO" || exit 1
mkdir -p "$LOG_DIR"
exec > >(tee -a "$LOG_DIR/daily-$(date +%F).log") 2>&1
exec 9>"$LOG_DIR/.daily.lock"
flock -n 9 || { echo "bubatz-daily: laeuft bereits, Ende"; exit 0; }

V="RAW_DIR=$RAW_DIR INTERIM_DIR=$INTERIM_DIR OUT_DIR=$OUT_DIR PUBLISH_DIR=$PUBLISH_DIR N_JOBS=$N_JOBS"
PBF=$RAW_DIR/germany-latest.osm.pbf
ping() { [ -n "${HEALTHCHECK_URL:-}" ] && curl -fsS -m 10 --retry 3 -o /dev/null "$HEALTHCHECK_URL$1"; true; }
START=$(date '+%F %T')
# Statistikseite (deploy/bin/build_stats.py, STATS_DIR = $PUBLISH_DIR/stats) nach jedem
# Ende, auch nach einem Fehler; kurz warten, bis tee das Log geschrieben hat.
stats() { sleep 1; BUBATZ_START="$START" BUBATZ_END="$(date '+%F %T')" BUBATZ_RESULT="$1" STATS_DIR="${STATS_DIR:-$PUBLISH_DIR/stats}" \
  python3 deploy/bin/build_stats.py 2>&1 || echo "stats: Seite nicht gebaut"; }
fail() { echo "== FEHLER: $1 ($(date +%T))"; ping /fail; stats "Fehler: $1"; exit 1; }
pbf_seq() { osmium fileinfo -e -g header.option.osmosis_replication_sequence_number "$PBF" 2>/dev/null || echo none; }

ping /start
echo "== bubatz-daily Start $START (N_JOBS=$N_JOBS)"
seq0=$(pbf_seq)

# 1. PBF: beim ersten Lauf laedt die Makefile-Regel den Geofabrik-Extrakt (~5,6 GB),
#    danach Tagesdiffs. update-pbf begrenzt die Diffmenge je Aufruf (--size);
#    deshalb wiederholen, bis sich die Folge nicht mehr bewegt.
prev=$seq0
for i in 1 2 3 4; do
  make update-pbf $V || fail "update-pbf (Durchgang $i)"
  cur=$(pbf_seq); echo "update-pbf Durchgang $i: Folge $prev -> $cur"
  [ "$cur" = "$prev" ] && break
  prev=$cur
done
seq1=$(pbf_seq)

if [ "$seq0" = "$seq1" ] && [ -f "$PUBLISH_DIR/zonen.pmtiles" ]; then
  echo "== keine neue Folge ($seq1), veroeffentlichter Stand bleibt; Ende $(date +%T)"
  ping ""; stats "keine neue Folge"; exit 0
fi

# 2. Lauf (extract, clip, zones inkrementell, tiles).
t=$(date +%s)
make germany $V || fail "make germany"
echo "== germany fertig in $(( $(date +%s) - t )) s"

# 3. Veroeffentlichen: Anker, Kachelaufblaehung, Plausibilitaet, pmtiles verify,
#    dann atomarer Tausch; scheitert eine Pruefung, bleibt der alte Stand.
make publish $V || fail "publish (alter Stand bleibt veroeffentlicht)"

ls -t "$LOG_DIR"/daily-*.log 2>/dev/null | tail -n +31 | xargs -r rm -f
echo "== bubatz-daily fertig $(date '+%F %T'), PBF-Folge $seq1"
ping ""
stats ok
