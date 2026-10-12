// Laufzeitkonfiguration des Viewers. Voreinstellung: alles liegt
// neben dieser Seite (lokal `bubatz serve --basemap …`, oder ein statischer
// Server). Fuer den Server schreibt der Deploy (bin/deploy-site des
// post-receive-Hooks, Rueckfall deploy/bin/deploy_vps.sh) eine config.js mit
// absoluten URLs des R2-Buckets; die Seite selbst bleibt gleich.
window.BUBATZ_CONFIG = {
  zones: "zonen.pmtiles",        // Verbotszonen (PMTiles, Range-Anfragen)
  basemap: "basemap.pmtiles",    // Protomaps-Grundkarte (PMTiles, Range-Anfragen)
  assets: "vendor/basemaps",     // Glyphen (fonts/) und Sprites (sprites/v4/) der Grundkarte
  stand: "stand.json",           // Datenstand (schreibt make publish)
};
