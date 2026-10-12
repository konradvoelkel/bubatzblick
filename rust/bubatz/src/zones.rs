//! `bubatz zones` -- Sichtzonen je Verbotsobjekt, Gitterzelle fuer Gitterzelle:
//! Gitter, Suchkasten je Zelle, Hindernisse je Objekt, Zone im Kern,
//! Baender, eine GeoParquet-Datei je Zelle (atomar), Manifest, Summary,
//! Fortschrittszeile; parallel ueber Zellen (dichteste zuerst).
//!
//! **Zellen-Fingerabdruck:** `process_cell` ist
//! deterministisch in drei Eingaben -- Konfiguration (Parameterversion,
//! Zellgroesse, Reichweite), Objekte der Zelle (in Zuordnungsreihenfolge) und
//! Gebaeude im Suchkasten. Ueber genau diese Bytes wird je Zelle ein
//! Fingerabdruck gebildet und als Parquet-Metadatum `bubatz.cell_fp` in die
//! Zellendatei geschrieben. Traegt die Vorlaufzelle (`--reuse-from`)
//! denselben Fingerabdruck, wird sie per Hardlink uebernommen -- ohne
//! Hindernisauswahl, `cache_key` und Schreiben. Sonst laeuft die Zelle wie
//! der objektweisen Wiederverwendung per `cache_key`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use bubatzcore::geom::{Shape, Shapes};
use clap::Args;
use rayon::prelude::*;
use tracing::info;

use crate::bstore::BuildingStore;
use crate::config::Config;
use crate::geoparquet::{ZoneRow, write_cell_with_meta};
use crate::grid::{Cell, build_grid, cell_index, centroid};
use crate::obstacles::{CellBuilding, cache_key, decode_cell_buildings, merged_obstacles};
use crate::parquet_io::{ProtectedRow, kv_metadata, read_previous_cell, read_protected};

#[derive(Args, Debug)]
pub struct ZonesArgs {
    #[arg(long, default_value = "data/interim/protected.parquet")]
    pub protected_path: PathBuf,
    #[arg(long, default_value = "data/interim/buildings.parquet")]
    pub buildings_path: PathBuf,
    #[arg(long, default_value = "data/interim/zones")]
    pub out_dir: PathBuf,
    /// Parallele Arbeiter (Threads), -1 = alle Kerne
    #[arg(long, default_value_t = -1)]
    pub n_jobs: i64,
    /// Ueberschreibt grid.cell_size_m
    #[arg(long)]
    pub cell_size_m: Option<f64>,
    /// Vorhandene Zellendateien ignorieren und neu rechnen
    #[arg(long, default_value_t = false)]
    pub force: bool,
    /// Zonenverzeichnis eines Vorlaufs (gleiche Zellgroesse): gleicher cache_key -> Baender uebernehmen
    #[arg(long)]
    pub reuse_from: Option<PathBuf>,
    /// Fortschrittszeile alle N Sekunden
    #[arg(long, default_value_t = 60.0)]
    pub log_interval_s: f64,
    /// Nur diese Zellen rechnen: Datei mit einer Zellen-ID je Zeile, oder ein
    /// Zonenverzeichnis (dessen *.parquet-Namen). Fuer Teilvergleiche.
    #[arg(long)]
    pub only_cells: Option<PathBuf>,
    /// Nur Zellenzuordnung als JSON schreiben (Diagnose)
    #[arg(long)]
    pub dump_cells: Option<PathBuf>,
    /// Nur cache_keys je Objekt als JSON schreiben (Diagnose)
    #[arg(long)]
    pub dump_keys: Option<PathBuf>,
    /// Nur Beobachterpunkte je Objekt (Bits als u64) als JSON schreiben (Diagnose)
    #[arg(long)]
    pub dump_points: Option<PathBuf>,
    /// Zellen-Fingerabdruck NICHT zum Uebernehmen ganzer Zellen aus --reuse-from nutzen (Vergleichslaeufe)
    #[arg(long, default_value_t = false)]
    pub no_cell_link: bool,
}

/// Parquet-Metadatum mit dem Zellen-Fingerabdruck (P1).
pub const CELL_FP_KEY: &str = "bubatz.cell_fp";
/// Schema des Fingerabdrucks; aendern, wenn sich die Eingabenliste aendert --
/// ODER das SCHEMA der geschriebenen Zellendatei. Sonst wird eine Zelle per
/// Hardlink uebernommen und behaelt still die alten Spalten.
///
/// Wer eine Spalte hinzufuegt und die Version stehen laesst, baut sich genau
/// diesen Fehler: die Zonenstufe uebernimmt alle Zellen per Fingerabdruck, die
/// Kachelstufe findet die neue Spalte nicht, laesst die Eigenschaft
/// (richtigerweise) weg -- und `publish` meldet trotzdem OK.
///
/// Ein Wechsel hier ist billig: die `cache_key` der Objekte bleiben gueltig,
/// `process_cell` laeuft zwar wieder, uebernimmt aber jedes Band aus dem
/// Vorlauf und rechnet kein Sichtfeld neu. Das unterscheidet ihn von
/// `param_version`, der auch die Geometrie entwertet.
const CELL_FP_VERSION: &str = "fp1";

