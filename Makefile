# cargo/maturin auch unter systemd-run (dort fehlt ~/.cargo/bin im PATH)
export PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: dev core test lint about check-publishable audit audit-python audit-rust duesseldorf nrw germany update-pbf publish viewer report clean-duesseldorf clean-nrw clean-germany

# bubatzblick -- Ziele: make dev | core | test | lint | about | check-publishable |
# audit | duesseldorf | nrw | germany | update-pbf | publish | viewer | report
# (README.md).

RAW_DIR := data/raw
INTERIM_DIR := data/interim
OUT_DIR := data/out
# Parallele Worker fuer `zones` und das Zellen-Vereinfachen in `tiles` (joblib);
# -1 = alle Kerne. Beim Aufruf ueberschreibbar: `make germany N_JOBS=8`.
# Immer als `--n-jobs=$(N_JOBS)` uebergeben: bei getrenntem Argument liest
# clap die -1 als Flag und bricht ab.
N_JOBS := -1
# Optional: Zonen-Verzeichnis eines frueheren Laufs fuer `zones --reuse-from`
# (inkrementelle Neuberechnung). Ohne Angabe wird ein
# vollstaendig durchgelaufener Vorlauf im SELBEN Interim-Verzeichnis
# automatisch nach `zones.prev` rotiert und wiederverwendet (siehe run_zones).
GERMANY_REUSE_ZONES ?=
DUESSELDORF_REUSE_ZONES ?=
# Alle Stufen rechnet das Binary `bubatz`. Python ist nur noch Testsuite und
# Orakel (tests/, scripts/rollout_check_*.py).
BUBATZ := rust/target/release/bubatz
ZONES_CMD = $(BUBATZ) zones
EXTRACT_CMD = $(BUBATZ) extract
CLIP_CMD = $(BUBATZ) clip
TILES_CMD = $(BUBATZ) tiles
# update-pbf laedt die Diffs selbst und wendet sie mit osmium-tool an
# (`osmium apply-changes`, apt install osmium-tool).
# Tagesdiffs von Geofabrik statt 1.440 Minutendiffs je Tag (Umstieg per
# Zeitstempel beim ersten Lauf); --diff-dir behaelt die angewandten Diffs fuer
# den Grenzcache, --keep-diffs begrenzt den Bestand.
# --size 500 MB je Aufruf (~10 Tagesdiffs; bubatz-daily.sh ruft bis zu 4x):
# osmium apply-changes haelt alle Diffs eines Aufrufs im Speicher, auf dem
# 8-GB-Server waeren 2 GB Diffs nach langem Stillstand zu viel.
# Gemessen: 1 bzw. 5 Tagesdiffs < 1 GB anonym.
GERMANY_REPLICATION_URL ?= https://download.geofabrik.de/europe/germany-updates
GERMANY_DIFF_DIR ?= $(RAW_DIR)/diffs
UPDATE_PBF_CMD = $(BUBATZ) update-pbf --size 500 $(if $(GERMANY_REPLICATION_URL),--replication-url $(GERMANY_REPLICATION_URL)) --diff-dir $(GERMANY_DIFF_DIR) -o $(GERMANY_PBF_NEW) $(GERMANY_PBF)

