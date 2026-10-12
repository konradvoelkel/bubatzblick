#!/usr/bin/bash
# Seite auf den Server -- RUECKFALL. Normalweg ist `git push` gegen das
# Server-Remote (post-receive-Hook, Infra-Repo); dieses Skript baut dasselbe
# lokal und rsynct es (DRY_RUN=1 zeigt nur, was passieren wuerde).
#
# Baut das Deploy-Verzeichnis (about.html, config.js mit den Archiv-URLs,
# __SITE_URL__ ersetzt), komprimiert es vor (Caddy `precompressed`) und rsynct
# es nach VPS_WWW. Die Header (CSP usw.) setzt Caddy im Infra-Repo. Laeuft nur,
# wenn sich die Seite aendert -- die Daten gehen taeglich direkt in den Bucket.
#
# Umgebung (Vorgabe aus deploy/site.env, Vorlage deploy/site.env.example):
#   VPS_HOST        ssh-Alias des Servers (Adresse und Schluessel: ~/.ssh/config)
#   VPS_WWW         Zielverzeichnis, Vorgabe /srv/bubatz/www
#   TILES_BASE_URL  https://<TILES_DOMAIN>
#   DRY_RUN=1       nur zeigen, was rsync taete
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -f deploy/site.env ] || { echo "deploy/site.env fehlt -- aus deploy/site.env.example anlegen" >&2; exit 1; }
. deploy/site.env
VPS_HOST=${VPS_HOST:?deploy/site.env: VPS_HOST fehlt}
VPS_WWW=${VPS_WWW:-/srv/bubatz/www}
TILES_BASE_URL=${TILES_BASE_URL:-https://$TILES_DOMAIN}
SITE_URL=https://$SITE_DOMAIN
VIEWER_DIR=${VIEWER_DIR:-viewer}
TILES_BASE_URL=${TILES_BASE_URL%/}

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
deploy/bin/build_about.py >/dev/null   # about.md -> about.html
# `--exclude='.*'` haelt streunende Punktdateien heraus; `.well-known/` muss
# aber mit (security.txt), deshalb vorher einschliessen -- bei rsync gewinnt die
# erste passende Regel.
rsync -a --exclude='about.md' --include='.well-known/' --include='.well-known/**' \
  --exclude='.*' "$VIEWER_DIR/" "$out/"
# Platzhalter in JEDER Seite ersetzen, nicht nur in den zwei namentlich bekannten, und danach
# pruefen, dass keiner uebrig ist: eine dritte Seite liefe sonst mit einer Karten-URL
# "__SITE_URL__/img/og.png" aus. Dieselbe Pruefung steht im Server-Hook (Infra-Repo,
# deploy-site) -- beide Skripte bauen gleich, aendert sich eins, zieht das andere nach.
find "$out" -name '*.html' -print0 | xargs -0 -r sed -i "s|__SITE_URL__|$SITE_URL|g"
rest=$(grep -rl '__[A-Z0-9_]*__' --include='*.html' "$out" || true)
[ -z "$rest" ] || { echo "deploy_vps: unersetzter Platzhalter in: $rest" >&2; exit 1; }
cat > "$out/config.js" <<CFG
// Von deploy/bin/deploy_vps.sh erzeugt ($(date -u +%FT%TZ)): Daten aus dem R2-Bucket,
// Seite, Glyphen und Sprites vom eigenen Server (derselbe Ursprung wie diese Datei).
window.BUBATZ_CONFIG = {
  zones: "$TILES_BASE_URL/zonen.pmtiles",
  basemap: "$TILES_BASE_URL/basemap.pmtiles",
  assets: "vendor/basemaps",
  stand: "$TILES_BASE_URL/stand.json",
};
CFG
# Vorkomprimieren (brotli/zstd/gzip, sofern vorhanden); Caddy liefert die passende Datei.
find "$out" -type f \( -name '*.html' -o -name '*.css' -o -name '*.js' -o -name '*.json' -o -name '*.svg' -o -name '*.txt' -o -name '*.pbf' \) -print0 |
  xargs -0 -r -P "$(nproc)" -I{} sh -c '
    command -v brotli >/dev/null && brotli -kf "$1"
    command -v zstd   >/dev/null && zstd -q -kf -19 "$1"
    gzip -kf -9 "$1"' _ {}
echo "deploy_vps: $(find "$out" -type f | wc -l) Dateien, $(du -sh "$out" | cut -f1), Seite $SITE_URL, Daten von $TILES_BASE_URL -> $VPS_HOST:$VPS_WWW"
# Kein `| grep` hinter rsync: die Pipe verdeckt den Exit-Code, ein teilweiser
# Upload (Exit 23) ginge als Erfolg durch -- und unter `pipefail` waere jeder
# gelungene Upload ein Fehler, sobald rsync seine Statistikzeilen weglaesst.
# --itemize-changes zeigt stattdessen, welche Dateien sich bewegt haben.
# --chmod: die Rechte des Servers gehoeren dem Server (Verzeichnisse 755,
# Dateien 644, Caddy liest nur), sonst traegt -a die umask des bauenden
# Rechners hinueber und das Zielverzeichnis schwankt je nach Deploy-Weg.
rsync -a --chmod=D755,F644 --delete --itemize-changes ${DRY_RUN:+--dry-run} "$out/" "$VPS_HOST:$VPS_WWW/"