/// Fingerabdruck einer Zelle ueber genau die Eingaben von `process_cell`:
/// Konfiguration, Objekte in Zuordnungsreihenfolge (osm_id, Kategorie, WKB)
/// und die Gebaeude des Suchkastens (WKB, sortiert -- die Verschmelzung
/// sortiert ebenfalls, die Reihenfolge der Indexabfrage ist ohne Bedeutung).
pub fn cell_fingerprint<'a>(
    param_version: &str,
    cell_size: f64,
    reach: f64,
    objects: impl Iterator<Item = (i64, &'a str, &'a [u8])>,
    mut building_wkbs: Vec<&[u8]>,
) -> String {
    use blake2::{Blake2b512, Digest};
    let mut h = Blake2b512::new();
    h.update(format!("{CELL_FP_VERSION}|{param_version}|{cell_size:?}|{reach:?}|").as_bytes());
    for (osm_id, category, wkb) in objects {
        h.update(format!("O|{osm_id}|{category}|{}|", wkb.len()).as_bytes());
        h.update(wkb);
    }
    building_wkbs.sort_unstable();
    h.update(format!("B|{}|", building_wkbs.len()).as_bytes());
    for w in building_wkbs {
        h.update((w.len() as u64).to_le_bytes());
        h.update(w);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub const MANIFEST_NAME: &str = "zones_manifest.json";

fn input_fingerprint(
    cfg: &Config,
    protected: &Path,
    buildings: &Path,
) -> Result<serde_json::Value> {
    let mut fp = serde_json::Map::new();
    fp.insert(
        "param_version".into(),
        serde_json::json!(cfg.param_version()),
    );
    for (name, path) in [("protected", protected), ("buildings", buildings)] {
        let md = std::fs::metadata(path).with_context(|| format!("{} lesen", path.display()))?;
        let mtime_ns = md
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i128)
            .unwrap_or(0);
        fp.insert(
            name.into(),
            serde_json::json!({
                "path": std::fs::canonicalize(path)?.to_string_lossy(),
                "size": md.len(),
                "mtime_ns": mtime_ns,
            }),
        );
    }
    Ok(serde_json::Value::Object(fp))
}

/// `--only-cells`: Zellen-IDs aus einer Textdatei (eine je Zeile, `#` = Kommentar)
/// oder aus den `*.parquet`-Dateinamen eines Zonenverzeichnisses.
fn read_cell_list(path: &Path) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    if path.is_dir() {
        for e in std::fs::read_dir(path)? {
            let name = e?.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".parquet") {
                out.insert(stem.to_string());
            }
        }
    } else {
        for line in std::fs::read_to_string(path)
            .with_context(|| format!("{} lesen", path.display()))?
            .lines()
        {
            let s = line.trim();
            if !s.is_empty() && !s.starts_with('#') {
                out.insert(s.to_string());
            }
        }
    }
    if out.is_empty() {
        bail!("--only-cells {}: keine Zellen-IDs", path.display());
    }
    Ok(out)
}

/// Manifest ohne die absoluten Pfade der Eingaben: fuer den Vergleich zaehlen
/// Parameterversion, Groesse und mtime -- ein per `rsync -a` auf einen anderen
/// Rechner (anderen Einhaengepunkt) uebertragenes Zonenverzeichnis bleibt so
/// wiederaufnehmbar -- ein Bundeslauf darf einen Abbruch ueberleben.
fn manifest_comparable(fp: &serde_json::Value) -> serde_json::Value {
    let mut v = fp.clone();
    if let Some(m) = v.as_object_mut() {
        for entry in m.values_mut() {
            if let Some(o) = entry.as_object_mut() {
                o.remove("path");
            }
        }
    }
    v
}

/// `_check_manifest`: Wiederaufnahme nur ueber unveraenderten Eingaben.
fn check_manifest(out_dir: &Path, fingerprint: &serde_json::Value, force: bool) -> Result<()> {
    let manifest = out_dir.join(MANIFEST_NAME);
    let has_cells = std::fs::read_dir(out_dir)?
        .flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "parquet"));
    if has_cells && !force && manifest.exists() {
        let previous: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest)?)?;
        if manifest_comparable(&previous) != manifest_comparable(fingerprint) {
            bail!(
                "{} enthaelt Zellen eines Laufs mit anderen Eingaben (Manifest: {previous}; jetzt: {fingerprint}). \
                 Fuer einen neuen Datenstand ein neues --out-dir waehlen (ggf. mit --reuse-from auf das alte), \
                 oder --force fuer eine komplette Neuberechnung.",
                out_dir.display()
            );
        }
    }
    let tmp = out_dir.join(format!("{MANIFEST_NAME}.tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(fingerprint)?)?;
    std::fs::rename(&tmp, &manifest)?;
    Ok(())
}

struct ParsedObject<'a> {
    row: &'a ProtectedRow,
    shapes: Shapes,
    bounds: [f64; 4],
}