# Canned recipe fuer den zones-Schritt (Duesseldorf und Deutschland gleich):
#   $(call run_zones,<zones-dir>,<protected.parquet>,<buildings.parquet>,<reuse-dir oder leer>)
# Existiert bereits ein VOLLSTAENDIGER Vorlauf (Markerdatei .stamp) -- also
# laeuft zones nur erneut, weil die Eingaben (neuer OSM-Stand) juenger sind --
# wird er nach <zones-dir>.prev rotiert und per --reuse-from wiederverwendet:
# Objekte mit unveraendertem cache_key (Quelle + Hindernisse in Reichweite)
# uebernehmen ihre Baender ohne Neuberechnung. Ein UNVOLLSTAENDIGER Vorlauf
# (kein .stamp) bleibt liegen und wird per Zellen-Wiederaufnahme fortgesetzt;
# zones selbst weist eine Wiederaufnahme ueber geaenderten
# Eingaben per Manifest ab, statt alte Zellen still fuer fertig zu halten.
define run_zones
@if [ -f $(1)/.stamp ]; then echo "zones: vollstaendiger Vorlauf -> $(1).prev, Wiederverwendung per cache_key"; rm -rf $(1).prev; mv $(1) $(1).prev; fi
$(ZONES_CMD) --protected-path $(2) --buildings-path $(3) --out-dir $(1) --n-jobs=$(N_JOBS) $$( if [ -n "$(4)" ]; then echo "--reuse-from $(4)"; elif [ -f $(1).prev/.stamp ]; then echo "--reuse-from $(1).prev"; fi )
touch $(1)/.stamp
endef

# Tag-Spezifikation (Verbotsobjekte) und Pipeline-Konfiguration sind Eingaben:
# eine neue Version (tags.json, pipeline.toml) stoesst extract bzw. zones neu an.
TAGS_SPEC := config/tags.json
PIPELINE_CFG := config/pipeline.toml

REGBEZ_PBF := $(RAW_DIR)/duesseldorf-regbez-latest.osm.pbf
GEOFABRIK_URL := https://download.geofabrik.de/europe/germany/nordrhein-westfalen/duesseldorf-regbez-latest.osm.pbf
OSMFR_URL := https://download.openstreetmap.fr/extracts/europe/germany/nordrhein-westfalen/duesseldorf-regbez-latest.osm.pbf

# Relation 62539, boundary=administrative, admin_level=6, name=Duesseldorf.
# Die extrahierte Flaeche von 217.4075 km^2 stimmt mit dem amtlichen Wert
# 217,41 km^2 auf die vierte signifikante Stelle ueberein.
DUESSELDORF_RELATION_ID := 62539
DUESSELDORF_DIR := $(INTERIM_DIR)/duesseldorf

dev:
	uv sync
	git config core.hooksPath .githooks   # pre-push = CI lokal (.githooks/pre-push)

# Rust bauen: das Binary `bubatz` (Produktionspfad) und den Kern als
# Python-Modul `bubatzcore` (Tests, Orakel). Braucht Rust stable (rustup)
# und maturin (dev-Gruppe). `make test` prueft beides.
core:
	cd rust && cargo build --release -p bubatz
	uv run maturin develop --release -m rust/bubatzcore-py/Cargo.toml

$(BUBATZ):
	cd rust && cargo build --release -p bubatz

test:
	uv run pytest -n auto
	cd rust && cargo test --workspace

# Was CI/Review vor einem Commit erwartet: Formatierung, Lint, Typen.
# mypy auch ueber scripts/ und deploy/bin/: dort liegen Pruef- und
# Betriebsskripte, die im Tageszyklus laufen.
lint:
	uv run ruff format --check .
	uv run ruff check .
	uv run mypy src tests scripts deploy/bin

# viewer/about.html wird erzeugt, nicht von Hand gepflegt. Die CI prueft, dass
# ein Neubau kein Diff ergibt.
about:
	deploy/bin/build_about.py

# Keine Entwicklungsspuren im Baum (Datumsangaben, interne Verweise, Pfade).
check-publishable:
	uv run --no-sync python scripts/check_publishable.py

# Sicherheitsmeldungen, Lizenzen und Quellen beider Seiten. Beide Haelften
# pruefen die GESPERRTE Aufloesung (Cargo.lock bzw. uv.lock), nicht das, was
# gerade in der Umgebung liegt -- sonst sagt die Pruefung nichts ueber das,
# was auf dem Server landet.
audit: audit-rust audit-python

audit-rust:
	@command -v cargo-deny >/dev/null || { echo "cargo-deny fehlt: cargo install cargo-deny --locked"; exit 1; }
	cargo deny --manifest-path rust/Cargo.toml check

