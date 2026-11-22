"""Was der Viewer von sich aus an Server schickt (viewer/about.md, "Datenschutz").

Die Ueber-Seite sagt: der Kartenausschnitt steht in der Adresszeile nach dem `#`
und wird nur uebertragen, wenn man die Adresse selbst weitergibt. Das Fragment
schickt der Browser nicht mit; eine Query schon, und dann steht sie im
Zugriffsprotokoll (als Pfad und als Referer der Folgeanfragen). Der Link zur
Ueber-Seite trug den Ausschnitt einmal als `?karte=`, nach "Mein Standort" also
die eigene Position. Diese Tests halten den Weg im Fragment fest.
"""

from __future__ import annotations

import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
VIEWER = ROOT / "viewer"


def test_ausschnitt_nie_in_der_query() -> None:
    for name in ("app.js", "about.js"):
        js = (VIEWER / name).read_text(encoding="utf-8")
        assert "?karte" not in js, name
        assert "location.search).get(" not in js, name


def test_link_zur_ueberseite_traegt_den_ausschnitt_im_fragment() -> None:
    js = (VIEWER / "app.js").read_text(encoding="utf-8")
    assert '"about.html" + (location.hash ? "#" + new URLSearchParams({ karte:' in js


def test_ueberseite_liest_den_ausschnitt_aus_dem_fragment() -> None:
    erwartet = 'new URLSearchParams(location.hash.slice(1)).get("karte")'
    assert erwartet in (VIEWER / "about.js").read_text(encoding="utf-8")
    # about.js wird erzeugt; die Vorlage muss dasselbe sagen.
    assert erwartet in (ROOT / "deploy" / "bin" / "build_about.py").read_text(encoding="utf-8")
