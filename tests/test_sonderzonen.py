"""Die lila Ebene: `viewer/sonderzonen.json` muss zu `config/sonderzonen.toml` passen.

Die Datei wird von Hand erzeugt (scripts/sonderzonen.py, Overpass) und eingecheckt;
die CI kann sie nicht neu bauen. Geprueft wird deshalb das Ergebnis: gueltige
Polygone in Deutschland, alle Pflichtfelder, ISO-Daten in richtiger Reihenfolge,
https-Quellen, dieselben Zonen wie in der TOML -- und dass der Viewer die Datei
unter genau diesem Namen holt.
"""

from __future__ import annotations

import datetime as dt
import json
import pathlib
import re
import tomllib

import pytest
from shapely.geometry import shape

ROOT = pathlib.Path(__file__).resolve().parents[1]
GEOJSON = ROOT / "viewer" / "sonderzonen.json"
TOML = ROOT / "config" / "sonderzonen.toml"
PFLICHT = (
    "id",
    "name",
    "ort",
    "traeger",
    "rechtsgrundlage",
    "verbot",
    "zeit",
    "gueltig_ab",
    "gueltig_bis",
    "quelle",
    "karte",
    "hinweis",
    "flaeche_m2",
    "osm",
)
DEUTSCHLAND = (5.8, 47.2, 15.1, 55.1)  # west, sued, ost, nord


def _fc() -> dict:
    return json.loads(GEOJSON.read_text(encoding="utf-8"))


def _toml() -> list[dict]:
    with TOML.open("rb") as f:
        return tomllib.load(f)["zone"]


def test_datei_ist_featurecollection_mit_stand() -> None:
    fc = _fc()
    assert fc["type"] == "FeatureCollection"
    dt.date.fromisoformat(fc["stand"])
    assert fc["features"], "keine Zonen"


@pytest.mark.parametrize("feature", _fc()["features"], ids=lambda f: f["id"])
def test_zone_ist_vollstaendig_und_gueltig(feature: dict) -> None:
    p = feature["properties"]
    for k in PFLICHT:
        assert k in p, f"{k} fehlt"
    assert feature["id"] == p["id"] and re.fullmatch(r"[a-z0-9-]+", p["id"])
    assert set(p["verbot"]) <= {"konsum", "mitfuehren"} and "konsum" in p["verbot"]
    ab = dt.date.fromisoformat(p["gueltig_ab"])
    if p["gueltig_bis"] is not None:
        assert dt.date.fromisoformat(p["gueltig_bis"]) >= ab
    for url in (p["quelle"], p["karte"]):
        assert url is None or url.startswith("https://"), url
    assert p["osm"] and all(re.fullmatch(r"(way|relation|node)/\d+", o) for o in p["osm"])
    g = shape(feature["geometry"])
    assert g.is_valid and g.geom_type in ("Polygon", "MultiPolygon")
    w, s, e, n = g.bounds
    assert DEUTSCHLAND[0] <= w and e <= DEUTSCHLAND[2]
    assert DEUTSCHLAND[1] <= s and n <= DEUTSCHLAND[3]
    # Eine Verbotszone einer Stadt ist Hektar bis wenige Dutzend Hektar gross, nie ein Stadtteil.
    assert 2_000 <= p["flaeche_m2"] <= 1_000_000


def test_geojson_und_toml_nennen_dieselben_zonen() -> None:
    ids_json = sorted(f["id"] for f in _fc()["features"])
    ids_toml = sorted(z["id"] for z in _toml())
    assert ids_json == ids_toml
    assert len(ids_toml) == len(set(ids_toml))


def test_toml_daten_stimmen_mit_geojson_ueberein() -> None:
    toml = {z["id"]: z for z in _toml()}
    for f in _fc()["features"]:
        z, p = toml[f["id"]], f["properties"]
        assert p["gueltig_ab"] == z["gueltig_ab"].isoformat()
        assert p["gueltig_bis"] == (z["gueltig_bis"].isoformat() if "gueltig_bis" in z else None)
        assert p["name"] == z["name"] and p["quelle"] == z["quelle"]


def test_viewer_holt_die_datei() -> None:
    js = (ROOT / "viewer" / "app.js").read_text(encoding="utf-8")
    assert '"sonderzonen.json"' in js
    assert 'getElementById("sonder-an")' in js
    html = (ROOT / "viewer" / "index.html").read_text(encoding="utf-8")
    assert 'id="sonder-an"' in html and "swatch-sonder" in html