# `--no-emit-project` laesst das eigene Paket weg (es ist nicht auf PyPI),
# `--no-project` haelt pip-audit von den Projektabhaengigkeiten fern.
# Die Temporaerdatei entsteht IM Rezept: ein `$(shell mktemp)` auf Makefile-
# Ebene liefe bei jedem `make`-Aufruf, auch bei `make test`.
audit-python:
	@req=$$(mktemp -t bubatz-req-XXXX.txt); trap 'rm -f "$$req"' EXIT; \
	  uv export --format requirements-txt --no-hashes --no-emit-project > "$$req" && \
	  uv run --no-project --with pip-audit pip-audit -r "$$req"

# -- Duesseldorf: vertikaler Schnitt fuer EINE Stadt ------------------------

$(REGBEZ_PBF):
	mkdir -p $(RAW_DIR)
	curl -fL -o $@ $(GEOFABRIK_URL) || curl -fL -o $@ $(OSMFR_URL)

# Gruppiertes Ziel (Make 4.3+ `&:`, nicht das aeltere `a b: c` mit
# gemeinsamer Regel): EIN `extract`-Aufruf erzeugt BEIDE Dateien. Ohne `&:`
# ruft Make bei einem frischen Checkout `extract` ZWEIMAL auf, einmal "fuer"
# jedes Ziel -- in `make -n` nachpruefbar.
$(INTERIM_DIR)/protected.parquet $(INTERIM_DIR)/buildings.parquet &: $(REGBEZ_PBF) $(TAGS_SPEC) | $(BUBATZ)
	$(EXTRACT_CMD) $< --out-dir $(INTERIM_DIR)

# Der Regierungsbezirk-Extrakt deckt ~30.000 Objekte in einem viel groesseren
# Gebiet ab als die Stadt Duesseldorf; dieses Ziel ist der vertikale Schnitt
# fuer EINE Stadt und filtert auf die echte Stadtgrenze plus
# Pipeline-Reichweite als Marge. Technisch kann `zones` auch den ganzen
# Bezirk rechnen.
$(DUESSELDORF_DIR)/protected.parquet: $(INTERIM_DIR)/protected.parquet $(REGBEZ_PBF)
	$(CLIP_CMD) \
		--pbf $(REGBEZ_PBF) \
		--admin-relation-id $(DUESSELDORF_RELATION_ID) \
		--protected-in $(INTERIM_DIR)/protected.parquet \
		--out-dir $(DUESSELDORF_DIR)

# Der Zellendateiname (x{col}_y{row}.parquet) haengt vom Datenausschnitt ab
# und ist deshalb VORHER nicht bekannt: `zones` schreibt eine Datei je
# nichtleerer Gitterzelle. Fuer ein Ziel mit a priori unbekannter
# Ausgabedateimenge ist eine `.stamp`-Markerdatei der korrekte Make-Ansatz --
# ein geratener Dateiname waere schlicht falsch.
$(DUESSELDORF_DIR)/zones/.stamp: $(DUESSELDORF_DIR)/protected.parquet $(INTERIM_DIR)/buildings.parquet $(PIPELINE_CFG) | $(BUBATZ)
	$(call run_zones,$(DUESSELDORF_DIR)/zones,$(DUESSELDORF_DIR)/protected.parquet,$(INTERIM_DIR)/buildings.parquet,$(DUESSELDORF_REUSE_ZONES))

# Das Binary ist hier eine ECHTE Abhaengigkeit, nicht nur reihenfolge-only wie
# bei den Zonen: der Kachelinhalt haengt an SIMPLIFY_VERSION und am
# Zeilenformat von cell_digest, und beides sieht make sonst nicht. Fehlt die
# Abhaengigkeit, bleibt die Kachelstufe nach einer Aenderung am Binary ganz
# aus und `make germany` haelt ein veraltetes zonen.pmtiles fuer aktuell.
# Teuer ist sie nicht: aendert sich der Abdruck nicht, findet die Stufe jeden
# Block im Cache wieder.
$(OUT_DIR)/duesseldorf/zonen.pmtiles: $(DUESSELDORF_DIR)/zones/.stamp $(BUBATZ)
	$(TILES_CMD) \
		--zones-dir $(DUESSELDORF_DIR)/zones \
		--buildings-path $(INTERIM_DIR)/buildings.parquet \
		--out-path $@ \
		--n-jobs=$(N_JOBS)

