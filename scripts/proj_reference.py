"""Referenzpunkte des Projektionstests pruefen oder neu erzeugen.

``rust/bubatz/src/proj.rs`` portiert den PROJ-Pfad EPSG:4326 -> EPSG:25832 und
vergleicht sich im Test bitweise mit pyproj. Die Bits haengen aber nicht nur am
Code: ``sin``, ``cos``, ``atan2``, ``hypot`` und ``asinh`` kommen bei PROJ wie bei
Rust aus der libm des Systems, und die aendert die letzten Bits zwischen Fassungen
(glibc 2.43 bringt ein neues, korrekt gerundetes ``asinh``: 731 der 9.998 Rechtswerte
verschieben sich um bis zu 2 ULP). ``tests/data/proj_points.json`` traegt darum
Generationen:

* ``x``/``y``: der Massstab, pyproj unter der juengsten glibc, auf der die CI laeuft;
* ``x_<name>``/``y_<name>``: aeltere Generationen (``glibc239`` gilt unveraendert bis
  glibc 2.41), damit der Test auf Maschinen mit aelterer libm weiter etwas aussagt.

Aufruf::

    uv run python scripts/proj_reference.py              # nur zaehlen, je Generation
    ... proj_reference.py --keep-as glibc243 --write     # Massstab aufheben, neu rechnen

Ohne ``--write`` aendert das Skript nichts. Fuer eine fremde glibc laeuft es in einem
Container, der die hiesige Umgebung nur liest (so entstand der Massstab unter
glibc 2.43; ``V`` ist das aufgeloeste ``.venv``, ``U`` das Python-Verzeichnis von
uv, beide unter demselben Pfad eingehaengt)::

    docker run --rm --network none --user "$(id -u):$(id -g)" \\
        -v "${V}:${V}:ro" -v "${U}:${U}:ro" -v "${PWD}/scripts:/s:ro" \\
        -v "${PWD}/rust/bubatz/tests/data:/d" ubuntu:26.04 \\
        "${V}/bin/python" -I /s/proj_reference.py --file /d/proj_points.json \\
        --keep-as glibc239 --write

Die Datei speichert jede Zahl als u64-Bitmuster des f64, nicht als Dezimalzahl.
"""

from __future__ import annotations

import argparse
import json
import platform
import struct
from pathlib import Path

import pyproj
from pyproj import Transformer

DATEI = Path(__file__).resolve().parent.parent / "rust/bubatz/tests/data/proj_points.json"


def _floats(bits: list[int]) -> list[float]:
    return [struct.unpack("<d", struct.pack("<Q", b))[0] for b in bits]


def _bits(werte: list[float]) -> list[int]:
    return [struct.unpack("<Q", struct.pack("<d", w))[0] for w in werte]


def rechne(lon: list[float], lat: list[float]) -> tuple[list[int], list[int]]:
    """x/y aller Punkte mit pyproj, als Bitmuster."""
    t = Transformer.from_crs("EPSG:4326", "EPSG:25832", always_xy=True)
    punkte = [t.transform(lo, la) for lo, la in zip(lon, lat, strict=True)]
    return _bits([float(p[0]) for p in punkte]), _bits([float(p[1]) for p in punkte])


def main() -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("--file", type=Path, default=DATEI, help="Referenzdatei")
    ap.add_argument(
        "--keep-as",
        metavar="NAME",
        help="den bisherigen Massstab als Generation x_NAME/y_NAME aufheben",
    )
    ap.add_argument(
        "--write",
        action="store_true",
        help="x/y mit dem Ergebnis dieser Maschine ueberschreiben",
    )
    args = ap.parse_args()

    daten: dict[str, list[int]] = json.loads(args.file.read_text())
    xb, yb = rechne(_floats(daten["lon"]), _floats(daten["lat"]))
    n = len(xb)
    print(
        f"glibc {platform.libc_ver()[1]}, pyproj {pyproj.__version__}, "
        f"PROJ {pyproj.proj_version_str}, {n} Punkte"
    )
    for key in sorted(k for k in daten if k == "x" or k.startswith("x_")):
        ys = daten["y" + key[1:]]
        gleich = sum(1 for i in range(n) if xb[i] == daten[key][i] and yb[i] == ys[i])
        print(f"  {key[2:] or 'Massstab'}: {gleich}/{n} bitgleich")

    if args.keep_as:
        daten[f"x_{args.keep_as}"] = daten["x"]
        daten[f"y_{args.keep_as}"] = daten["y"]
    if args.write:
        daten["x"], daten["y"] = xb, yb
    if args.keep_as or args.write:
        args.file.write_text(json.dumps(daten))
        print(f"geschrieben: {args.file}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
