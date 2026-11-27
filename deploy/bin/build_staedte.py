#!/usr/bin/env -S uv run --with markdown --script
"""Staedteseiten viewer/stadt/<slug>.html aus staedte/staedte.toml, staedte/<slug>.json und
staedte/<slug>.md (`make staedte`).

Der Fliesstext steht von Hand in staedte/<slug>.md; Zahlen stehen darin als {{schluessel}} und
kommen aus der JSON, damit ein neuer Lauf den Text nicht anfasst. Eine Zeile [[bild:kurz]]
wird zur Karte viewer/img/stadt/<slug>-<kurz>.webp mit Unterschrift. Ein unbekannter Schluessel
bricht den Bau ab, statt eine leere Stelle auszuliefern.

Die erzeugten Seiten werden committet wie about.html; die CI prueft, dass ein Neubau kein Diff
ergibt.
"""

import datetime as dt
import functools
import html
import json
import math
import pathlib
import re
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from build_about import STYLE, version

ROOT = pathlib.Path(__file__).resolve().parents[2]
QUELLE = ROOT / "staedte"
ZIEL = ROOT / "viewer" / "stadt"

KATEGORIEN = {  # Reihenfolge und Wortlaut wie in der Legende der Karte
    "school": "Schulen",
    "kindergarten": "Kitas und Kindergärten",
    "childcare": "Kindertagespflege",
    "playground": "Spielplätze",
    "youth": "Kinder- und Jugendeinrichtungen",
    "sports": "Sportstätten",
    "cannabis_club": "Anbauvereinigungen",
}
ARTNAME = {
    "park": "Park",
    "garten": "Garten",
    "wald": "Wald",
    "hochschule": "Hochschule",
    "see": "See",
    "fluss": "Fluss",
    "strand": "Strand",
    "platz": "Platz",
}

STADT_STYLE = (
    STYLE
    + """
  .knopf { display: inline-block; margin: .3em .4em .3em 0; }
  figure.karte img { width: 100%; height: auto; border: 1px solid var(--linie);
                     border-radius: 4px; }
  .lead { font-size: 1.12em; }
  .gross { font-size: 2.4em; font-weight: 700; color: var(--marke-dunkel); line-height: 1.1; }
  td.zahl, th.zahl { text-align: end; font-variant-numeric: tabular-nums; }
  .stand { font-size: .9em; opacity: .85; margin-top: .2em; }
  .tabellennotiz { font-size: .88em; color: var(--text-schwach); margin: .4em 0 1.2em; }
  .hinweis { font-size: .92em; color: var(--text-schwach);
             border-inline-start: 4px solid var(--linie); padding: .4em 1em; margin: 1.5em 0; }
  @media (max-width: 480px) { th, td { padding: 6px 6px; } th.land, td.land { display: none; } }
  figure.deutschland { margin: 1em auto; max-width: 560px; }
  svg.deutschland { width: 100%; height: auto; display: block; }
  svg.deutschland .land { fill: #eef0eb; stroke: #9aa39a; stroke-width: 1; }
  svg.deutschland .grenze { fill: none; stroke: #b9c0b8; stroke-width: .8; stroke-dasharray: 3 2; }
  svg.deutschland circle { r: 5; fill: #c0392b; stroke: #fff; stroke-width: 1.5; }
  svg.deutschland text { font-size: 15px; font-weight: 700; fill: #222; paint-order: stroke;
                         stroke: #fff; stroke-width: 3.5px; stroke-linejoin: round; }
  svg.deutschland .anteil { font-weight: 400; fill: #c0392b; }
  svg.deutschland a:hover circle, svg.deutschland a:focus circle { r: 7; fill: #8e2a20; }
  svg.deutschland a:hover text, svg.deutschland a:focus text { text-decoration: underline; }
  ul.grau { list-style: none; padding: 0; display: flex; flex-wrap: wrap; gap: .3em .9em; }
  ul.grau a { color: var(--text-schwach); text-decoration: none;
              border-bottom: 1px dotted currentColor; }
  ul.grau a:hover, ul.grau a:focus { color: var(--marke); }
  svg.deutschland line { stroke: #8a918a; stroke-width: 1; }
  /* Auf dem Handy nur die Namen, dafuer groesser; die Anteile stehen in der Tabelle darunter. */
  @media (max-width: 480px) { svg.deutschland text { font-size: 19px; }
                              svg.deutschland .anteil { display: none; }
                              svg.deutschland circle { r: 7; }
                              svg.deutschland a.ohne-handy text,
                              svg.deutschland a.ohne-handy line { display: none; } }
"""
)


def zahl(x: float, stellen: int = 0) -> str:
    """Deutsche Schreibweise: schmales Leerzeichen als Tausender-, Komma als Dezimaltrenner."""
    return f"{x:,.{stellen}f}".replace(",", "\u202f").replace(".", ",")


def prozent(x: float, stellen: int = 0) -> str:
    return zahl(100 * x, stellen) + "\u00a0%"


