#!/usr/bin/bash
# Die drei Pruefungen nach einem Bundeslauf am Stueck: Ankerflaeche, Kachel-
# aufblaehung am Anker, Kennzahlen samt Diff zum Vorlauf
# (README.md "Pruefungen je Lauf").
#
# Gedacht fuer den Lauf VON HAND nach einer Aenderung an zones/tiles/tags --
# im Tageszyklus laeuft die Kachelpruefung ohnehin als Tor vor der
# Veroeffentlichung (scripts/publish_germany.sh).
#
# Verzeichnisse heissen wie im Makefile; Voreinstellung ist der Bestand im
# Repo:
#
#   scripts/rollout_checks_germany.sh
#   INTERIM_DIR=/pfad/data/interim OUT_DIR=/pfad/data/out \
#     scripts/rollout_checks_germany.sh
#
# ANCHOR_ID ist der oeffentliche Spielplatz aus README.md.
set -u
P=$(cd "$(dirname "$0")/.." && pwd)
cd "$P" || exit 1
INTERIM_DIR=${INTERIM_DIR:-data/interim}
OUT_DIR=${OUT_DIR:-data/out}
ANCHOR_ID=${ANCHOR_ID:-29359108}
OUT=${OUT:-$(mktemp -d -t bubatz-checks-XXXX)}

Z=$INTERIM_DIR/germany/zones
PROT=$INTERIM_DIR/germany/protected.parquet
BUBATZ=${BUBATZ:-rust/target/release/bubatz}
for f in "$Z/.stamp" "$PROT" "$BUBATZ"; do
  [ -e "$f" ] || { echo "FEHLT: $f -- Lauf unvollstaendig oder falsches Verzeichnis"; exit 2; }
done
echo "== Pruefungen fuer $INTERIM_DIR / $OUT_DIR -> $OUT"
rc=0

echo "== anchor $(date +%T)"
"$BUBATZ" verify anchor --zones-dir "$Z" --protected-path "$PROT" > "$OUT/anchor.txt" 2>&1
e=$?; echo "anchor EXIT=$e"; tail -3 "$OUT/anchor.txt"; [ $e -eq 0 ] || rc=1

echo "== tiles $(date +%T)"
uv run --no-sync python scripts/rollout_check_tiles.py "$OUT_DIR/germany" "$ANCHOR_ID" "$Z" > "$OUT/tiles.txt" 2>&1
e=$?; echo "tiles EXIT=$e"; tail -5 "$OUT/tiles.txt"; [ $e -eq 0 ] || rc=1

echo "== report $(date +%T)"
prev=""
[ -f "$INTERIM_DIR/germany/zones.prev/.stamp" ] && prev="--previous $INTERIM_DIR/germany/zones.prev"
# shellcheck disable=SC2086
"$BUBATZ" report --zones-dir "$Z" $prev --out-json "$OUT/report.json" > "$OUT/report.txt" 2>&1
e=$?; echo "report EXIT=$e"; head -40 "$OUT/report.txt"; [ $e -eq 0 ] || rc=1

echo "== fertig $(date +%T), Protokolle in $OUT (Gesamtergebnis $rc)"
exit $rc
