"""Jede Symbolart der Grundkarte findet ein Bild im mitgelieferten Sprite.

Der Stil @protomaps/basemaps (viewer/vendor/basemaps/basemaps.js) nimmt fuer
die POI-Ebene das Symbol nach `kind`, mit Umbenennungen in einem `match`.
Fehlt ein Bild im Sprite, zeichnet MapLibre nichts und warnt bei jedem Laden
in der Konsole -- so bei "townhall", das basemaps-assets nie ausgeliefert hat.
viewer/app.js ergaenzt dafuer Umbenennungen; dieser Test haelt Stil, app.js und
Sprite zusammen, auch nach einem Update des Stils oder des Sprites.
"""

from __future__ import annotations

import json
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]
STIL = ROOT / "viewer/vendor/basemaps/basemaps.js"
SPRITE = ROOT / "viewer/vendor/basemaps/sprites/v4"
APP = ROOT / "viewer/app.js"


def _poi_arten(js: str) -> list[str]:
    m = re.search(
        r'"source-layer":"pois",filter:\["all",\["in",\["get","kind"\],\["literal",\[(.*?)\]\]\]',
        js,
    )
    assert m, "Filter der POI-Ebene im Stil nicht gefunden"
    return re.findall(r'"([a-z_]+)"', m.group(1))


def _umbenennungen_stil(js: str) -> dict[str, str]:
    m = re.search(r'"icon-image":\["match",\["get","kind"\],(.*?),\["get","kind"\]\]', js)
    assert m, "icon-image der POI-Ebene im Stil nicht gefunden"
    paare = re.findall(r'"([a-z_]+)","([a-z_]+)"', m.group(1))
    return dict(paare)


def _umbenennungen_app(js: str) -> dict[str, str]:
    m = re.search(r"bild\.slice\(0, -1\), ((?:\"[a-z_]+\", \"[a-z_]+\", )+)bild\[", js)
    assert m, "Ergaenzung der POI-Symbole in viewer/app.js nicht gefunden"
    return dict(re.findall(r'"([a-z_]+)", "([a-z_]+)"', m.group(1)))


def test_jede_poi_art_hat_ein_bild() -> None:
    stil = STIL.read_text(encoding="utf-8")
    umbenannt = _umbenennungen_stil(stil) | _umbenennungen_app(APP.read_text(encoding="utf-8"))
    for datei in ("light.json", "light@2x.json"):
        bilder = set(json.loads((SPRITE / datei).read_text(encoding="utf-8")))
        fehlt = [k for k in _poi_arten(stil) if umbenannt.get(k, k) not in bilder]
        assert not fehlt, f"{datei}: kein Bild fuer {fehlt}"


def test_ergaenzung_nur_wo_noetig() -> None:
    """app.js benennt nur um, was der Stil nicht selbst abdeckt; das Ziel gibt es."""
    stil = STIL.read_text(encoding="utf-8")
    bilder = set(json.loads((SPRITE / "light.json").read_text(encoding="utf-8")))
    for art, ziel in _umbenennungen_app(APP.read_text(encoding="utf-8")).items():
        assert art not in bilder, f"{art} hat ein eigenes Bild, die Umbenennung kann weg"
        assert art not in _umbenennungen_stil(stil)
        assert ziel in bilder
