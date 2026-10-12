# Die Pipeline im Einzelnen

Alle Stufen rechnet ein Binary: `bubatz --config config/pipeline.toml <stufe>`.
Jede schreibt neben ihrer Ausgabe ein `*_summary.json` mit Kennzahlen und
Laufzeiten.

| Stufe | Ein → Aus |
|---|---|
| `extract <pbf>` | PBF → `protected.parquet`, `buildings.parquet` (EPSG:25832) |
| `clip` | auf Verwaltungsgrenze + Reichweite filtern (Grenze gecacht) |
| `zones` | Sichtfeld je Objekt, 2-km-Zellen parallel, eine Parquet-Datei je Zelle; `--reuse-from` übernimmt unveränderte Objekte und Zellen |
| `tiles` | Zellen → `zonen.pmtiles`: tippecanoe je Zoomband und Zellenblock (gecacht), `tile-join`, Metadaten bereinigt |
| `pedestrian <pbf>` | Fußgängerzonen (§ 5 Abs. 2 Nr. 5) als eigene Ebene, ohne Puffer; Metadaten bereinigt |
| `report` | Kennzahlen je Kategorie, größte Zonen, `--previous` = Diff |
| `verify identical A B` | zwei Zonenstände byte-genau vergleichen |
| `verify anchor` | Fläche des Referenzobjekts gegen den Sollwert |
| `update-pbf <pbf>` | Geofabrik-Tagesdiffs laden, `osmium apply-changes` |
| `serve` | Vorschau + Kacheln aus Archiven |

## Kachelvertrag

Layer `zonen`. Attribute je Merkmal, und sonst keine:

| Attribut | Werte |
|---|---|
| `osm_id` | Kennung in OpenStreetMap |
| `osm_type` | `way`, `relation` oder `node` |
| `category` | Schlüssel aus `config/tags.json` |
| `band_max_m` | Sichtweite des Bandes, derzeit immer 100 |

Jedes Merkmal liegt innerhalb von `100 m + 2 m + 3 × Toleranz` um seine Quelle.
Die Toleranzen je Zoomband: 200 m (z8–9), 80 m (z10–11), 25 m (z12–13),
0,5 m (z14). Der Faktor 3 ist die Schranke der einseitigen Vereinfachung
(`rust/bubatz/src/simplify.rs`), die 2 m sind die Datenqualitätsmarge.

Ab z14 sind die opaken Gebäudegrundrisse abgezogen: Rot endet an der Hauswand.

Die Metadaten beider Archive nennen Layer und Felder (`vector_layers`),
Attribution, Format und Werkzeugversion – sonst nichts. tippecanoe und
`tile-join` schreiben außerdem Pfade und vollständige Kommandozeilen hinein
(`name`, `description`, `generator_options`); `tiles::scrub_metadata` entfernt
sie nach dem Bau und bricht ab, wenn danach noch ein Textwert einen Pfad enthält.

## Versionen, die den Cache steuern

Drei Werte entscheiden, was ein neuer Lauf von einem alten übernimmt. Wer am
Verfahren etwas ändert, muss den passenden mitziehen — sonst werden Ergebnisse
aus einem anderen Verfahren weiterverwendet.

| Wert | Wo | Was er schützt |
|---|---|---|
| `PARAM_VERSION` | `rust/bubatz/src/config.rs` | Teil jedes `cache_key`: die Zonengeometrie je Objekt |
| `CELL_FP_VERSION` | `rust/bubatz/src/zones.rs` | Fingerabdruck einer Zelle, also auch ihr Spaltensatz |
| `SIMPLIFY_VERSION` | `rust/bubatz/src/tiles.rs` | Zellen- und Blockarchive der Kachelstufe |

## Prüfungen vor einer Veröffentlichung

`make publish` (`scripts/publish_germany.sh`) tauscht das Archiv erst, wenn die
Kette hält:

1. Fläche des Referenzobjekts ± 2 % (`bubatz verify anchor`).
2. Kachelaufblähung am Referenzobjekt ≤ 3 %
   (`scripts/rollout_check_tiles.py`).
3. Plausibilität gegen den letzten Stand: Bänder und Archivgröße zwischen
   0,9× und 1,1×, Objektzahl je Kategorie innerhalb der Toleranz, kein Objekt
   größer als das Vielfache seiner Kategoriegrenze.
4. `pmtiles verify`.

Scheitert eine Prüfung, bleibt der alte Stand online und `make` schlägt fehl.
Alle Prüfungen am Stück, ohne zu veröffentlichen:
`scripts/rollout_checks_germany.sh`.

Dazu zwei Konsistenzproben, die nicht automatisch laufen:
`bubatz verify identical` vergleicht denselben Ausschnitt aus zwei
verschiedenen Extrakten (Stadt in Land, Land in Bund) byte-genau, und der
Tageszyklus wird gegen einen Volllauf gestellt.
