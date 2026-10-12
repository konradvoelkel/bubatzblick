"""Jede oeffentliche Seite traegt den vollen Open-Graph-Satz, mit absoluten URLs.

Teiler (Messenger, Mastodon, LinkedIn) loesen nichts relativ auf: `og:url`,
`og:image` und `canonical` muessen absolute URLs sein. Die Seiten kennen die
Domain nicht (nur deploy/site.env), deshalb steht `__SITE_URL__` davor und der
Deploy ersetzt es -- in jeder Seite, mit Abbruch bei einem Rest. Hier wird der
Quellstand geprueft: Satz vollstaendig, Platzhalter an jeder URL, Bildmasse
wie im Kopf angegeben, Bildbeschreibung auf beiden Seiten gleich.
"""

from __future__ import annotations

import pathlib
import re
import struct

import pytest

ROOT = pathlib.Path(__file__).resolve().parents[1]

# Seite -> ihre kanonische URL (ueber den Platzhalter).
PAGES = {
    "viewer/index.html": "__SITE_URL__/",
    "viewer/about.html": "__SITE_URL__/about.html",
}

PROPERTIES = [
    "og:type",
    "og:site_name",
    "og:locale",
    "og:url",
    "og:title",
    "og:description",
    "og:image",
    "og:image:type",
    "og:image:width",
    "og:image:height",
    "og:image:alt",
]


def head(page: str) -> str:
    text = (ROOT / page).read_text(encoding="utf-8")
    return text.split("</head>", 1)[0]


def prop(h: str, name: str) -> str:
    m = re.search(rf'<meta property="{re.escape(name)}" content="([^"]*)"', h)
    assert m, f"{name} fehlt"
    return m.group(1)


@pytest.mark.parametrize(("page", "url"), sorted(PAGES.items()))
def test_open_graph_complete(page: str, url: str) -> None:
    h = head(page)
    for name in PROPERTIES:
        assert f'property="{name}"' in h, f"{page}: {name} fehlt"
    assert '<meta name="twitter:card" content="summary_large_image">' in h
    assert f'<link rel="canonical" href="{url}">' in h
    assert prop(h, "og:url") == url
    assert prop(h, "og:locale") == "de_DE"
    assert prop(h, "og:image") == "__SITE_URL__/img/og.png"
    assert prop(h, "og:image:type") == "image/png"
    assert prop(h, "og:image:alt").strip()
    # Alle Karten-URLs absolut ueber den Platzhalter, keine relative darunter.
    for m in re.finditer(
        r'(?:property="og:(?:url|image)" content|rel="canonical" href)="([^"]+)"', h
    ):
        assert m.group(1).startswith("__SITE_URL__/"), m.group(0)


def test_card_alt_identical_on_both_pages() -> None:
    alts = {page: prop(head(page), "og:image:alt") for page in PAGES}
    assert len(set(alts.values())) == 1, alts


def test_card_size_matches_head() -> None:
    png = (ROOT / "viewer/img/og.png").read_bytes()
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    width, height = struct.unpack(">II", png[16:24])
    for page in PAGES:
        h = head(page)
        assert (prop(h, "og:image:width"), prop(h, "og:image:height")) == (str(width), str(height))
    assert (width, height) == (1200, 630)
