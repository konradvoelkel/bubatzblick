//! `bubatz extract <pbf> --out-dir …`: Verbotsobjekte
//! (Flaechen + Punkt-Fallback) und Gebaeude aus der PBF, EPSG:25832, zwei
//! GeoParquet-Dateien, `extract_summary.json`.
//!
//! Reihenfolge der Zeilen: Punkt-Fallbacks (Knoten nach ID), dann Flaechen aus
//! geschlossenen Ways (nach Way-ID), dann aus Relationen (nach Relation-ID) --
//! ein Stream-Leser liefert eine andere Reihenfolge; fuer `zones` ist die
//! Reihenfolge ohne Bedeutung (Zellenzuordnung und `cache_key` sind
//! reihenfolgeunabhaengig, Vergleiche laufen ueber `osm_id`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use bubatzcore::geom::{Shape, Shapes};
use clap::Args;
use tracing::info;

use crate::assemble::{Assembler, Loc, Ring};
use crate::geoparquet::{BuildingOut, ProtectedOut, StreamWriter};
use crate::pbf::{PbfData, Tags};
use crate::proj::Utm32;
use crate::tags::TagSpec;

#[derive(Args, Debug)]
pub struct ExtractArgs {
    /// Pfad zur .osm.pbf
    pub pbf_path: PathBuf,
    #[arg(long, default_value = "data/interim")]
    pub out_dir: PathBuf,
    /// Tag-Spezifikation (Default: config/tags.json neben pipeline.toml)
    #[arg(long)]
    pub tags: Option<PathBuf>,
    /// Max. Anzahl Verbotsobjekte (Debug/Test)
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long, default_value_t = 200_000)]
    pub batch_size: usize,
}

pub const POINT_FALLBACK_RADIUS_M: f64 = 15.0;
pub const POINT_FALLBACK_QUAD_SEGS: u32 = 16;

pub fn tags_path(args: &Option<PathBuf>, config_path: &Path) -> PathBuf {
    args.clone().unwrap_or_else(|| {
        config_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("tags.json")
    })
}

/// Ringe (Festkomma) -> Shapes in EPSG:25832 (Aussenring + Loecher, offen).
pub fn project_rings(rings: &[Ring], proj: &Utm32) -> Shapes {
    let conv = |locs: &[Loc]| -> Vec<[f64; 2]> {
        // Ring ist geschlossen (Start wiederholt) -> offen speichern
        let n = if locs.len() >= 2 && locs.first() == locs.last() {
            locs.len() - 1
        } else {
            locs.len()
        };
        locs[..n]
            .iter()
            .map(|l| proj.forward(l[0] as f64 / 1e7, l[1] as f64 / 1e7))
            .collect()
    };
    rings
        .iter()
        .map(|r| {
            let mut shape: Shape = vec![conv(&r.locs)];
            shape.extend(r.inner.iter().map(|h| conv(h)));
            shape
        })
        .collect()
}

pub fn assemble_way(data: &PbfData, refs: &[i64]) -> Option<Vec<Ring>> {
    let mut a = Assembler::new();
    let locs: Vec<Option<Loc>> = refs.iter().map(|id| data.location(*id)).collect();
    if locs.first().copied().flatten().is_none() || locs.last().copied().flatten().is_none() {
        return None;
    }
    a.add_way(&locs, false, 0);
    a.create_rings().ok()
}

pub enum RelationOutcome {
    Rings(Vec<Ring>),
    /// Mitglieds-Way nicht in der PBF: libosmiums MultipolygonManager
    /// assembliert nur vollstaendige Relationen (kein Fehler, still).
    Incomplete,
    Invalid,
}

pub fn assemble_relation(data: &PbfData, rel: &crate::pbf::Relation) -> RelationOutcome {
    let mut a = Assembler::new();
    let mut any = false;
    for (i, m) in rel.members.iter().enumerate() {
        let Some(&wi) = data.way_index.get(&m.way_id) else {
            return RelationOutcome::Incomplete;
        };
        let way = &data.ways[wi];
        let locs: Vec<Option<Loc>> = way.refs.iter().map(|id| data.location(*id)).collect();
        a.add_way(&locs, m.role == "inner", i);
        any = true;
    }
    if !any {
        return RelationOutcome::Incomplete;
    }
    match a.create_rings() {
        Ok(r) => RelationOutcome::Rings(r),
        Err(_) => RelationOutcome::Invalid,
    }
}

