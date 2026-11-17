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
- Abhängigkeiten nach den Sicherheitsmeldungen vom Oktober 2026: certifi
  2026.10.23 und webpki-roots 1.0.11 (ohne die emSign-Wurzeln, deren
  Zwischenzertifikate beim eMudhra-Vorfall kompromittiert wurden),
  rustls-webpki 0.103.16 (misstraut diesen Zwischenzertifikaten), rustls
  0.23.46, arrow-data 60.1.0 (mit arrow-buffer und arrow-schema), ctutils
  0.4.8, zstd 0.14.3 (mit zstd-safe 8.2.0 und zstd-sys 2.1.2, Parquet-
  Kompression). Betroffen war der tägliche PBF-Download über HTTPS. Die
  Zonengeometrie bleibt gleich: Düsseldorf-Zonen aus altem und neuem Lock
  sind byte-identisch (`bubatz verify identical`, 2 715 Bänder).
- Abhängigkeiten: i_overlay 9.1.0, pmtiles 0.25.0, serde_json 1.0.154,
  proptest 1.12.0, hypothesis 6.170.0, pydantic-settings 2.16.0, ruff 0.17.1,
  numpy 2.5.4, types-geopandas, setup-uv 10.3.3, rust-cache 2.9.3; dazu tokio
  1.54.1 (RUSTSEC-2026-0386, `AsyncFd`-Konstruktoren). pmtiles 0.25 hat
  `add_raw_tile` gestrichen, betroffen war nur ein Testhelfer. Zonengeometrie
  und Kacheln bleiben gleich: Düsseldorf-Zonen aus altem und neuem Lock sind
  byte-identisch (`bubatz verify identical`, 2 715 Bänder), das daraus gebaute
  `zonen.pmtiles` ist sha256-gleich.
- Tests laufen auf Ubuntu 26.04 (glibc 2.43), im Workflow als Fassung genannt
  statt `ubuntu-latest`. Die neue glibc rundet `asinh` korrekt; dadurch
  verschieben sich 731 von 9 998 Referenzpunkten der Projektion im Rechtswert
  um höchstens 2 ULP (2,3e-10 m). Der Code ist unverändert, die neuen Bits
  sind der Maßstab: `tests/data/proj_points.json` trägt jetzt pyproj unter
  glibc 2.43 und daneben die bisherige Generation (glibc 2.39 bis 2.41), der
  Test verlangt höchstens 10 nm Abstand zu pyproj und 90 % Bitgleichheit mit
  einer der Generationen. Der Extrakt-Test (`tests/extract_tiny.rs`)
  vergleicht das Gerüst der WKB exakt und jede Koordinate bis 10 nm statt
  byte-gleich. Neu: `scripts/proj_reference.py` erzeugt und prüft die
  Referenz. Die Karte ändert sich dadurch nicht.

## [2.0.0] - 2026-10-13

Erste Veröffentlichung. Was sie kann, steht in `README.md`.

Die Zählung beginnt bei 2: die 1 war die Python-Fassung des Verfahrens aus
der Bachelorarbeit, die nie veröffentlicht wurde; 2.0.0 ist der Rust-Port.
Die Nummer steht in `rust/Cargo.toml`; `bubatz --version`, `stand.json`
(`pipeline_version`) und die Fußzeile der Über-Seite zeigen sie.