duesseldorf: $(OUT_DIR)/duesseldorf/zonen.pmtiles

clean-duesseldorf:
	rm -rf $(DUESSELDORF_DIR) $(OUT_DIR)/duesseldorf

# -- Nordrhein-Westfalen: Zwischenstufe --------------------------------------
#
# Zwischen "eine Stadt" (3.233 Objekte) und "Bundeslauf" (~1 Mio) fehlte eine
# Stufe, auf der sich ein Verfahrenswechsel pruefen laesst, ohne einen
# Volllauf zu verbrennen: NRW hat ~110.000 Verbotsobjekte, deckt Grossstadt,
# Vorort und laendlichen Raum ab und enthaelt Duesseldorf vollstaendig --
# die dortigen Zonen muessen also identisch herauskommen (Konsistenzprobe
# gegen `make duesseldorf`, das aus einer ANDEREN PBF speist). Die Struktur
# ist absichtlich identisch zum Deutschland-Ziel (roher Extrakt in <dir>/raw,
# grenzgefilterte Objekte darueber), damit beide dieselben Regeln teilen.
# Plattenplatz ~30 GB -- typischerweise mit umgeleiteten Verzeichnissen:
#   make nrw INTERIM_DIR=/media/.../interim OUT_DIR=/media/.../out

NRW_PBF := $(RAW_DIR)/nordrhein-westfalen-latest.osm.pbf
NRW_GEOFABRIK_URL := https://download.geofabrik.de/europe/germany/nordrhein-westfalen-latest.osm.pbf
NRW_OSMFR_URL := https://download.openstreetmap.fr/extracts/europe/germany/nordrhein-westfalen-latest.osm.pbf
NRW_DIR := $(INTERIM_DIR)/nrw
NRW_RAW_DIR := $(NRW_DIR)/raw
NRW_REUSE_ZONES ?=

$(NRW_PBF):
	mkdir -p $(RAW_DIR)
	curl -fL -o $@ $(NRW_GEOFABRIK_URL) || curl -fL -o $@ $(NRW_OSMFR_URL)

# Gruppiertes Ziel (siehe Kommentar bei der Duesseldorf-Regel oben).
$(NRW_RAW_DIR)/protected.parquet $(NRW_RAW_DIR)/buildings.parquet &: $(NRW_PBF) $(TAGS_SPEC) | $(BUBATZ)
	$(EXTRACT_CMD) $< --out-dir $(NRW_RAW_DIR)

# Wie beim Bundeslauf: Relation-ID per --admin-name/--admin-level aus der
# PBF selbst nachgeschlagen (admin_level=4 = Bundesland). Hindernisse
# bleiben ungefiltert -- ein Gebaeude knapp jenseits der Landesgrenze
# verdeckt trotzdem (siehe clip.rs).
$(NRW_DIR)/protected.parquet: $(NRW_RAW_DIR)/protected.parquet $(NRW_PBF)
	$(CLIP_CMD) \
		--pbf $(NRW_PBF) \
		--admin-name Nordrhein-Westfalen \
		--admin-level 4 \
		--protected-in $(NRW_RAW_DIR)/protected.parquet \
		--out-dir $(NRW_DIR)

$(NRW_DIR)/zones/.stamp: $(NRW_DIR)/protected.parquet $(NRW_RAW_DIR)/buildings.parquet $(PIPELINE_CFG) | $(BUBATZ)
	$(call run_zones,$(NRW_DIR)/zones,$(NRW_DIR)/protected.parquet,$(NRW_RAW_DIR)/buildings.parquet,$(NRW_REUSE_ZONES))