def werte(d: dict) -> dict[str, str]:
    """Alle Schluessel, die ein Text als {{...}} benutzen darf."""
    w = {
        "anteil": prozent(d["anteil"], 1),
        "zonen_km2": zahl(d["zonen_km2"]),
        "flaeche_km2": zahl(d["stadtflaeche_osm_km2"]),
        "objekte": zahl(d["objekte_gesamt"]),
        "fz_ha": zahl(d["fussgaengerzonen"]["flaeche_ha"]),
        "stand": d["stand"]["datum_lang"],
    }
    if "wald" in d:
        w["wald_km2"] = zahl(d["wald"]["flaeche_km2"])
        w["wald_anteil"] = prozent(d["wald"]["anteil_stadt"])
        w["wald_zone"] = prozent(d["wald"]["anteil_zone"] or 0, 1)
        w["ohne_wald_zone"] = prozent(d["wald"]["anteil_zone_ohne_wald"], 1)
    for kat, v in d["kategorien"].items():
        w[f"objekte:{kat}"] = zahl(v["objekte"])
        w[f"zonen_km2:{kat}"] = zahl(v["zonen_km2"], 1)
    for art, ms in d["merkmale"].items():
        for m in ms:
            w[f"{art}:{m['name']}"] = prozent(m["anteil_zone"] or 0)
            w[f"{art}:{m['name']}:ha"] = zahl(m["flaeche_ha"]) + "\u00a0ha"
    for u in d.get("uferstreifen_100m", []):
        w[f"ufer:{u['name']}"] = prozent(u["anteil_zone"] or 0)
    # Einzelwerte, die nur eine Stadt hat (etwa der Anteil im Berliner S-Bahn-Ring)
    for k, v in d.get("extra", {}).items():
        w[k] = prozent(v["wert"]) if v["format"] == "prozent" else zahl(v["wert"])
    for o in d.get("orte", []):
        w[f"ort:{o['name']}"] = prozent(o["anteil_zone"] or 0)
        w[f"ort:{o['name']}:ha"] = zahl(o["flaeche_ha"]) + " ha"
    return w


def einsetzen(text: str, w: dict[str, str], wo: str) -> str:
    def ersetze(m: re.Match) -> str:
        k = m.group(1).strip()
        if k not in w:
            raise SystemExit(f"{wo}: unbekannter Schluessel {{{{{k}}}}}")
        return w[k]

    return re.sub(r"\{\{([^}]+)\}\}", ersetze, text)


def bild_figur(slug: str, kurz: str, d: dict) -> str:
    b = next((x for x in d["bilder"] if x[2] == kurz), None)
    if b is None:
        raise SystemExit(f"{slug}: [[bild:{kurz}]] steht nicht in der Bildliste")
    art, name, _ = b
    # "ort": ein benannter Ort (Wiese, Platz, Strasse), sonst ein Merkmal (Park, Campus ...)
    quelle = d["orte"] if art == "ort" else d["merkmale"][art]
    m = next(x for x in quelle if x["name"] == name)
    datei = ZIEL.parent / "img" / "stadt" / f"{slug}-{kurz}.webp"
    if not datei.exists():
        raise SystemExit(f"{slug}: Bild {datei.relative_to(ROOT)} fehlt")
    anteil = prozent(m["anteil_zone"] or 0)
    alt = (
        f"Karte: {name} (grün umrandet) mit den roten Verbotszonen; "
        f"etwa {anteil} der Fläche liegen in Sichtweite einer Einrichtung"
    )
    kartenlink = f"../#16/{m['lat']}/{m['lon']}"
    return (
        f'<figure class="karte"><img src="../img/stadt/{slug}-{kurz}.webp" '
        'width="800" height="500" '
        f'loading="lazy" decoding="async" alt="{html.escape(alt)}">'
        f"<figcaption>{html.escape(name)}: {m['flaeche_ha']:.0f} ha, davon {anteil} Verbotszone. "
        f'<a href="{kartenlink}">Auf der Karte ansehen</a></figcaption></figure>'
    )


# Weiche Trennstellen fuer die Tabelle: auf 360 px Breite passen die langen Woerter sonst nicht.
TRENNSTELLEN = {
    "Jugendeinrichtungen": "Jugend\u00adeinrichtungen",
    "Kindergärten": "Kinder\u00adgärten",
    "Kindertagespflege": "Kinder\u00adtages\u00adpflege",
    "Sportstätten": "Sport\u00adstätten",
}


def getrennt(text: str) -> str:
    for wort, mit in TRENNSTELLEN.items():
        text = text.replace(wort, mit)
    return text


def flaeche_zelle(km2: float) -> str:
    """Zonenflaeche fuer die Tabelle; eine einzelne kleine Zone nicht als "0,0 km²"."""
    return "unter 0,1\u00a0km²" if 0 < km2 < 0.05 else f"{zahl(km2, 1)}\u00a0km²"


