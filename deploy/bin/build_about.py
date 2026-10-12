#!/usr/bin/env -S uv run --with markdown --script
"""viewer/about.md -> viewer/about.html (Ueber-Seite).

Redaktionelle Hinweise (HTML-Kommentar am Anfang, kursive ⚠-Anmerkungen)
werden entfernt, die Platzhalter fuer Datenstand und E-Mail durch JavaScript
gefuellt. Die Liste der Verbotsobjekt-Kategorien wird NICHT von Hand
gepflegt: `[KATEGORIEN AUS tags.json EINBLENDEN]` wird aus genau der Tag-Datei
gerendert, die das Makefile (`TAGS_SPEC`) der Pipeline gibt -- Titel,
Beschreibung und Regelkommentare stehen in der JSON-Datei selbst.
`tests/test_about_tags.py` prueft, dass about.html dazu passt.

Aufruf: deploy/bin/build_about.py  (im Repo-Wurzelverzeichnis)
"""

from __future__ import annotations

import json
import pathlib
import re
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]


def version(cargo_toml: pathlib.Path = ROOT / "rust/Cargo.toml") -> str:
    """Versionsnummer des Projekts; einzige Quelle ist der Cargo-Workspace."""
    with cargo_toml.open("rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


PLACEHOLDER_CATEGORIES = "[KATEGORIEN AUS tags.json EINBLENDEN]"
PLACEHOLDER_TAGS_FILE = "[TAGS-DATEI EINBLENDEN]"
PLACEHOLDER_STAND = "[STAND AUS stand.json EINBLENDEN]"
KCANG = {
    1: "§ 5 Abs. 2 Nr. 1",
    2: "§ 5 Abs. 2 Nr. 2",
    3: "§ 5 Abs. 2 Nr. 3",
    4: "§ 5 Abs. 2 Nr. 4",
    6: "§ 5 Abs. 2 Nr. 6",
}


def tags_spec_path(makefile: pathlib.Path = ROOT / "Makefile") -> pathlib.Path:
    """Die Tag-Datei, die `make` tatsaechlich benutzt (`TAGS_SPEC := ...`)."""
    m = re.search(r"^TAGS_SPEC\s*:=\s*(\S+)", makefile.read_text(encoding="utf-8"), re.M)
    if not m:
        raise SystemExit("TAGS_SPEC nicht im Makefile gefunden")
    return ROOT / m.group(1)


def load_spec(path: pathlib.Path | None = None) -> tuple[pathlib.Path, dict]:
    path = path or tags_spec_path()
    return path, json.loads(path.read_text(encoding="utf-8"))


def _rule_md(rule: dict) -> str:
    values = " | ".join(rule["values"])  # mit Leerzeichen: lange Listen brechen an den Trennern um
    text = f"`{rule['key']}={values}`"
    if rule.get("split"):
        text += " (auch als Teil einer Liste)"
    if rule.get("comment"):
        text += f" — {rule['comment']}"
    return text


def render_categories(spec: dict) -> str:
    """Markdown-Liste der Kategorien in Gesetzesreihenfolge, dann Dateireihenfolge."""
    cats = list(spec["categories"].items())
    cats.sort(key=lambda kv: kv[1].get("kcang_nr", 99))
    out: list[str] = []
    for name, c in cats:
        nr = KCANG.get(c.get("kcang_nr"), "")
        head = f"- **{c['title']}**" + (f" ({nr})" if nr else "") + f" — {c['description']}"
        out.append(head)
        out.extend(f"    - {_rule_md(rule)}" for rule in c["osm_tags"])
        out.extend(f"    - {nm['comment']}" for nm in c.get("name_match", []))
        out.extend(f"    - Ausgenommen: {ex['comment']}" for ex in c.get("exclude", []))
        out.extend(f"    - Zugang: {po['comment']}" for po in c.get("public_only", []))
        out.append(f"    - Kategorie im Kartenpopup: `{name}`")
    return "\n".join(out)


def build(src_md: str, spec_path: pathlib.Path, spec: dict) -> str:
    """Markdown mit Platzhaltern -> Markdown, wie es die Seite zeigt."""
    src = re.sub(r"<!--.*?-->\s*", "", src_md, count=1, flags=re.S)  # Redaktionskommentar
    src = re.sub(r"\s*⚠\s*\*.*?\*", "", src, flags=re.S)  # ⚠ *Anmerkung*
    if PLACEHOLDER_CATEGORIES not in src:
        raise SystemExit(f"{PLACEHOLDER_CATEGORIES} fehlt in about.md")
    src = src.replace(PLACEHOLDER_CATEGORIES, render_categories(spec))
    src = src.replace(
        PLACEHOLDER_TAGS_FILE,
        f"`{spec_path.relative_to(ROOT).as_posix()}` (Version {spec['version']})",
    )
    src = src.replace(PLACEHOLDER_STAND, '<span id="stand">wird geladen</span>')
    return src.replace(
        "kontakt [at] bubatzblick [punkt] de",
        '<span id="mail">(JavaScript nötig)</span>',
    )


DESCRIPTION = (
    "Wo der öffentliche Konsum von Cannabis in Sichtweite von Schulen, Spielplätzen, "
    "Jugendeinrichtungen und Sportstätten verboten ist \u2013 berechnet aus OpenStreetMap, "
    "täglich aktualisiert."
)

# Markenfarbe des Projekts: oklch(0.52 0.12 150) = #287c42.
# Abgeleitet, nicht erfunden -- Hover oklch(0.44 0.12 150) = #03642b (Weiss darauf 7,3:1),
# heller Grund oklch(0.95 0.03 150) = #e1f5e4. Nicht das Zonenrot: das ist die
# Datenfarbe der Karte und ausdruecklich KEINE Markenfarbe.
STYLE = """
  body { margin: 0; font: 16px/1.55 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
         color: #222; background: #f2efe9; }
  header { background: #fff; border-bottom: 1px solid #ddd; }
  header .inner { max-width: 760px; margin: 0 auto; padding: 24px 20px 12px; display: flex;
                  gap: 24px; align-items: center; flex-wrap: wrap; }
  header img { width: 200px; height: 200px; flex: none; }
  header h1 { font-size: 1.9em; line-height: 1.2; margin: 0 0 8px; color: #287c42; }
  header p { margin: 0 0 8px; color: #555; }
  nav a { display: inline-block; padding: 6px 12px; border-radius: 6px; background: #287c42;
          color: #fff; text-decoration: none; font-weight: 600; }
  nav a:hover { background: #03642b; }
  main { max-width: 760px; margin: 0 auto; padding: 8px 20px 60px; background: #fff; }
  main h2 { margin-top: 2em; padding-top: .5em; border-top: 1px solid #eee; }
  main h3 { margin-top: 1.5em; }
  blockquote { margin: 1em 0; padding: .6em 1em; border-left: 4px solid #287c42;
               background: #e1f5e4; }
  blockquote ol { padding-left: 1.4em; }
  table { border-collapse: collapse; width: 100%; }
  th, td { text-align: left; padding: 6px 10px; border-bottom: 1px solid #e5e5e5;
           vertical-align: top; }
  code { font-size: .9em; background: #f3f3f3; padding: 1px 4px; border-radius: 3px;
         overflow-wrap: anywhere; }
  pre { overflow-x: auto; background: #f3f3f3; padding: 10px; border-radius: 6px; }
  a { color: #287c42; }
  footer { max-width: 760px; margin: 0 auto; padding: 16px 20px; color: #666; font-size: .9em; }
"""

# Eigene Datei viewer/about.js (kein Inline-Skript: CSP script-src 'self').
SCRIPT = """// Von deploy/bin/build_about.py erzeugt -- nicht von Hand aendern.
(async () => {
  const cfg = Object.assign({ stand: "stand.json" }, window.BUBATZ_CONFIG || {});
  const el = document.getElementById("stand");
  try { const r = await fetch(cfg.stand); const j = await r.json();
    el.textContent = (j.pbf_timestamp || "").slice(0, 10)
      + " (veröffentlicht " + (j.published || "").slice(0, 10) + ")"; }
  catch (e) { el.textContent = "unbekannt"; }
  // Der Ausschnitt reist als ?karte=<hash> mit und geht so zurueck: vorher
  // stand man nach "Zur Karte" wieder im Anfangsausschnitt.
  const karte = new URLSearchParams(location.search).get("karte");
  if (karte) {
    for (const a of document.querySelectorAll("a.zur-karte")) a.href = "./#" + karte;
  }
  const m = document.getElementById("mail"), u = "kontakt",
        d = ["bubatzblick", "de"].join(".");
  const a = document.createElement("a"); a.href = "mailto:" + u + "@" + d;
  a.textContent = u + "@" + d; m.replaceWith(a);
})();
"""

# Beschreibung der Teilerkarte img/og.png (deploy/img/og.py); dieselbe Zeile steht in
# viewer/index.html, tests/test_og_tags.py haelt beide gleich.
OG_ALT = (
    "Vorschaukarte: ein Spielplatz, dessen rote Sichtweitenzone zwischen den Häuserblöcken "
    "die Form eines Hanfblatts hat, daneben der Schriftzug BubatzBlick – Bubatzkarte 2.0, "
    "wo Kiffen in Sichtweite verboten ist"
)

ALT = (
    "Schematische Karte: ein Spielplatz in der Mitte, die rote Sichtweitenzone zwischen "
    "den Häuserblöcken hat die Form eines Hanfblatts"
)


def page(body: str, title: str) -> str:
    return f"""<!doctype html>
<!-- Erzeugt aus viewer/about.md durch deploy/bin/build_about.py (`make about`).
     Nicht von Hand aendern: die CI prueft, dass ein Neubau kein Diff ergibt. -->
<html lang="de">
<head>
<meta charset="utf-8">
<title>{title} \u2013 Bubatzkarte 2.0</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="{DESCRIPTION}">
<link rel="canonical" href="__SITE_URL__/about.html">
<meta property="og:type" content="website">
<meta property="og:site_name" content="BubatzBlick">
<meta property="og:locale" content="de_DE">
<meta property="og:url" content="__SITE_URL__/about.html">
<meta property="og:title" content="{title} \u2013 Bubatzkarte 2.0">
<meta property="og:description" content="{DESCRIPTION}">
<meta property="og:image" content="__SITE_URL__/img/og.png">
<meta property="og:image:type" content="image/png">
<meta property="og:image:width" content="1200"><meta property="og:image:height" content="630">
<meta property="og:image:alt" content="{OG_ALT}">
<meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="img/favicon.svg" type="image/svg+xml">
<link rel="icon" href="img/favicon-32.png" sizes="32x32" type="image/png">
<link rel="apple-touch-icon" href="img/apple-touch-icon.png">
<style>{STYLE}</style>
</head>
<body>
<header><div class="inner">
  <img src="img/sichtfeld-blatt.svg" alt="{ALT}">
  <div>
    <h1>{title}</h1>
    <p>Konsumverbot in Sichtweite nach § 5 Abs. 2 KCanG, berechnet aus OpenStreetMap.</p>
    <nav><a href="./" class="zur-karte">← Zur Karte</a></nav>
  </div>
</div></header>
<main>
{body}
</main>
<footer>BubatzBlick \u2013 Bubatzkarte 2.0 · Version {version()} ·
Daten © OpenStreetMap-Mitwirkende (ODbL) · <a href="./" class="zur-karte">Karte</a></footer>
<script src="config.js"></script>
<script src="about.js"></script>
</body>
</html>
"""


def main() -> None:
    import markdown

    spec_path, spec = load_spec()
    src = build((ROOT / "viewer/about.md").read_text(encoding="utf-8"), spec_path, spec)
    body = markdown.markdown(src, extensions=["tables", "footnotes", "sane_lists"])
    m = re.search(r"<h1>(.*?)</h1>", body)
    if not m:
        raise SystemExit("about.md ohne H1")
    title = m.group(1)
    body = body.replace(f"<h1>{title}</h1>", "", 1)
    out = ROOT / "viewer/about.html"
    out.write_text(page(body, title), encoding="utf-8")
    (ROOT / "viewer/about.js").write_text(SCRIPT, encoding="utf-8")
    print(out.relative_to(ROOT))


if __name__ == "__main__":
    main()
