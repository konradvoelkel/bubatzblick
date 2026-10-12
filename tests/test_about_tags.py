"""Die Ueber-Seite darf nie eine andere Tag-Liste zeigen als die Pipeline benutzt.

Quelle ist `TAGS_SPEC` im Makefile. Geprueft wird: der Rust-Default
(`extract.rs`) zeigt auf dieselbe Datei; `viewer/about.html` (eingecheckt,
von `deploy/bin/build_about.py` erzeugt) enthaelt Titel, Beschreibung, jede
Tag-Regel und jeden Regelkommentar dieser Datei samt Versionsnummer -- also
muss nach jeder Aenderung an tags.v<N>.json oder about.md neu gebaut werden.
"""

from __future__ import annotations

import html
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


def _about_text() -> str:
    raw = (ROOT / "viewer" / "about.html").read_text(encoding="utf-8")
    return html.unescape(re.sub(r"<[^>]+>", "", raw))


def test_rust_default_matches_makefile() -> None:
    ba = _build_about()
    used = ba.tags_spec_path().relative_to(ROOT).as_posix()
    extract_rs = (ROOT / "rust" / "bubatz" / "src" / "extract.rs").read_text(encoding="utf-8")
    assert f'.join("{pathlib.Path(used).name}")' in extract_rs, (
        f"extract.rs Default != Makefile TAGS_SPEC ({used})"
    )


def test_about_html_lists_exactly_the_used_tags() -> None:
    ba = _build_about()
    spec_path, spec = ba.load_spec()
    text = _about_text()
    assert f"{spec_path.relative_to(ROOT).as_posix()} (Version {spec['version']})" in text
    for name, c in spec["categories"].items():
        assert c["title"] in text, name
        assert c["description"] in text, name
        assert f"Kategorie im Kartenpopup: {name}" in text
        for rule in c["osm_tags"]:
            assert f"{rule['key']}={' | '.join(rule['values'])}" in text, (name, rule)
            if rule.get("comment"):
                assert rule["comment"] in text, (name, rule)
        for extra in c.get("name_match", []) + c.get("exclude", []) + c.get("public_only", []):
            assert extra["comment"] in text, (name, extra)
    # Es gibt genau EINE Tag-Datei; alte, versionierte Namen duerfen nicht
    # mehr als Quelle auftauchen.
    assert not re.findall(r"tags\.v\d+\.json", text)


def test_no_stale_tag_versions_in_docs() -> None:
    """Nirgends darf noch ein versionierter Tag-Dateiname stehen."""
    for rel in ("README.md", "Makefile", "rust/bubatz/src/extract.rs", "viewer/about.md"):
        found = set(re.findall(r"tags\.v\d+\.json", (ROOT / rel).read_text(encoding="utf-8")))
        assert not found, (rel, found)