def kategorien_tabelle(d: dict) -> str:
    zeilen = []
    for kat, name in KATEGORIEN.items():
        v = d["kategorien"].get(kat)
        if not v or not v["objekte"]:
            continue
        zeilen.append(
            f'<tr><td>{getrennt(name)}</td><td class="zahl">{zahl(v["objekte"])}</td>'
            f'<td class="zahl">{flaeche_zelle(v["zonen_km2"])}</td></tr>'
        )
    return (
        '<table><thead><tr><th>Einrichtung</th><th class="zahl">Anzahl</th>'
        '<th class="zahl">Zonen\u00adfläche</th></tr></thead><tbody>'
        + "".join(zeilen)
        + "</tbody></table>"
        + '<p class="tabellennotiz">Jede Art für sich gezählt, Überlappungen also mehrfach. '
        "Anbauvereinigungen nur, soweit in OpenStreetMap eingetragen.</p>"
    )


def kopf(
    titel: str, beschreibung: str, url: str, bild: str, bild_alt: str, ld: dict, quelle: str
) -> str:
    """Kopf einer Seite unter /stadt/ (Titel, Beschreibung, Teilerbild, strukturierte Daten)."""
    return f"""<!doctype html>
<!-- Erzeugt aus {quelle} durch deploy/bin/build_staedte.py
     (`make staedte`). Nicht von Hand aendern: die CI prueft, dass ein Neubau kein Diff ergibt. -->
<html lang="de">
<head>
<meta charset="utf-8">
<title>{titel} | BubatzBlick</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="light">
<meta name="description" content="{html.escape(beschreibung)}">
<link rel="canonical" href="{url}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="BubatzBlick">
<meta property="og:locale" content="de_DE">
<meta property="og:url" content="{url}">
<meta property="og:title" content="{titel}">
<meta property="og:description" content="{html.escape(beschreibung)}">
<meta property="og:image" content="{bild}">
<meta property="og:image:type" content="image/jpeg">
<meta property="og:image:width" content="1200"><meta property="og:image:height" content="630">
<meta property="og:image:alt" content="{html.escape(bild_alt)}">
<meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="../img/favicon.svg" type="image/svg+xml">
<link rel="icon" href="../img/favicon-32.png" sizes="32x32" type="image/png">
<link rel="apple-touch-icon" href="../img/apple-touch-icon.png">
<script type="application/ld+json">
{json.dumps(ld, ensure_ascii=False, indent=1)}
</script>
<style>{STADT_STYLE}</style>
</head>
"""


def rahmen(
    h1: str,
    unterzeile: str,
    knoepfe: list[tuple[str, str]],
    body: str,
    stand: str,
    zusatz: str = "",
) -> str:
    """Kopfleiste, Inhalt und Fuss; knoepfe sind (Adresse, Text), zusatz eine Zeile unter der
    Unterzeile (Stand der Zahlen und der Recherche)."""
    zusatz_html = f'\n    <p class="stand">{zusatz}</p>' if zusatz else ""
    nav = "\n      ".join(f'<a href="{h}" class="knopf">{html.escape(x)}</a>' for h, x in knoepfe)
    return f"""<body>
<header><div class="inner">
  <div>
    <h1>{h1}</h1>
    <p>{unterzeile}</p>{zusatz_html}
    <nav>{nav}</nav>
  </div>
</div></header>
<main>
{body}
</main>
<footer>BubatzBlick – Bubatzkarte 2.0 · Version {version()} ·
Daten © OpenStreetMap-Mitwirkende (ODbL) · Stand {stand} ·
<a href="../">Zur Karte</a> · <a href="./">Alle Städte</a> ·
<a href="../about.html#kontakt-und-impressum">Impressum</a></footer>
</body>
</html>
"""


def krume(*stufen: tuple[str, str]) -> dict:
    """BreadcrumbList aus (Name, Adresse)."""
    return {
        "@type": "BreadcrumbList",
        "itemListElement": [
            {"@type": "ListItem", "position": i, "name": n, "item": u}
            for i, (n, u) in enumerate(stufen, 1)
        ],
    }


MONATE = (
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
)


def recherche(stadt: dict) -> dt.date:
    """Wann Regeln und Quellen der Seite zuletzt recherchiert wurden (staedte.toml). Anders als die
    Zahlen wird das nicht neu gerechnet: bei einer neuen Recherche von Hand nachziehen."""
    r = stadt.get("recherche")
    if not isinstance(r, dt.date):
        raise SystemExit(f"{stadt['slug']}: recherche = JJJJ-MM-TT fehlt in staedte.toml")
    return r


def anker(text: str) -> str:
    """Anker einer Zwischenueberschrift, wie die toc-Erweiterung von Markdown ihn vergibt."""
    from markdown.extensions.toc import slugify

    return slugify(text, "-")


