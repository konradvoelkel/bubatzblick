# BubatzBlick

Konservativ (überschätzend) berechnete Cannabis-Verbotszonen nach § 5 KCanG für
ganz Deutschland, aus OpenStreetMap — [bubatzblick.de](https://bubatzblick.de).
Ein Rust-Binary (`bubatz`) rechnet die Pipeline, das Ergebnis ist eine statische
Karte (`zonen.pmtiles` + MapLibre) mit täglicher Aktualisierung.

Lizenz AGPL-3.0-or-later (`LICENSE`). Kartendaten © OpenStreetMap-Mitwirkende,
ODbL. Sicherheitslücken per E-Mail, nicht als Issue (`SECURITY.md`).
Mitarbeit: `CONTRIBUTING.md`.

## Verfahren

- **Verbotsobjekte** nach `config/tags.json`: Schulen, Spielplätze, Kinder- und
  Jugendeinrichtungen, öffentliche Sportstätten, Anbauvereinigungen. Diese
  Datei ist die einzige Quelle — Pipeline, Über-Seite und Tests lesen sie.
- **Sichtweite** 100 m (§ 5 Abs. 2 KCanG), gerechnet bis 102 m
  (2 m Datenqualitätsmarge).
- **Sichtfeld:** Beobachterpunkte alle 0,5 m auf der Objektkontur, je Punkt ein
  exaktes Sichtbarkeitspolygon gegen die OSM-Gebäude (Rotationssweep in
  `rust/bubatzcore`), Vereinigung, 0,5 m Marge nach außen. Das Verfahren
  folgt der Bachelorarbeit von Oliver Kleisch, *2D-Sichtbarkeit anhand von
  Gebäudekonturen aus Openstreetmaps* (Heinrich-Heine-Universität Düsseldorf,
  2025; [PDF](https://bubatzblick.de/kleisch-2025-2d-sichtbarkeit-osm.pdf)).
- **Konservativ:** Die Zone enthält Quelle und freien Kreis, deckt eine 4×
  dichtere Abtastung ab, und mehr Hindernisse verkleinern sie nur. Das sind
  Tests (`rust/bubatz/src/contract_tests.rs`) gegen eingefrorene Objekte.
- **Fußgängerzonen** (§ 5 Abs. 2 Nr. 5) als eigene Ebene, ohne Puffer: dort
  gilt das Verbot nur zwischen 7 und 20 Uhr.
- **Karte:** z8–z14, je Zoomstufe nur nach außen vereinfacht; in z14 sind die
  Gebäudegrundrisse abgezogen.

Größenordnung Deutschland: ~500 000 Verbotsobjekte, 39 Mio. Gebäude,
~19 200 km² Zonenfläche, `zonen.pmtiles` ~365 MB.

Stufen, Kachelvertrag und Prüfkette: **[`docs/pipeline.md`](docs/pipeline.md)**.

## Voraussetzungen

Rust (Version aus `rust/rust-toolchain.toml`, rustup holt sie selbst),
[uv](https://docs.astral.sh/uv/) für Python 3.13 (nur Tests), `osmium-tool`,
[tippecanoe](https://github.com/felt/tippecanoe),
[pmtiles](https://github.com/protomaps/go-pmtiles).
Bundeslauf: ~36 GB Platte, 8 GB RAM.

## Schnellstart

```bash
make dev              # uv sync + pre-push-Hook
make core             # Binary + PyO3-Modul bubatzcore (Testorakel)
make test             # cargo test --workspace + pytest
make duesseldorf      # laden -> extract -> clip -> zones -> tiles (Minuten)
make nrw              # ~25 min auf 16 Threads
make germany          # ~2 h auf 16 Threads
make viewer           # Vorschau im Browser
```

Daten unter `data/{raw,interim,out}` (ignoriert); `RAW_DIR=… INTERIM_DIR=…
OUT_DIR=…` für ein anderes Volume, `N_JOBS=4` für die Parallelität.

## Betrieb

```bash
make update-pbf && make germany && make publish
```

`germany` rechnet nur Geändertes: `extract`/`clip` komplett, `zones` übernimmt
nahezu alles aus dem Vorlauf, `tiles` nur geänderte Blöcke — rund 10 min auf
16 Threads. `make publish` tauscht das Archiv atomar und nur nach bestandener
Prüfkette (`docs/pipeline.md`).

## Karte

`viewer/index.html`: statisch, MapLibre, Bibliotheken unter `viewer/vendor/`.
Grundkarte Protomaps aus eigener `basemap.pmtiles`, Zonen aus `zonen.pmtiles`,
beides per HTTP-Range. Pfade in `viewer/config.js` (setzt der Deploy).
Alternativ `?pmtiles=<URL>`, `?tiles=<Vorlage>` oder `bubatz serve`.

Zwei zuschaltbare Ebenen neben dem Rot: Fußgängerzonen aus OpenStreetMap
(blau, 7–20 Uhr) und örtliche Verbote außerhalb des KCanG (lila) — kommunale
Verordnungen mit Fläche und Laufzeit, von Hand in `config/sonderzonen.toml`
gepflegt; `scripts/sonderzonen.py` zeichnet daraus `viewer/sonderzonen.json`
über die im Rechtsakt genannten Straßen und Plätze (Overpass).

Die Ortssuche im Bedienfeld geht über Nominatim — die einzige Anfrage an einen
fremden Server, nur beim Absenden, nie beim Tippen.

Lokal: `make viewer PMTILES_DIR=data/out/germany BASEMAP=/pfad/basemap.pmtiles`.
Ohne Grundkarte: OSM-Raster (nur Vorschau).

## Repository

```
rust/bubatzcore/      Geometriekern: Sweep, Vereinigung (i_overlay), Marge, Bänder
rust/bubatz/          Binary: alle Stufen, Vertrags- und Golden-Tests
rust/bubatzcore-py/   PyO3-Brücke für die Python-Gegenprobe
src/bubatzblick/      Python-Referenz des Sweeps (sampling_ref.py), nur Tests
tests/                pytest: Rust gegen shapely
config/               pipeline.toml, tags.json, sonderzonen.toml
scripts/              publish_germany.sh, Prüf- und Wartungsskripte
viewer/               index.html + vendor/ (MapLibre GL JS, pmtiles.js)
deploy/               Seite auf den Server, Tageszyklus, Bildgeneratoren
docs/                 Pipeline im Einzelnen
```

## Entwicklung

```bash
make lint                 # ruff format --check, ruff check, mypy
make test
make audit                # cargo deny + pip-audit, beide gegen die Lockfiles
make check-publishable    # keine Entwicklungsspuren im Baum
make about                # viewer/about.html aus about.md erzeugen
```

CI (`.github/workflows/tests.yml`) prüft denselben Satz über den ganzen
Workspace. `make audit` läuft dort als eigener Job: `cargo deny` für
Advisories, Lizenzverträglichkeit mit der AGPL und erlaubte Quellen
(`rust/deny.toml`), `pip-audit` für die Python-Seite. Beide prüfen die
gesperrte Auflösung, nicht die gerade installierte Umgebung.
Lint und Tests laufen lokal als `pre-push`-Hook (`make dev`;
`git push --no-verify` überspringt).

Kommentare, Meldungen und Dokumentation sind deutsch: die Rechtsgrundlage ist
deutsch und die Zielgruppe ebenso. Was beim Schreiben von Kommentaren erwartet
wird, steht in `CONTRIBUTING.md`.

## Datenquelle

`download.geofabrik.de` (`europe/germany-latest.osm.pbf`, Tagesdiffs unter
`europe/germany-updates/`), Ausweich `download.openstreetmap.fr/extracts/…`.
Der Erst-Download wird gegen Geofabriks `.md5` geprüft.
