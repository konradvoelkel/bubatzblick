//! `bubatz verify` -- die Pruefungen der Veroeffentlichung, soweit ohne Python:
//! `identical` (zwei Zonenlaeufe byte-genau je Band, wie
//! `rollout_check_zones_identical.py`), `anchor` (Flaeche der Zone eines
//! Objekts bis 100 m aus den Zonendaten -- dort
//! wird allerdings aus den Rohdaten neu gerechnet; hier: Baender <= 100 m
//! geschnitten mit dem 100-m-Kreis um die Quelle), `protected` (Kennzahlen der
//! Verbotsobjekte fuer die Pruefkette in `make publish`: Anzahl je Kategorie,
//! Plausibilitaetswarnungen, uebergrosse Objekte -- gegen manipulierte oder
//! kaputte OSM-Daten).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use crate::parquet_io::read_zone_cell;

#[derive(Args, Debug)]
pub struct VerifyArgs {
    #[command(subcommand)]
    pub what: VerifyWhat,
}

#[derive(Subcommand, Debug)]
pub enum VerifyWhat {
    /// Zwei Zonenverzeichnisse je (osm_id, band) byte-genau vergleichen
    Identical {
        zones_a: PathBuf,
        zones_b: PathBuf,
        /// Baender gelten als gleich, wenn sich ihre Flaeche um hoechstens so viel unterscheidet
        #[arg(long, default_value_t = 0.0)]
        tolerance_m2: f64,
    },
    /// Flaeche der Zone eines Objekts bis `radius` (Baender <= radius, geschnitten mit dem Kreis um die Quelle)
    Anchor {
        #[arg(long)]
        zones_dir: PathBuf,
        #[arg(long)]
        protected_path: PathBuf,
        #[arg(long, default_value_t = 29359108)]
        osm_id: i64,
        #[arg(long, default_value_t = 100.0)]
        radius_m: f64,
    },
    /// Kennzahlen von protected.parquet als JSON (objects, warnings, categories{},
    /// oversize[]): "oversize" = Flaeche > hard_factor x Plausibilitaetsgrenze der
    /// Kategorie (groesste Grenze ihrer Tags aus tags.json) -- ein einziges
    /// falsch getaggtes Riesenpolygon wuerde sonst eine ganze Stadt rot faerben.
    Protected {
        #[arg(long)]
        protected_path: PathBuf,
        /// Tag-Datei (Vorgabe wie extract: tags.json neben pipeline.toml)
        #[arg(long)]
        tags: Option<PathBuf>,
        #[arg(long, default_value_t = 10.0)]
        hard_factor: f64,
    },
}

/// Plausibilitaetsgrenze je Kategorie: die groesste Grenze ueber alle
/// `key=value` ihrer Regeln (sonst `default`). Die Zeilen in protected.parquet
/// tragen keine Tags mehr, nur die Kategorie.
fn category_limits(spec: &crate::tags::TagSpec) -> BTreeMap<String, f64> {
    let default = spec.plausibility_limits_m2["default"];
    spec.categories
        .iter()
        .map(|c| {
            let limit = c
                .rules
                .iter()
                .flat_map(|r| r.values.iter().map(move |v| format!("{}={v}", r.key)))
                .filter_map(|kv| spec.plausibility_limits_m2.get(&kv).copied())
                .fold(default, f64::max);
            (c.name.clone(), limit)
        })
        .collect()
}