@functools.cache
def alle_staedte() -> tuple[tuple[dict, dict, str], ...]:
    """(Eintrag, Zahlen, Text) aller fertigen Staedte, fuer Querverweise."""
    liste = tomllib.loads((QUELLE / "staedte.toml").read_text(encoding="utf-8"))["stadt"]
    return tuple(
        (
            st,
            json.loads((QUELLE / f"{st['slug']}.json").read_text(encoding="utf-8")),
            (QUELLE / f"{st['slug']}.md").read_text(encoding="utf-8"),
        )
        for st in liste
        if st.get("fertig")
    )


def ueberschriften(md: str) -> list[tuple[str, str]]:
    """(Ueberschrift, Abschnittstext) je Zwischenueberschrift zweiter Ebene."""
    teile = re.split(r"^## (.+)$", md, flags=re.M)
    return [(h.strip(), t) for h, t in zip(teile[1::2], teile[2::2], strict=True)]


def abschnitt_fussgaenger(md: str) -> str | None:
    """Anker des Abschnitts, der die Fussgaengerzonen beschreibt (dort steht, was blau ist)."""
    for h, t in ueberschriften(md):
        if h != "Woher die Zonen kommen" and "blau" in t and "Fußgänger" in t:
            return anker(h)
    return None


def entfernung_km(a: dict, b: dict) -> float:
    la1, lo1, la2, lo2 = map(math.radians, (a["lat"], a["lon"], b["lat"], b["lon"]))
    h = (
        math.sin((la2 - la1) / 2) ** 2
        + math.cos(la1) * math.cos(la2) * math.sin((lo2 - lo1) / 2) ** 2
    )
    return 2 * 6371 * math.asin(math.sqrt(h))


NACHBARN = 3
UEBERSCHRIFT_NACHBARN = "Nachbarstädte im Vergleich"
# Ueberschrift im Text -> Ueberschrift auf der Seite; der Anker kommt vom Text links.
STANDARD_UEBERSCHRIFTEN = {
    "Woher die Zonen kommen": "Woher die Zonen in {name} kommen",
    "Was sonst noch gilt": "Was sonst noch in {name} gilt",
    "Häufige Fragen": "Häufige Fragen zu {name}",
    UEBERSCHRIFT_NACHBARN: "Nachbarstädte von {name} im Vergleich",
}


def nachbarn_md(stadt: dict, d: dict) -> str:
    """Abschnitt mit den naechsten Staedten und den Staedten im selben Land, verlinkt bis in deren
    Abschnitte zu Fussgaengerzonen und Landesregeln."""
    andere = sorted(
        (entfernung_km(d["ansicht_stadt"], d2["ansicht_stadt"]), st2, d2, md2)
        for st2, d2, md2 in alle_staedte()
        if st2["slug"] != stadt["slug"]
    )
    # Als <nav>: es ist Navigation zwischen den Seiten, kein Text ueber diese Stadt.
    zeilen = [
        '<nav class="nachbarn" aria-label="Nachbarstädte" markdown="1">',
        "",
        f"## {UEBERSCHRIFT_NACHBARN}",
        "",
        f"In {stadt['name']} liegen {prozent(d['anteil'], 1)} der Stadtfläche in einer "
        "Verbotszone. Die nächsten Städte mit eigener Seite:",
        "",
    ]
    for km, st2, d2, md2 in andere[:NACHBARN]:
        verweise = []
        fz = abschnitt_fussgaenger(md2)
        if fz:
            verweise.append(f"[Fußgängerzonen in {st2['name']}]({st2['slug']}#{fz})")
        regeln = (
            f"Regeln in {st2['name']}"
            if st2["land"] == stadt["land"]
            else f"Regeln in {st2['name']} ({st2['land']})"
        )
        verweise.append(f"[{regeln}]({st2['slug']}#was-sonst-noch-gilt)")
        zeilen.append(
            f"- **[{st2['name']}]({st2['slug']})**, {zahl(km)} km entfernt: "
            f"{prozent(d2['anteil'], 1)} der Stadtfläche; " + ", ".join(verweise)
        )
    gleich = [
        st2
        for _, st2, _, _ in sorted(andere, key=lambda e: sortierschluessel(e[1]["name"]))
        if st2["land"] == stadt["land"]
    ]
    if gleich:
        links = ", ".join(f"[{st2['name']}]({st2['slug']}#was-sonst-noch-gilt)" for st2 in gleich)
        zeilen += ["", f"Dieselben Landesregeln ({stadt['land']}) gelten auch in {links}."]
    sonder = [
        st2
        for st2, _, _ in sorted(alle_staedte(), key=lambda e: sortierschluessel(e[0]["name"]))
        if st2.get("sonderzone") and st2["slug"] != stadt["slug"]
    ]
    if stadt.get("sonderzone") and sonder:
        links = ", ".join(f"[{st2['name']}]({st2['slug']}#{st2['sonderzone']})" for st2 in sonder)
        zeilen += [
            "",
            f"Eigene Verbotszonen über § 5 KCanG hinaus wie in {stadt['name']} haben auch {links}.",
        ]
    zeilen += ["", "</nav>"]
    return "\n".join(zeilen) + "\n\n"