/// Baender eines Objekts (wie `zones_from_context`): Zone im Kern, Ringzerlegung,
/// leere Baender entfallen. Rueckgabe je Band: (band_max_m, Shapes).
/// Die Zone eines Objekts (V_hat, Parameterversion v7): Beobachterpunkte
/// auf der Kontur, Sichtfeld im Kern, Marge, Quelle, Freikreis. Getrennt
/// von der Ringzerlegung, damit die Vertragstests (`contract_tests.rs`)
/// genau diese Funktion pruefen koennen.
pub(crate) fn visible_zone(
    cfg: &Config,
    shapes: &[Shape],
    bounds: [f64; 4],
    merged: &[Shape],
) -> Shapes {
    let v = &cfg.visibility;
    let reach = cfg.effective_reach_m();
    let t0 = Instant::now();
    let points = crate::sample::sample_contour(shapes, v.spacing_m, v.min_points, v.max_points);
    prof_add(&PROF_SAMPLE_NS, t0);
    let margin = if v.margin_m > 0.0 {
        v.margin_m
            .max(crate::sample::contour_spacing(shapes, points.len()) / 4.0)
    } else {
        0.0
    };
    let t0 = Instant::now();
    let free = bubatzcore::free_circle(shapes, reach);
    bubatzcore::profile::add(&bubatzcore::profile::FREE_NS, t0);
    let obstacle_rings: Vec<Vec<[f64; 2]>> = merged.iter().flatten().cloned().collect();
    let v_hat = if obstacle_rings.is_empty() {
        free
    } else {
        bubatzcore::visible_region(
            &obstacle_rings,
            reach,
            reach + (bounds[2] - bounds[0]).hypot(bounds[3] - bounds[1]),
            &points,
            shapes,
            &free,
            margin,
            bubatzcore::ROUND_STEP_QUAD_SEGS_16,
            bubatzcore::DEFAULT_CHUNK,
        )
    };
    // Python-Roundtrip (`_geom_of` -> `_shapes_of`): Ringe unter 3 Punkten entfallen.
    drop_degenerate(&v_hat)
}

fn compute_bands(cfg: &Config, obj: &ParsedObject, merged: &[Shape]) -> Vec<(i64, Shapes)> {
    let v_hat = visible_zone(cfg, &obj.shapes, obj.bounds, merged);
    let radii: Vec<f64> = {
        let mut r: Vec<f64> = cfg.reach.bands_m.iter().map(|&x| x as f64).collect();
        r.sort_by(f64::total_cmp);
        r
    };
    let t0 = Instant::now();
    let bands = bubatzcore::decompose_into_bands(&v_hat, &obj.shapes, &radii);
    bubatzcore::profile::add(&bubatzcore::profile::BANDS_NS, t0);
    radii
        .iter()
        .zip(bands)
        .filter_map(|(r, b)| {
            let b = drop_degenerate(&b);
            if b.is_empty() {
                None
            } else {
                Some((*r as i64, b))
            }
        })
        .collect()
}

