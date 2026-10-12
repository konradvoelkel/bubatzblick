//! `bubatz tiles`: Zonen-Zellen -> je Zoomband einseitig vereinfachte, nach
//! EPSG:4326 projizierte GeoJSONSeq (inhaltsadressierter Zellen-Cache) ->
//! tippecanoe je Band und Block -> tile-join -> Metadaten bereinigen
//! (`scrub_metadata`) -> `pmtiles verify` -> Summary.
//!
//! **Kachelbloecke:** tippecanoe laeuft je Zoomband und raeumlichem Block
//! (`--block-cells` Zellen Kantenlaenge, Standard 25 = 50 km); das Blockarchiv
//! ist inhaltsadressiert (`<stem>.tiles-blocks/<band>/<block>__<digest>.pmtiles`,
//! Digest ueber tippecanoe-Version, Flags, Band und die Cache-Kennungen der
//! Zellen) und wird bei gleicher Kennung uebernommen. `tile-join` vereinigt
//! alle Blockarchive; weil tippecanoe ohne Merkmalslimit, Groessenlimit und
//! Vereinfachung laeuft, ist jede Kachel bis auf die Reihenfolge der
//! Merkmale gleich der aus einem Lauf ueber alles (Pruefung:
//! `scripts/rollout_check_tiles_equal.py`).
//!
//! `one_sided_simplify` ist Douglas-Peucker + i_overlay; aendert sich daran
//! etwas, muss `SIMPLIFY_VERSION` wechseln, sonst wird ein warmer Cache mit
//! anderen Bits wiederverwendet. Koordinaten mit 7 Nachkommastellen wie bei
//! GDALs GeoJSON-Schreiber.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use bubatzcore::geom::{Shape, Shapes};
use clap::Args;
use rayon::prelude::*;
use tracing::{info, warn};

use crate::bstore::BuildingStore;
use crate::config::Config;
use crate::parquet_io::{geo_bbox, read_zone_cell};
use crate::proj::Utm32;
use crate::simplify::{one_sided_simplify, subtract, union_many};

pub const LAYER_NAME: &str = "zonen";
// Cache-Generation von Zellen- und Blockarchiven: aendert sich, was in einer
// Kachel landet (Vereinfachung, Zeilenformat, Merkmalssatz), muss dieser Wert
// weiterzaehlen -- sonst wird ein warmer Cache mit anderem Inhalt
// wiederverwendet. `scripts/rollout_check_tiles.py` spiegelt ihn.
pub const SIMPLIFY_VERSION: &str = "v1";
pub const DEFAULT_BLOCK_CELLS: i64 = 25;
/// tippecanoe-Flags; gehen in die Blockkennung ein: kein Merkmals-/
/// Groessenlimit, keine Vereinfachung -> partitionierbar.
pub const TIPPECANOE_FLAGS: [&str; 6] = [
    "--force",
    "--read-parallel",
    "--no-line-simplification",
    "--no-tiny-polygon-reduction",
    "--no-feature-limit",
    "--no-tile-size-limit",
];
/// Beschreibung des Zonenarchivs in seinen Metadaten (`scrub_metadata`).
pub const ZONES_DESCRIPTION: &str =
    "Konsumverbotszonen nach Paragraf 5 Abs. 2 KCanG (Sichtweite), berechnet aus OpenStreetMap";
/// Attribution in jedem veroeffentlichten Archiv.
pub const ATTRIBUTION: &str = "\u{a9} OpenStreetMap-Mitwirkende (ODbL)";
/// Blockarchive, die ein Lauf nicht referenziert, werden nach dieser Frist geloescht.
const BLOCK_PRUNE_AGE_S: u64 = 7 * 86_400;