def pruefe_anker(slug: str, body: str) -> None:
    """Jeder Link auf einen Anker muss treffen: auf dieser Seite oder auf einer anderen
    Staedteseite. Sonst bricht der Bau ab, statt einen toten Sprung auszuliefern."""
    eigene = set(re.findall(r' id="([^"]+)"', body))
    for ziel in re.findall(r'href="#([^"]+)"', body):
        if ziel not in eigene:
            raise SystemExit(f"{slug}: Anker #{ziel} gibt es auf der Seite nicht")
    andere = {st["slug"]: md for st, _, md in alle_staedte()}
    for zslug, ziel in re.findall(r'href="([a-z-]+)#([^"]+)"', body):
        if zslug not in andere:
            raise SystemExit(f"{slug}: Link auf {zslug}#{ziel}, aber {zslug} hat keine Seite")
        ids = {anker(h) for h, _ in ueberschriften(andere[zslug])} | {anker(UEBERSCHRIFT_NACHBARN)}
        if ziel not in ids:
            raise SystemExit(f"{slug}: Anker #{ziel} gibt es auf der Seite {zslug} nicht")


def seite(stadt: dict, d: dict, body: str) -> str:
    slug, name = stadt["slug"], stadt["name"]
    titel = f"Kiffen in {name}: wo es verboten ist"
    # Mit " | BubatzBlick" ueber 60 Zeichen schneidet die Trefferliste ab: dann der Kurzname
    # ("Frankfurt"); Ueberschrift und Text behalten den vollen Namen.
    if len(titel) + len(" | BubatzBlick") > 60 and stadt.get("kurzname"):
        titel = f"Kiffen in {stadt['kurzname']}: wo es verboten ist"
    beschreibung = (
        f"In {name} liegen {prozent(d['anteil'], 1)} der Stadtfläche ({zahl(d['zonen_km2'])} km²) "
        "in Sichtweite von Schulen, Kitas, Spielplätzen und Sportstätten. "
        "Karte, Parks, Regeln vor Ort."
    )
    og_alt = (
        f"Karte der Innenstadt von {name} mit den roten Verbotszonen, daneben: "
        f"Kiffen in {name}, {prozent(d['anteil'], 1)} der Stadtfläche liegen in Sichtweite"
    )
    url = f"__SITE_URL__/stadt/{slug}"
    a = d["ansicht_stadt"]
    ld = {
        "@context": "https://schema.org",
        "@graph": [
            {
                "@type": "WebPage",
                "@id": url,
                "url": url,
                "name": titel,
                "description": beschreibung,
                "inLanguage": "de",
                "isPartOf": {"@id": "__SITE_URL__/#website"},
                "dateModified": max(d["stand"]["datum"], recherche(stadt).isoformat()),
                "about": {
                    "@type": "City",
                    "name": name,
                    "sameAs": f"https://www.wikidata.org/wiki/{stadt['wikidata']}",
                    # Berlin und Hamburg sind selbst Land (die Stadt Bremen liegt im Land Bremen)
                    "containedInPlace": {"@type": "Country", "name": "Deutschland"}
                    if stadt["land"] in ("Berlin", "Hamburg")
                    else {"@type": "State", "name": stadt["land"]},
                },
                "primaryImageOfPage": f"__SITE_URL__/img/stadt/{slug}.jpg",
            },
            krume(("Karte", "__SITE_URL__/"), ("Städte", "__SITE_URL__/stadt/"), (name, url)),
        ],
    }
    karte = f"../#{12 if d['stadtflaeche_osm_km2'] > 150 else 13}/{a['lat']}/{a['lon']}"
    return kopf(
        titel,
        beschreibung,
        url,
        f"__SITE_URL__/img/stadt/{slug}.jpg",
        og_alt,
        ld,
        f"staedte/{slug}.md und staedte/{slug}.json",
    ) + rahmen(
        f"Kiffen in {name}",
        "Wo das Konsumverbot nach § 5 KCanG gilt, berechnet aus OpenStreetMap.",
        [
            (karte, f"{name} auf der Karte"),
            ("./", "Alle Städte"),
            ("../about.html", "Wie gerechnet wird"),
        ],
        body,
        d["stand"]["datum_lang"],
        f"Zahlen vom {d['stand']['datum_lang']} · Recherche zu Regeln vor Ort: "
        f"{MONATE[recherche(stadt).month - 1]} {recherche(stadt).year}",
    )


# -- Uebersicht /stadt/ ------------------------------------------------------

KARTE_BREITE = 600  # SVG-Einheiten; die Karte skaliert mit der Seite
RAND_LINKS = 90  # Platz fuer Beschriftungen westlich von NRW und dem Saarland


def merc(lon: float, lat: float) -> tuple[float, float]:
    return math.radians(lon), math.log(math.tan(math.pi / 4 + math.radians(lat) / 2))


