"""Jede oeffentliche Seite traegt den vollen Open-Graph-Satz, mit absoluten URLs.

Teiler (Messenger, Mastodon, LinkedIn) loesen nichts relativ auf: `og:url`,
`og:image` und `canonical` muessen absolute URLs sein. Die Seiten kennen die
Domain nicht (nur deploy/site.env), deshalb steht `__SITE_URL__` davor und der
Deploy ersetzt es -- in jeder Seite, mit Abbruch bei einem Rest. Hier wird der
Quellstand geprueft: Satz vollstaendig, Platzhalter an jeder URL, Bildmasse
wie im Kopf angegeben, Bildbeschreibung auf beiden Seiten gleich.

Titel und Beschreibung dagegen gehoeren je Seite sich selbst: bis November 2026
kopierte das Bauskript die Beschreibung der Karte in die Ueber-Seite, 182
Zeichen auf beiden. Eine Suchmaschine schneidet bei rund 155 ab und zeigt fuer
zwei Seiten denselben Text. Geprueft wird deshalb auch: je Seite ein eigener
Titel und eine eigene Beschreibung, beide kurz genug, und die Karte fuers
Teilen (`og:title`, `og:description`) sagt dasselbe wie der Kopf.
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


# Was ein Suchergebnis ungekuerzt zeigt. Keine Normen, sondern die ueblichen Grenzen:
# Titel werden nach Breite gekuerzt (rund 60 Zeichen), Beschreibungen bei rund 155.
MAX_TITLE = 60
MAX_DESCRIPTION = 155


def title(h: str) -> str:
    m = re.search(r"<title>(.*?)</title>", h, re.S)
    assert m, "title fehlt"
    return m.group(1).strip()


def description(h: str) -> str:
    m = re.search(r'<meta name="description" content="([^"]*)"', h)
    assert m, "description fehlt"
    return m.group(1)


def test_title_and_description_per_page_own_and_short() -> None:
    titles, descriptions = {}, {}
    for page in PAGES:
        h = head(page)
        titles[page] = title(h)
        descriptions[page] = description(h)
        assert prop(h, "og:title") == titles[page], page
        assert prop(h, "og:description") == descriptions[page], page
        assert len(titles[page]) <= MAX_TITLE, f"{page}: Titel {len(titles[page])} Zeichen"
        assert len(descriptions[page]) <= MAX_DESCRIPTION, (
            f"{page}: Beschreibung {len(descriptions[page])} Zeichen"
        )
    assert len(set(titles.values())) == len(PAGES), titles
    assert len(set(descriptions.values())) == len(PAGES), descriptions


def test_website_block_als_json_ld() -> None:
    """Der WebSite-Block der Kartenseite ist gueltiges JSON und nennt Namen und Adresse.

    Ein Datenblock, kein Skript: die CSP fuehrt ihn nicht aus. Die Adressen tragen
    den Platzhalter wie canonical und og:url; der Deploy ersetzt ihn in jeder Seite.
    """
    import json
    import re

    h = (ROOT / "viewer" / "index.html").read_text(encoding="utf-8")
    bloecke = re.findall(r'<script type="application/ld\+json">\s*(.*?)\s*</script>', h, re.S)
    assert len(bloecke) == 1
    d = json.loads(bloecke[0])
    assert d["@type"] == "WebSite" and d["name"] == "BubatzBlick"
    assert d["alternateName"] == "Bubatzkarte 2.0"
    assert d["url"] == "__SITE_URL__/" and d["@id"] == "__SITE_URL__/#website"
