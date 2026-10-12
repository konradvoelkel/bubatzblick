//! `bubatz report` -- Port von `report.py`: Kennzahlen je Kategorie
//! (Objekte, Baender, Flaeche bis 100 m, Mittel je Objekt),
//! groesste Zonen, Diff gegen einen Vorlauf per `cache_key`; Text- und
//! JSON-Ausgabe im selben Format (ROLLOUT-Tabellen bleiben vergleichbar).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::Args;

use crate::parquet_io::read_zone_cell;

pub const CUMULATIVE_RADII_M: [i64; 1] = [100];

#[derive(Args, Debug)]
pub struct ReportArgs {
    #[arg(long, default_value = "data/interim/zones")]
    pub zones_dir: PathBuf,
    /// Zonen-Verzeichnis eines Vorlaufs (z. B. zones.prev) fuer den Diff
    #[arg(long)]
    pub previous: Option<PathBuf>,
    /// Zusaetzlich als JSON schreiben
    #[arg(long)]
    pub out_json: Option<PathBuf>,
    #[arg(long, default_value_t = 10)]
    pub n_largest: usize,
}

struct ObjectRow {
    category: String,
    cache_key: String,
    n_bands: usize,
    area: BTreeMap<i64, f64>,
}

struct Bands {
    n_cells: usize,
    n_bands: usize,
    objects: BTreeMap<i64, ObjectRow>,
}

fn read_bands(dir: &Path) -> Result<Bands> {
    let mut parts: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "parquet"))
        .collect();
    parts.sort();
    if parts.is_empty() {
        bail!("keine Zonen-Parquet-Dateien in {}", dir.display());
    }
    let mut objects: BTreeMap<i64, ObjectRow> = BTreeMap::new();
    let mut n_bands = 0usize;
    for part in &parts {
        for r in read_zone_cell(part)? {
            n_bands += 1;
            let area = crate::geos::shapes_area(&crate::wkb::parse(&r.wkb)?);
            let o = objects.entry(r.osm_id).or_insert_with(|| ObjectRow {
                category: r.category.clone(),
                cache_key: r.cache_key.clone(),
                n_bands: 0,
                area: CUMULATIVE_RADII_M.iter().map(|&x| (x, 0.0)).collect(),
            });
            o.n_bands += 1;
            for &rad in &CUMULATIVE_RADII_M {
                if r.band_max_m <= rad {
                    *o.area.get_mut(&rad).unwrap() += area;
                }
            }
        }
    }
    Ok(Bands {
        n_cells: parts.len(),
        n_bands,
        objects,
    })
}

fn round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (v * f).round() / f
}