def strich_ende(tx: float, ty: float, anker: str, py: float) -> tuple[float, float]:
    """Wo der Strich vom Punkt an die Beschriftung stoesst: an ihrer Anfangs- oder Endkante auf
    halber Hoehe, bei mittig gesetzter an der Ober- oder Unterkante."""
    if anker == "start":
        return tx - 2, ty - 5
    if anker == "end":
        return tx + 2, ty - 5
    return tx, (ty - 14 if ty > py else ty + 3)


def svg_karte(eintraege: list[tuple[dict, dict]]) -> str:
    """Deutschland mit Laendergrenzen (staedte/deutschland.json) und je Stadt ein Punkt mit Link."""
    de = json.loads((QUELLE / "deutschland.json").read_text(encoding="utf-8"))
    punkte = [merc(*k) for teil in de["land"] for ring in teil for k in ring]
    x0, x1 = min(p[0] for p in punkte), max(p[0] for p in punkte)
    y0, y1 = min(p[1] for p in punkte), max(p[1] for p in punkte)
    rand = 12
    s = (KARTE_BREITE - RAND_LINKS - rand) / (x1 - x0)
    hoehe = round((y1 - y0) * s + 2 * rand)

    def xy(lon: float, lat: float) -> tuple[float, float]:
        x, y = merc(lon, lat)
        return round((x - x0) * s + RAND_LINKS, 1), round((y1 - y) * s + rand, 1)

    def pfad(ringe: list, zu: bool) -> str:
        teile = []
        for ring in ringe:
            pts = [xy(*k) for k in ring]
            teile.append("M" + " ".join(f"{x:g},{y:g}" for x, y in pts) + ("Z" if zu else ""))
        return "".join(teile)

    land = pfad([r for teil in de["land"] for r in teil], True)
    grenzen = pfad(de["grenzen"], False)
    orte = []
    for stadt, d in eintraege:
        a = d["ansicht_stadt"]
        x, y = xy(a["lon"], a["lat"])
        lage = stadt.get("beschriftung", "rechts")
        kurz = stadt.get("kurzname", stadt["name"])  # auf der Karte, etwa "Frankfurt"
        # In dichten Gegenden steht auf dem Handy nur der Punkt; die Tabelle nennt alle Staedte
        klasse = "" if stadt.get("name_handy", True) else ' class="ohne-handy"'
        strich = ""
        if isinstance(lage, dict):
            # Freie Lage fuer dichte Gegenden (Ruhrgebiet, Rhein-Main): Textanfang relativ zum
            # Punkt, auf Wunsch mit Strich vom Punkt bis an den Rand der Beschriftung
            tx, ty = round(x + lage["dx"], 1), round(y + lage["dy"], 1)
            anker = lage.get("anker", "start")
            if lage.get("strich"):
                ex, ey = strich_ende(tx, ty, anker, y)
                strich = f'<line x1="{x:g}" y1="{y:g}" x2="{ex:g}" y2="{ey:g}"/>'
        else:
            tx, ty, anker = {
                "rechts": (x + 10, y + 5, "start"),
                "links": (x - 10, y + 5, "end"),
                "oben": (x, y - 9, "middle"),
                "unten": (x, y + 21, "middle"),
            }[lage]
        orte.append(
            f'<a href="{stadt["slug"]}"{klasse}><title>{html.escape(stadt["name"])}: '
            f"{prozent(d['anteil'], 1)} der Stadtfläche</title>"
            f'{strich}<circle cx="{x:g}" cy="{y:g}" r="7"/>'
            f'<text x="{tx:g}" y="{ty:g}" text-anchor="{anker}">{html.escape(kurz)} '
            f'<tspan class="anteil">{prozent(d["anteil"])}</tspan></text></a>'
        )
    return (
        f'<svg class="deutschland" viewBox="0 0 {KARTE_BREITE} {hoehe}" role="group" '
        f'aria-label="Deutschlandkarte mit den Städten, für die es eine Seite gibt">'
        f'<path class="land" d="{land}"/><path class="grenze" d="{grenzen}"/>'
        + "".join(orte)
        + "</svg>"
    )


def sortierschluessel(name: str) -> str:
    ersetzen: dict[str, str | int | None] = {"ä": "a", "ö": "o", "ü": "u", "ß": "ss"}
    return name.lower().translate(str.maketrans(ersetzen))