$(OUT_DIR)/nrw/zonen.pmtiles: $(NRW_DIR)/zones/.stamp $(BUBATZ)
	$(TILES_CMD) \
		--zones-dir $(NRW_DIR)/zones \
		--buildings-path $(NRW_RAW_DIR)/buildings.parquet \
		--out-path $@ \
		--n-jobs=$(N_JOBS)

nrw: $(OUT_DIR)/nrw/zonen.pmtiles

clean-nrw:
	rm -rf $(NRW_DIR) $(OUT_DIR)/nrw

# -- Deutschland: Bundeslauf -------------------------------------------------
#
# Verdrahtet alle Bausteine (Gitterparallelisierung, Wiederaufnahmefaehigkeit
# je Zelle/Block, Grenzbehandlung) zum vollstaendigen Bundeslauf.
# Plattenplatz: Download ~5,6 GB, gesamter Datensatz (raw/interim/out) ~54 GB
# -- passt nicht neben einen knappen Primaerdatentraeger, deshalb typischerweise
# mit umgeleiteten Verzeichnissen aufrufen, z. B.
#   make germany RAW_DIR=/media/.../raw INTERIM_DIR=/media/.../interim OUT_DIR=/media/.../out

GERMANY_PBF := $(RAW_DIR)/germany-latest.osm.pbf
GERMANY_GEOFABRIK_URL := https://download.geofabrik.de/europe/germany-latest.osm.pbf
GERMANY_OSMFR_URL := https://download.openstreetmap.fr/extracts/europe/germany-latest.osm.pbf
GERMANY_DIR := $(INTERIM_DIR)/germany
GERMANY_RAW_DIR := $(GERMANY_DIR)/raw

# Geofabrik liefert eine .md5 neben dem Extrakt -- pruefen, bevor 5,6 GB als
# Eingabe gelten; der osm.fr-Ausweichpfad hat keine.
$(GERMANY_PBF):
	mkdir -p $(RAW_DIR)
	if curl -fL -o $@.tmp $(GERMANY_GEOFABRIK_URL) && curl -fsL -o $@.md5 $(GERMANY_GEOFABRIK_URL).md5; then \
	  echo "$$(cut -d' ' -f1 $@.md5)  $@.tmp" | md5sum -c - || { rm -f $@.tmp; exit 1; }; \
	else curl -fL -o $@.tmp $(GERMANY_OSMFR_URL) || exit 1; fi
	mv $@.tmp $@

# Gruppiertes Ziel (siehe Kommentar bei der analogen Duesseldorf-Regel oben).
$(GERMANY_RAW_DIR)/protected.parquet $(GERMANY_RAW_DIR)/buildings.parquet &: $(GERMANY_PBF) $(TAGS_SPEC) | $(BUBATZ)
	$(EXTRACT_CMD) $< --out-dir $(GERMANY_RAW_DIR)

# Grenzbehandlung: verwirft die kleine Minderheit an Verbotsobjekten, die eine
# Geofabrik-Deutschland-PBF knapp jenseits der echten Staatsgrenze mitliefert
# (auf Landesebene gemessen ~0,3 % der Objekte) -- Hindernisse
# (buildings.parquet) bleiben UNGEFILTERT (siehe clip.rs). Die Relation-ID
# wird per --admin-name/--admin-level aus der PBF selbst nachgeschlagen, statt
# als Zahl im Code zu stehen.
# Grenzcache: der PBF-Pass fuer die Staatsgrenze entfaellt, solange die seit
# dem Cache angewandten Diffs (update-pbf --diff-dir) die Relation nicht
# beruehren.
$(GERMANY_DIR)/protected.parquet: $(GERMANY_RAW_DIR)/protected.parquet $(GERMANY_PBF)
	$(CLIP_CMD) \
		--pbf $(GERMANY_PBF) \
		--admin-name Deutschland \
		--admin-level 2 \
		--protected-in $(GERMANY_RAW_DIR)/protected.parquet \
		--out-dir $(GERMANY_DIR) \
		--boundary-cache $(GERMANY_DIR)/boundary.json --diffs-dir $(GERMANY_DIFF_DIR)

