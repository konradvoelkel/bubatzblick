#!/usr/bin/env python3
"""Glyphenbloecke der mitgelieferten Schriften auf das reduzieren, was eine
Karte mit deutschen Beschriftungen abfragt.

    scripts/trim_glyphs.py --dry-run     # nur zeigen
    scripts/trim_glyphs.py               # loeschen

MapLibre holt je Beschriftung genau den 256er-Block, in dem die Zeichen liegen.
Der Protomaps-Stil liefert vier Schriftschnitte mit je 256 Bloecken aus -- rund
14 MB, von denen eine Deutschlandkarte eine Handvoll braucht. Fehlt ein Block,
antwortet der Server mit 404 und die betroffene Beschriftung bleibt leer; das
betrifft nur einzelne Namen, nicht die Karte.

Die Auswahl ist bewusst grosszuegiger als "nur Deutsch": in deutschen OSM-Daten
stehen polnische, tuerkische, vietnamesische, kyrillische, arabische, thai- und
Devanagari-Namen -- Gaststaetten und Laeden tragen sie im `name`-Tag.

CJK und Hangul fehlen mit Absicht und ohne Verlust: MapLibre rendert sie per
`localIdeographFontFamily` (Vorgabe `sans-serif`) mit der Schrift des Systems
und fragt dafuer gar keinen Block ab. Im Browser nachgemessen -- mit der
Vorgabe werden diese Bereiche nie angefordert, erst `localIdeographFontFamily:
false` laesst MapLibre sie laden.

Wer eine weitere Schrift braucht, nimmt ihren Block hier auf und holt die
Datei aus dem Protomaps-Build zurueck.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

FONTS = Path(__file__).resolve().parents[1] / "viewer/vendor/basemaps/fonts"

# (Startcodepoint, wofuer) -- jeder Eintrag ist ein 256er-Block.
BEHALTEN: dict[int, str] = {
    0: "Basic Latin + Latin-1 (Umlaute, ß)",
    256: "Latin Extended-A (polnisch, tschechisch, türkisch)",
    512: "Latin Extended-B",
    768: "kombinierende Akzente, Griechisch",
    1024: "Kyrillisch",
    1280: "Kyrillisch (Ergänzung), Hebräisch",
    1536: "Arabisch",
    2304: "Devanagari",
    3584: "Thai",
    7680: "Latin Extended Additional (vietnamesisch)",
    8192: "Satzzeichen, Währungszeichen",
    8448: "Buchstabenähnliche Zeichen, Zahlformen",
    8704: "mathematische Operatoren (2 KiB, der Vollstaendigkeit)",
}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    if not FONTS.is_dir():
        sys.exit(f"{FONTS} fehlt")
    behalten = {f"{lo}-{lo + 255}.pbf" for lo in BEHALTEN}
    weg, bytes_weg, bleibt = 0, 0, 0
    for schnitt in sorted(p for p in FONTS.iterdir() if p.is_dir()):
        for pbf in sorted(schnitt.glob("*.pbf")):
            if pbf.name in behalten:
                bleibt += pbf.stat().st_size
                continue
            weg += 1
            bytes_weg += pbf.stat().st_size
            if not args.dry_run:
                pbf.unlink()
    wort = "zu loeschen" if args.dry_run else "geloescht"
    print(
        f"trim_glyphs: {weg} Bloecke {wort} ({bytes_weg / 1e6:.1f} MB), "
        f"{len(behalten)} je Schnitt bleiben ({bleibt / 1e6:.1f} MB)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
