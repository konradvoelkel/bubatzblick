# Änderungen

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
Versionierung nach [SemVer](https://semver.org/lang/de/).

Hier steht, was sich an der Software ändert. Dass die Karte täglich neue
OpenStreetMap-Daten bekommt, ist kein Eintrag — der Datenstand steht auf der
Karte selbst.

## [Unveröffentlicht]

### Hinzugefügt

- Ebene „Örtliche Verbote“ (lila): kommunale Konsumverbote außerhalb von
  § 5 KCanG mit Fläche und Laufzeit, von Hand gepflegt in
  `config/sonderzonen.toml`, gezeichnet von `scripts/sonderzonen.py` aus den im
  Rechtsakt genannten Straßen (Overpass) nach `viewer/sonderzonen.json`. Zum
  Start: Nürnberg Hauptbahnhof (CanVVO, Art. 30 LStVG) und München Alter
  Botanischer Garten (ACVV). Der Viewer zeigt nur, was am heutigen Tag gilt.

### Geändert

- Abhängigkeiten: i_overlay 9.0.1, shapely 2.2.0, pyarrow 26.0.0, pyproj 3.8.0,
  ruff 0.17.0, hypothesis, types-geopandas, toml-Crates, setup-uv 10.3.0. Die
  Zonengeometrie bleibt gleich: die Düsseldorf-Zonen aus i_overlay 9.0.0 und
  9.0.1 sind byte-identisch (`bubatz verify identical`, 2 715 Bänder).

## [2.0.0] - 2026-10-13

Erste Veröffentlichung. Was sie kann, steht in `README.md`.

Die Zählung beginnt bei 2: die 1 war die Python-Fassung des Verfahrens aus
der Bachelorarbeit, die nie veröffentlicht wurde; 2.0.0 ist der Rust-Port.
Die Nummer steht in `rust/Cargo.toml`; `bubatz --version`, `stand.json`
(`pipeline_version`) und die Fußzeile der Über-Seite zeigen sie.
