#!/usr/bin/env python3
"""Amtlich belegte Fussgaengerzonen, die OpenStreetMap nicht als highway=pedestrian fuehrt.

Liest `config/fussgaengerzonen.toml` (je Stadt eine amtliche Quelle: Satzung,
Strassenverzeichnis, Widmung, Teileinziehung, mit den dort genannten Strassen und
Abschnitten), holt diese Strassen aus OpenStreetMap (Overpass, `out geom`) und schreibt
nur die Abschnitte, die OSM NICHT schon als `highway=pedestrian` fuehrt, als
GeoJSONSeq nach `config/fussgaengerzonen.geojsonl`. `bubatz pedestrian
--extra-geojson` haengt die Datei an die Fussgaengerzonen aus OSM an, sie landet also
in denselben Kacheln (`fussgaengerzonen.pmtiles`).

Die Datei ist eingecheckt; dieses Skript ist ihre Herkunft und laeuft von Hand, wenn
eine Quelle dazukommt oder OSM nachgezogen hat (dann faellt der Abschnitt hier weg,
weil er in OSM steht).

    uv run python scripts/fussgaengerzonen.py            # schreibt die GeoJSONSeq
    uv run python scripts/fussgaengerzonen.py --check    # nur pruefen, ob sie zur TOML passt

Wege bleiben Linien wie in `bubatz pedestrian` (kein Puffer), Plaetze mit `area=yes`
oder `place=square` werden Flaechen. Texte im Popup stammen aus der TOML, nicht aus OSM.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from pathlib import Path
from typing import Any

import shapely
from pyproj import Geod
from shapely.geometry import LineString, Polygon, mapping, shape

sys.path.insert(0, str(Path(__file__).resolve().parent))
import sonderzonen as sz  # Overpass-Abfrage und Filter teilen

ROOT = Path(__file__).resolve().parents[1]
QUELLE = ROOT / "config" / "fussgaengerzonen.toml"
ZIEL = ROOT / "config" / "fussgaengerzonen.geojsonl"
PFLICHT = ("id", "ort", "name", "beleg", "quelle", "bbox", "teil")
GEOD = Geod(ellps="WGS84")


def geometrie(el: dict, ausschnitt: Any) -> Any:
    """Way als Linie (oder Platz als Flaeche), in WGS84, auf den Ausschnitt zugeschnitten."""
    pts = [(c["lon"], c["lat"]) for c in el["geometry"]]
    if len(pts) < 2:
        return None
    tags = el.get("tags", {})
    geschlossen = pts[0] == pts[-1] and len(pts) >= 4
    if geschlossen and (tags.get("area") == "yes" or tags.get("place") == "square"):
        g: Any = Polygon(pts)
    else:
        g = LineString(pts)
    if ausschnitt is not None:
        g = g.intersection(ausschnitt)
    return None if g.is_empty else g


def zone_bauen(zone: dict, zeige: bool = False) -> tuple[list[dict], int]:
    for k in PFLICHT:
        if k not in zone:
            raise SystemExit(f"{zone.get('id', '?')}: {k} fehlt")
    abfrage_zone = {"bbox": zone["bbox"], "teil": [{"typ": ["way"], **t} for t in zone["teil"]]}
    antwort = sz.overpass(sz.abfrage(abfrage_zone))
    merkmale: dict[int, dict] = {}
    schon_in_osm: set[int] = set()
    for t in abfrage_zone["teil"]:
        ausschnitt = None
        if "ausschnitt" in t:
            s, w, n, o = t["ausschnitt"]
            ausschnitt = shapely.box(w, s, o, n)
        treffer = 0
        for el in antwort["elements"]:
            if el["type"] != "way" or not sz.passt(t, el) or "geometry" not in el:
                continue
            tags = el.get("tags", {})
            if "highway" not in tags and tags.get("place") != "square":
                continue  # Gebaeude, Grenzen und Aehnliches mit demselben Namen
            treffer += 1
            if tags.get("highway") == "pedestrian":
                schon_in_osm.add(el["id"])
                continue
            g = geometrie(el, ausschnitt)
            if g is None or el["id"] in merkmale:
                continue
            if zeige:
                a, b = el["geometry"][0], el["geometry"][-1]
                print(
                    f"  + way/{el['id']} {tags.get('name', '?')} [{tags.get('highway', '-')}] "
                    f"({a['lon']:.5f},{a['lat']:.5f})->({b['lon']:.5f},{b['lat']:.5f}) "
                    f"{GEOD.geometry_length(g):.0f} m"
                )
            geom = json.loads(json.dumps(mapping(g)), parse_float=lambda x: round(float(x), 6))
            merkmale[el["id"]] = {
                "type": "Feature",
                "properties": {
                    "amtlich": True,
                    "zone": zone["id"],
                    "ort": zone["ort"],
                    "name": zone["name"],
                    "beleg": zone["beleg"],
                    "quelle": zone["quelle"],
                    "osm_type": "way",
                    "osm_id": el["id"],
                },
                "geometry": geom,
            }
        if treffer == 0 and not t.get("optional", False):
            raise SystemExit(f"{zone['id']}: kein OSM-Weg fuer {t}")
    return [merkmale[k] for k in sorted(merkmale)], len(schon_in_osm)


def main() -> int:
    p = argparse.ArgumentParser(description=(__doc__ or "").split("\n", 1)[0])
    p.add_argument("--check", action="store_true", help="nur pruefen, nichts schreiben")
    p.add_argument("--nur", nargs="*", help="nur diese Zonen-IDs rechnen und ausgeben (Probe)")
    p.add_argument("--toml", type=Path, default=QUELLE, help="andere TOML (nur mit --nur)")
    args = p.parse_args()
    if args.toml != QUELLE and not args.nur:
        raise SystemExit("--toml nur zusammen mit --nur (Probe), geschrieben wird aus config/")
    with args.toml.open("rb") as f:
        zonen = tomllib.load(f).get("zone", [])
    ids = [z["id"] for z in zonen]
    if len(ids) != len(set(ids)):
        raise SystemExit("doppelte Zonen-IDs in fussgaengerzonen.toml")
    if args.nur:
        zonen = [z for z in zonen if z["id"] in set(args.nur)]
    alle: list[dict] = []
    for z in zonen:
        merkmale, osm = zone_bauen(z, zeige=bool(args.nur))
        laenge = sum(GEOD.geometry_length(shape(m["geometry"])) for m in merkmale)
        print(
            f"{z['id']}: {len(merkmale)} Wege ergaenzt ({laenge:.0f} m), "
            f"{osm} schon in OSM als Fussgaengerzone"
        )
        alle.extend(merkmale)
    zeilen = [json.dumps(m, ensure_ascii=False, separators=(",", ":")) for m in alle]
    text = "".join(z + "\n" for z in zeilen)
    if args.nur:
        return 0  # Probe: die Liste oben reicht
    if args.check:
        alt = ZIEL.read_text(encoding="utf-8") if ZIEL.exists() else ""
        gleich = alt == text
        print("Datei passt zur TOML" if gleich else "Datei weicht ab: neu erzeugen")
        return 0 if gleich else 1
    ZIEL.write_text(text, encoding="utf-8")
    print(f"{ZIEL.relative_to(ROOT)}: {len(alle)} Wege aus {len(zonen)} Quellen, {len(text)} Bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
