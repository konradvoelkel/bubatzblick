# Mitarbeit

Fehler in den Kartendaten gehören **nach OpenStreetMap**, nicht hierher: fehlt
eine Kita oder ist ein Schulgelände falsch abgegrenzt, dann wird das in OSM
korrigiert und ist nach einem Tageszyklus auf der Karte. Dieses Repository
enthält nur die Berechnung.

Sicherheitslücken nicht als Issue, sondern per E-Mail — siehe `SECURITY.md`.

## Aufsetzen

```bash
make dev      # uv sync, pre-push-Hook einhängen
make core     # Binary bauen + PyO3-Modul bubatzcore (Testorakel)
make test     # cargo test --workspace + pytest
make lint     # ruff format --check, ruff check, mypy
make audit    # cargo deny + pip-audit (braucht `cargo install cargo-deny`)
```

Gebraucht werden Rust (Version aus `rust/rust-toolchain.toml`, rustup holt sie
selbst), [uv](https://docs.astral.sh/uv/) für Python 3.13, dazu `osmium-tool`,
[tippecanoe](https://github.com/felt/tippecanoe) und
[pmtiles](https://github.com/protomaps/go-pmtiles) für die Pipeline-Stufen.

`make core` ist keine Nebensache: die Python-Tests prüfen den Rust-Kern gegen
eine unabhängige shapely-Referenz und brauchen dafür das gebaute Modul.

## Vor dem Push

`make dev` setzt `core.hooksPath` auf `.githooks`; der `pre-push`-Hook fährt
dasselbe wie die CI (Rust fmt/clippy/Tests über den ganzen Workspace, dann
Python-Lint und -Tests). Er dauert ein bis zwei Minuten.
`git push --no-verify` überspringt ihn — dann prüft die CI.

Zusätzlich läuft `make check-publishable`: eine Mustersuche über die
versionierten Dateien, die Entwicklungsspuren abfängt — Datumsangaben in
Kommentaren, Verweise auf Dokumente, die es hier nicht gibt, Pfade aus
Arbeitsumgebungen. Ein Treffer ist kein Vorwurf, sondern eine Frage: entweder
die Stelle umschreiben oder sie in `scripts/check_publishable.py` unter
`ERLAUBT` eintragen, mit Begründung.

## Was wir von Kommentaren erwarten

Ein Kommentar nennt die Invariante und den Grund, nicht die Geschichte. Also
„der Deckel ist so hoch, dass ihn kein bekanntes Objekt erreicht (größtes
national: 13 185 m Umfang)" und nicht „am Dienstag ist uns aufgefallen, dass
…". Kein Datum, keine Sitzungsnummer, kein Verweis auf eine Notiz, die
außerhalb des Repositories liegt. `make check-publishable` erzwingt das.

Die Begründungen mit Zahlen sind ausdrücklich erwünscht — sie sind der Grund,
warum die Schwellen in `config/pipeline.toml` so stehen, wie sie stehen.

## Konventionen

- **Sprache:** Kommentare, Meldungen und Dokumentation sind deutsch. Die
  Rechtsgrundlage ist deutsch, die Zielgruppe ebenso.
- **Geometrie:** Was die Zonengeometrie berührt, berührt `PARAM_VERSION`
  (`rust/bubatz/src/config.rs`). Ohne Mitziehen übernimmt der nächste Lauf
  Bänder aus einem anderen Verfahren. Dasselbe gilt für `SIMPLIFY_VERSION`
  (Kachelinhalt) und `CELL_FP_VERSION` (Spalten einer Zonen-Zelle).
- **Versionsnummer:** SemVer, Quelle ist `version` in `rust/Cargo.toml`;
  `pyproject.toml`, `CITATION.cff` und der oberste CHANGELOG-Eintrag müssen
  gleich lauten, die Lockfiles auch (`cargo update -w`, `uv lock`);
  `tests/test_version.py` prüft das. Patch für Korrekturen, Minor für Neues
  (eine weitere Ebene, ein Report-Feld), Major, wenn sich Verfahren oder
  Kachelvertrag ändern. Jede veröffentlichte Fassung bekommt ein Tag `v<Nummer>`.
  Die Cache-Generationen (`PARAM_VERSION` usw.) sind davon unabhängig.
- **Konservativ bleiben:** Die Zone darf wachsen, nie schrumpfen. Die
  Verträge in `rust/bubatz/src/contract_tests.rs` und
  `rust/bubatz/src/simplify.rs` halten das fest; wer sie ändert, ändert eine
  Zusage an die Nutzer.
- **`viewer/about.html` nicht von Hand ändern** — es wird aus `viewer/about.md`
  erzeugt (`make about`), und die CI prüft, dass ein Neubau kein Diff ergibt.
- **`viewer/.well-known/security.txt`** trägt nach RFC 9116 ein Ablaufdatum.
  Läuft es ab, gilt die Datei als ungültig — einmal im Jahr `Expires`
  hochsetzen.
- **`viewer/robots.txt`** nennt nur das Präfix der unverlinkten Betriebsseite,
  nie ihren Pfad: die Datei ist öffentlich. Sie schützt auch nichts, sie bittet
  nur — der Schutz ist der unratbare Pfad.
- **`viewer/favicon.ico`** ist der Rückfall für Clients, die die deklarierten
  `img/favicon.svg`, `img/favicon-32.png` und `img/apple-touch-icon.png` nicht
  auswerten und stattdessen den alten Wurzelpfad anfragen. Erzeugt aus dem
  32er-PNG, zwei Größen in einer Datei:
  `convert viewer/img/favicon-32.png -define icon:auto-resize=32,16 viewer/favicon.ico`
- **Örtliche Verbote** (lila Ebene) stehen in `config/sonderzonen.toml`: nur
  Rechtsakte mit Fläche und Laufzeit von Monaten oder Jahren, mit Quelle, Karte
  und Ablaufdatum; Volksfest-Verfügungen über Tage nicht. Nach jeder Änderung
  `uv run python scripts/sonderzonen.py` (holt die Straßen aus Overpass) und
  die erzeugte `viewer/sonderzonen.json` mit einchecken; `tests/test_sonderzonen.py`
  prüft Geometrie, Pflichtfelder und Daten.
- **Tag-Entscheidungen** stehen in `config/tags.json`, auch die Begründung: die
  Kommentare dort erscheinen wortgleich auf der Über-Seite. Eine Änderung dort
  ist eine Aussage über Recht, nicht über Code.