/// Streamt nur die Kennzahlspalten (kein WKB): Bund = 0,5 Mio. Zeilen.
fn protected_stats(
    path: &std::path::Path,
    limits: &BTreeMap<String, f64>,
    hard_factor: f64,
) -> Result<serde_json::Value> {
    use arrow::array::AsArray;
    use parquet::arrow::ProjectionMask;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let file = std::fs::File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.parquet_schema();
    let idx = |name: &str| {
        schema
            .columns()
            .iter()
            .position(|c| c.name() == name)
            .ok_or_else(|| anyhow::anyhow!("Spalte {name} fehlt in {}", path.display()))
    };
    let mask = ProjectionMask::roots(
        schema,
        [
            idx("osm_id")?,
            idx("category")?,
            idx("area_m2")?,
            idx("plausibility_warning")?,
        ],
    );
    let reader = builder
        .with_projection(mask)
        .with_batch_size(65_536)
        .build()?;
    let mut objects = 0usize;
    let mut warnings = 0usize;
    let mut per_cat: BTreeMap<String, usize> = BTreeMap::new();
    let mut oversize = Vec::new();
    for batch in reader {
        let batch = batch?;
        let osm = batch
            .column_by_name("osm_id")
            .unwrap()
            .as_primitive::<arrow::datatypes::Int64Type>();
        let cat = batch.column_by_name("category").unwrap().clone();
        let area = batch
            .column_by_name("area_m2")
            .unwrap()
            .as_primitive::<arrow::datatypes::Float64Type>();
        let warn = batch
            .column_by_name("plausibility_warning")
            .unwrap()
            .as_boolean();
        for i in 0..batch.num_rows() {
            let c = crate::parquet_io::string_at(&cat, i)?;
            objects += 1;
            warnings += warn.value(i) as usize;
            *per_cat.entry(c.clone()).or_default() += 1;
            let limit = limits.get(&c).copied().unwrap_or(0.0) * hard_factor;
            if limit > 0.0 && area.value(i) > limit {
                oversize.push(serde_json::json!({
                    "osm_id": osm.value(i), "category": c,
                    "area_m2": area.value(i).round(), "limit_m2": limit.round(),
                }));
            }
        }
    }
    Ok(serde_json::json!({
        "objects": objects,
        "warnings": warnings,
        "categories": per_cat,
        "hard_factor": hard_factor,
        "oversize": oversize,
    }))
}

/// (osm_id, band) -> (WKB, Flaeche)
type BandMap = BTreeMap<(i64, i64), (Vec<u8>, f64)>;

fn cell_files(dir: &std::path::Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".parquet"))
        .collect();
    names.sort();
    Ok(names)
}

/// Eine Zellendatei als Karte; fehlt sie, leer. Zellenweise statt alles auf
/// einmal: der Bundeslauf hat 54.000 Zellen mit 4,1 Mio. Baendern (WKB
/// zweistellige GB) -- der erste `verify anchor` daran lief in den OOM-Killer.
fn load_cell(path: &std::path::Path) -> Result<BandMap> {
    let mut m = BTreeMap::new();
    if !path.exists() {
        return Ok(m);
    }
    for r in read_zone_cell(path)? {
        let area = crate::geos::shapes_area(&crate::wkb::parse(&r.wkb)?);
        m.insert((r.osm_id, r.band_max_m), (r.wkb, area));
    }
    Ok(m)
}

fn collect_bands(
    m: &BandMap,
    osm_id: i64,
    radius_m: f64,
    shapes: &mut Vec<bubatzcore::geom::Shape>,
) -> Result<()> {
    for ((id, band), (wkb, _)) in m {
        if *id == osm_id && (*band as f64) <= radius_m {
            shapes.extend(crate::wkb::parse(wkb)?);
        }
    }
    Ok(())
}