#[derive(Args, Debug)]
pub struct TilesArgs {
    #[arg(long, default_value = "data/interim/zones")]
    pub zones_dir: PathBuf,
    #[arg(long, default_value = "data/out/zonen.pmtiles")]
    pub out_path: PathBuf,
    #[arg(long, default_value_t = 8)]
    pub min_zoom: i32,
    #[arg(long, default_value_t = 14)]
    pub max_zoom: i32,
    #[arg(long, default_value_t = -1)]
    pub n_jobs: i64,
    /// Arbeitsverzeichnis (Zellen-Cache, Blockarchive) verwerfen und neu beginnen
    #[arg(long, default_value_t = false)]
    pub force: bool,
    /// Zellen-Cache verwerfen
    #[arg(long, default_value_t = false)]
    pub clear_cache: bool,
    /// buildings.parquet: opake Grundrisse aus den feinen Baendern abziehen
    #[arg(long)]
    pub buildings_path: Option<PathBuf>,
    /// Kantenlaenge der Kachelbloecke in Zellen (tippecanoe je Block, Archiv gecacht)
    #[arg(long, default_value_t = DEFAULT_BLOCK_CELLS)]
    pub block_cells: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoomLevel {
    pub min_zoom: i32,
    pub max_zoom: i32,
    pub tolerance_m: f64,
    pub merge_bands: bool,
    pub subtract_buildings: bool,
}

fn fmt_g(v: f64) -> String {
    // Python `:g` fuer die hier vorkommenden Werte (200, 80, 25, 0.5)
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

impl ZoomLevel {
    pub fn tag(&self) -> String {
        format!(
            "z{}-{}_t{}{}{}",
            self.min_zoom,
            self.max_zoom,
            fmt_g(self.tolerance_m),
            if self.merge_bands { "_merged" } else { "" },
            if self.subtract_buildings {
                "_nobld"
            } else {
                ""
            }
        )
    }
    pub fn sub_dir(&self) -> String {
        format!(
            "t{}{}{}",
            fmt_g(self.tolerance_m),
            if self.merge_bands { "_merged" } else { "" },
            if self.subtract_buildings {
                "_nobld"
            } else {
                ""
            }
        )
    }
}

/// `zoom_levels`: Treppe aus `tolerances_m`.
pub fn zoom_levels(
    tolerances: &BTreeMap<String, f64>,
    min_zoom: i32,
    max_zoom: i32,
    merge_up_to: i32,
    subtract_max_tol: f64,
) -> Result<Vec<ZoomLevel>> {
    if min_zoom > max_zoom {
        bail!("zoom_levels: min_zoom {min_zoom} > max_zoom {max_zoom}");
    }
    let mut steps: Vec<(i32, f64)> = tolerances
        .iter()
        .map(|(k, v)| (k.trim_start_matches('z').parse::<i32>().unwrap_or(0), *v))
        .collect();
    steps.sort_by_key(|s| s.0);
    if steps.is_empty() {
        return Ok(vec![ZoomLevel {
            min_zoom,
            max_zoom,
            tolerance_m: 0.0,
            merge_bands: max_zoom <= merge_up_to,
            subtract_buildings: subtract_max_tol >= 0.0,
        }]);
    }
    let mut levels = Vec::new();
    for (i, (zoom, tol)) in steps.iter().enumerate() {
        let lo = if i > 0 { *zoom } else { (*zoom).min(min_zoom) };
        let hi = if i + 1 < steps.len() {
            steps[i + 1].0 - 1
        } else {
            max_zoom.max(*zoom)
        };
        let (lo, hi) = (lo.max(min_zoom), hi.min(max_zoom));
        if lo <= hi {
            levels.push(ZoomLevel {
                min_zoom: lo,
                max_zoom: hi,
                tolerance_m: *tol,
                merge_bands: hi <= merge_up_to,
                subtract_buildings: *tol <= subtract_max_tol,
            });
        }
    }
    Ok(levels)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `cell_digest`: blake2b(12) ueber
/// `"<version>|<crs_out>|osm_id,osm_type,category,band_max_m,cache_key|"`
/// plus `json.dumps(rows, sort_keys=True)` der Zeilen (Python-Formatierung).
pub fn cell_digest(rows: &[crate::parquet_io::ZoneCellRow], crs_out: &str) -> String {
    use blake2::{Blake2b, Digest, digest::consts::U12};
    let mut h = Blake2b::<U12>::new();
    h.update(
        format!("{SIMPLIFY_VERSION}|{crs_out}|osm_id,osm_type,category,band_max_m,cache_key|")
            .as_bytes(),
    );
    let mut s = String::with_capacity(rows.len() * 200);
    s.push('[');
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!(
            "{{\"band_max_m\": {}, \"cache_key\": \"{}\", \"category\": \"{}\", \"osm_id\": {}, \"osm_type\": \"{}\"}}",
            r.band_max_m,
            r.cache_key,
            r.category,
            r.osm_id,
            r.osm_type
        ));
    }
    s.push(']');
    h.update(s.as_bytes());
    let out = h.finalize();
    hex(&out)
}

pub fn buildings_digest(wkbs: &mut [&[u8]]) -> String {
    use blake2::{Blake2b, Digest, digest::consts::U8};
    wkbs.sort_unstable();
    let mut h = Blake2b::<U8>::new();
    for w in wkbs.iter() {
        h.update(w);
    }
    let out = h.finalize();
    hex(&out)
}

pub fn cache_entry_path(
    cache_dir: &Path,
    cell_name: &str,
    level: &ZoomLevel,
    digest: &str,
    bdigest: &str,
) -> PathBuf {
    let key = if level.subtract_buildings && !bdigest.is_empty() {
        format!("{digest}-{bdigest}")
    } else {
        digest.to_string()
    };
    cache_dir
        .join(level.sub_dir())
        .join(format!("{cell_name}__{key}.geojsonl"))
}

struct Feature {
    osm_id: i64,
    osm_type: String,
    category: String,
    band_max_m: i64,
    shapes: Shapes,
}

fn fmt7(v: f64) -> String {
    let s = format!("{v:.7}");
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn geojson_line(f: &Feature, proj: &Utm32, out: &mut String) {
    // Ringe geschlossen ausgeben (GeoJSON), Koordinaten in Grad mit 7 Stellen
    let ring_json = |r: &[[f64; 2]], s: &mut String| {
        s.push_str("[ ");
        let n = r.len();
        for i in 0..=n {
            let p = r[i % n];
            let [lon, lat] = proj.inverse(p[0], p[1]);
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(&format!("[ {}, {} ]", fmt7(lon), fmt7(lat)));
        }
        s.push_str(" ]");
    };
    out.push_str(&format!(
        "{{ \"type\": \"Feature\", \"properties\": {{ \"osm_id\": {}, \"osm_type\": \"{}\", \"category\": \"{}\", \"band_max_m\": {} }}, \"geometry\": {{ ",
        f.osm_id, f.osm_type, f.category, f.band_max_m
    ));
    if f.shapes.len() == 1 {
        out.push_str("\"type\": \"Polygon\", \"coordinates\": [ ");
        for (k, r) in f.shapes[0].iter().enumerate() {
            if k > 0 {
                out.push_str(", ");
            }
            ring_json(r, out);
        }
        out.push_str(" ]");
    } else {
        out.push_str("\"type\": \"MultiPolygon\", \"coordinates\": [ ");
        for (j, s) in f.shapes.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str("[ ");
            for (k, r) in s.iter().enumerate() {
                if k > 0 {
                    out.push_str(", ");
                }
                ring_json(r, out);
            }
            out.push_str(" ]");
        }
        out.push_str(" ]");
    }
    out.push_str(" } }\n");
}

fn merge_bands(features: &[Feature]) -> Vec<Feature> {
    let mut groups: BTreeMap<(i64, String), Vec<&Feature>> = BTreeMap::new();
    for f in features {
        groups
            .entry((f.osm_id, f.category.clone()))
            .or_default()
            .push(f);
    }
    groups
        .into_iter()
        .map(|((osm_id, category), fs)| Feature {
            osm_id,
            // Alle Baender eines Objekts tragen denselben Typ.
            osm_type: fs[0].osm_type.clone(),
            category,
            band_max_m: fs.iter().map(|f| f.band_max_m).min().unwrap_or(0),
            shapes: union_many(&fs.iter().map(|f| f.shapes.clone()).collect::<Vec<_>>()),
        })
        .collect()
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension(format!("geojsonl.tmp{}", std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// `write_cell_levels` fuer EINE Zelle: Kette feinstes -> groebstes Band.
fn process_cell(
    part: &Path,
    digest: &str,
    bdigest: &str,
    cell_buildings: &[Shape],
    levels: &[ZoomLevel],
    cache_dir: &Path,
    proj: &Utm32,
) -> Result<usize> {
    let rows = read_zone_cell(part)?;
    let cell_name = part
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cell")
        .to_string();
    let mut ordered: Vec<&ZoomLevel> = levels.iter().collect();
    ordered.sort_by(|a, b| {
        a.tolerance_m
            .partial_cmp(&b.tolerance_m)
            .unwrap()
            .then(a.merge_bands.cmp(&b.merge_bands))
    });
    let targets: Vec<PathBuf> = ordered
        .iter()
        .map(|lv| cache_entry_path(cache_dir, &cell_name, lv, digest, bdigest))
        .collect();
    if targets.iter().all(|t| t.exists()) {
        return Ok(rows.len());
    }
    let mut current: Vec<Feature> = rows
        .iter()
        .map(|r| {
            Ok(Feature {
                osm_id: r.osm_id,
                osm_type: r.osm_type.clone(),
                category: r.category.clone(),
                band_max_m: r.band_max_m,
                shapes: crate::wkb::parse(&r.wkb)?,
            })
        })
        .collect::<Result<_>>()?;
    let n_rows = rows.len();
    for (i, (level, target)) in ordered.iter().zip(&targets).enumerate() {
        if targets[i..].iter().all(|t| t.exists()) {
            break;
        }
        if level.merge_bands {
            current = merge_bands(&current);
        }
        if level.tolerance_m > 0.0 {
            for f in &mut current {
                f.shapes = one_sided_simplify(&f.shapes, level.tolerance_m);
            }
        }
        if target.exists() {
            continue;
        }
        std::fs::create_dir_all(target.parent().unwrap())?;
        let mut out = String::new();
        for f in &current {
            if level.subtract_buildings {
                let fb = shapes_bounds(&f.shapes);
                let near: Vec<Shape> = cell_buildings
                    .iter()
                    .filter(|b| {
                        let bb = shapes_bounds(std::slice::from_ref(*b));
                        !(bb[0] > fb[2] || bb[2] < fb[0] || bb[1] > fb[3] || bb[3] < fb[1])
                    })
                    .cloned()
                    .collect();
                let cut = if near.is_empty() {
                    f.shapes.clone()
                } else {
                    subtract(&f.shapes, &near)
                };
                if cut.is_empty() {
                    continue;
                }
                geojson_line(
                    &Feature {
                        osm_id: f.osm_id,
                        osm_type: f.osm_type.clone(),
                        category: f.category.clone(),
                        band_max_m: f.band_max_m,
                        shapes: cut,
                    },
                    proj,
                    &mut out,
                );
            } else {
                geojson_line(f, proj, &mut out);
            }
        }
        // Eintraege derselben Zelle in diesem Band verwerfen
        if let Ok(rd) = std::fs::read_dir(target.parent().unwrap()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&format!("{cell_name}__")) && name.ends_with(".geojsonl") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        write_atomic(target, &out)?;
    }
    Ok(n_rows)
}

fn shapes_bounds(shapes: &[Shape]) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in shapes.iter().flatten().flatten() {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

fn parts_fingerprint(
    parts: &[PathBuf],
    levels: &[ZoomLevel],
    crs_out: &str,
) -> Result<serde_json::Value> {
    use blake2::{Blake2b, Digest, digest::consts::U16};
    let mut h = Blake2b::<U16>::new();
    for p in parts {
        let md = std::fs::metadata(p)?;
        let mtime_ns = md
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i128)
            .unwrap_or(0);
        h.update(
            format!(
                "{}:{}:{}\n",
                p.file_name().unwrap().to_string_lossy(),
                md.len(),
                mtime_ns
            )
            .as_bytes(),
        );
    }
    let out = h.finalize();
    Ok(serde_json::json!({
        "n_parts": parts.len(),
        "parts_digest": hex(&out),
        "levels": levels.iter().map(|l| l.tag()).collect::<Vec<_>>(),
        "crs_out": crs_out,
        "simplify_version": SIMPLIFY_VERSION,
    }))
}

fn ensure_work_dir(work_dir: &Path, fingerprint: &serde_json::Value) -> Result<()> {
    let inputs = work_dir.join("inputs.json");
    if work_dir.exists() && inputs.exists() {
        let previous: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&inputs)?)?;
        if &previous != fingerprint {
            warn!(
                "tiles: Arbeitsverzeichnis {} stammt von anderen Eingaben -- wird verworfen",
                work_dir.display()
            );
            std::fs::remove_dir_all(work_dir)?;
        }
    } else if work_dir.exists() && std::fs::read_dir(work_dir)?.next().is_some() {
        warn!(
            "tiles: Arbeitsverzeichnis {} ohne Eingabekennung -- wird verworfen",
            work_dir.display()
        );
        std::fs::remove_dir_all(work_dir)?;
    }
    std::fs::create_dir_all(work_dir)?;
    if !inputs.exists() {
        std::fs::write(&inputs, serde_json::to_string_pretty(fingerprint)?)?;
    }
    Ok(())
}

fn tippecanoe_command(
    inputs: &[PathBuf],
    out: &Path,
    work_dir: &Path,
    min_zoom: i32,
    max_zoom: i32,
) -> Command {
    let mut c = Command::new("tippecanoe");
    c.args(TIPPECANOE_FLAGS)
        .arg(format!("--minimum-zoom={min_zoom}"))
        .arg(format!("--maximum-zoom={max_zoom}"))
        .arg(format!("--layer={LAYER_NAME}"))
        .arg(format!("--temporary-directory={}", work_dir.display()))
        .arg(format!("--output={}", out.display()));
    for i in inputs {
        c.arg(i);
    }
    c
}

/// Metadaten, wie sie ein veroeffentlichtes Archiv tragen darf. tippecanoe und
/// tile-join schreiben die Pfade ihrer Ein- und Ausgaben nach `name` und
/// `description` und jede Kommandozeile nach `generator_options` -- beim
/// Bundeslauf mehrere Megabyte, alle mit dem Datenverzeichnis des Rechners.
/// Das Archiv liegt oeffentlich; was darin steht, muss es auch sein. Bleiben
/// sollen Layer und Felder (`vector_layers`), Werkzeugversion und Format.
pub fn scrubbed_metadata(
    mut meta: serde_json::Value,
    name: &str,
    description: &str,
) -> serde_json::Value {
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("generator_options");
        obj.insert("name".into(), name.into());
        obj.insert("description".into(), description.into());
        obj.insert("attribution".into(), ATTRIBUTION.into());
    }
    meta
}

/// Alle Textwerte in den Metadaten, die wie ein Pfad aussehen (mit `/`),
/// beliebig tief. Leer heisst sauber.
pub fn metadata_paths(v: &serde_json::Value) -> Vec<String> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) if s.contains('/') => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out);
    out
}