pub fn run(args: ReportArgs) -> Result<()> {
    let bands = read_bands(&args.zones_dir)?;
    // Kategorien
    let mut by_cat: BTreeMap<&str, Vec<&ObjectRow>> = BTreeMap::new();
    for o in bands.objects.values() {
        by_cat.entry(o.category.as_str()).or_default().push(o);
    }
    let mut categories = Vec::new();
    for (cat, objs) in &by_cat {
        let mut area_km2 = serde_json::Map::new();
        let mut mean_m2 = serde_json::Map::new();
        for &r in &CUMULATIVE_RADII_M {
            let sum: f64 = objs.iter().map(|o| o.area[&r]).sum();
            area_km2.insert(format!("{r}m"), serde_json::json!(round(sum / 1e6, 3)));
            mean_m2.insert(
                format!("{r}m"),
                serde_json::json!(round(sum / objs.len() as f64, 1)),
            );
        }
        categories.push(serde_json::json!({
            "category": cat,
            "n_objects": objs.len(),
            "n_bands": objs.iter().map(|o| o.n_bands).sum::<usize>(),
            "area_km2_by_radius": area_km2,
            "mean_area_m2_by_radius": mean_m2,
        }));
    }
    let mut largest: Vec<(i64, &ObjectRow)> = bands.objects.iter().map(|(k, v)| (*k, v)).collect();
    largest.sort_by(|a, b| {
        b.1.area[&100]
            .partial_cmp(&a.1.area[&100])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let largest: Vec<serde_json::Value> = largest
        .iter()
        .take(args.n_largest)
        .map(|(id, o)| serde_json::json!({"osm_id": id, "category": o.category, "area_100m_m2": o.area[&100].round() as i64}))
        .collect();
    let mut report = serde_json::json!({
        "zones_dir": args.zones_dir.to_string_lossy(),
        "n_cells": bands.n_cells,
        "n_objects": bands.objects.len(),
        "n_bands": bands.n_bands,
        "categories": categories,
        "largest_objects": largest,
        "previous": serde_json::Value::Null,
        "changes": serde_json::Value::Null,
        "area_km2_diff_by_category": serde_json::Value::Null,
        "largest_changes": serde_json::Value::Null,
    });
    let mut lines = vec![
        format!("Zonen-Verzeichnis: {}", args.zones_dir.display()),
        format!(
            "Zellen: {}, Objekte: {}, Baender: {}",
            bands.n_cells,
            bands.objects.len(),
            bands.n_bands
        ),
        String::new(),
        format!(
            "{:<15}{:>10}{:>10}{:>13}{:>13}",
            "Kategorie", "Objekte", "Baender", "km2 <=100m", "m2/Obj 100m"
        ),
    ];
    for c in report["categories"].as_array().unwrap() {
        lines.push(format!(
            "{:<15}{:>10}{:>10}{:>13.1}{:>13.0}",
            c["category"].as_str().unwrap(),
            c["n_objects"].as_u64().unwrap(),
            c["n_bands"].as_u64().unwrap(),
            c["area_km2_by_radius"]["100m"].as_f64().unwrap(),
            c["mean_area_m2_by_radius"]["100m"].as_f64().unwrap()
        ));
    }
    lines.push(String::new());
    lines.push("Groesste Zonen (Flaeche bis 100 m):".into());
    for o in report["largest_objects"].as_array().unwrap() {
        lines.push(format!(
            "  osm_id {:<12} {:<13} {:>12} m2",
            o["osm_id"].as_i64().unwrap(),
            o["category"].as_str().unwrap(),
            o["area_100m_m2"].as_i64().unwrap()
        ));
    }
    if let Some(prev) = &args.previous {
        let pb = read_bands(prev)?;
        let (mut added, mut changed) = (0usize, 0usize);
        for (id, o) in &bands.objects {
            match pb.objects.get(id) {
                None => added += 1,
                Some(p) if p.cache_key != o.cache_key => changed += 1,
                _ => {}
            }
        }
        let removed = pb
            .objects
            .keys()
            .filter(|k| !bands.objects.contains_key(k))
            .count();
        let mut cats: std::collections::BTreeSet<&str> = by_cat.keys().copied().collect();
        let mut prev_area: BTreeMap<&str, f64> = BTreeMap::new();
        for o in pb.objects.values() {
            *prev_area.entry(o.category.as_str()).or_default() += o.area[&100];
            cats.insert(o.category.as_str());
        }
        let mut diff = serde_json::Map::new();
        let mut diff_text = Vec::new();
        for c in cats {
            let now: f64 = by_cat
                .get(c)
                .map(|v| v.iter().map(|o| o.area[&100]).sum())
                .unwrap_or(0.0);
            let d = round((now - prev_area.get(c).copied().unwrap_or(0.0)) / 1e6, 3);
            diff.insert(c.to_string(), serde_json::json!(d));
            diff_text.push(format!("{c} {d:+.3}"));
        }
        // Groesste Einzelaenderungen: die Zaehlung sagt, DASS sich etwas geaendert
        // hat, nicht wo es weh tut. Sortiert nach Betrag der Flaechenaenderung,
        // damit ein Tagesdiff in einem Blick beurteilbar ist.
        let mut ids: std::collections::BTreeSet<i64> = bands.objects.keys().copied().collect();
        ids.extend(pb.objects.keys().copied());
        let mut deltas: Vec<(i64, &str, f64, f64, &'static str)> = ids
            .iter()
            .filter_map(|id| {
                let now = bands.objects.get(id);
                let was = pb.objects.get(id);
                let a_now = now.map(|o| o.area[&100]).unwrap_or(0.0);
                let a_was = was.map(|o| o.area[&100]).unwrap_or(0.0);
                let art = match (now, was) {
                    (Some(_), None) => "neu",
                    (None, Some(_)) => "entfallen",
                    (Some(n), Some(p)) if n.cache_key != p.cache_key => "geaendert",
                    _ => return None,
                };
                let cat = now.or(was).map(|o| o.category.as_str()).unwrap_or("");
                Some((*id, cat, a_now - a_was, a_now, art))
            })
            .collect();
        deltas.sort_by(|a, b| {
            b.2.abs()
                .partial_cmp(&a.2.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let groesste: Vec<serde_json::Value> = deltas
            .iter()
            .take(args.n_largest)
            .map(|(id, cat, d, now, art)| {
                serde_json::json!({"osm_id": id, "category": cat, "kind": art,
                                   "area_100m_m2": now.round() as i64,
                                   "delta_m2": d.round() as i64})
            })
            .collect();

        report["previous"] = serde_json::json!(prev.to_string_lossy());
        report["changes"] =
            serde_json::json!({"added": added, "removed": removed, "changed": changed});
        report["area_km2_diff_by_category"] = serde_json::Value::Object(diff);
        report["largest_changes"] = serde_json::json!(groesste);
        lines.push(String::new());
        lines.push(format!("Diff gegen {}:", prev.display()));
        lines.push(format!(
            "  Objekte neu {added}, entfallen {removed}, geaendert {changed}"
        ));
        lines.push(format!(
            "  Flaechenaenderung bis 100 m je Kategorie (km2): {}",
            diff_text.join(", ")
        ));
        if !deltas.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "Groesste Aenderungen (von {} betroffenen Objekten):",
                deltas.len()
            ));
            for c in report["largest_changes"].as_array().unwrap() {
                lines.push(format!(
                    "  osm_id {:<12} {:<13} {:<10} {:>+12} m2  (jetzt {} m2)",
                    c["osm_id"].as_i64().unwrap(),
                    c["category"].as_str().unwrap(),
                    c["kind"].as_str().unwrap(),
                    c["delta_m2"].as_i64().unwrap(),
                    c["area_100m_m2"].as_i64().unwrap()
                ));
            }
        }
    }
    println!("{}", lines.join("\n"));
    if let Some(p) = &args.out_json {
        std::fs::write(p, serde_json::to_string_pretty(&report)?)?;
        println!("-> {}", p.display());
    }
    Ok(())
}