pub fn run(config_path: &std::path::Path, args: VerifyArgs) -> Result<()> {
    match args.what {
        VerifyWhat::Protected {
            protected_path,
            tags,
            hard_factor,
        } => {
            let spec = crate::tags::TagSpec::load(&crate::extract::tags_path(&tags, config_path))?;
            let stats = protected_stats(&protected_path, &category_limits(&spec), hard_factor)?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
            Ok(())
        }
        VerifyWhat::Identical {
            zones_a,
            zones_b,
            tolerance_m2,
        } => {
            let mut names: std::collections::BTreeSet<String> =
                cell_files(&zones_a)?.into_iter().collect();
            names.extend(cell_files(&zones_b)?);
            let mut objs_a = std::collections::BTreeSet::new();
            let mut objs_b = std::collections::BTreeSet::new();
            let (mut n_a, mut n_b, mut only_a, mut only_b) = (0usize, 0usize, 0usize, 0usize);
            let mut common = 0usize;
            let mut same = 0usize;
            let mut diff_objs: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
            for name in &names {
                let a = load_cell(&zones_a.join(name))?;
                let b = load_cell(&zones_b.join(name))?;
                n_a += a.len();
                n_b += b.len();
                objs_a.extend(a.keys().map(|k| k.0));
                objs_b.extend(b.keys().map(|k| k.0));
                only_a += a.keys().filter(|k| !b.contains_key(k)).count();
                only_b += b.keys().filter(|k| !a.contains_key(k)).count();
                for (k, (wa, aa)) in &a {
                    if let Some((wb, ab)) = b.get(k) {
                        common += 1;
                        let equal = if tolerance_m2 > 0.0 {
                            (aa - ab).abs() <= tolerance_m2
                        } else {
                            wa == wb
                        };
                        if equal {
                            same += 1;
                        } else {
                            let e = diff_objs.entry(k.0).or_insert((0.0, 0.0));
                            e.0 += aa;
                            e.1 += ab;
                        }
                    }
                }
            }
            println!("A: {} Objekte / {} Baender", objs_a.len(), n_a);
            println!("B: {} Objekte / {} Baender", objs_b.len(), n_b);
            println!("nur in A: {only_a} Baender, nur in B: {only_b}");
            println!(
                "gemeinsame Baender: {common}, {}: {same} ({:.3} %)",
                if tolerance_m2 > 0.0 {
                    "flaechengleich"
                } else {
                    "byte-identisch"
                },
                100.0 * same as f64 / common.max(1) as f64
            );
            println!("\nabweichende Objekte: {}", diff_objs.len());
            for (id, (fa, fb)) in diff_objs.iter().take(20) {
                println!(
                    "  osm_id={id:<12} Flaeche A {fa:12.1} -> B {fb:12.1} m2 ({:+.3} %)",
                    100.0 * (fb - fa) / fa.max(1e-9)
                );
            }
            if only_a == 0 && only_b == 0 && diff_objs.is_empty() {
                println!("\nBestanden.");
                Ok(())
            } else {
                println!("\nNICHT bestanden.");
                std::process::exit(1)
            }
        }
        VerifyWhat::Anchor {
            zones_dir,
            protected_path,
            osm_id,
            radius_m,
        } => {
            let rows = crate::parquet_io::read_protected(&protected_path)?;
            let Some(src) = rows.iter().find(|r| r.osm_id == osm_id) else {
                bail!("osm_id {osm_id} nicht in {}", protected_path.display());
            };
            let source = crate::wkb::parse(&src.wkb)?;
            let circle = bubatzcore::free_circle(&source, radius_m);
            let cfg = crate::config::load(config_path)?;
            let c = crate::grid::centroid(&source);
            let (col, row) = crate::grid::cell_index(c[0], c[1], cfg.grid.cell_size_m);
            let own = zones_dir.join(format!("x{col}_y{row}.parquet"));
            let mut shapes = Vec::new();
            collect_bands(&load_cell(&own)?, osm_id, radius_m, &mut shapes)?;
            if shapes.is_empty() {
                // Objekt nicht in der erwarteten Zelle: alle Zellen einzeln durchsehen
                for name in cell_files(&zones_dir)? {
                    collect_bands(
                        &load_cell(&zones_dir.join(&name))?,
                        osm_id,
                        radius_m,
                        &mut shapes,
                    )?;
                    if !shapes.is_empty() {
                        break;
                    }
                }
            }
            if shapes.is_empty() {
                bail!(
                    "keine Baender <= {radius_m} m fuer osm_id {osm_id} in {}",
                    zones_dir.display()
                );
            }
            let zone = bubatzcore::to_ogc(&bubatzcore::union_cascade(shapes));
            let clipped = bubatzcore::union::overlay_pair(
                &zone,
                &circle,
                i_overlay::core::overlay_rule::OverlayRule::Intersect,
            );
            let area = bubatzcore::geom::shapes_area(&clipped);
            println!(
                "osm_id {osm_id}: Zone <= {radius_m:.0} m: {} m2 (Baender-Vereinigung {:.0} m2, Kreis mit {} Ecken)",
                area.round(),
                bubatzcore::geom::shapes_area(&zone),
                circle.iter().flatten().map(|r| r.len()).sum::<usize>()
            );
            Ok(())
        }
    }
}