pub fn run(config_path: &Path, args: ExtractArgs) -> Result<()> {
    let t0 = Instant::now();
    let spec = TagSpec::load(&tags_path(&args.tags, config_path))?;
    let keys: HashSet<&str> = spec.relevant_keys.iter().map(String::as_str).collect();
    let keep: HashSet<&str> = spec.read_keys.iter().map(String::as_str).collect();
    let data = crate::pbf::read(
        &args.pbf_path,
        &keys,
        &keep,
        &|_t: &Tags| false,
        &|t: &Tags| spec.match_category(t).is_some(),
    )?;
    info!(
        "extract: {} Relationen, {} Mitglieds-Ways, {} geschlossene Ways kompakt ({:.0} MB), {} Knoten mit Lage, {} Knoten mit Tags gelesen ({:.0} s)",
        data.relations.len(),
        data.ways.len(),
        data.closed.len(),
        data.closed.bytes() as f64 / 1e6,
        data.node_ids.len(),
        data.tagged_nodes.len(),
        t0.elapsed().as_secs_f64()
    );
    std::fs::create_dir_all(&args.out_dir)?;
    let proj = Utm32::new();
    let mut pw = StreamWriter::protected(&args.out_dir.join("protected.parquet"))?;
    let mut bw = StreamWriter::buildings(&args.out_dir.join("buildings.parquet"))?;
    let mut protected: Vec<ProtectedOut> = Vec::new();
    let mut buildings: Vec<BuildingOut> = Vec::new();
    let mut n_protected = 0usize;
    let mut n_buildings = 0usize;
    let mut n_errors = 0usize;
    let mut n_seen_protected = 0usize;
    let mut n_incomplete = 0usize;

    // Punkt-Fallbacks
    for node in &data.tagged_nodes {
        if args.limit.is_some_and(|l| n_seen_protected >= l) {
            break;
        }
        let Some(m) = spec.match_category(&node.tags) else {
            continue;
        };
        if m.min_area_m2.is_some_and(|min| min > 0.0) {
            // Punkt-Fallback ohne echte Flaeche (z. B. Pool ohne access-Tag): weg
            continue;
        }
        let cat = m.category;
        let [x, y] = proj.forward(node.loc[0] as f64 / 1e7, node.loc[1] as f64 / 1e7);
        let ring =
            crate::geos::point_buffer(x, y, POINT_FALLBACK_RADIUS_M, POINT_FALLBACK_QUAD_SEGS);
        let open: Vec<[f64; 2]> = ring[..ring.len() - 1].to_vec();
        let area = crate::geos::shapes_area(&[vec![ring.clone()]]);
        protected.push(ProtectedOut {
            osm_type: "node",
            osm_id: node.id,
            category: cat.to_string(),
            is_point_fallback: true,
            area_m2: area,
            plausibility_warning: false,
            wkb: crate::wkb::write(&[vec![open]]),
        });
        n_seen_protected += 1;
    }

    let mut flush = |protected: &mut Vec<ProtectedOut>,
                     buildings: &mut Vec<BuildingOut>,
                     force: bool|
     -> Result<()> {
        if force || protected.len() + buildings.len() >= args.batch_size {
            pw.write_protected(protected)?;
            bw.write_buildings(buildings)?;
            n_protected += protected.len();
            n_buildings += buildings.len();
            protected.clear();
            buildings.clear();
        }
        Ok(())
    };

    // Flaechen: geschlossene Ways, dann Relationen
    let mut handle_area = |osm_type: &'static str,
                           id: i64,
                           tags: &Tags,
                           rings: Option<Vec<Ring>>,
                           protected: &mut Vec<ProtectedOut>,
                           buildings: &mut Vec<BuildingOut>| {
        let matched = spec.match_category(tags);
        let is_building = tags.has("building");
        if matched.is_none() && !is_building {
            return;
        }
        let Some(rings) = rings else {
            n_errors += 1;
            return;
        };
        let shapes = project_rings(&rings, &proj);
        let area = crate::geos::shapes_area(&shapes);
        if shapes.is_empty() || area.is_nan() || area <= 0.0 {
            n_errors += 1;
            return;
        }
        // Flaechen immer als MultiPolygon-WKB (wie osmiums create_multipolygon)
        let wkb = crate::wkb::write_multipolygon(&shapes);
        // `public_only` ohne access-Tag: Mindestflaeche
        let category = matched.filter(|m| m.min_area_m2.is_none_or(|min| area >= min));
        if let Some(m) = category {
            let cat = m.category;
            protected.push(ProtectedOut {
                osm_type,
                osm_id: id,
                category: cat.to_string(),
                is_point_fallback: false,
                area_m2: area,
                plausibility_warning: area > spec.plausibility_limit_m2(tags),
                wkb: wkb.clone(),
            });
        }
        if is_building {
            buildings.push(BuildingOut {
                osm_id: id,
                is_opaque: spec.is_opaque_building(tags),
                building_tag: tags.get("building").unwrap_or("").to_string(),
                wkb,
            });
        }
    };
    data.for_each_way_by_id(|id, refs, tags| -> Result<bool> {
        if args.limit.is_some_and(|l| n_seen_protected >= l) {
            return Ok(false);
        }
        let closed = refs.len() >= 4 && refs.first() == refs.last();
        if !closed || tags.get("area") == Some("no") {
            return Ok(true);
        }
        if !tags.0.iter().any(|(k, _)| keys.contains(k.as_str())) {
            return Ok(true);
        }
        let is_cat = spec.match_category(&tags).is_some();
        handle_area(
            "way",
            id,
            &tags,
            assemble_way(&data, refs),
            &mut protected,
            &mut buildings,
        );
        if is_cat {
            n_seen_protected += 1;
        }
        flush(&mut protected, &mut buildings, false)?;
        Ok(true)
    })?;
    for rel in &data.relations {
        if args.limit.is_some_and(|l| n_seen_protected >= l) {
            break;
        }
        if !rel.tags.0.iter().any(|(k, _)| keys.contains(k.as_str())) {
            continue;
        }
        let is_cat = spec.match_category(&rel.tags).is_some();
        let rings = match assemble_relation(&data, rel) {
            RelationOutcome::Rings(r) => Some(r),
            RelationOutcome::Incomplete => {
                n_incomplete += 1;
                continue;
            }
            RelationOutcome::Invalid => None,
        };
        handle_area(
            "relation",
            rel.id,
            &rel.tags,
            rings,
            &mut protected,
            &mut buildings,
        );
        if is_cat {
            n_seen_protected += 1;
        }
        flush(&mut protected, &mut buildings, false)?;
    }
    flush(&mut protected, &mut buildings, true)?;
    pw.close()?;
    bw.close()?;
    let summary = serde_json::json!({
        "stage": "extract",
        "elapsed_s": (t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
        "pbf": args.pbf_path.to_string_lossy(),
        // Welche Tag-Spezifikation diese Objekte ausgewaehlt hat -- ohne das
        // laesst sich ein Datenstand hinterher nicht zuordnen.
        "tags_spec": tags_path(&args.tags, config_path).to_string_lossy(),
        "tags_spec_version": spec.version,
        "n_protected": n_protected,
        "n_buildings": n_buildings,
        "n_geometry_errors": n_errors,
        "n_incomplete_relations": n_incomplete,
        "engine": "bubatz",
    });
    std::fs::write(
        args.out_dir.join("extract_summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    println!(
        "extract: {n_protected} Verbotsobjekte, {n_buildings} Gebaeude, {n_errors} Geometriefehler -> {} ({:.0} s)",
        args.out_dir.display(),
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
