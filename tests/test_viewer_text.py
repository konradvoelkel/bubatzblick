"""Sichtbarer Text der Karte traegt echte Umlaute.

Im Bedienfeld standen „Kinderspielplaetzen“, „oeffentlich zugaenglichen
Sportstaetten“ und „naeherungsweise“ -- wenige Zeilen ueber den korrekt
geschriebenen Kategorienamen aus `app.js`. Auf der
Startseite eines deutschsprachigen Angebots ist das der erste sichtbare
Qualitaetsmangel, und er faellt beim Schreiben nicht auf, weil die Kommentare
im Quelltext bewusst umschrieben sind.

Geprueft wird deshalb ausschliesslich der SICHTBARE Text: `<style>`, `<script>`
und HTML-Kommentare werden vorher entfernt. Die Umschreibung in Kommentaren und
Bezeichnern bleibt erlaubt.
"""

from __future__ import annotations

import pathlib
import re

ROOT = pathlib.Path(__file__).resolve().parents[1]

# Muster, die nur als fehlender Umlaut vorkommen koennen. Bewusst eng gefasst:
# „muss“, „dass“ oder „Adresse“ duerfen nicht anschlagen.
UMSCHRIEBEN = [
    r"\bfuer\b",
    r"\bueber\b",
    r"\boeffentlich",
    r"staetten\b",
    r"\bnaeherungs",
    r"tatsaechlich",
    r"unvollstaendig",
    r"Gebaeude",
    r"spielplaetze",
    r"zugaeng",
    r"\bgross\b",
    r"\bgroesse",
    r"\bkoennen\b",
    r"\bwaehrend\b",
    r"\bStrassen\b",
    r"verfuegbar",
    r"schliess",
    r"rueck",
    r"pruef",
    r"gewoehnlich",
    r"zurueck",
    r"moeglich",
    r"naechst",
    r"spaeter",
    r"fuehr",
    r"muess",
]


def _sichtbarer_text(html: str) -> str:
    ohne = re.sub(r"<!--.*?-->", " ", html, flags=re.S)
    ohne = re.sub(r"<style\b.*?</style>", " ", ohne, flags=re.S | re.I)
    ohne = re.sub(r"<script\b.*?</script>", " ", ohne, flags=re.S | re.I)
    # Attribute mitnehmen: placeholder, title und aria-label sind sichtbar bzw.
    # werden vorgelesen.
    sichtbar = re.findall(r'(?:placeholder|title|aria-label|alt)="([^"]*)"', ohne)
    ohne = re.sub(r"<[^>]+>", " ", ohne)
    return ohne + " " + " ".join(sichtbar)


def test_bedienfeld_schreibt_umlaute() -> None:
    text = _sichtbarer_text((ROOT / "viewer" / "index.html").read_text(encoding="utf-8"))
    treffer = [m for p in UMSCHRIEBEN for m in re.findall(p, text)]
    assert not treffer, f"umschriebene Umlaute im sichtbaren Text: {sorted(set(treffer))}"


def test_ueberseite_schreibt_umlaute() -> None:
    text = _sichtbarer_text((ROOT / "viewer" / "about.html").read_text(encoding="utf-8"))
    treffer = [m for p in UMSCHRIEBEN for m in re.findall(p, text)]
    assert not treffer, f"umschriebene Umlaute auf der Ueber-Seite: {sorted(set(treffer))}"


def _js_texte(js: str) -> list[str]:
    """Zeichenkettenliterale aus `app.js`, ohne Kommentare und ohne Bezeichner.

    Die Kommentare im Quelltext sind bewusst umschrieben (ASCII), die Texte
    fuer die Oberflaeche nicht. Reine Kleinschreib-Kennungen wie `fuss-an`
    oder `ueber-link` sind CSS-/DOM-Namen und kein Text.
    """
    ohne = re.sub(r"/\*.*?\*/", " ", js, flags=re.S)
    ohne = re.sub(r"^\s*//.*$", " ", ohne, flags=re.M)
    lits = re.findall(r'"((?:[^"\\]|\\.)*)"', ohne) + re.findall(r"`([^`]*)`", ohne)
    return [lit for lit in lits if not re.fullmatch(r"[a-z0-9-]+", lit)]


def test_meldungen_der_karte_schreiben_umlaute() -> None:
    """Auch die Texte, die erst zur Laufzeit erscheinen.

    Die MapLibre-Uebersetzung, die Standortfehler und der Ladehinweis stehen
    als Literale in `app.js` und tauchen in keinem HTML auf.
    """
    js = (ROOT / "viewer" / "app.js").read_text(encoding="utf-8")
    treffer = [m for t in _js_texte(js) for p in UMSCHRIEBEN for m in re.findall(p, t)]
    assert not treffer, f"umschriebene Umlaute in Oberflaechentexten: {sorted(set(treffer))}"
