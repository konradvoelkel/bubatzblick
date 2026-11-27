"""Amtlich belegte Fussgaengerzonen: `config/fussgaengerzonen.geojsonl` muss zu
`config/fussgaengerzonen.toml` passen.

Die Datei wird von Hand erzeugt (scripts/fussgaengerzonen.py, Overpass) und eingecheckt;
die CI kann sie nicht neu bauen. Geprueft wird deshalb das Ergebnis: jede Zeile ein
Feature mit `amtlich: true`, Texte wie in der TOML, https-Quelle, gueltige Linien oder
Flaechen in der bbox ihrer Quelle -- und dass Makefile und Viewer die Datei so benutzen.
"""

from __future__ import annotations

import json
import pathlib
import re
import tomllib

from shapely.geometry import box, shape

ROOT = pathlib.Path(__file__).resolve().parents[1]
GEOJSONL = ROOT / "config" / "fussgaengerzonen.geojsonl"
TOML = ROOT / "config" / "fussgaengerzonen.toml"
PFLICHT = ("id", "ort", "name", "beleg", "quelle", "bbox", "teil")


def _zonen() -> dict[str, dict]:
    with TOML.open("rb") as f:
        zonen = tomllib.load(f).get("zone", [])
    ids = [z["id"] for z in zonen]
    assert len(ids) == len(set(ids)), "doppelte ids"
    return {z["id"]: z for z in zonen}


def _merkmale() -> list[dict]:
    text = GEOJSONL.read_text(encoding="utf-8")
    return [json.loads(z) for z in text.splitlines() if z.strip()]


def test_toml_quellen_vollstaendig() -> None:
    for zid, z in _zonen().items():
        for k in PFLICHT:
            assert k in z, f"{zid}: {k} fehlt"
        assert re.fullmatch(r"[a-z0-9-]+", zid), zid
        assert z["quelle"].startswith("https://"), zid
        s, w, n, o = z["bbox"]
        assert 47.2 <= s < n <= 55.1 and 5.8 <= w < o <= 15.1, zid
        assert z["teil"], f"{zid}: keine Teile"


def test_ergaenzungen_passen_zur_toml() -> None:
    zonen = _zonen()
    gesehen: set[tuple[str, int]] = set()
    for m in _merkmale():
        p = m["properties"]
        assert m["type"] == "Feature" and p["amtlich"] is True
        z = zonen[p["zone"]]  # KeyError: Zeile ohne Quelle in der TOML
        for k in ("ort", "name", "beleg", "quelle"):
            assert p[k] == z[k], f"{p['zone']}: {k} weicht von der TOML ab, neu erzeugen"
        assert p["osm_type"] == "way" and isinstance(p["osm_id"], int)
        assert (p["zone"], p["osm_id"]) not in gesehen, f"doppelt: {p['zone']} way/{p['osm_id']}"
        gesehen.add((p["zone"], p["osm_id"]))
        g = shape(m["geometry"])
        assert g.is_valid and not g.is_empty
        assert g.geom_type in ("LineString", "MultiLineString", "Polygon", "MultiPolygon")
        s, w, n, o = z["bbox"]
        rahmen = box(w, s, o, n).buffer(0.01)
        assert rahmen.contains(g), f"{p['zone']}: way/{p['osm_id']} ausserhalb"


def test_makefile_und_viewer_benutzen_die_datei() -> None:
    mk = (ROOT / "Makefile").read_text(encoding="utf-8")
    assert "FUSSGAENGER_AMTLICH := config/fussgaengerzonen.geojsonl" in mk
    assert "--extra-geojson $(FUSSGAENGER_AMTLICH)" in mk
    js = (ROOT / "viewer" / "app.js").read_text(encoding="utf-8")
    assert "p.amtlich" in js, "Popup kennt die amtlichen Ergaenzungen nicht"
