"""Die Sitemap nennt genau die oeffentlichen Seiten, und robots.txt zeigt auf sie.

`viewer/sitemap.xml` ist von Hand geschrieben. Ohne Pruefung veraltet sie beim
ersten Mal, wenn eine dritte Seite dazukommt oder eine umzieht -- und eine
Suchmaschine, der man die Sitemap eingereicht hat, haelt sie fuer die Liste.
Geprueft wird deshalb der Quellstand:

- Jede `viewer/*.html` steht mit ihrer kanonischen Adresse in der Sitemap, und
  die Sitemap nennt nichts darueber hinaus.
- robots.txt nennt die Sitemap genau einmal, unter derselben Herkunft.
- Keine Adresse der Sitemap faellt unter ein `Disallow` von robots.txt.
- Die Herkunft ist dieselbe wie in `.well-known/security.txt`: die Domain steht
  an diesen drei Stellen fest, weil der Deploy den Platzhalter fuer die
  Seitenadresse nur in `*.html` ersetzt.
"""

from __future__ import annotations

import pathlib
import re
import xml.etree.ElementTree as ET
from urllib.parse import urlsplit

ROOT = pathlib.Path(__file__).resolve().parents[1]
VIEWER = ROOT / "viewer"
PLATZHALTER = "__SITE_URL__"
NS = {"sm": "http://www.sitemaps.org/schemas/sitemap/0.9"}


def sitemap_adressen() -> list[str]:
    wurzel = ET.parse(VIEWER / "sitemap.xml").getroot()
    assert wurzel.tag == f"{{{NS['sm']}}}urlset", wurzel.tag
    adressen = [(loc.text or "").strip() for loc in wurzel.findall("sm:url/sm:loc", NS)]
    assert adressen, "Sitemap ohne Eintrag"
    return adressen


def herkunft(adressen: list[str]) -> str:
    herkuenfte = {f"{u.scheme}://{u.netloc}" for u in map(urlsplit, adressen)}
    assert len(herkuenfte) == 1, f"mehrere Herkuenfte in der Sitemap: {sorted(herkuenfte)}"
    (eine,) = herkuenfte
    assert eine.startswith("https://"), eine
    return eine


def robots_zeilen(feld: str) -> list[str]:
    werte = []
    for zeile in (VIEWER / "robots.txt").read_text(encoding="utf-8").splitlines():
        name, trenner, wert = zeile.partition(":")
        if trenner and name.strip().lower() == feld:
            werte.append(wert.strip())
    return werte


def test_sitemap_nennt_genau_die_seiten() -> None:
    adressen = sitemap_adressen()
    assert len(adressen) == len(set(adressen)), "doppelter Eintrag in der Sitemap"
    basis = herkunft(adressen)
    erwartet = set()
    for seite in sorted(VIEWER.glob("*.html")):
        text = seite.read_text(encoding="utf-8")
        m = re.search(r'<link rel="canonical" href="([^"]+)"', text)
        assert m, f"{seite.name}: kein canonical"
        assert m.group(1).startswith(PLATZHALTER + "/"), m.group(1)
        erwartet.add(basis + m.group(1)[len(PLATZHALTER) :])
    assert set(adressen) == erwartet


def test_robots_zeigt_auf_die_sitemap() -> None:
    basis = herkunft(sitemap_adressen())
    assert robots_zeilen("sitemap") == [f"{basis}/sitemap.xml"]


def test_sitemap_nennt_nichts_gesperrtes() -> None:
    gesperrt = [p for p in robots_zeilen("disallow") if p]
    for adresse in sitemap_adressen():
        pfad = urlsplit(adresse).path
        assert not any(pfad.startswith(p) for p in gesperrt), adresse


def test_herkunft_wie_in_security_txt() -> None:
    text = (VIEWER / ".well-known" / "security.txt").read_text(encoding="utf-8")
    m = re.search(r"^Canonical:\s*(\S+)$", text, re.M)
    assert m, "security.txt ohne Canonical"
    u = urlsplit(m.group(1))
    assert f"{u.scheme}://{u.netloc}" == herkunft(sitemap_adressen())
