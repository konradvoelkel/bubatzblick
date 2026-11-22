"""Der pre-push-Hook faehrt jeden Befehl des CI-Jobs `tests`.

Fehlt dort einer, geht der Push durch und erst die CI wird rot -- so geschehen
mit `make check-publishable`, das der Hook lange nicht kannte. Flags, die nur die
Ausgabe aendern, zaehlen nicht; Befehle, die lokal bewusst fehlen, stehen mit
Grund in NUR_CI.
"""

from __future__ import annotations

import pathlib
import re
import shlex

ROOT = pathlib.Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/tests.yml"
HOOK = ROOT / ".githooks/pre-push"

# Flags ohne Einfluss auf das Ergebnis (Ausgabe, Abbruch beim ersten Fehler).
NUR_AUSGABE = {"-q", "-s", "--no-fail-fast"}
NUR_CI = {
    # Der Hook laeuft in der Entwicklungsumgebung von `make dev`; ein Sync dort
    # wuerde lokale Aenderungen an der Umgebung bei jedem Push ueberschreiben.
    ("uv", "sync", "--frozen"),
}


def _befehle(text: str) -> set[tuple[str, ...]]:
    """Einfache Befehle aus Shell-Text: an &&, ;, | und Klammern getrennt."""
    out: set[tuple[str, ...]] = set()
    for zeile in text.splitlines():
        zeile = zeile.strip()
        if not zeile or zeile.startswith("#"):
            continue
        for teil in re.split(r"&&|;|\||\(|\)", zeile):
            try:
                woerter = [w for w in shlex.split(teil) if w not in NUR_AUSGABE]
            except ValueError:
                continue
            # Umleitungen gehoeren nicht zum Befehl.
            woerter = [w for w in woerter if not re.fullmatch(r"\d?>.*|>/dev/null", w)]
            if woerter and woerter[0] != "cd":
                out.add(tuple(woerter))
    return out


def _ci_befehle() -> set[tuple[str, ...]]:
    text = WORKFLOW.read_text(encoding="utf-8")
    job = re.search(r"^  tests:\n(.*?)(?=^  [a-z][\w-]*:\n|\Z)", text, re.S | re.M)
    assert job, "Job tests nicht gefunden"
    shell: list[str] = []
    zeilen = job.group(1).splitlines()
    i = 0
    while i < len(zeilen):
        m = re.match(r"^(\s+)run:\s*(.*)$", zeilen[i])
        if m and m.group(2) not in ("|", ">"):
            shell.append(m.group(2))
        elif m:
            tiefe = len(m.group(1))
            i += 1
            while i < len(zeilen) and (
                not zeilen[i].strip() or len(zeilen[i]) - len(zeilen[i].lstrip()) > tiefe
            ):
                shell.append(zeilen[i])
                i += 1
            continue
        i += 1
    return _befehle("\n".join(shell))


def test_ci_job_hat_befehle() -> None:
    assert ("make", "check-publishable") in _ci_befehle()


def test_hook_faehrt_jeden_ci_befehl() -> None:
    hook = _befehle(HOOK.read_text(encoding="utf-8"))
    fehlt = sorted(" ".join(b) for b in _ci_befehle() - NUR_CI - hook)
    assert not fehlt, f"im pre-push-Hook fehlt: {fehlt}"
