"""Was unter viewer/ liegt, geht auf die Seite (Server-Hook: der gepushte Stand).

Ein versehentlich eingechecktes Arbeitsdokument -- Notiz, Entwurf, Export,
Sicherungskopie -- waere damit oeffentlich. Die Deploy-Wege kennen nur eine
Sperrliste (about.md, Punktdateien); dieser Test ist die Erlaubnisliste.
Eine neue Dateiart unter viewer/ heisst: hier bewusst eintragen.
"""

from __future__ import annotations

import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]

ENDUNGEN = {".html", ".js", ".mjs", ".json", ".png", ".svg", ".ico", ".pdf", ".txt", ".xml"}
# Einzelne Dateien mit anderer Endung, jede mit Grund.
EINZELN = {
    "viewer/about.md",  # Quelle von about.html; der Deploy schliesst sie aus
}


def _getrackt() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "viewer"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout
    return [z for z in out.splitlines() if not z.startswith("viewer/vendor/")]


def test_nur_erlaubte_dateiarten_unter_viewer() -> None:
    fremd = [p for p in _getrackt() if pathlib.Path(p).suffix not in ENDUNGEN and p not in EINZELN]
    assert not fremd, f"unerwartete Dateien unter viewer/ (Arbeitsdokument?): {fremd}"


def test_keine_sicherungs_oder_entwurfsnamen() -> None:
    verdaechtig = (
        "entwurf",
        "draft",
        "notiz",
        "todo",
        "kopie",
        "copy",
        "backup",
        "alt.",
        ".bak",
        "~",
    )
    treffer = [p for p in _getrackt() if any(v in p.lower() for v in verdaechtig)]
    assert not treffer, f"Dateiname klingt nach Arbeitsdokument: {treffer}"
