"""Merkmalsgleichheit zweier Kachelarchive auf einer Stichprobe.

tippecanoe je Block + tile-join (P2) muss je Kachel dieselbe MENGE an
Merkmalen liefern wie ein tippecanoe-Lauf ueber alles -- die Reihenfolge
darf abweichen. Geprueft wird auf einer Stichprobe von Kacheln, die aus
zufaelligen Objekten der Zonendaten (alle Zoomstufen) und der Umgebung des
Ankers abgeleitet wird; beide Archive werden je Kachel mit
`tippecanoe-decode` dekodiert, jedes Merkmal auf (Layer, Attribute,
Geometrietyp, Koordinaten auf 7 Stellen) normiert und als Multimenge
verglichen. Aufruf::

    uv run python scripts/rollout_check_tiles_equal.py <a.pmtiles> <b.pmtiles> <zones-dir> \\
        [--n 300] [--seed 1] [--anchor 29359108]

Exit 0 = alle Stichprobenkacheln gleich, 1 = Abweichung (Details auf stdout).
"""

from __future__ import annotations

import argparse
import json
import math
import random
import subprocess
from collections import Counter
from pathlib import Path

import pyarrow.parquet as pq
import shapely
from pyproj import Transformer

ZOOMS = range(8, 15)


def tile_xy(lon: float, lat: float, z: int) -> tuple[int, int]:
    n = 2**z
    x = int((lon + 180.0) / 360.0 * n)
    lat_r = math.radians(lat)
    y = int((1.0 - math.log(math.tan(lat_r) + 1.0 / math.cos(lat_r)) / math.pi) / 2.0 * n)
    return min(max(x, 0), n - 1), min(max(y, 0), n - 1)


def decode(archive: Path, z: int, x: int, y: int) -> Counter:
    r = subprocess.run(
        ["tippecanoe-decode", str(archive), str(z), str(x), str(y)], capture_output=True, text=True
    )
    if r.returncode != 0 or not r.stdout.strip():
        return Counter()
    fc = json.loads(r.stdout)
    feats = fc.get("features", [])
    # tippecanoe-decode: bei Kacheln eine FeatureCollection je Layer, verschachtelt
    out: Counter = Counter()
    stack = list(feats)
    while stack:
        f = stack.pop()
        if f.get("type") == "FeatureCollection":
            stack.extend(f.get("features", []))
            continue
        layer = f.get("tippecanoe", {}).get("layer", "")
        props = tuple(sorted(f.get("properties", {}).items()))
        g = f.get("geometry") or {}
        coords = json.dumps(g.get("coordinates"), separators=(",", ":"))
        out[(layer, props, g.get("type"), coords)] += 1
    return out


def sample_points(zones_dir: Path, n: int, seed: int) -> list[tuple[float, float]]:
    rng = random.Random(seed)
    files = sorted(zones_dir.glob("*.parquet"))
    rng.shuffle(files)
    tr = Transformer.from_crs("EPSG:25832", "EPSG:4326", always_xy=True)
    pts: list[tuple[float, float]] = []
    for f in files:
        t = pq.read_table(f, columns=["geometry"])
        rows = t.column("geometry").to_pylist()
        for w in rng.sample(rows, min(3, len(rows))):
            c = shapely.from_wkb(w).centroid
            lon, lat = tr.transform(c.x, c.y)
            pts.append((lon, lat))
        if len(pts) >= n:
            break
    return pts[:n]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("a", type=Path)
    ap.add_argument("b", type=Path)
    ap.add_argument("zones_dir", type=Path)
    ap.add_argument(
        "--n", type=int, default=300, help="Objekte fuer die Stichprobe (je Objekt alle Zoomstufen)"
    )
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--anchor", type=int, default=29359108)
    args = ap.parse_args()

    pts = sample_points(args.zones_dir, args.n, args.seed)
    tiles: set[tuple[int, int, int]] = set()
    for lon, lat in pts:
        for z in ZOOMS:
            x, y = tile_xy(lon, lat, z)
            tiles.add((z, x, y))
    # Anker: seine Kachel und die 8 Nachbarn je Zoom
    for f in sorted(args.zones_dir.glob("*.parquet")):
        t = pq.read_table(
            f, columns=["osm_id", "geometry"], filters=[("osm_id", "==", args.anchor)]
        )
        if t.num_rows:
            tr = Transformer.from_crs("EPSG:25832", "EPSG:4326", always_xy=True)
            c = shapely.from_wkb(t.column("geometry")[0].as_py()).centroid
            lon, lat = tr.transform(c.x, c.y)
            for z in ZOOMS:
                x, y = tile_xy(lon, lat, z)
                for dx in (-1, 0, 1):
                    for dy in (-1, 0, 1):
                        tiles.add((z, x + dx, y + dy))
            break
    tiles_sorted = sorted(tiles)
    n_equal = n_diff = n_empty = 0
    total_a = total_b = 0
    diffs: list[str] = []
    for z, x, y in tiles_sorted:
        ca, cb = decode(args.a, z, x, y), decode(args.b, z, x, y)
        total_a += sum(ca.values())
        total_b += sum(cb.values())
        if not ca and not cb:
            n_empty += 1
        if ca == cb:
            n_equal += 1
        else:
            n_diff += 1
            only_a = sum((ca - cb).values())
            only_b = sum((cb - ca).values())
            if len(diffs) < 10:
                diffs.append(
                    f"  {z}/{x}/{y}: a={sum(ca.values())} b={sum(cb.values())} "
                    f"nur_a={only_a} nur_b={only_b}"
                )
    print(
        f"Kacheln: {len(tiles_sorted)} (aus {len(pts)} Objekten + Anker), "
        f"gleich {n_equal}, verschieden {n_diff}, leer {n_empty}"
    )
    print(f"Merkmale gesamt: a={total_a} b={total_b}")
    if diffs:
        print("Abweichungen:")
        print("\n".join(diffs))
        raise SystemExit(1)
    print("OK: Merkmalsmengen je Kachel identisch (Reihenfolge nicht verglichen)")


if __name__ == "__main__":
    main()