def uebersicht(eintraege: list[tuple[dict, dict]], grau: list[dict]) -> pathlib.Path:
    eintraege = sorted(eintraege, key=lambda e: sortierschluessel(e[0]["name"]))
    n = len(eintraege)
    url = "__SITE_URL__/stadt/"
    titel = "Kiffen in deutschen Städten: die Verbotszonen"
    beschreibung = (
        f"Wie viel Stadtfläche in Sichtweite von Schulen, Kitas, Spielplätzen und Sportstätten "
        f"liegt: {n} Städte mit Karte, Parks, Fußgängerzonen und Regeln vor Ort."
    )
    neueste = max(eintraege, key=lambda e: e[1]["stand"]["datum"])[1]["stand"]
    zeilen = "".join(
        f'<tr><td><a href="{st["slug"]}">{html.escape(st["name"])}</a></td>'
        f'<td class="land">{html.escape(st["land"])}</td>'
        f'<td class="zahl">{zahl(d["zonen_km2"])}\u00a0km²</td>'
        f'<td class="zahl">{prozent(d["anteil"], 1)}</td></tr>'
        for st, d in eintraege
    )
    grau_liste = "".join(
        f'<li><a href="{s["slug"]}" rel="nofollow">{html.escape(s["name"])}</a></li>' for s in grau
    )
    body = f"""<p class="lead">Für jede Stadt eine eigene Seite: wie viel der Stadtfläche in
Sichtweite von Schulen, Kitas, Spielplätzen und Sportstätten liegt, was in den großen Parks, auf
Plätzen und in
Fußgängerzonen gilt und welche Regeln Stadt und Land dazu haben. Bisher sind es {n} Städte, weitere
kommen dazu.</p>
<figure class="deutschland">{svg_karte(eintraege)}
<figcaption>Ein Klick auf eine Stadt öffnet ihre Seite.
Grenzen: © OpenStreetMap-Mitwirkende.</figcaption></figure>
<h2 id="alle-staedte">Alle Städte</h2>
<table><thead><tr><th>Stadt</th><th class="land">Land</th><th class="zahl">Verbots\u00adzone</th>
<th class="zahl">Anteil</th></tr></thead><tbody>{zeilen}</tbody></table>
<p class="hinweis">Die Zahlen gelten innerhalb der Stadtgrenze aus OpenStreetMap und zählen jede
Stelle einmal, auch wo sich Zonen überlappen; Fußgängerzonen sind nicht eingerechnet. Den Anteil
für ganz
Deutschland und je Bundesland nennt die
<a href="../about.html#zonenflache-je-bundesland">Über-Seite</a>.</p>
<h2 id="vorschlaege">Welche Stadt sollen wir noch ausführlicher beschreiben?</h2>
<p>Für diese Großstädte gibt es noch keine eigene Seite. Ein Klick zählt als Stimme und führt
gleich zur Karte der Stadt.</p>
<ul class="grau">{grau_liste}</ul>"""
    ld = {
        "@context": "https://schema.org",
        "@graph": [
            {
                "@type": "CollectionPage",
                "@id": url,
                "url": url,
                "name": titel,
                "description": beschreibung,
                "inLanguage": "de",
                "isPartOf": {"@id": "__SITE_URL__/#website"},
                "dateModified": neueste["datum"],
                "mainEntity": {
                    "@type": "ItemList",
                    "itemListElement": [
                        {
                            "@type": "ListItem",
                            "position": i,
                            "name": f"Kiffen in {st['name']}",
                            "url": f"__SITE_URL__/stadt/{st['slug']}",
                        }
                        for i, (st, _) in enumerate(eintraege, 1)
                    ],
                },
            },
            krume(("Karte", "__SITE_URL__/"), ("Städte", url)),
        ],
    }
    out = ZIEL / "index.html"
    out.write_text(
        kopf(
            titel,
            beschreibung,
            url,
            "__SITE_URL__/img/stadt/uebersicht.jpg",
            "Deutschlandkarte mit den Städten, für die es eine Seite gibt, "
            "und ihrem Anteil Verbotszone",
            ld,
            "staedte/staedte.toml, staedte/<slug>.json und staedte/deutschland.json",
        )
        + rahmen(
            "Kiffen in deutschen Städten",
            "Wie viel der Stadtfläche in Sichtweite liegt, berechnet aus OpenStreetMap.",
            [("../", "Zur Karte"), ("../about.html", "Wie gerechnet wird")],
            body,
            neueste["datum_lang"],
        ),
        encoding="utf-8",
    )
    return out


def baue(stadt: dict) -> pathlib.Path:
    import markdown

    slug = stadt["slug"]
    d = json.loads((QUELLE / f"{slug}.json").read_text(encoding="utf-8"))
    w = werte(d)
    src = einsetzen((QUELLE / f"{slug}.md").read_text(encoding="utf-8"), w, f"staedte/{slug}.md")
    src = src.replace("[[kategorien]]", kategorien_tabelle(d))
    stadtbild = (
        f'<figure class="karte"><img src="../img/stadt/{slug}-stadt.webp" width="960" height="600" '
        f'alt="Karte von {html.escape(stadt["name"])} mit Stadtgrenze und den roten Verbotszonen">'
        f"<figcaption>{html.escape(stadt['name'])}: {w['zonen_km2']}\u00a0km² Verbotszone auf "
        f"{w['flaeche_km2']}\u00a0km² Stadtfläche. Rot: in Sichtweite einer Einrichtung."
        "</figcaption></figure>"
    )
    src = src.replace("[[stadtkarte]]", stadtbild)
    if "\n## Häufige Fragen" not in src:
        raise SystemExit(f"staedte/{slug}.md: Abschnitt „Häufige Fragen“ fehlt")
    src = src.replace("\n## Häufige Fragen", "\n" + nachbarn_md(stadt, d) + "## Häufige Fragen", 1)
    # Die Standard-Ueberschriften mit dem Stadtnamen, die Anker bleiben die alten (Links von
    # anderen Seiten zeigen darauf).
    for alt, neu in STANDARD_UEBERSCHRIFTEN.items():
        src = re.sub(
            rf"^## {re.escape(alt)}$",
            f"## {neu.format(name=stadt['name'])} {{#{anker(alt)}}}",
            src,
            flags=re.M,
        )
    src = re.sub(
        r"^\[\[bild:([a-z0-9-]+)\]\]$", lambda m: bild_figur(slug, m.group(1), d), src, flags=re.M
    )
    body = markdown.markdown(
        src, extensions=["tables", "sane_lists", "attr_list", "toc", "md_in_html"]
    )
    pruefe_anker(slug, body)
    ZIEL.mkdir(parents=True, exist_ok=True)
    out = ZIEL / f"{slug}.html"
    out.write_text(seite(stadt, d, body), encoding="utf-8")
    return out