# Siehe Kommentar bei $(DUESSELDORF_DIR)/zones/.stamp oben: die Menge der
# Zellendateien ist vorher unbekannt, eine Markerdatei ist hier der korrekte
# Ansatz -- bei national ueber 50.000 nichtleeren Zellen ohnehin die einzig
# praktikable Wahl.
$(GERMANY_DIR)/zones/.stamp: $(GERMANY_DIR)/protected.parquet $(GERMANY_RAW_DIR)/buildings.parquet $(PIPELINE_CFG) | $(BUBATZ)
	$(call run_zones,$(GERMANY_DIR)/zones,$(GERMANY_DIR)/protected.parquet,$(GERMANY_RAW_DIR)/buildings.parquet,$(GERMANY_REUSE_ZONES))

$(OUT_DIR)/germany/zonen.pmtiles: $(GERMANY_DIR)/zones/.stamp $(BUBATZ)
	$(TILES_CMD) \
		--zones-dir $(GERMANY_DIR)/zones \
		--buildings-path $(GERMANY_RAW_DIR)/buildings.parquet \
		--out-path $@ \
		--n-jobs=$(N_JOBS)

germany: $(OUT_DIR)/germany/zonen.pmtiles $(OUT_DIR)/germany/fussgaengerzonen.pmtiles

# -- Fussgaengerzonen (§ 5 Abs. 2 Nr. 5): eigene Ebene, ohne Sichtweite ---------
# Haengt nur an der PBF, nicht an zones/tiles: drei Durchgaenge ueber die Datei
# (Ways+Relationen, Mitglieds-Ways, Knoten), 36 s und 0,4 GB fuer den Bund
# (auf 14 Threads). Billig genug, um im Tageszyklus mitzulaufen -- die
# Ebene ist damit so frisch wie die Zonen.
$(OUT_DIR)/germany/fussgaengerzonen.pmtiles: $(GERMANY_PBF) $(BUBATZ)
	$(BUBATZ) pedestrian $(GERMANY_PBF) \
		--out-geojson $(GERMANY_DIR)/fussgaengerzonen.geojsonl \
		--out-path $@

# -- Kennzahlen + Diff zum Vorlauf ---------------------------------------------
report: $(BUBATZ)
	$(BUBATZ) report --zones-dir $(GERMANY_DIR)/zones \
		$$( [ -f $(GERMANY_DIR)/zones.prev/.stamp ] && echo "--previous $(GERMANY_DIR)/zones.prev" ) \
		--out-json $(GERMANY_DIR)/zones/report.json

# -- Vorschau im Browser (`bubatz serve`) --------------------------------------
#
# `bubatz serve` liefert in EINEM Prozess die Seite viewer/index.html (samt
# MapLibre/pmtiles.js aus viewer/vendor/, ins Binary eingebettet) und die
# Kacheln eines oder mehrerer Archive als /<name>/zonen/{z}/{x}/{y}.mvt
# (TileJSON /<name>/tiles.json, Liste /datasets). Ein neuer Lauf wird ohne
# Neustart erkannt (Inode/mtime des Archivs). Aufruf z. B.
#   make viewer PMTILES_DIR=/media/.../out/germany            # Name = germany
#   make viewer VIEWER_DATASETS="germany=/a/out/germany nrw=/b/out/nrw"
# und dann http://127.0.0.1:8787/ (Archivwahl im Panel bzw. ?ds=<name>).
# Fuer eine veroeffentlichte .pmtiles-Datei (Range-Requests) reicht
# index.html?pmtiles=https://.../zonen.pmtiles ohne Server (vendor/ daneben).
PMTILES_DIR ?= $(OUT_DIR)/germany
VIEWER_DATASETS ?= $(notdir $(PMTILES_DIR))=$(PMTILES_DIR)
VIEWER_BIND ?= 127.0.0.1:8787
# Protomaps-Grundkarte (PMTiles, `pmtiles extract` aus build.protomaps.com,
# Grundkarte); ohne sie OSM-Raster als Notnagel.
BASEMAP ?=
viewer: $(BUBATZ)
	@echo "Vorschau: http://$(VIEWER_BIND)/"
	$(BUBATZ) serve --bind $(VIEWER_BIND) --viewer-dir viewer $(if $(BASEMAP),--basemap $(BASEMAP)) $(foreach d,$(VIEWER_DATASETS),--dataset $(d))

