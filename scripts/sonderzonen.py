#!/usr/bin/env python3
"""Oertliche Konsumverbote ausserhalb von Paragraf 5 KCanG als Kartenebene.

Liest `config/sonderzonen.toml` (Rechtsakte mit Flaeche und Laufzeit), holt die dort
genannten Strassen und Plaetze aus OpenStreetMap (Overpass, `out geom`), puffert Wege
auf ihre Breite, vereinigt alles je Zone und schreibt `viewer/sonderzonen.json`
(GeoJSON, WGS84). Die Datei ist eingecheckt; dieses Skript ist ihre Herkunft und
laeuft von Hand, wenn ein Rechtsakt dazukommt, sich aendert oder auslaeuft.

    uv run python scripts/sonderzonen.py            # schreibt viewer/sonderzonen.json
    uv run python scripts/sonderzonen.py --check    # nur pruefen, ob die Datei zur TOML passt

Die Grenze ist eine Naeherung: Verordnungen nennen Strassen und verweisen auf einen
Lageplan; die Karte der Stadt bleibt massgeblich und steht im Popup. Gerechnet wird in
EPSG:25832 (Meter), Wege nach Klasse gepuffert (siehe BREITE_M).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
import time
import tomllib
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

import shapely
from pyproj import Transformer
from shapely.geometry import LineString, MultiPolygon, Polygon, mapping
from shapely.geometry.base import BaseGeometry
from shapely.ops import polygonize, transform, unary_union

ROOT = Path(__file__).resolve().parents[1]
QUELLE = ROOT / "config" / "sonderzonen.toml"
ZIEL = ROOT / "viewer" / "sonderzonen.json"
OVERPASS = "https://overpass-api.de/api/interpreter"
UA = "bubatzblick sonderzonen (https://bubatzblick.de, kontakt@bubatzblick.de)"

# Halbe Breite je Wegklasse in Metern (Fahrbahn plus Gehwege). Grob, aber die
# Verordnungen meinen die ganze Strasse "mit Gehwegen beidseitig".
BREITE_M = {
    "secondary": 14.0,
    "secondary_link": 10.0,
    "tertiary": 12.0,
    "residential": 9.0,
    "living_street": 8.0,
    "pedestrian": 8.0,
    "service": 6.0,
    "footway": 4.0,
    "path": 4.0,
    "cycleway": 4.0,
    "steps": 3.0,
}
BREITE_SONST_M = 6.0
FLAECHEN_RAND_M = 3.0  # Plaetze und Parks: Gehweg am Rand

WGS84_ZU_UTM = Transformer.from_crs("EPSG:4326", "EPSG:25832", always_xy=True)
UTM_ZU_WGS84 = Transformer.from_crs("EPSG:25832", "EPSG:4326", always_xy=True)


def overpass(query: str, versuche: int = 4) -> dict:
    """Eine Overpass-Abfrage mit Wiederholung: der Dienst antwortet bei Last mit
    HTML statt JSON, und das ist kein Fehler der Abfrage."""
    daten = urllib.parse.urlencode({"data": query}).encode()
    for i in range(versuche):
        req = urllib.request.Request(OVERPASS, data=daten, headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                text = r.read().decode("utf-8")
            return json.loads(text)
        except (json.JSONDecodeError, OSError) as e:
            if i == versuche - 1:
                raise SystemExit(f"Overpass antwortet nicht brauchbar: {e}") from e
            time.sleep(10 * (i + 1))
    raise AssertionError


def abfrage(zone: dict) -> str:
    s, w, n, o = zone["bbox"]
    bbox = f"({s},{w},{n},{o})"
    teile: list[str] = []
    for t in zone["teil"]:
        typen = t.get("typ", ["way", "relation"])
        if "ids" in t:
            ids = ",".join(str(i) for i in t["ids"])
            teile.extend(f"{typ}(id:{ids});" for typ in typen)
        elif "tags" in t:
            k, v = t["tags"].split("=", 1)
            teile.extend(f'{typ}["{k}"="{v}"]{bbox};' for typ in typen)
        else:
            teile.extend(f'{typ}["name"~"{t["name"]}"]{bbox};' for typ in typen)
    return f"[out:json][timeout:120];({''.join(teile)});out geom;"


def passt(t: dict, el: dict) -> bool:
    """Mittelpunktfilter (lon_min/lon_max/lat_min/lat_max) und Ausschlussliste."""
    if el["id"] in set(t.get("ohne_ids", [])):
        return False
    if "ids" in t:
        return el["id"] in set(t["ids"])
    if "tags" in t:
        k, v = t["tags"].split("=", 1)
        if el.get("tags", {}).get(k) != v:
            return False
    elif not re.search(t["name"], el.get("tags", {}).get("name", "")):
        return False
    c = mittelpunkt(el)
    if c is None:
        return False
    lon, lat = c
    for schluessel, op in (
        ("lon_min", lon.__ge__),
        ("lon_max", lon.__le__),
        ("lat_min", lat.__ge__),
        ("lat_max", lat.__le__),
    ):
        if schluessel in t and not op(t[schluessel]):
            return False
    return True


def mittelpunkt(el: dict) -> tuple[float, float] | None:
    if "bounds" in el:
        b = el["bounds"]
        return ((b["minlon"] + b["maxlon"]) / 2, (b["minlat"] + b["maxlat"]) / 2)
    if "lon" in el:
        return (el["lon"], el["lat"])
    return None


def ist_flaeche(tags: dict, geschlossen: bool) -> bool:
    if not geschlossen:
        return False
    if tags.get("area") == "yes":
        return True
    return any(k in tags for k in ("leisure", "place", "amenity", "landuse", "natural", "building"))


def utm(coords: list[dict]) -> list[tuple[float, float]]:
    return [WGS84_ZU_UTM.transform(c["lon"], c["lat"]) for c in coords]


def geometrie(el: dict, puffer: float | None) -> BaseGeometry | None:
    tags = el.get("tags", {})
    if el["type"] == "way":
        pts = utm(el["geometry"])
        if len(pts) < 2:
            return None
        geschlossen = pts[0] == pts[-1] and len(pts) >= 4
        if ist_flaeche(tags, geschlossen):
            return Polygon(pts).buffer(puffer if puffer is not None else FLAECHEN_RAND_M)
        breite = (
            puffer if puffer is not None else BREITE_M.get(tags.get("highway", ""), BREITE_SONST_M)
        )
        return LineString(pts).buffer(breite, cap_style="flat")
    if el["type"] == "relation":
        linien = []
        for m in el.get("members", []):
            if m["type"] == "way" and "geometry" in m:
                pts = utm(m["geometry"])
                if len(pts) >= 2:
                    linien.append(LineString(pts))
        if not linien:
            return None
        flaechen = list(polygonize(unary_union(linien)))
        if flaechen:
            return unary_union(flaechen).buffer(puffer if puffer is not None else FLAECHEN_RAND_M)
        breite = puffer if puffer is not None else BREITE_SONST_M
        return unary_union(linien).buffer(breite, cap_style="flat")
    if el["type"] == "node":
        x, y = WGS84_ZU_UTM.transform(el["lon"], el["lat"])
        return shapely.Point(x, y).buffer(puffer if puffer is not None else 15.0)
    return None


def zone_bauen(zone: dict) -> tuple[BaseGeometry, list[str]]:
    antwort = overpass(abfrage(zone))
    teile: list[BaseGeometry] = []
    herkunft: list[str] = []
    for t in zone["teil"]:
        puffer = t.get("puffer_m")
        n = 0
        for el in antwort["elements"]:
            if not passt(t, el):
                continue
            g = geometrie(el, puffer)
            if g is None or g.is_empty:
                continue
            teile.append(g)
            herkunft.append(f"{el['type']}/{el['id']}")
            n += 1
        if n == 0 and not t.get("optional", False):
            raise SystemExit(f"{zone['id']}: kein OSM-Objekt fuer {t}")
    g = unary_union(teile)
    # Spalten zwischen Nachbarwegen schliessen, Ecken glaetten, dann leicht vereinfachen.
    g = g.buffer(4.0).buffer(-4.0).simplify(0.5)
    # Loecher sind Artefakte der Vereinigung (ein Block zwischen zwei Wegen, ein Beet im
    # Park); ein Rechtsakt nennt Ausnahmen ausdruecklich, sonst gilt die ganze Flaeche.
    g = ohne_loecher(g)
    return g, sorted(set(herkunft))


def _wgs84(x: Any, y: Any, z: Any = None) -> tuple[Any, Any]:
    return UTM_ZU_WGS84.transform(x, y)


def ohne_loecher(g: BaseGeometry) -> BaseGeometry:
    polys = list(g.geoms) if isinstance(g, MultiPolygon) else [g]
    return unary_union([Polygon(p.exterior) for p in polys if isinstance(p, Polygon)])


def nach_wgs84(g: BaseGeometry) -> BaseGeometry:
    return transform(_wgs84, g)


def merkmale(zonen: list[dict[str, Any]]) -> list[dict[str, Any]]:
    out = []
    for z in zonen:
        g, herkunft = zone_bauen(z)
        props = {
            "id": z["id"],
            "name": z["name"],
            "ort": z["ort"],
            "traeger": z["traeger"],
            "rechtsgrundlage": z["rechtsgrundlage"],
            "verbot": z["verbot"],
            "zeit": z["zeit"],
            "gueltig_ab": z["gueltig_ab"].isoformat(),
            "gueltig_bis": z["gueltig_bis"].isoformat() if "gueltig_bis" in z else None,
            "quelle": z["quelle"],
            "karte": z.get("karte"),
            "hinweis": z.get("hinweis", ""),
            "flaeche_m2": round(g.area),
            "osm": herkunft,
        }
        wgs = nach_wgs84(g)
        geom = json.loads(json.dumps(mapping(wgs)))
        # 6 Nachkommastellen (~10 cm) reichen; kleinere Datei.
        geom = json.loads(json.dumps(geom), parse_float=lambda s: round(float(s), 6))
        out.append({"type": "Feature", "id": z["id"], "properties": props, "geometry": geom})
        print(f"{z['id']}: {round(g.area / 10000, 2)} ha aus {len(herkunft)} OSM-Objekten")
    return out


def main() -> int:
    p = argparse.ArgumentParser(description=(__doc__ or "").split("\n", 1)[0])
    p.add_argument("--check", action="store_true", help="nur pruefen, nichts schreiben")
    args = p.parse_args()
    with QUELLE.open("rb") as f:
        konfig = tomllib.load(f)
    zonen = konfig["zone"]
    ids = [z["id"] for z in zonen]
    if len(ids) != len(set(ids)):
        raise SystemExit("doppelte Zonen-IDs in sonderzonen.toml")
    fc: dict[str, Any] = {
        "type": "FeatureCollection",
        "name": "sonderzonen",
        "stand": dt.date.today().isoformat(),
        "beschreibung": "Oertliche Cannabis-Konsumverbote ausserhalb von Paragraf 5 KCanG; "
        "Grenzen naeherungsweise aus OpenStreetMap nach den Rechtsakten. "
        "Quelle: config/sonderzonen.toml, erzeugt von scripts/sonderzonen.py.",
        "features": merkmale(zonen),
    }
    text = json.dumps(fc, ensure_ascii=False, separators=(",", ":")) + "\n"
    if args.check:
        alt = json.loads(ZIEL.read_text(encoding="utf-8")) if ZIEL.exists() else {}
        gleich = {f["id"]: f["properties"] for f in alt.get("features", [])} == {
            f["id"]: f["properties"] for f in fc["features"]
        }
        print("Eigenschaften gleich" if gleich else "Eigenschaften weichen ab")
        return 0 if gleich else 1
    ZIEL.write_text(text, encoding="utf-8")
    print(f"{ZIEL.relative_to(ROOT)}: {len(fc['features'])} Zonen, {len(text)} Bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