def vorschlag(s: dict) -> pathlib.Path:
    """Platzhalter /stadt/<slug> fuer eine Grossstadt ohne eigene Seite. Der Abruf zaehlt im
    Zugriffsprotokoll als Stimme (aggregate_access.py); die Seite leitet sofort zur Karte weiter,
    die dort einen Hinweis zeigt (?interesse=<slug>, app.js). Nicht indexieren, nicht in die
    Sitemap: wird die Stadt beschrieben, ersetzt ihre Seite diese Datei unter derselben Adresse."""
    ziel = f"../?interesse={s['slug']}#12/{s['lat']}/{s['lon']}"
    name = html.escape(s["name"])
    out = ZIEL / f"{s['slug']}.html"
    out.write_text(
        f"""<!doctype html>
<!-- Platzhalter, erzeugt durch deploy/bin/build_staedte.py aus staedte/grossstaedte.json. -->
<html lang="de">
<head>
<meta charset="utf-8">
<title>Kiffen in {name} | BubatzBlick</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<meta http-equiv="refresh" content="0; url={ziel}">
</head>
<body>
<p>Weiter zur <a href="{ziel}">Karte von {name}</a>.</p>
</body>
</html>
""",
        encoding="utf-8",
    )
    return out


def sitemap(slugs: list[str]) -> None:
    """Staedteseiten und Uebersicht in viewer/sitemap.xml: alte /stadt/-Zeilen raus, neue ans Ende.
    Die Herkunft kommt aus der ersten Adresse dort (die Domain steht in der Sitemap fest)."""
    datei = ROOT / "viewer" / "sitemap.xml"
    zeilen = [z for z in datei.read_text(encoding="utf-8").splitlines() if "/stadt/" not in z]
    m = re.search(r"<loc>(https://[^/<]+)/", "\n".join(zeilen))
    if not m:
        raise SystemExit("viewer/sitemap.xml: keine Adresse, aus der die Herkunft hervorgeht")
    ende = zeilen.index("</urlset>")
    neu = [
        f"  <url><loc>{m.group(1)}/stadt/{s}</loc></url>" for s in ["", *slugs]
    ]  # "" = Uebersicht
    datei.write_text("\n".join(zeilen[:ende] + neu + zeilen[ende:]) + "\n", encoding="utf-8")


def main() -> None:
    liste = tomllib.loads((QUELLE / "staedte.toml").read_text(encoding="utf-8"))["stadt"]
    fertig = [stadt for stadt in liste if stadt.get("fertig")]
    for stadt in fertig:
        print(baue(stadt).relative_to(ROOT))
    daten = [
        (st, json.loads((QUELLE / f"{st['slug']}.json").read_text(encoding="utf-8")))
        for st in fertig
    ]
    gross = json.loads((QUELLE / "grossstaedte.json").read_text(encoding="utf-8"))["staedte"]
    mit_seite = {st["slug"] for st in fertig}
    grau = sorted(
        (s for s in gross if s["slug"] not in mit_seite), key=lambda s: sortierschluessel(s["name"])
    )
    print(uebersicht(daten, grau).relative_to(ROOT))
    for s in grau:
        vorschlag(s)
    print(f"viewer/stadt/: {len(grau)} Platzhalter")
    # Namen fuer den Hinweis auf der Karte (app.js liest sie nur bei ?interesse=)
    (ZIEL / "vorschlaege.json").write_text(
        json.dumps({s["slug"]: s["name"] for s in grau}, ensure_ascii=False, indent=0) + "\n",
        encoding="utf-8",
    )
    # Seiten von Staedten, die aus beiden Listen gefallen sind, nicht stehen lassen
    erwartet = {"index"} | mit_seite | {s["slug"] for s in grau}
    for weg in ZIEL.glob("*.html"):
        if weg.stem not in erwartet:
            weg.unlink()
            print(f"entfernt: {weg.relative_to(ROOT)}")
    sitemap([stadt["slug"] for stadt in fertig])


if __name__ == "__main__":
    main()
