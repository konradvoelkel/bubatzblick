# Änderungen

Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
Versionierung nach [SemVer](https://semver.org/lang/de/).

Hier steht, was sich an der Software ändert. Dass die Karte täglich neue
OpenStreetMap-Daten bekommt, ist kein Eintrag — der Datenstand steht auf der
Karte selbst.

## [Unveröffentlicht]

### Hinzugefügt

- `sitemap.xml` mit den beiden Seiten der Karte, dazu der Verweis darauf in
  `robots.txt`. Ein Test hält die Liste, die Seiten und `robots.txt` zusammen.

### Geändert

- Bedienfeld und Datenschutzabschnitt sagen, dass „Mein Standort“ die Position
  nur im Browser abfragt und die Seite sie nicht sendet.
- Karte und Über-Seite tragen je einen eigenen Titel und eine eigene
  Beschreibung, beide kurz genug für ein Suchergebnis; bisher stand auf beiden
  Seiten dieselbe Beschreibung, und der Titel der Karte nannte nur den Namen.
  Die Einleitung im Bedienfeld sagt jetzt „Cannabis-Konsumverbot“ und nennt den
  Unterschied zu 100-m-Kreisen. Die Über-Seite spricht durchgehend von
  BubatzBlick und grenzt sich gleich oben von der Bubatzkarte von 2023 ab.
- Abhängigkeiten: toml 1.2.1, ruff 0.17.5, hypothesis 6.170.15,
  pydantic-settings 2.16.3, types-geopandas. Zonengeometrie und Kacheln
  bleiben gleich (Düsseldorf-Zonen byte-identisch, `zonen.pmtiles`
  sha256-gleich).
- Rust 1.100 statt 1.97 (`rust/rust-toolchain.toml` und `rust-version`).
  Zonengeometrie und Kacheln bleiben gleich: die Düsseldorf-Zonen aus beiden
  Binaries sind byte-identisch (`bubatz verify identical`, 2 715 Bänder),
  `zonen.pmtiles` ist sha256-gleich. `rust-version` gilt jetzt für jedes
  Crate; bisher erbte es keines, und cargo 1.100 warnt davor. Deshalb baut
  maturin die Python-Bindung aus `rust/` heraus (Makefile, pre-push-Hook,
  CI): nur dort gilt die gepinnte Toolchain, sonst nahm rustup die
  Standard-Toolchain.

### Behoben

- Start der Karte: Die Prüfabfragen an den Kachelspeicher hatten keine Frist;
  hing eine Antwort, blieb „Karte lädt …“ ohne Meldung stehen. Jetzt nach 10 s
  eine Meldung, ebenso wenn Kartendaten oder Grundkarte fehlen; der
  OSM-Rasterersatz greift nur noch in der lokalen Vorschau. Die drei Abfragen
  laufen gleichzeitig, und Suche, Standort und Teilen warten nicht mehr auf die
  örtlichen Verbote. Enter in der Suche lud vor dem Kartenaufbau die Seite neu.
- Bedienung mit Tastatur und Hilfstechnik (Prüfung gegen Googles „Modern Web
  Guidance“): Popups schließen mit Esc und geben den Fokus an die Karte zurück;
  Karte und Kartenknöpfe zeigen einen sichtbaren Fokus, auch bei erzwungenen
  Farben; Suchergebnisse und „Adresse kopiert“ werden angesagt, nach der
  Trefferwahl bleibt der Fokus im Suchfeld; eine IME-Eingabe löst keine Suche
  mehr aus; die letzte aktive Kategorie bleibt erreichbar und sagt, warum sie
  sich nicht abwählen lässt; Kategorien als Gruppe, Seite mit `main` und
  `search`.
- Darstellung: Schriftgrößen in rem und die Umbruchgrenze in em, damit die
  Schriftgröße des Browsers wirkt; Ränder von Suchfeld und Farbfeldern mit
  genug Kontrast; die Farbfelder der Legende behalten bei erzwungenen Farben
  ihre Farbe; die Kategoriezeilen stehen wie gedacht (eine allgemeinere
  CSS-Regel hatte sie überschrieben); der Standortpunkt pulsiert nicht, wenn
  weniger Bewegung eingestellt ist.
- Der Link „Über diese Karte“ trug den Kartenausschnitt in der Abfragezeile
  (`about.html?karte=…`). Damit stand er im Zugriffsprotokoll des Servers,
  nach „Mein Standort“ also die eigene Position, entgegen der Zusage auf der
  Über-Seite. Jetzt reist er im Fragment (`about.html#karte=…`), das der
  Browser nicht an den Server schickt.
- `deploy/bin/check_access_log.sh` zeigte mit seinen Vorgaben auf alte Pfade
  und prüfte die Aufbewahrungsfrist in der Datei im Repo statt in der Kopie,
  die der Server ausführt. Die Betriebsskripte nennen die beiden Einheiten
  der Zugriffsverdichtung jetzt richtig.

## [2.1.0] - 2026-11-17

Eine weitere Ebene, Pflege der Abhängigkeiten und eine Fehlermeldung, wo die
Seite bisher schwieg. Verfahren, Zonengeometrie und Kachelvertrag sind
gegenüber 2.0.0 unverändert.

### Hinzugefügt

- Ebene „Örtliche Verbote“ (lila): kommunale Konsumverbote außerhalb von
  § 5 KCanG mit Fläche und Laufzeit, von Hand gepflegt in
  `config/sonderzonen.toml`, gezeichnet von `scripts/sonderzonen.py` aus den im
  Rechtsakt genannten Straßen (Overpass) nach `viewer/sonderzonen.json`. Zum
  Start: Nürnberg Hauptbahnhof (CanVVO, Art. 30 LStVG) und München Alter
  Botanischer Garten (ACVV). Der Viewer zeigt nur, was am heutigen Tag gilt.
  Auf der Karte seit dem 13. Oktober 2026.

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
- Abhängigkeiten: pmtiles 0.28.1, arrow und parquet 60.1.0, toml 1.2.0,
  flate2 1.1.11, pyarrow 26.0.1, hypothesis 6.170.14, pydantic-settings
  2.16.2, ruff 0.17.4, types-geopandas. pmtiles liest lokale Archive seit 0.27
  mit positionierten Lesezugriffen statt über ein Speicherabbild;
  `bubatz serve` bleibt beim Speicherabbild (`new_with_mmap_path`), sein
  Verhalten ändert sich nicht. Zonengeometrie und Kacheln bleiben gleich:
  Düsseldorf-Zonen aus altem und neuem Lock sind byte-identisch
  (`bubatz verify identical`, 2 715 Bänder), das daraus gebaute
  `zonen.pmtiles` ist sha256-gleich. Die Zellendateien unterscheiden sich
  nur im Fuß der Parquet-Datei (Versionsstempel der Bibliothek), ihr Inhalt
  ist gleich.
- Über-Seite und README nennen die Zahlen des Laufs vom 17. November 2026
  (496 217 Einrichtungen, 39,6 Millionen Gebäude, Archiv 372 MB) statt dreier
  verschiedener Stände; die Zonenfläche steht als runder Wert da. Der Absatz
  zur Hindernissuche beschreibt den Gebäudespeicher auf der Platte statt des
  früheren R-Baums.

### Behoben

- Ohne WebGL blieb die Karte stumm bei „Karte lädt …“ stehen, der Fehler stand
  nur in der Konsole. Jetzt sagt die Seite, woran es liegt und was hilft; dasselbe
  gilt für jeden anderen Abbruch beim Start und für abgeschaltetes JavaScript.

## [2.0.0] - 2026-10-13

Erste Veröffentlichung. Was sie kann, steht in `README.md`.

Die Zählung beginnt bei 2: die 1 war die Python-Fassung des Verfahrens aus
der Bachelorarbeit, die nie veröffentlicht wurde; 2.0.0 ist der Rust-Port.
Die Nummer steht in `rust/Cargo.toml`; `bubatz --version`, `stand.json`
(`pipeline_version`) und die Fußzeile der Über-Seite zeigen sie.
