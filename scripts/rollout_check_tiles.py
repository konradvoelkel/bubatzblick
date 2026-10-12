"""Kachel-Eingabe gegen Zonendaten fuer EIN Objekt.

Kacheln sind kein reiner Darstellungsschritt -- `one_sided_simplify` darf
nach aussen wachsen. Bei 8 m Toleranz blaeht das das Referenzobjekt um 92 %
auf, bei 0,5 m sind es rund +1 %. Deshalb nach jedem `tiles`-Lauf messen,
statt es anzunehmen. Aufruf::

    uv run python scripts/rollout_check_tiles.py <out-dir> <osm_id> <zones-dir> \\
        [<protected.parquet>]

``<out-dir>`` ist das Verzeichnis mit ``zonen.tiles-cache/``, ``<zones-dir>``
das Zonenverzeichnis des Laufs; ``protected.parquet`` liegt standardmaessig
neben ``<zones-dir>`` (``interim/<gebiet>/protected.parquet``). Groessenordnung
fuer das Referenzobjekt ``osm_id=29359108``: Zonendaten 8.461 m2, Kachel rund
1 % darunter -- im z14-Band sind die Gebaeudegrundrisse abgezogen.

Das Skript liest nur die EINE Zelle des Objekts statt aller 52.000
Zellendateien (Sekunden statt knapp einer halben Stunde):
Zelle per Schwerpunkt wie ``grid.cell_index`` (Rust ``grid.rs``), Cache-
Eintrag per ``cell_digest`` wie ``tiles.rs`` (``SIMPLIFY_VERSION`` und das
Zeilenformat sind dort definiert und hier gespiegelt -- aendert sich eines,
findet das Skript keinen Eintrag und sagt das).
"""

from __future__ import annotations

import hashlib
import json
import math
import sys
import tomllib
from pathlib import Path

import pyarrow.parquet as pq
import shapely
from pyproj import Transformer
from shapely.geometry import shape
from shapely.ops import transform

RADIUS_M = 100
SIMPLIFY_VERSION = "v1"  # tiles.rs
CACHE_LEVEL = "t0.5_nobld"  # feinste Stufe, z14, mit Gebaeudeabzug


def cell_index(x: float, y: float, cell_size: float) -> tuple[int, int]:
    """Wie ``grid.rs::cell_index``: floor, dann gegen die exakten Grenzen justiert."""
    col, row = math.floor(x / cell_size), math.floor(y / cell_size)
    if x < col * cell_size:
        col -= 1
    if x >= (col + 1) * cell_size:
        col += 1
    if y < row * cell_size:
        row -= 1
    if y >= (row + 1) * cell_size:
        row += 1
    return col, row


def cell_digest(table, crs_out: str) -> str:
    """Wie ``tiles.rs::cell_digest`` (blake2b-12, Python-``json.dumps``-Format)."""
    # `osm_type` steht im Abdruck (Kachelvertrag) -- fehlt es, stimmt die
    # Zelle nicht mit dem, was `tiles.rs` erwartet, und das soll auffallen.
    rows = table.select(["osm_id", "category", "band_max_m", "cache_key", "osm_type"]).to_pylist()
    body = json.dumps(rows, sort_keys=True)
    h = hashlib.blake2b(digest_size=12)
    h.update(
        f"{SIMPLIFY_VERSION}|{crs_out}|osm_id,osm_type,category,band_max_m,cache_key|".encode()
    )
    h.update(body.encode())
    return h.hexdigest()


def main() -> None:
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    out_dir, osm_id, zones_dir = Path(sys.argv[1]), int(sys.argv[2]), Path(sys.argv[3])
    protected = Path(sys.argv[4]) if len(sys.argv) > 4 else zones_dir.parent / "protected.parquet"
    cfg = tomllib.loads(
        (Path(__file__).resolve().parents[1] / "config" / "pipeline.toml").read_text()
    )
    cell_size = float(cfg["grid"]["cell_size_m"])
    crs_out = cfg["crs"]["output"]

    # 1. Zelle des Objekts: Schwerpunkt der Quelle (EPSG:25832) wie die Zuordnung.
    src = pq.read_table(
        protected, columns=["osm_id", "geometry"], filters=[("osm_id", "==", osm_id)]
    )
    if src.num_rows == 0:
        raise SystemExit(f"osm_id={osm_id} nicht in {protected}")
    c = shapely.from_wkb(src.column("geometry")[0].as_py()).centroid
    col, row = cell_index(c.x, c.y, cell_size)
    cell = f"x{col}_y{row}"
    cell_path = zones_dir / f"{cell}.parquet"
    if not cell_path.exists():
        raise SystemExit(f"Zelle {cell} fehlt in {zones_dir}")

    # 2. Zonendaten dieser Zelle.
    zt = pq.read_table(cell_path)
    mask = [
        (o == osm_id and b <= RADIUS_M)
        for o, b in zip(
            zt.column("osm_id").to_pylist(), zt.column("band_max_m").to_pylist(), strict=True
        )
    ]
    geoms_col = zt.column("geometry").to_pylist()
    zgeoms = [shapely.from_wkb(w) for w, m in zip(geoms_col, mask, strict=True) if m]
    if not zgeoms:
        raise SystemExit(f"osm_id={osm_id} nicht in Zelle {cell}")
    z = shapely.union_all(shapely.make_valid(zgeoms)).area

    # 3. Kachel-Eingabe: Cache-Eintrag dieser Zelle mit dem aktuellen digest
    #    (der Gebaeude-Anteil des Schluessels ist unbekannt -> Muster; bei
    #    mehreren Treffern der juengste).
    digest = cell_digest(zt, crs_out)
    cache = out_dir / "zonen.tiles-cache" / CACHE_LEVEL
    hits = sorted(cache.glob(f"{cell}__{digest}-*.geojsonl"), key=lambda p: p.stat().st_mtime)
    if not hits:
        raise SystemExit(
            f"kein Cache-Eintrag {cell}__{digest}-*.geojsonl unter {cache} "
            "(SIMPLIFY_VERSION/Zeilenformat in tiles.rs geaendert, "
            "oder tiles noch nicht gelaufen?)"
        )
    tr = Transformer.from_crs(crs_out, "EPSG:25832", always_xy=True)
    geoms = []
    with hits[-1].open() as fh:
        for line in fh:
            o = json.loads(line)
            pr = o["properties"]
            if pr.get("osm_id") == osm_id and pr["band_max_m"] <= RADIUS_M:
                geoms.append(shape(o["geometry"]))
    if not geoms:
        raise SystemExit(f"osm_id={osm_id} nicht in {hits[-1]}")

    def nach_utm(x: float, y: float, z: float | None = None) -> tuple[float, ...]:
        return tuple(tr.transform(x, y))

    u = transform(nach_utm, shapely.union_all(shapely.make_valid(geoms)))
    print(f"Zelle:          {cell}  ({hits[-1].name})")
    print(f"Kachel-Eingabe: {round(u.area)} m2  ({len(geoms)} Teilgeometrien)")
    print(f"Zonendaten:     {round(z)} m2")
    print(f"Aufblaehung:    {100 * (u.area / z - 1):+.1f} %  (Soll: hoechstens ~1 %)")


if __name__ == "__main__":
    main()
