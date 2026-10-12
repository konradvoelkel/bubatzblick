#!/usr/bin/env -S uv run --with pillow --script
"""Open-Graph-Vorschaubild viewer/img/og.png (1200x630) aus dem Sichtfeld-Blatt.
Bewusst ohne Domain im Bild (Domain nur in deploy/site.env); der Teiler zeigt
die URL selbst an. Braucht inkscape fuer das SVG-Rendering."""

import pathlib
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFont

tmp = pathlib.Path(tempfile.mkdtemp())
subprocess.run(
    ["inkscape", "viewer/img/sichtfeld-blatt.svg", "-w", "560", "-o", tmp / "leaf.png"],
    check=True,
    capture_output=True,
)
og = Image.new("RGB", (1200, 630), (242, 239, 233))
leaf = Image.open(tmp / "leaf.png").convert("RGBA")
og.paste(leaf, (10, 35), leaf)
d = ImageDraw.Draw(og)


def font(sz, bold=False):
    return ImageFont.truetype(
        "/usr/share/fonts/truetype/dejavu/DejaVuSans%s.ttf" % ("-Bold" if bold else ""), sz
    )


x = 600
W = 1200 - x - 40


def fit(text, sz, bold=False):
    while d.textlength(text, font=font(sz, bold)) > W and sz > 12:
        sz -= 1
    return font(sz, bold)


d.text((x, 150), "BubatzBlick", font=fit("BubatzBlick", 60, True), fill=(34, 34, 34))
d.text(
    (x, 230),
    "Bubatzkarte 2.0 – wo Kiffen in Sichtweite verboten ist",
    font=fit("Bubatzkarte 2.0 – wo Kiffen in Sichtweite verboten ist", 30),
    fill=(85, 85, 85),
)
d.rectangle((x, 285, x + 70, 291), fill=(192, 57, 43))
lines = [
    "Konsumverbot nach § 5 Abs. 2 KCanG: Schulen,",
    "Spielplätze, Jugendeinrichtungen, Sportstätten,",
    "Anbauvereinigungen — bis 100 m, und nur dort,",
    "wo kein Gebäude die Sicht verdeckt.",
    "",
    "Aus OpenStreetMap, täglich neu berechnet.",
]
y = 320
for line in lines:
    d.text((x, y), line, font=fit(line, 24), fill=(60, 60, 60))
    y += 36
og.save("viewer/img/og.png", optimize=True)
print("viewer/img/og.png")