fn drop_degenerate(shapes: &[Shape]) -> Shapes {
    shapes
        .iter()
        .filter(|s| s.first().is_some_and(|r| r.len() >= 3))
        .map(|s| {
            let mut out: Shape = Vec::with_capacity(s.len());
            out.push(s[0].clone());
            out.extend(s[1..].iter().filter(|h| h.len() >= 3).cloned());
            out
        })
        .collect()
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

static PROF_OBSTACLES_NS: AtomicU64 = AtomicU64::new(0);
static PROF_KEY_NS: AtomicU64 = AtomicU64::new(0);
static PROF_SAMPLE_NS: AtomicU64 = AtomicU64::new(0);
static PROF_WRITE_NS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn prof_add(c: &AtomicU64, t0: Instant) {
    c.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

/// `BUBATZ_PROFILE=1`: Phasenzeiten (CPU-Sekunden ueber alle Threads).
fn print_profile(wall_s: f64, n_jobs: usize) {
    if std::env::var_os("BUBATZ_PROFILE").is_none() {
        return;
    }
    let s = |c: &AtomicU64| c.load(Ordering::Relaxed) as f64 / 1e9;
    let mut rows = vec![
        ("obstacles (dwithin+union)", s(&PROF_OBSTACLES_NS)),
        ("cache_key", s(&PROF_KEY_NS)),
        ("sample", s(&PROF_SAMPLE_NS)),
    ];
    rows.extend(bubatzcore::profile::snapshot());
    rows.push(("write", s(&PROF_WRITE_NS)));
    let total: f64 = rows.iter().map(|r| r.1).sum();
    eprintln!(
        "profile: {:.1} CPU-s in Phasen, Wand {:.1} s x {} Threads = {:.1} s; Sweep-Punkte {}",
        total,
        wall_s,
        n_jobs,
        wall_s * n_jobs as f64,
        bubatzcore::profile::sweep_points()
    );
    for (name, secs) in rows {
        eprintln!(
            "  {name:<28} {secs:9.1} s  {:5.1} %",
            100.0 * secs / total.max(1e-9)
        );
    }
    let (segs, angs) = bubatzcore::profile::sweep_means();
    eprintln!("  je Punkt: {segs:.0} Kanten, {angs:.0} Kandidatenwinkel");
}

struct CellResult {
    n_objects: usize,
    n_zones: usize,
    n_reused: usize,
}

/// Baender des Vorlaufs derselben Zelle nach (`osm_id`, `cache_key`), je
/// `band_max_m` genau eines.
///
/// Der Schluessel ist (`osm_id`, `cache_key`), nicht der `cache_key` allein:
/// zwei Objekte mit identischer Geometrie (z. B. Way und Relation desselben
/// Umrisses) teilen ihn, und jedes erbte sonst die Baender BEIDER -- je Zyklus
/// verdoppelt. Die `BTreeMap` je Objekt haelt die Invariante: ein Band je
/// `band_max_m`, in aufsteigender Reihenfolge.
type PreviousMap = HashMap<(i64, String), BTreeMap<i64, crate::parquet_io::PreviousBand>>;

fn previous_by_key(reuse_from: Option<&Path>, cell_file: &str) -> Result<PreviousMap> {
    let mut m: PreviousMap = HashMap::new();
    if let Some(dir) = reuse_from {
        for b in read_previous_cell(&dir.join(cell_file))? {
            m.entry((b.osm_id, b.cache_key.clone()))
                .or_default()
                .entry(b.band_max_m)
                .or_insert(b);
        }
    }
    Ok(m)
}

fn process_cell(
    cfg: &Config,
    cell: &Cell,
    objects: &[ParsedObject],
    cell_buildings: &[CellBuilding],
    out_path: &Path,
    reuse_from: Option<&Path>,
    cell_fp: &str,
) -> Result<CellResult> {
    let reach = cfg.effective_reach_m();
    let version = cfg.param_version();
    let previous = previous_by_key(reuse_from, &format!("{}.parquet", cell.id()))?;
    // Objekte parallel (rayon, verschachtelt im Zellen-Arbeiter): im
    // Hauptteil stehlen sich die Threads nichts, am Schwanz teilen sich
    // freie Threads die letzten grossen Zellen. Ergebnisreihenfolge bleibt.
    let per_object: Vec<Result<(Vec<ZoneRow>, bool)>> = {
        use rayon::prelude::*;
        objects
            .par_iter()
            .map(|obj| -> Result<(Vec<ZoneRow>, bool)> {
                let mut rows: Vec<ZoneRow> = Vec::new();
                let t0 = Instant::now();
                let merged = merged_obstacles(cell_buildings, &obj.shapes, obj.bounds, reach);
                prof_add(&PROF_OBSTACLES_NS, t0);
                let t0 = Instant::now();
                let key = cache_key(&obj.row.wkb, &merged, version);
                prof_add(&PROF_KEY_NS, t0);
                if let Some(prev) = previous.get(&(obj.row.osm_id, key.clone())) {
                    // Wie `PreviousZones.zones_for`: Baender des Vorlaufs uebernehmen.
                    for b in prev.values() {
                        let shapes = crate::wkb::parse(&b.wkb)?;
                        rows.push(ZoneRow {
                            osm_id: obj.row.osm_id,
                            // Kennung und Kategorie kommen aus dem AKTUELLEN
                            // Objekt, nur die Geometrie aus dem Vorlauf --
                            // deshalb braucht `zones.prev` kein osm_type.
                            osm_type: obj.row.osm_type.clone(),
                            category: obj.row.category.clone(),
                            band_max_m: b.band_max_m,
                            wkb: b.wkb.clone(),
                            is_multi: shapes.len() > 1,
                            bounds: shapes_bounds(&shapes),
                            cache_key: key.clone(),
                        });
                    }
                    return Ok((rows, true));
                }
                for (band_max_m, shapes) in compute_bands(cfg, obj, &merged) {
                    rows.push(ZoneRow {
                        osm_id: obj.row.osm_id,
                        osm_type: obj.row.osm_type.clone(),
                        category: obj.row.category.clone(),
                        band_max_m,
                        wkb: crate::wkb::write(&shapes),
                        is_multi: shapes.len() > 1,
                        bounds: shapes_bounds(&shapes),
                        cache_key: key.clone(),
                    });
                }
                Ok((rows, false))
            })
            .collect()
    };
    let mut rows: Vec<ZoneRow> = Vec::new();
    let mut n_reused = 0usize;
    for r in per_object {
        let (mut obj_rows, reused) = r?;
        if reused {
            n_reused += 1;
        }
        rows.append(&mut obj_rows);
    }
    let t0 = Instant::now();
    write_cell_with_meta(out_path, &rows, &[(CELL_FP_KEY, cell_fp)])
        .with_context(|| format!("Zelle {}", cell.id()))?;
    prof_add(&PROF_WRITE_NS, t0);
    Ok(CellResult {
        n_objects: objects.len(),
        n_zones: rows.len(),
        n_reused,
    })
}

pub fn run(cfg: &Config, args: ZonesArgs) -> Result<()> {
    let t0 = Instant::now();
    if args.n_jobs > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.n_jobs as usize)
            .build_global()
            .ok();
    }
    let protected = read_protected(&args.protected_path)?;
    // Gebaeude bleiben auf Platte (bstore, je Zelle abgefragt) -- national
    // ~39 Mio. Grundrisse samt R-Baum kosten im Speicher ~9,5 GB.
    let store = BuildingStore::open_or_build(&args.buildings_path)?;
    info!(
        "zones: {} Verbotsobjekte, {} opake Gebaeude im Grundrissspeicher {} ({:.0} s)",
        protected.len(),
        store.len(),
        store.path().display(),
        t0.elapsed().as_secs_f64()
    );
    let cell_size = args.cell_size_m.unwrap_or(cfg.grid.cell_size_m);

    // Objekte parsen, total_bounds, Schwerpunkte, Zellen.
    let mut tb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut parsed: Vec<ParsedObject> = Vec::with_capacity(protected.len());
    for row in &protected {
        let shapes =
            crate::wkb::parse(&row.wkb).with_context(|| format!("osm_id {}", row.osm_id))?;
        let bounds = shapes_bounds(&shapes);
        tb = [
            tb[0].min(bounds[0]),
            tb[1].min(bounds[1]),
            tb[2].max(bounds[2]),
            tb[3].max(bounds[3]),
        ];
        parsed.push(ParsedObject {
            row,
            shapes,
            bounds,
        });
    }
    let cells = build_grid(tb, cell_size);
    let by_index: HashMap<(i64, i64), &Cell> = cells.iter().map(|c| ((c.col, c.row), c)).collect();
    let mut assignment: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut missing = 0usize;
    for (i, obj) in parsed.iter().enumerate() {
        let c = centroid(&obj.shapes);
        match by_index.get(&cell_index(c[0], c[1], cell_size)) {
            Some(cell) => assignment.entry(cell.id()).or_default().push(i),
            None => missing += 1,
        }
    }
    if missing > 0 {
        bail!("assign_to_cells: {missing} Verbotsobjekt(e) keiner Zelle zugeordnet");
    }

    // Diagnose-Ausgaben, dann Ende.
    if let Some(path) = &args.dump_cells {
        let m: BTreeMap<&String, Vec<i64>> = assignment
            .iter()
            .map(|(k, v)| (k, v.iter().map(|&i| parsed[i].row.osm_id).collect()))
            .collect();
        std::fs::write(path, serde_json::to_string_pretty(&m)?)?;
        info!("Zellenzuordnung -> {}", path.display());
    }
    if let Some(path) = &args.dump_points {
        let v = &cfg.visibility;
        let mut out = serde_json::Map::new();
        for obj in &parsed {
            let pts =
                crate::sample::sample_contour(&obj.shapes, v.spacing_m, v.min_points, v.max_points);
            let margin = v
                .margin_m
                .max(crate::sample::contour_spacing(&obj.shapes, pts.len()) / 4.0);
            let bits: Vec<u64> = pts
                .iter()
                .flat_map(|p| [p[0].to_bits(), p[1].to_bits()])
                .collect();
            out.insert(
                obj.row.osm_id.to_string(),
                serde_json::json!({"n": pts.len(), "margin": margin, "xy": bits}),
            );
        }
        std::fs::write(path, serde_json::to_string(&out)?)?;
        info!(
            "Beobachterpunkte fuer {} Objekte -> {}",
            out.len(),
            path.display()
        );
    }
    let by_cell: HashMap<String, &Cell> = cells.iter().map(|c| (c.id(), c)).collect();
    let reach = cfg.effective_reach_m();
    let query_box = |cell: &Cell, ids: &[usize]| -> [f64; 4] {
        let mut q = cell.bounds;
        for &i in ids {
            let b = parsed[i].bounds;
            q = [
                q[0].min(b[0]),
                q[1].min(b[1]),
                q[2].max(b[2]),
                q[3].max(b[3]),
            ];
        }
        [q[0] - reach, q[1] - reach, q[2] + reach, q[3] + reach]
    };
    if let Some(path) = &args.dump_keys {
        let version = cfg.param_version();
        let keys: Vec<Result<Vec<(i64, String)>>> = assignment
            .par_iter()
            .map(|(id, ids)| {
                let cell = by_cell[id];
                let cb = decode_cell_buildings(&store.query(query_box(cell, ids))?)?;
                Ok(ids
                    .iter()
                    .map(|&i| {
                        let o = &parsed[i];
                        (
                            o.row.osm_id,
                            cache_key(
                                &o.row.wkb,
                                &merged_obstacles(&cb, &o.shapes, o.bounds, reach),
                                version,
                            ),
                        )
                    })
                    .collect())
            })
            .collect();
        let mut m = BTreeMap::new();
        for k in keys {
            for (id, key) in k? {
                m.insert(id, key);
            }
        }
        std::fs::write(path, serde_json::to_string_pretty(&m)?)?;
        info!("cache_keys fuer {} Objekte -> {}", m.len(), path.display());
    }
    if args.dump_cells.is_some() || args.dump_points.is_some() || args.dump_keys.is_some() {
        return Ok(());
    }

    // Produktion: Manifest, anstehende Zellen, parallel rechnen.
    std::fs::create_dir_all(&args.out_dir)?;
    let fingerprint = input_fingerprint(cfg, &args.protected_path, &args.buildings_path)?;
    check_manifest(&args.out_dir, &fingerprint, args.force)?;
    if let Some(dir) = &args.reuse_from
        && !dir.is_dir()
    {
        bail!("--reuse-from {}: kein Verzeichnis", dir.display());
    }
    let only: Option<HashSet<String>> = match &args.only_cells {
        Some(p) => Some(read_cell_list(p)?),
        None => None,
    };
    let mut pending: Vec<(&Cell, &Vec<usize>, PathBuf)> = Vec::new();
    let mut skipped = 0usize;
    let mut filtered = 0usize;
    for (id, ids) in &assignment {
        if let Some(only) = &only
            && !only.contains(id)
        {
            filtered += 1;
            continue;
        }
        let out_path = args.out_dir.join(format!("{id}.parquet"));
        if !args.force && out_path.exists() {
            skipped += 1;
            continue;
        }
        pending.push((by_cell[id], ids, out_path));
    }
    // Groesste Arbeit zuerst (LPT): Kosten je Zelle ~ Summe der
    // Beobachterpunkte ihrer Objekte (Umfang / Abstand, gedeckelt), nicht die
    // blosse Objektzahl -- wenige riesige Objekte sind die teuersten Zellen.
    let est_points = |o: &ParsedObject| -> f64 {
        let v = &cfg.visibility;
        let perim: f64 = o
            .shapes
            .iter()
            .map(|s| crate::sample::ring_length(&s[0]))
            .sum();
        (perim / v.spacing_m.max(1e-9))
            .max(v.min_points as f64)
            .min(v.max_points as f64)
    };
    let cost = |ids: &Vec<usize>| -> f64 { ids.iter().map(|&i| est_points(&parsed[i])).sum() };
    pending.sort_by(|a, b| cost(b.1).total_cmp(&cost(a.1)));

    // P1: Fingerabdruck je anstehender Zelle; bei --reuse-from und gleichem
    // Fingerabdruck der Vorlaufzelle diese per Hardlink uebernehmen.
    let t_fp = Instant::now();
    let version = cfg.param_version();
    let link_from = if args.no_cell_link {
        None
    } else {
        args.reuse_from.as_deref()
    };
    let fps: Vec<Result<(String, bool)>> = pending
        .par_iter()
        .map(|(cell, ids, out_path)| -> Result<(String, bool)> {
            let hits = store.query(query_box(cell, ids))?;
            let wkbs = hits.wkbs();
            let fp = cell_fingerprint(
                version,
                cell_size,
                reach,
                ids.iter().map(|&i| {
                    let r = parsed[i].row;
                    (r.osm_id, r.category.as_str(), r.wkb.as_slice())
                }),
                wkbs,
            );
            if let Some(prev_dir) = link_from {
                let prev = prev_dir.join(format!("{}.parquet", cell.id()));
                if prev.is_file()
                    && kv_metadata(&prev, CELL_FP_KEY)?.as_deref() == Some(fp.as_str())
                {
                    if std::fs::hard_link(&prev, out_path).is_err() {
                        std::fs::copy(&prev, out_path)
                            .with_context(|| format!("{} uebernehmen", prev.display()))?;
                    }
                    return Ok((fp, true));
                }
            }
            Ok((fp, false))
        })
        .collect();
    let mut linked = 0usize;
    let mut pending_fp: Vec<(&Cell, &Vec<usize>, PathBuf, String)> =
        Vec::with_capacity(pending.len());
    for ((cell, ids, out_path), r) in pending.into_iter().zip(fps) {
        let (fp, was_linked) = r?;
        if was_linked {
            linked += 1;
        } else {
            pending_fp.push((cell, ids, out_path, fp));
        }
    }
    let pending = pending_fp;
    let fp_phase_s = t_fp.elapsed().as_secs_f64();
    if let Some(dir) = link_from {
        info!(
            "zones: {linked} Zellen per Fingerabdruck aus {} uebernommen ({:.0} s)",
            dir.display(),
            fp_phase_s
        );
    }
    let n_pending_objects: usize = pending.iter().map(|(_, ids, _, _)| ids.len()).sum();
    if let Some(only) = &only {
        info!(
            "zones: --only-cells {}: {} Zellen gelistet, {} Zellen des Gitters ausgelassen",
            args.only_cells.as_ref().unwrap().display(),
            only.len(),
            filtered
        );
    }
    info!(
        "zones: {} Zellen zu berechnen ({} Objekte), {} Zellen bereits vorhanden, {} Hindernisse, cell_size_m={}, reuse_from={:?}",
        pending.len(),
        n_pending_objects,
        skipped,
        store.len(),
        cell_size,
        args.reuse_from
    );

    let done_cells = AtomicUsize::new(0);
    let done_objects = AtomicUsize::new(0);
    let done_zones = AtomicUsize::new(0);
    let done_reused = AtomicUsize::new(0);
    let last_log = Mutex::new(Instant::now());
    let n_pending = pending.len();
    // Dynamische Warteschlange in Listenreihenfolge (groesste Zellen zuerst,
    // LPT): jeder Arbeiter zieht die naechste Zelle. `par_iter` teilt die
    // Liste stattdessen in Bereiche auf; die schweren Zellen am Anfang
    // landeten so bei wenigen Threads, und der Bundeslauf 12 brauchte fuer
    // die letzten 10 % der Objekte 3,5 von 8,6 Stunden.
    let next = AtomicUsize::new(0);
    let errors: Mutex<Vec<anyhow::Error>> = Mutex::new(Vec::new());
    let work = |i: usize| -> Result<()> {
        let (cell, ids, out_path, cell_fp) = &pending[i];
        let cb = decode_cell_buildings(&store.query(query_box(cell, ids))?)?;
        let objects: Vec<&ParsedObject> = ids.iter().map(|&i| &parsed[i]).collect();
        let objects: Vec<ParsedObject> = objects
            .into_iter()
            .map(|o| ParsedObject {
                row: o.row,
                shapes: o.shapes.clone(),
                bounds: o.bounds,
            })
            .collect();
        let res = process_cell(
            cfg,
            cell,
            &objects,
            &cb,
            out_path,
            args.reuse_from.as_deref(),
            cell_fp,
        )?;
        let c = done_cells.fetch_add(1, Ordering::Relaxed) + 1;
        let o = done_objects.fetch_add(res.n_objects, Ordering::Relaxed) + res.n_objects;
        let z = done_zones.fetch_add(res.n_zones, Ordering::Relaxed) + res.n_zones;
        let ru = done_reused.fetch_add(res.n_reused, Ordering::Relaxed) + res.n_reused;
        let mut last = last_log.lock().unwrap();
        if last.elapsed().as_secs_f64() >= args.log_interval_s || c == n_pending {
            let elapsed = t0.elapsed().as_secs_f64();
            let frac = if n_pending_objects > 0 {
                o as f64 / n_pending_objects as f64
            } else {
                1.0
            };
            let eta = if frac > 0.0 {
                elapsed / frac - elapsed
            } else {
                f64::NAN
            };
            info!(
                "zones: {c}/{n_pending} Zellen, {o}/{n_pending_objects} Objekte ({:.1} %, davon {ru} wiederverwendet), {z} Baender, {:.0} min vergangen, ~{:.0} min verbleibend",
                100.0 * frac,
                elapsed / 60.0,
                eta / 60.0
            );
            *last = Instant::now();
        }
        Ok(())
    };
    rayon::broadcast(|_| {
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= n_pending {
                break;
            }
            if let Err(e) = work(i) {
                errors.lock().unwrap().push(e);
                // Restarbeit abbrechen: Zaehler ans Ende setzen
                next.store(n_pending, Ordering::Relaxed);
            }
        }
    });
    if let Some(e) = errors.into_inner().unwrap().into_iter().next() {
        return Err(e);
    }
    print_profile(t0.elapsed().as_secs_f64(), rayon::current_num_threads());
    let total_zones = done_zones.load(Ordering::Relaxed);
    let reused = done_reused.load(Ordering::Relaxed);
    // Gesamtzahl ueber alle Zellendateien (auch uebersprungene), wie Python
    // aus den Parquet-Metadaten.
    let mut all_zones = 0usize;
    for e in std::fs::read_dir(&args.out_dir)?.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "parquet") {
            let f = std::fs::File::open(&p)?;
            all_zones += parquet::file::reader::SerializedFileReader::new(f)
                .map(|r| {
                    parquet::file::reader::FileReader::metadata(&r)
                        .file_metadata()
                        .num_rows() as usize
                })
                .unwrap_or(0);
        }
    }
    let summary = serde_json::json!({
        "stage": "zones",
        "elapsed_s": (t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
        "computed_cells": n_pending,
        "skipped_cells": skipped,
        "computed_zones": total_zones,
        "computed_objects": n_pending_objects,
        "decompose_hits": 0,
        "decompose_misses": 0,
        "reused_objects": reused,
        "linked_cells": linked,
        "fp_phase_s": fp_phase_s,
        "inputs": fingerprint,
        "total_zones": all_zones,
        "n_jobs": args.n_jobs,
        "engine": "bubatz",
    });
    std::fs::write(
        args.out_dir.join("zones_summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    println!(
        "Zonen (Baender): {total_zones} in {n_pending} Zellen ({reused} von {n_pending_objects} Objekten aus --reuse-from uebernommen, {linked} Zellen per Fingerabdruck verlinkt), {skipped} Zellen bereits vorhanden (uebersprungen), {all_zones} insgesamt -> {}",
        args.out_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_vergleich_ignoriert_absolute_pfade() {
        let a = serde_json::json!({"param_version": "v7",
            "protected": {"path": "/daten/protected.parquet", "size": 7, "mtime_ns": 1},
            "buildings": {"path": "/daten/buildings.parquet", "size": 9, "mtime_ns": 2}});
        let b = serde_json::json!({"param_version": "v7",
            "protected": {"path": "/srv/y/protected.parquet", "size": 7, "mtime_ns": 1},
            "buildings": {"path": "/srv/y/buildings.parquet", "size": 9, "mtime_ns": 2}});
        let c = serde_json::json!({"param_version": "v7",
            "protected": {"path": "/daten/protected.parquet", "size": 8, "mtime_ns": 1},
            "buildings": {"path": "/daten/buildings.parquet", "size": 9, "mtime_ns": 2}});
        assert_eq!(manifest_comparable(&a), manifest_comparable(&b));
        assert_ne!(manifest_comparable(&a), manifest_comparable(&c));
    }

    fn fp(
        objs: &[(i64, &str, &[u8])],
        blds: &[&[u8]],
        version: &str,
        cs: f64,
        reach: f64,
    ) -> String {
        cell_fingerprint(
            version,
            cs,
            reach,
            objs.iter().map(|(i, c, w)| (*i, *c, *w)),
            blds.to_vec(),
        )
    }

    #[test]
    fn previous_bands_are_keyed_by_object_and_deduplicated() {
        let d = std::env::temp_dir().join(format!("bubatz-prev-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let row = |osm_id: i64, band: i64, key: &str| ZoneRow {
            osm_type: "way".to_string(),
            osm_id,
            category: "school".into(),
            band_max_m: band,
            wkb: vec![1, 2, 3],
            is_multi: false,
            bounds: [0.0, 0.0, 1.0, 1.0],
            cache_key: key.into(),
        };
        write_cell_with_meta(
            &d.join("c.parquet"),
            &[
                row(1, 100, "k"),
                row(2, 100, "k"),
                row(1, 100, "k"),
                row(3, 100, "k3"),
            ],
            &[],
        )
        .unwrap();
        let m = previous_by_key(Some(&d), "c.parquet").unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(
            m[&(1, "k".to_string())].len(),
            1,
            "Duplikat zusammengefasst"
        );
        assert_eq!(m[&(2, "k".to_string())].len(), 1, "nur eigene Baender");
        assert_eq!(m[&(3, "k3".to_string())].len(), 1);
        assert!(!m.contains_key(&(4, "k".to_string())));
        assert!(
            previous_by_key(Some(&d), "fehlt.parquet")
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn cell_fingerprint_reacts_to_every_input() {
        let o1: (i64, &str, &[u8]) = (1, "school", b"wkb-1");
        let o2: (i64, &str, &[u8]) = (2, "sports", b"wkb-2");
        let b1: &[u8] = b"bld-a";
        let b2: &[u8] = b"bld-b";
        let base = fp(&[o1, o2], &[b1, b2], "v1", 2000.0, 102.0);
        assert_eq!(base.len(), 128);
        assert_eq!(
            base,
            fp(&[o1, o2], &[b2, b1], "v1", 2000.0, 102.0),
            "Gebaeudereihenfolge egal"
        );
        assert_ne!(
            base,
            fp(&[o2, o1], &[b1, b2], "v1", 2000.0, 102.0),
            "Objektreihenfolge = Zeilenreihenfolge"
        );
        assert_ne!(
            base,
            fp(
                &[o1, (2, "sports", b"wkb-2x")],
                &[b1, b2],
                "v1",
                2000.0,
                102.0
            ),
            "Objekt-WKB"
        );
        assert_ne!(
            base,
            fp(
                &[o1, (2, "school", b"wkb-2")],
                &[b1, b2],
                "v1",
                2000.0,
                102.0
            ),
            "Kategorie"
        );
        assert_ne!(
            base,
            fp(
                &[o1, (3, "sports", b"wkb-2")],
                &[b1, b2],
                "v1",
                2000.0,
                102.0
            ),
            "osm_id"
        );
        assert_ne!(
            base,
            fp(&[o1, o2], &[b1], "v1", 2000.0, 102.0),
            "Gebaeudemenge"
        );
        assert_ne!(
            base,
            fp(&[o1, o2], &[b1, b"bld-c"], "v1", 2000.0, 102.0),
            "Gebaeude-WKB"
        );
        assert_ne!(
            base,
            fp(&[o1, o2], &[b1, b2], "v8-rust", 2000.0, 102.0),
            "Parameterversion"
        );
        assert_ne!(
            base,
            fp(&[o1, o2], &[b1, b2], "v1", 1000.0, 102.0),
            "Zellgroesse"
        );
        assert_ne!(
            base,
            fp(&[o1, o2], &[b1, b2], "v1", 2000.0, 100.0),
            "Reichweite"
        );
        // Laengenpraefixe: Verschiebung von Bytes zwischen Feldern aendert den Wert
        assert_ne!(
            fp(&[(1, "ab", b"c")], &[], "v", 1.0, 1.0),
            fp(&[(1, "a", b"bc")], &[], "v", 1.0, 1.0)
        );
        assert_ne!(
            fp(&[], &[b"ab", b"c"], "v", 1.0, 1.0),
            fp(&[], &[b"a", b"bc"], "v", 1.0, 1.0)
        );
    }
}
