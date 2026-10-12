"""Zonen zweier Laeufe ueber dasselbe Gebiet vergleichen.

Gleicher ``cache_key`` heisst gleiche Quellgeometrie UND gleiche
Hindernisse in Reichweite. Speisen die Laeufe aus verschiedenen PBF-
Staenden (etwa Stadt-Extrakt gegen Landes-Extrakt), sind Abweichungen
erlaubt -- aber NUR, wo sich die PBFs tatsaechlich unterscheiden. Die Quote
allein beweist nichts: 98,4 % identisch kann voellig in Ordnung sein.
Deshalb loest das Skript jede Abweichung bis auf das einzelne Gebaeude auf:

1. Quellgeometrie identisch?  Nein -> PBF-Drift an der Quelle.
2. Sonst: Gebaeudebestand im Suchkasten (osm_id + Geometrie) in beiden
   ``buildings.parquet`` vergleichen.  Unterschied -> PBF-Drift an den
   Hindernissen.
3. Sonst: UNERKLAERT -> Hindernis-Auswahl oder Grenzfilter pruefen.

Bestanden heisst: **0 unerklaerte** Abweichungen. Aufruf::

    uv run python scripts/rollout_check_identity.py \\
        --zones-a data/interim/duesseldorf/zones \\
        --zones-b /media/.../nrw/zones \\
        --protected-a data/interim/duesseldorf/protected.parquet \\
        --protected-b /media/.../nrw/protected.parquet \\
        --buildings-a data/interim/buildings.parquet \\
        --buildings-b /media/.../nrw/raw/buildings.parquet
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import geopandas as gpd
import pandas as pd
import pyarrow.parquet as pq

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from bubatzblick.config import load_config


def keys(zones_dir: Path) -> pd.DataFrame:
    frames = [
        pq.read_table(f, columns=["osm_id", "band_max_m", "cache_key"]).to_pandas()
        for f in sorted(zones_dir.glob("*.parquet"))
    ]
    return pd.concat(frames).drop_duplicates(["osm_id", "band_max_m"])


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    for side in "ab":
        ap.add_argument(f"--zones-{side}", type=Path, required=True)
        ap.add_argument(f"--protected-{side}", type=Path, required=True)
        ap.add_argument(f"--buildings-{side}", type=Path, required=True)
    args = ap.parse_args()

    a, b = keys(args.zones_a), keys(args.zones_b)
    print(f"A: {a.osm_id.nunique():,} Objekte, {len(a):,} Baender")
    print(f"B: {b.osm_id.nunique():,} Objekte, {len(b):,} Baender")
    m = a.merge(b, on=["osm_id", "band_max_m"], suffixes=("_a", "_b"))
    same = int((m.cache_key_a == m.cache_key_b).sum())
    pct = 100 * same / len(m)
    print(f"gemeinsame Baender: {len(m):,}, identischer cache_key: {same:,} ({pct:.2f} %)")
    only_in_a = len(set(a.osm_id) - set(b.osm_id))
    only_in_b = len(set(b.osm_id) - set(a.osm_id))
    print(f"nur in A: {only_in_a:,} Objekte, nur in B: {only_in_b:,}")
    diff_ids = sorted(set(m.loc[m.cache_key_a != m.cache_key_b, "osm_id"]))
    print(f"\n{len(diff_ids)} abweichende Objekte -- Aufloesung:")
    if not diff_ids:
        print("  keine. Bestanden.")
        return

    reach = load_config().effective_reach_m
    pa = gpd.read_parquet(args.protected_a).set_index("osm_id")
    pb = gpd.read_parquet(args.protected_b).set_index("osm_id")
    ba = gpd.read_parquet(args.buildings_a)
    bb = gpd.read_parquet(args.buildings_b)
    ba, bb = ba.loc[ba.is_opaque], bb.loc[bb.is_opaque]
    sa, sb = ba.sindex, bb.sindex

    src_diff = 0
    obst_diff = 0
    only_a = only_b = changed = 0
    unexplained: list[int] = []
    for oid in diff_ids:
        ga, gb = pa.loc[oid, "geometry"], pb.loc[oid, "geometry"]
        if not ga.equals(gb):
            src_diff += 1
            continue
        box = ga.buffer(reach + 20)
        na = ba.iloc[sa.query(box, predicate="intersects")]
        nb = bb.iloc[sb.query(box, predicate="intersects")]
        da = dict(zip(na.osm_id, na.geometry, strict=True))
        db = dict(zip(nb.osm_id, nb.geometry, strict=True))
        oa, ob = set(da) - set(db), set(db) - set(da)
        ch = [k for k in set(da) & set(db) if not da[k].equals(db[k])]
        if oa or ob or ch:
            obst_diff += 1
            only_a += len(oa)
            only_b += len(ob)
            changed += len(ch)
        else:
            unexplained.append(oid)

    print(f"  Quellgeometrie verschieden (PBF-Drift, Quelle):       {src_diff}")
    print(f"  Quelle gleich, Gebaeudebestand in Reichweite anders:  {obst_diff}")
    print(f"     Gebaeude nur in A: {only_a}, nur in B: {only_b}, Geometrie geaendert: {changed}")
    print(f"  UNERKLAERT (gleiche Quelle, gleiche Gebaeude):         {len(unexplained)}")
    if unexplained:
        print(f"     {unexplained[:20]}")
        print("  NICHT bestanden -- Hindernis-Auswahl oder Grenzfilter pruefen.")
    else:
        print("  Bestanden: jede Abweichung ist PBF-Drift.")


if __name__ == "__main__":
    main()