# -- Inkrementelle Aktualisierung ----------------------------------------------
#
# Bringt die lokale Deutschland-PBF per OSM-Replikations-Diffs auf den
# aktuellen Stand. Den Diff-Server und den Startpunkt liest
# `update-pbf` aus dem osmosis-Header der Datei selbst (Geofabrik-
# Downloads verweisen auf die taeglichen Geofabrik-Diffs, openstreetmap.fr-
# Downloads auf dessen Minuten-Diffs); --size begrenzt die Diff-Menge je
# Aufruf (2 GB ~ eine Woche Deutschland-Minutendiffs) -- bei aelteren
# Staenden mehrfach aufrufen (Exit 1 = "weiter verfuegbar"). Das Ergebnis
# ersetzt die PBF atomar (und loest damit alle Hardlinks auf einen aelteren
# Stand), danach genuegt `make germany`: extract/Grenze/tiles laufen neu,
# zones rotiert den Vorlauf und rechnet nur veraenderte Objekte
# (siehe run_zones). Bewusst NICHT automatisch Teil von `make germany`.
# Temporaerer Name MUSS auf .osm.pbf enden: osmium erkennt das Ausgabeformat
# an der Dateiendung (".osm.pbf.new" -> "Could not detect file format").
GERMANY_PBF_NEW := $(RAW_DIR)/germany-latest.new.osm.pbf

# Ist die PBF schon auf dem Serverstand, schreibt update-pbf KEINE neue Datei
# (nur eine Meldung) -- dann darf hier auch nichts verschoben werden, sonst
# scheitert `mv` und ein Timer-Zyklus `make update-pbf && make germany`
# bricht ab, obwohl nichts zu tun ist.
update-pbf: $(GERMANY_PBF)
	$(UPDATE_PBF_CMD)
	@if [ -f $(GERMANY_PBF_NEW) ]; then mv $(GERMANY_PBF_NEW) $(GERMANY_PBF); else echo "update-pbf: nichts zu tun, $(GERMANY_PBF) bleibt"; fi

# -- Veroeffentlichen ----------------------------------------------------------
#
# Ersetzt das oeffentliche Archiv in PUBLISH_DIR (Webwurzel, gleiches
# Dateisystem) NUR nach bestandener Pruefkette (Anker, Kachelaufblaehung,
# Plausibilitaet gegen den zuletzt veroeffentlichten Stand, pmtiles verify),
# atomar und mit Vorgaenger `zonen.pmtiles.prev`; scheitert eine Pruefung,
# bleibt der alte Stand und `make` schlaegt fehl (OnFailure= der Einheit).
# Tageszyklus im Betrieb: `make update-pbf && make germany && make publish`.
# Details und Schwellen: scripts/publish_germany.sh (PUBLISH_DRY=1 = nur pruefen).
PUBLISH_DIR ?= /srv/bubatz/www
publish:
	BUBATZ=$(BUBATZ) OUT_DIR=$(OUT_DIR) INTERIM_DIR=$(INTERIM_DIR) GERMANY_PBF=$(GERMANY_PBF) PUBLISH_DIR=$(PUBLISH_DIR) scripts/publish_germany.sh

clean-germany:
	rm -rf $(GERMANY_DIR) $(OUT_DIR)/germany
