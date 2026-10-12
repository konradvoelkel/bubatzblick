"""Eine Versionsnummer, vier Stellen: rust/Cargo.toml ist die Quelle,
pyproject.toml, CITATION.cff und der oberste CHANGELOG-Eintrag folgen ihr.
Die Lockfiles tragen sie ebenfalls (cargo update -w, uv lock), sonst scheitert
`--locked` bzw. `--locked` in der CI.
"""

from __future__ import annotations

import pathlib
import re
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
SEMVER = re.compile(r"^\d+\.\d+\.\d+$")


def cargo_version() -> str:
    with (ROOT / "rust/Cargo.toml").open("rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def test_cargo_version_is_semver() -> None:
    assert SEMVER.match(cargo_version())


def test_pyproject_matches() -> None:
    with (ROOT / "pyproject.toml").open("rb") as f:
        assert tomllib.load(f)["project"]["version"] == cargo_version()


def test_citation_matches() -> None:
    text = (ROOT / "CITATION.cff").read_text(encoding="utf-8")
    m = re.search(r"^version: (\S+)$", text, re.M)
    assert m and m.group(1) == cargo_version()
    assert re.search(r'^date-released: "\d{4}-\d{2}-\d{2}"$', text, re.M)


def test_changelog_top_entry_matches() -> None:
    text = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    entries = re.findall(r"^## \[(\d+\.\d+\.\d+)\]", text, re.M)
    assert entries, "kein versionierter Eintrag im CHANGELOG"
    assert entries[0] == cargo_version()


def test_lockfiles_match() -> None:
    cargo_lock = (ROOT / "rust/Cargo.lock").read_text(encoding="utf-8")
    for crate in ("bubatz", "bubatzcore", "bubatzcore-py"):
        m = re.search(rf'^name = "{crate}"\nversion = "([^"]+)"', cargo_lock, re.M)
        assert m and m.group(1) == cargo_version(), crate
    uv_lock = (ROOT / "uv.lock").read_text(encoding="utf-8")
    m = re.search(r'^name = "bubatzblick"\nversion = "([^"]+)"', uv_lock, re.M)
    assert m and m.group(1) == cargo_version()
