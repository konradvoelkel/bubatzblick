"""Der Viewer darf keine anderen Kategorien kennen als die Pipeline erzeugt.

`viewer/app.js` haelt die Namensliste fuer Legende und Popup fest (die Seite ist
statisch und liest die Tag-Datei nicht zur Laufzeit). Damit sie beim naechsten
`tags.v<N+1>.json` nicht stillschweigend auseinanderlaeuft, prueft dieser Test
Schluessel, Titel und Reihenfolge gegen die Quelle.
"""

from __future__ import annotations

import importlib.util
import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]


def _build_about():
    spec = importlib.util.spec_from_file_location(
        "build_about", ROOT / "deploy" / "bin" / "build_about.py"
    )
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _viewer_categories() -> list[tuple[str, str]]:
    js = (ROOT / "viewer" / "app.js").read_text(encoding="utf-8")
    block = re.search(r"const KATEGORIEN = \[(.*?)\n\];", js, re.S)
    assert block, "KATEGORIEN fehlt in viewer/app.js"
    paare = re.findall(r'\["([a-z_]+)", "((?:[^"\\]|\\.)*)"\]', block.group(1))
    # \uXXXX wie im JS-Quelltext aufloesen (Umlaute stehen dort escaped)
    return [(k, v.encode().decode("unicode_escape")) for k, v in paare]


def test_viewer_knows_exactly_the_pipeline_categories() -> None:
    _, spec = _build_about().load_spec()
    erwartet = sorted(
        ((name, c["title"], c.get("kcang_nr", 99)) for name, c in spec["categories"].items()),
        key=lambda t: t[2],
    )
    assert _viewer_categories() == [(n, t) for n, t, _ in erwartet]


def test_viewer_uses_the_pedestrian_layer_name_from_rust() -> None:
    """Die Ebene im Kachelarchiv heisst so, wie `bubatz pedestrian` sie nennt.

    tippecanoe schreibt den Namen aus `pedestrian::LAYER_NAME`; der Viewer
    spricht ihn als `source-layer` an. Laufen die auseinander, bleibt die
    Karte einfach leer -- ohne Fehlermeldung.
    """
    rs = (ROOT / "rust" / "bubatz" / "src" / "pedestrian.rs").read_text(encoding="utf-8")
    m = re.search(r'pub const LAYER_NAME: &str = "([^"]+)"', rs)
    assert m, "LAYER_NAME fehlt in pedestrian.rs"
    js = (ROOT / "viewer" / "app.js").read_text(encoding="utf-8")
    benutzt = set(re.findall(r'"source-layer": "([^"]+)"', js))
    assert m.group(1) in benutzt, (m.group(1), benutzt)
