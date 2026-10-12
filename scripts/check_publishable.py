#!/usr/bin/env python3
"""Prueft die versionierten Dateien auf Spuren, die nicht veroeffentlicht werden
sollen: Datumsangaben aus der Entwicklung, Verweise auf interne Dokumente,
Pfade und Namen aus Arbeitsumgebungen, Server- und Zonenkennungen.

    scripts/check_publishable.py          # Repo-Wurzel
    make check-publishable

Exit 0 = sauber, 1 = Treffer (werden mit Datei, Zeile und Muster gemeldet).

Das Muster-Verzeichnis ist Absicht und keine Schikane: Kommentare, die eine
Entwicklungsgeschichte erzaehlen, sind beim naechsten Mal genauso schnell
wieder da wie beim letzten. Wer einen Treffer bewusst stehen lassen will,
traegt ihn unter ERLAUBT ein -- mit Begruendung.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

# (Name, Muster) -- der Name steht im Bericht, also sprechend halten.
MUSTER: list[tuple[str, str]] = [
    ("Datum (TT.MM.JJJJ)", r"\b\d{1,2}\.\d{2}\.20\d{2}\b"),
    ("Datum (TT.MM.)", r"(?<![\d.])\d{1,2}\.\d{2}\.(?!\d)"),
    ("Sitzungsnummer", r"\bSitzung\s+\d"),
    ("Phasenmarke", r"\bP\d\.\d|\bP\d Phase|\bPhase \d\b"),
    (
        "internes Dokument",
        r"PROGRESS|RUSTPORT|ROLLOUT\.md|UPDATE\.md@|NOTES\.md|DEPLOY\.md|BRANDING|PLAYBOOK|BACHELORARBEIT",
    ),
    ("Arbeitsverzeichnis des Agenten", r"claude/"),
    ("Commit-Kennung im Verweis", r"@eefd123"),
    ("UX-Lauf mit Personen", r"UX-Lauf"),
    ("abgeloeste Engine", r"\bNumba\b|\bnumba\b"),
    ("Pfad einer Arbeitsumgebung", r"/media/[a-z]+/|xtraspace|/home/[a-z]+/"),
    ("Serveradresse", r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b"),
    ("Cloudflare-Zonenkennung", r"\b[0-9a-f]{32}\b"),
]

# Pfade, die gar nicht geprueft werden (Fremdcode, Lizenztexte, Sperrdateien).
AUS = (
    "viewer/vendor/",
    "LICENSE",
    "uv.lock",
    "rust/Cargo.lock",
    # Enthaelt die Muster selbst.
    "scripts/check_publishable.py",
)
AUS_ENDUNGEN = (".pbf", ".png", ".svg", ".parquet", ".gz", ".osm", ".geojson", ".ico")

# Begruendete Ausnahmen: (Datei, Mustername) -> warum.
ERLAUBT: dict[tuple[str, str], str] = {
    # Gleitkomma-Bereiche, keine Datumsangaben.
    (
        "rust/bubatz/tests/pedestrian_tiny.rs",
        "Datum (TT.MM.)",
    ): "Koordinatenbereiche wie 6.79..6.792",
    ("rust/bubatzcore/tests/ogc_repro.rs", "Datum (TT.MM.)"): "Versionsnummern von i_overlay",
    # Lokale Vorschau, keine Serveradresse.
    ("Makefile", "Serveradresse"): "127.0.0.1 als Bind-Adresse der Vorschau",
    ("README.md", "Serveradresse"): "127.0.0.1 als Bind-Adresse der Vorschau",
    ("rust/bubatz/src/serve.rs", "Serveradresse"): "127.0.0.1 als Bind-Adresse der Vorschau",
    ("viewer/app.js", "Serveradresse"): "Hinweis auf localhost in der Standortfreigabe",
}


def tracked(root: Path) -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [
        f
        for f in out.split("\0")
        if f and not f.startswith(AUS) and not f.endswith(AUS_ENDUNGEN) and f not in AUS
    ]


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    kompiliert = [(name, re.compile(pat)) for name, pat in MUSTER]
    treffer: list[str] = []
    for rel in tracked(root):
        try:
            text = (root / rel).read_text(encoding="utf-8")
        except (UnicodeDecodeError, FileNotFoundError, IsADirectoryError):
            continue
        for nr, zeile in enumerate(text.splitlines(), 1):
            for name, rx in kompiliert:
                if (rel, name) in ERLAUBT:
                    continue
                if m := rx.search(zeile):
                    treffer.append(f"{rel}:{nr}: {name}: {m.group(0)!r}  |  {zeile.strip()[:90]}")
    if treffer:
        print(f"check-publishable: {len(treffer)} Treffer\n")
        print("\n".join(treffer))
        print(
            "\nEntweder die Stelle aendern oder sie in scripts/check_publishable.py"
            " unter ERLAUBT eintragen -- mit Begruendung."
        )
        return 1
    print(f"check-publishable: sauber ({len(tracked(root))} Dateien)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