/// Metadaten eines fertigen Archivs an Ort und Stelle bereinigen:
/// `pmtiles show --metadata` -> `scrubbed_metadata` -> `pmtiles edit
/// --metadata` (schreibt das Archiv neu, die Kacheln bleiben), danach die
/// Gegenprobe am Archiv selbst: kein Textwert mit `/` mehr.
pub fn scrub_metadata(path: &Path, name: &str, description: &str) -> Result<()> {
    let clean = scrubbed_metadata(read_metadata(path)?, name, description);
    let tmp = path.with_extension("metadata.json");
    std::fs::write(&tmp, serde_json::to_vec(&clean)?)?;
    let out = Command::new("pmtiles")
        .arg("edit")
        .arg(path)
        .arg(format!("--metadata={}", tmp.display()))
        .output()
        .context("pmtiles edit starten")?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        bail!(
            "pmtiles edit fehlgeschlagen fuer {}:\n{}\n{}",
            path.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let paths = metadata_paths(&read_metadata(path)?);
    if !paths.is_empty() {
        bail!(
            "Metadaten von {} nennen weiterhin Pfade: {}",
            path.display(),
            paths.join(", ")
        );
    }
    Ok(())
}

fn read_metadata(path: &Path) -> Result<serde_json::Value> {
    let out = Command::new("pmtiles")
        .args(["show", "--metadata"])
        .arg(path)
        .output()
        .context("pmtiles show starten")?;
    if !out.status.success() {
        bail!(
            "pmtiles show --metadata fehlgeschlagen fuer {}:\n{}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout)
        .with_context(|| format!("Metadaten von {} lesen", path.display()))
}

fn verify(path: &Path) -> Result<String> {
    let out = Command::new("pmtiles")
        .arg("verify")
        .arg(path)
        .output()
        .context("pmtiles verify starten")?;
    if !out.status.success() {
        bail!(
            "pmtiles verify fehlgeschlagen fuer {}:\n{}\n{}",
            path.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Block einer Zelle `x<col>_y<row>`: `b<col div n>_<row div n>` (Euklidisch,
/// negative Koordinaten fallen in den Block darunter/links).
pub fn block_of(cell_name: &str, block_cells: i64) -> Option<String> {
    let rest = cell_name.strip_prefix('x')?;
    let (col, row) = rest.split_once("_y")?;
    let (col, row) = (col.parse::<i64>().ok()?, row.parse::<i64>().ok()?);
    Some(format!(
        "b{}_{}",
        col.div_euclid(block_cells),
        row.div_euclid(block_cells)
    ))
}

fn tippecanoe_version() -> Result<String> {
    let out = Command::new("tippecanoe")
        .arg("--version")
        .output()
        .context("tippecanoe --version")?;
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let v = text.lines().next().unwrap_or("").trim().to_string();
    if v.is_empty() {
        bail!("tippecanoe --version ohne Ausgabe");
    }
    Ok(v)
}

/// Blockkennung: blake2b-16 ueber tippecanoe-Version, Flags, Band (Tag,
/// Zoombereich) und die sortierten Cache-Kennungen der Zellen.
pub fn block_digest(tc_version: &str, level: &ZoomLevel, cells: &[CellKey]) -> String {
    use blake2::{Blake2b, Digest, digest::consts::U16};
    let mut h = Blake2b::<U16>::new();
    h.update(tc_version.as_bytes());
    h.update(TIPPECANOE_FLAGS.join(" ").as_bytes());
    h.update(format!("|{}|{}|{}|", level.tag(), level.min_zoom, level.max_zoom).as_bytes());
    let mut sorted: Vec<&CellKey> = cells.iter().collect();
    sorted.sort();
    for (c, d, b) in sorted {
        h.update(format!("{c}:{d}:{b}\n").as_bytes());
    }
    let out = h.finalize();
    hex(&out)
}

/// Zelle im Block: (Zellname, cell_digest, buildings_digest).
type CellKey = (String, String, String);

struct BlockResult {
    archives: Vec<PathBuf>,
    n_features: usize,
    built: usize,
    reused: usize,
    tippecanoe_s: f64,
}

/// tippecanoe je Block mit inhaltsadressiertem Archiv (P2). Eingaben sind die
/// Cache-Dateien der Zellen direkt; die Merkmalszahl eines Blocks steht im
/// Sidecar `<archiv>.json`, damit uebernommene Bloecke nicht gelesen werden.
#[allow(clippy::too_many_arguments)]
fn build_blocks(
    parts: &[PathBuf],
    digests: &BTreeMap<String, (String, String)>,
    level: &ZoomLevel,
    cache_dir: &Path,
    blocks_dir: &Path,
    block_cells: i64,
    tc_version: &str,
    work_dir: &Path,
    concurrency: usize,
    threads_per_proc: usize,
) -> Result<BlockResult> {
    let level_dir = blocks_dir.join(level.tag());
    std::fs::create_dir_all(&level_dir)?;
    // Zellen je Block
    let mut blocks: BTreeMap<String, Vec<CellKey>> = BTreeMap::new();
    for part in parts {
        let cell_name = part.file_stem().unwrap().to_string_lossy().to_string();
        let (digest, bdigest) = &digests[&cell_name];
        let block = block_of(&cell_name, block_cells)
            .with_context(|| format!("Zellname {cell_name} ohne x<col>_y<row>"))?;
        blocks
            .entry(block)
            .or_default()
            .push((cell_name, digest.clone(), bdigest.clone()));
    }
    let mut archives = Vec::with_capacity(blocks.len());
    let mut n_features = 0usize;
    let mut reused = 0usize;
    let mut todo: Vec<(String, PathBuf, Vec<CellKey>)> = Vec::new();
    for (block, cells) in blocks {
        let digest = block_digest(tc_version, level, &cells);
        let archive = level_dir.join(format!("{block}__{digest}.pmtiles"));
        let sidecar = archive.with_extension("pmtiles.json");
        if archive.exists() && sidecar.exists() {
            let meta: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&sidecar)?)?;
            n_features += meta["n_features"].as_u64().unwrap_or(0) as usize;
            reused += 1;
            archives.push(archive);
        } else {
            todo.push((block, archive, cells));
        }
    }
    // Groesste Bloecke zuerst (LPT), begrenzt nebenlaeufig, tippecanoe je
    // Prozess auf wenige Threads gedeckelt.
    todo.sort_by_key(|(_, _, cells)| std::cmp::Reverse(cells.len()));
    let built = todo.len();
    let t_tc = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(concurrency.max(1))
        .build()?;
    let results: Vec<Result<(PathBuf, usize)>> = pool.install(|| {
        todo.par_iter()
            .map(|(block, archive, cells)| -> Result<(PathBuf, usize)> {
                let inputs: Vec<PathBuf> = cells
                    .iter()
                    .map(|(c, d, b)| cache_entry_path(cache_dir, c, level, d, b))
                    .collect();
                let mut n = 0usize;
                for i in &inputs {
                    let data = std::fs::read(i)
                        .with_context(|| format!("Cache-Eintrag {}", i.display()))?;
                    n += data.iter().filter(|&&x| x == b'\n').count();
                }
                let tmp_dir = work_dir.join("blocks").join(level.tag()).join(block);
                std::fs::create_dir_all(&tmp_dir)?;
                let tmp = archive.with_extension("building.pmtiles");
                let out =
                    tippecanoe_command(&inputs, &tmp, &tmp_dir, level.min_zoom, level.max_zoom)
                        .env("TIPPECANOE_MAX_THREADS", threads_per_proc.to_string())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .output()
                        .context("tippecanoe starten")?;
                if !out.status.success() {
                    let text = String::from_utf8_lossy(&out.stderr).to_string();
                    bail!(
                        "tippecanoe {} {block} (Exit {:?}):\n{}",
                        level.tag(),
                        out.status.code(),
                        &text[text.len().saturating_sub(4000)..]
                    );
                }
                verify(&tmp)?;
                let sidecar = archive.with_extension("pmtiles.json");
                std::fs::write(
                    sidecar.with_extension("json.tmp"),
                    serde_json::to_string(&serde_json::json!({
                        "block": block, "level": level.tag(), "n_cells": cells.len(),
                        "n_features": n, "tippecanoe": tc_version,
                    }))?,
                )?;
                std::fs::rename(&tmp, archive)?;
                std::fs::rename(sidecar.with_extension("json.tmp"), &sidecar)?;
                let _ = std::fs::remove_dir_all(&tmp_dir);
                Ok((archive.clone(), n))
            })
            .collect()
    });
    for r in results {
        let (a, n) = r?;
        archives.push(a);
        n_features += n;
    }
    archives.sort();
    info!(
        "tiles: {}: {} Bloecke, {} uebernommen, {} gebaut ({:.0} s)",
        level.tag(),
        built + reused,
        reused,
        built,
        t_tc.elapsed().as_secs_f64()
    );
    Ok(BlockResult {
        archives,
        n_features,
        built,
        reused,
        tippecanoe_s: t_tc.elapsed().as_secs_f64(),
    })
}

/// Nicht referenzierte Blockarchive (und Sidecars) aelter als die Frist loeschen.
fn prune_blocks(blocks_dir: &Path, referenced: &[PathBuf]) -> Result<usize> {
    let keep: std::collections::HashSet<PathBuf> = referenced
        .iter()
        .flat_map(|a| [a.clone(), a.with_extension("pmtiles.json")])
        .collect();
    let now = std::time::SystemTime::now();
    let mut n = 0usize;
    for level in std::fs::read_dir(blocks_dir)?.flatten() {
        if !level.path().is_dir() {
            continue;
        }
        for e in std::fs::read_dir(level.path())?.flatten() {
            let p = e.path();
            if keep.contains(&p) {
                continue;
            }
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|d| d.as_secs() > BLOCK_PRUNE_AGE_S);
            if old {
                std::fs::remove_file(&p)?;
                n += 1;
            }
        }
    }
    Ok(n)
}

pub fn run(cfg: &Config, args: TilesArgs) -> Result<()> {
    // `block_of` teilt durch diesen Wert.
    if args.block_cells < 1 {
        bail!("--block-cells muss >= 1 sein (Standard {DEFAULT_BLOCK_CELLS})");
    }
    let t0 = Instant::now();
    if args.n_jobs > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.n_jobs as usize)
            .build_global()
            .ok();
    }
    let mut parts: Vec<PathBuf> = std::fs::read_dir(&args.zones_dir)
        .with_context(|| format!("{} lesen", args.zones_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "parquet"))
        .collect();
    parts.sort();
    if parts.is_empty() {
        bail!(
            "keine Zonen-Parquet-Dateien in {}",
            args.zones_dir.display()
        );
    }
    let s = &cfg.simplify;
    let levels = zoom_levels(
        &s.tolerances_m,
        args.min_zoom,
        args.max_zoom,
        s.merge_bands_up_to_zoom,
        if args.buildings_path.is_some() {
            s.subtract_buildings_max_tolerance_m
        } else {
            -1.0
        },
    )?;
    // Gebaeude fuer den Grundrissabzug aus dem Gebaeudespeicher (bstore, je
    // Zelle abgefragt) statt aller ~39 Mio. Grundrisse samt R-Baum im Speicher.
    let store: Option<BuildingStore> = match &args.buildings_path {
        Some(p) => {
            let s = BuildingStore::open_or_build(p)?;
            info!(
                "tiles: {} opake Gebaeude fuer den Grundrissabzug in {}",
                s.len(),
                s.path().display()
            );
            Some(s)
        }
        None => None,
    };
    let crs_out = cfg.crs.output.clone();
    let out_dir = args.out_path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(out_dir)?;
    let stem = args
        .out_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let work_dir = out_dir.join(format!("{stem}.tiles-work"));
    let cache_dir = out_dir.join(format!("{stem}.tiles-cache"));
    if args.force && work_dir.exists() {
        std::fs::remove_dir_all(&work_dir)?;
    }
    if args.clear_cache && cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir)?;
    }
    ensure_work_dir(&work_dir, &parts_fingerprint(&parts, &levels, &crs_out)?)?;
    std::fs::create_dir_all(&cache_dir)?;

    // Cache-Phase
    let t_cache = Instant::now();
    let needs_buildings = levels.iter().any(|l| l.subtract_buildings);
    if needs_buildings && store.is_none() {
        bail!(
            "Gebaeudeabzug konfiguriert (subtract_buildings_max_tolerance_m), aber kein --buildings-path angegeben"
        );
    }
    let proj = Utm32::new();
    // Kennungen je Zelle (parallel: Zeilen lesen, Gebaeude des Suchkastens
    // aus dem Speicher hashen). Je anstehender Zelle bleibt nur ihr
    // Suchkasten; die Grundrisse werden in der parallelen Phase erneut
    // abgefragt und erst dort dekodiert.
    let mut digests: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut pending: Vec<(PathBuf, Option<[f64; 4]>)> = Vec::new();
    // (Zellname, cell_digest, buildings_digest, Suchkasten)
    type CellInfo = (String, String, String, Option<[f64; 4]>);
    let infos: Vec<Result<CellInfo>> = parts
        .par_iter()
        .map(|part| {
            let rows = read_zone_cell(part)?;
            let digest = cell_digest(&rows, &crs_out);
            let cell_name = part.file_stem().unwrap().to_string_lossy().to_string();
            let mut bdigest = String::new();
            let mut bbox = None;
            if let Some(s) = &store {
                let b = match geo_bbox(part)? {
                    Some(b) => b,
                    None => {
                        let shapes: Vec<Shapes> = rows
                            .iter()
                            .map(|r| crate::wkb::parse(&r.wkb))
                            .collect::<Result<_>>()?;
                        shapes_bounds(&shapes.into_iter().flatten().collect::<Vec<_>>())
                    }
                };
                let hits = s.query(b)?;
                let mut wkbs = hits.wkbs();
                bdigest = buildings_digest(&mut wkbs);
                bbox = Some(b);
            }
            Ok((cell_name, digest, bdigest, bbox))
        })
        .collect();
    for (part, info) in parts.iter().zip(infos) {
        let (cell_name, digest, bdigest, bbox) = info?;
        if !levels
            .iter()
            .all(|lv| cache_entry_path(&cache_dir, &cell_name, lv, &digest, &bdigest).exists())
        {
            pending.push((part.clone(), bbox));
        }
        digests.insert(cell_name, (digest, bdigest));
    }
    info!(
        "tiles: {} Zellen, davon {} im Cache vorhanden, {} zu vereinfachen ({} Zoombaender)",
        parts.len(),
        parts.len() - pending.len(),
        pending.len(),
        levels.len()
    );
    let done = AtomicUsize::new(0);
    let n_pending = pending.len();
    let results: Vec<Result<usize>> = pending
        .par_iter()
        .map(|(part, bbox)| {
            let cell_name = part.file_stem().unwrap().to_string_lossy().to_string();
            let (digest, bdigest) = &digests[&cell_name];
            let cb: Vec<Shape> = match (&store, bbox) {
                (Some(s), Some(b)) => {
                    let hits = s.query(*b)?;
                    (0..hits.len())
                        .map(|i| crate::wkb::parse(hits.wkb(i)))
                        .collect::<Result<Vec<Shapes>>>()?
                        .into_iter()
                        .flatten()
                        .collect()
                }
                _ => Vec::new(),
            };
            let r = process_cell(part, digest, bdigest, &cb, &levels, &cache_dir, &proj);
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if d.is_multiple_of(200) || d == n_pending {
                info!(
                    "tiles: {d}/{n_pending} Zellen vereinfacht ({:.0} min)",
                    t_cache.elapsed().as_secs_f64() / 60.0
                );
            }
            r
        })
        .collect();
    for r in results {
        r?;
    }
    let cache_phase_s = t_cache.elapsed().as_secs_f64();

    // tippecanoe je Zoomband und raeumlichem Block; jedes Blockarchiv ist
    // inhaltsadressiert und wird bei gleicher Kennung uebernommen.
    let t_blocks = Instant::now();
    let mut level_inputs: Vec<(ZoomLevel, Vec<PathBuf>)> = Vec::new();
    let mut n_features: Option<usize> = None;
    let mut subtract_seen = false;
    let blocks_dir = out_dir.join(format!("{stem}.tiles-blocks"));
    let tc_version = tippecanoe_version()?;
    if args.clear_cache && blocks_dir.exists() {
        std::fs::remove_dir_all(&blocks_dir)?;
    }
    let n_jobs = if args.n_jobs > 0 {
        args.n_jobs as usize
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    };
    let threads_per_proc = 2usize;
    let concurrency = (n_jobs / threads_per_proc).max(1);
    let (mut blocks_built, mut blocks_reused, mut blocks_tc_s) = (0usize, 0usize, 0f64);
    let mut referenced: Vec<PathBuf> = Vec::new();
    for level in &levels {
        let r = build_blocks(
            &parts,
            &digests,
            level,
            &cache_dir,
            &blocks_dir,
            args.block_cells,
            &tc_version,
            &work_dir,
            concurrency,
            threads_per_proc,
        )?;
        blocks_built += r.built;
        blocks_reused += r.reused;
        blocks_tc_s += r.tippecanoe_s;
        referenced.extend(r.archives.iter().cloned());
        let n = r.n_features;
        level_inputs.push((level.clone(), r.archives));
        if level.merge_bands {
            info!("tiles: {}: {n} vereinigte Merkmale", level.tag());
            continue;
        }
        if level.subtract_buildings {
            info!("tiles: {}: {n} Merkmale nach Gebaeudeabzug", level.tag());
            if n_features.is_some_and(|nf| n > nf) {
                bail!(
                    "Merkmalszahl mit Gebaeudeabzug ({n}) groesser als ohne ({})",
                    n_features.unwrap()
                );
            }
            if n_features.is_none() {
                n_features = Some(n);
            }
            subtract_seen = true;
            continue;
        }
        if n_features.is_none() || subtract_seen {
            n_features = Some(n);
            subtract_seen = false;
        } else if Some(n) != n_features {
            bail!(
                "Merkmalszahl weicht zwischen Zoombaendern ab: {n} vs. {:?}",
                n_features
            );
        }
    }
    let block_phase_s = t_blocks.elapsed().as_secs_f64() - blocks_tc_s;
    let n_features = n_features.unwrap_or(digests.len());

    // Die Blockarchive sind schon pmtiles; es bleibt das tile-join.
    let t_tc = Instant::now();
    let archives: Vec<PathBuf> = level_inputs.iter().flat_map(|(_, a)| a.clone()).collect();
    let tippecanoe_s = t_tc.elapsed().as_secs_f64() + blocks_tc_s;
    let t_join = Instant::now();
    let tmp_out = work_dir.join(args.out_path.file_name().unwrap());
    info!("tiles: tile-join ueber {} Teilarchive", archives.len());
    let list = work_dir.join("join_inputs.txt");
    std::fs::write(
        &list,
        archives
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )?;
    let out = Command::new("tile-join")
        .args(["--force", "--no-tile-size-limit", "--no-tile-stats"])
        .arg(format!("--output={}", tmp_out.display()))
        .arg(format!("--read-from={}", list.display()))
        .output()
        .context("tile-join starten")?;
    if !out.status.success() {
        bail!(
            "tile-join fehlgeschlagen (Exit {:?}):\n{}\n{}\nZwischendateien bleiben erhalten unter {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            work_dir.display()
        );
    }
    scrub_metadata(&tmp_out, LAYER_NAME, ZONES_DESCRIPTION)?;
    info!("tiles: pmtiles verify: {}", verify(&tmp_out)?);
    std::fs::rename(&tmp_out, &args.out_path)?;
    std::fs::remove_dir_all(&work_dir)?;
    let join_s = t_join.elapsed().as_secs_f64();
    let n = prune_blocks(&blocks_dir, &referenced)?;
    if n > 0 {
        info!("tiles: {n} nicht mehr gebrauchte Blockarchive entfernt");
    }
    let summary = serde_json::json!({
        "stage": "tiles",
        "elapsed_s": (t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
        "n_features": n_features,
        "n_cells": parts.len(),
        "n_cells_reused": parts.len() - n_pending,
        "levels": levels.iter().map(|l| l.tag()).collect::<Vec<_>>(),
        "out_path": args.out_path.to_string_lossy(),
        "cache_phase_s": cache_phase_s,
        "block_phase_s": block_phase_s,
        "tippecanoe_s": tippecanoe_s,
        "join_s": join_s,
        "block_cells": args.block_cells,
        "blocks_built": blocks_built,
        "blocks_reused": blocks_reused,
        "tippecanoe": tc_version,
        "min_zoom": args.min_zoom,
        "max_zoom": args.max_zoom,
        "n_jobs": args.n_jobs,
        "engine": "bubatz",
        "simplify_version": SIMPLIFY_VERSION,
    });
    std::fs::write(
        out_dir.join(format!("{stem}_tiles_summary.json")),
        serde_json::to_string_pretty(&summary)?,
    )?;
    println!(
        "Zonen: {n_features} in {} Zellen ({} aus dem Cache), Zoombaender {} -> {}",
        parts.len(),
        parts.len() - n_pending,
        levels
            .iter()
            .map(|l| l.tag())
            .collect::<Vec<_>>()
            .join(", "),
        args.out_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_levels_match_python_staircase_and_tags() {
        let tol: BTreeMap<String, f64> =
            [("z8", 200.0), ("z10", 80.0), ("z12", 25.0), ("z14", 0.5)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        let lv = zoom_levels(&tol, 8, 14, 11, 8.0).unwrap();
        let tags: Vec<String> = lv.iter().map(|l| l.tag()).collect();
        assert_eq!(
            tags,
            [
                "z8-9_t200_merged",
                "z10-11_t80_merged",
                "z12-13_t25",
                "z14-14_t0.5_nobld"
            ]
        );
        assert_eq!(lv[3].sub_dir(), "t0.5_nobld");
        assert_eq!(lv[0].sub_dir(), "t200_merged");
    }

    #[test]
    fn blocks_partition_cells_euclidean() {
        assert_eq!(block_of("x173_y2836", 25).as_deref(), Some("b6_113"));
        assert_eq!(block_of("x0_y0", 25).as_deref(), Some("b0_0"));
        assert_eq!(block_of("x-1_y-26", 25).as_deref(), Some("b-1_-2"));
        assert_eq!(block_of("x24_y49", 25).as_deref(), Some("b0_1"));
        assert_eq!(block_of("x25_y50", 25).as_deref(), Some("b1_2"));
        assert_eq!(block_of("cell", 25), None);
    }

    #[test]
    fn block_digest_reacts_to_every_input_and_ignores_order() {
        let lv = ZoomLevel {
            min_zoom: 12,
            max_zoom: 13,
            tolerance_m: 25.0,
            merge_bands: false,
            subtract_buildings: false,
        };
        let a = ("x1_y1".to_string(), "d1".to_string(), "b1".to_string());
        let b = ("x1_y2".to_string(), "d2".to_string(), "".to_string());
        let base = block_digest("tippecanoe v2.82.0", &lv, &[a.clone(), b.clone()]);
        assert_eq!(
            base,
            block_digest("tippecanoe v2.82.0", &lv, &[b.clone(), a.clone()]),
            "Reihenfolge egal"
        );
        assert_ne!(
            base,
            block_digest("tippecanoe v2.83.0", &lv, &[a.clone(), b.clone()]),
            "Version"
        );
        let lv2 = ZoomLevel {
            max_zoom: 14,
            ..lv.clone()
        };
        assert_ne!(
            base,
            block_digest("tippecanoe v2.82.0", &lv2, &[a.clone(), b.clone()]),
            "Zoombereich"
        );
        let a2 = (a.0.clone(), "d9".to_string(), a.2.clone());
        assert_ne!(
            base,
            block_digest("tippecanoe v2.82.0", &lv, &[a2, b.clone()]),
            "Zellkennung"
        );
        let a3 = (a.0.clone(), a.1.clone(), "b9".to_string());
        assert_ne!(
            base,
            block_digest("tippecanoe v2.82.0", &lv, &[a3, b.clone()]),
            "Gebaeudekennung"
        );
        assert_ne!(
            base,
            block_digest("tippecanoe v2.82.0", &lv, std::slice::from_ref(&a)),
            "Zellmenge"
        );
    }

    #[test]
    fn coordinates_are_formatted_like_gdal() {
        assert_eq!(fmt7(6.7054117), "6.7054117");
        assert_eq!(fmt7(51.230789), "51.230789");
        assert_eq!(fmt7(51.2), "51.2");
        assert_eq!(fmt7(7.0), "7");
    }

    fn quadrat() -> Shapes {
        vec![vec![vec![
            [340_000.0, 5_680_000.0],
            [340_010.0, 5_680_000.0],
            [340_010.0, 5_680_010.0],
            [340_000.0, 5_680_010.0],
        ]]]
    }

    /// Kachelvertrag: `osm_id`, `osm_type`, `category`, `band_max_m` -- und
    /// sonst nichts. Der Viewer baut aus Typ und Kennung den Link nach
    /// OpenStreetMap.
    #[test]
    fn tile_properties_carry_the_osm_identity() {
        let proj = Utm32::new();
        let mut out = String::new();
        geojson_line(
            &Feature {
                osm_id: 29_359_108,
                osm_type: "way".to_string(),
                category: "playground".to_string(),
                band_max_m: 100,
                shapes: quadrat(),
            },
            &proj,
            &mut out,
        );
        let v: serde_json::Value = serde_json::from_str(out.trim()).expect("GeoJSON");
        let props = v["properties"].as_object().expect("properties");
        assert_eq!(props["osm_id"], 29_359_108);
        assert_eq!(props["osm_type"], "way");
        assert_eq!(props["category"], "playground");
        assert_eq!(props["band_max_m"], 100);
        assert_eq!(props.len(), 4, "keine weiteren Eigenschaften: {props:?}");
    }

    /// Der Zellenabdruck entscheidet, welche Zelle aus dem Kachel-Cache kommt,
    /// und `scripts/rollout_check_tiles.py` rechnet ihn mit Pythons
    /// `hashlib.blake2b` nach. Aendert eine neue Fassung der Hash-Kiste die
    /// Bytes, findet die Pruefung vor der Veroeffentlichung keinen Eintrag mehr
    /// -- also hier festnageln, nicht dort entdecken.
    #[test]
    fn cell_digest_bytes_are_pinned() {
        let r = crate::parquet_io::ZoneCellRow {
            osm_id: 7,
            osm_type: "way".to_string(),
            category: "school".to_string(),
            band_max_m: 100,
            cache_key: "k".to_string(),
            wkb: Vec::new(),
        };
        assert_eq!(cell_digest(&[r], "EPSG:25832"), "ae01683765fcd3da09a4e731");
    }

    /// Der Zellenabdruck muss auf osm_type reagieren, sonst liefe eine Zelle
    /// mit geaenderten Kacheln aus dem Cache.
    #[test]
    fn cell_digest_reacts_to_osm_type() {
        let zeile = |t: &str| crate::parquet_io::ZoneCellRow {
            osm_id: 7,
            osm_type: t.to_string(),
            category: "school".to_string(),
            band_max_m: 100,
            cache_key: "k".to_string(),
            wkb: Vec::new(),
        };
        assert_ne!(
            cell_digest(&[zeile("way")], "EPSG:25832"),
            cell_digest(&[zeile("relation")], "EPSG:25832")
        );
    }

    #[test]
    fn scrubbed_metadata_drops_paths_and_command_lines() {
        let raw = serde_json::json!({
            "name": "/srv/x/out/zonen.tiles-blocks/a.pmtiles + /srv/x/out/b.pmtiles",
            "description": "/srv/x/out/c.pmtiles",
            "generator": "tile-join v2.79.0",
            "generator_options": "tippecanoe --force '--output=/srv/x/out/a.pmtiles' /srv/x/cache/x1.geojsonl",
            "format": "pbf",
            "type": "overlay",
            "version": "2",
            "vector_layers": [{"id": "zonen", "fields": {"osm_id": "Number"}}]
        });
        assert_eq!(metadata_paths(&raw).len(), 3);
        let clean = scrubbed_metadata(raw, LAYER_NAME, ZONES_DESCRIPTION);
        assert!(clean.get("generator_options").is_none());
        assert_eq!(clean["name"], LAYER_NAME);
        assert_eq!(clean["description"], ZONES_DESCRIPTION);
        assert_eq!(clean["attribution"], ATTRIBUTION);
        assert_eq!(clean["generator"], "tile-join v2.79.0");
        assert_eq!(clean["vector_layers"][0]["fields"]["osm_id"], "Number");
        assert!(metadata_paths(&clean).is_empty(), "{clean}");
    }

    #[test]
    fn metadata_paths_looks_into_nested_values() {
        let v = serde_json::json!({"a": {"b": ["x", "/tmp/y"]}, "c": "ohne", "d": 3});
        assert_eq!(metadata_paths(&v), vec!["/tmp/y".to_string()]);
    }
}
