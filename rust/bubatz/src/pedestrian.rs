//! `bubatz pedestrian` -- Fussgaengerzonen (§ 5 Abs. 2 Nr. 5 KCanG) als eigene,
//! zuschaltbare Ebene: `highway=pedestrian` direkt als Geometrie, OHNE Puffer
//! und ohne Sichtweite. Nr. 5 verbietet den Konsum *in* der Fussgaengerzone
//! zwischen 7 und 20 Uhr, nicht in ihrer Sichtweite -- es gibt hier also
//! nichts zu rechnen, nur zu zeichnen.
//!
//! **Warum kein Puffer** (Datenlage am Bundes-PBF):
//! von 49 991 `highway=pedestrian` in Deutschland sind nur 31,6 % als Flaeche
//! erfasst, 3,0 % tragen `width`/`est_width` und 3,8 % das Verkehrszeichen
//! DE:242. Ein Puffer muesste die Breite also fuer 97 % raten und saehe dabei
//! gerechnet aus. Flaechen bleiben Flaechen, Wege bleiben Linien. Die
//! Bubatzkarte-App macht es genauso.
//!
//! **Uebererfassung ist gewollt und begrenzt:** `highway=pedestrian` umfasst
//! auch Parkwege und Bahnhofsvorplaetze, die keine Fussgaengerzone im
//! Rechtssinn sind. Nur auf das Schild DE:242 zu filtern liesse 96 % der
//! echten Zonen verschwinden -- die schlechtere Irrefuehrung. Die Ebene ist
//! deshalb zuschaltbar und fliesst NICHT in die roten Sichtweitenzonen ein.
//!
//! Keine Projektion, kein WKB, kein Parquet: der einzige Abnehmer ist
//! tippecanoe, und ohne metrische Rechnung bleibt alles in EPSG:4326.
//! Flaechenregel wie bei osmium: geschlossener Way UND `area=yes`.
//!
//! **Multipolygon-Relationen** (`type=multipolygon` + `highway=pedestrian`)
//! laufen mit: 2 618 in Deutschland neben
//! 49 524 Ways, rund 5 %, darunter gerade die grossen, aus mehreren Ringen
//! zusammengesetzten Plaetze. Ihre Mitglieds-Ways kommen in einem eigenen
//! Durchgang aus der PBF; die Ringe baut `assemble::Assembler` nach den
//! libosmium-Regeln (Aussenring gegen den Uhrzeigersinn, Loch mit -- so will
//! es auch GeoJSON), die Ausgabe ist ein MultiPolygon. Relationen mit einem
//! Mitglied ausserhalb der Datei (unvollstaendig) oder mit offenen oder sich
//! kreuzenden Ringen (ungueltig) werden gezaehlt und weggelassen, wie in
//! `extract`.
//!
//! **Durchgaenge:** 1. Ways und Relationen mit dem Tag, 2. nur mit Relationen:
//! die Knotenreferenzen ihrer Mitglieds-Ways, 3. die Lagen der gebrauchten
//! Knoten. Drei statt einem, weil sonst die Lage ALLER Knoten Deutschlands im
//! Speicher stehen muesste; so sind es nur die der Fussgaengerzonen.
//!
//! **Eigenschaften je Merkmal:** `osm_id` und `osm_type` (`way` oder
//! `relation`) -- beide IDs zaehlen getrennt, ohne den Typ waere die Zahl im
//! Popup nicht nachschlagbar. Keine OSM-Freitexte: Fremdtext aus den Daten
//! wird nicht in die Kacheln uebernommen.

use std::collections::{HashMap, HashSet};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::Args;
use osmpbf::{BlobDecode, BlobReader, Element, RelMemberType};
use rayon::iter::{ParallelBridge, ParallelIterator};

use crate::assemble::{Assembler, Loc, Ring};

/// Name der Ebene im Kachelarchiv; der Viewer spricht sie so an.
pub const LAYER_NAME: &str = "fussgaengerzonen";
/// Beschreibung des Archivs in seinen Metadaten (`tiles::scrub_metadata`).
pub const DESCRIPTION: &str =
    "Fussgaengerzonen (highway=pedestrian) aus OpenStreetMap, Paragraf 5 Abs. 2 Nr. 5 KCanG";

#[derive(Args, Debug)]
pub struct PedestrianArgs {
    /// Pfad zur .osm.pbf
    pub pbf_path: PathBuf,
    /// GeoJSONSeq (Zwischenprodukt, bleibt zum Nachsehen liegen)
    #[arg(long, default_value = "data/interim/fussgaengerzonen.geojsonl")]
    pub out_geojson: PathBuf,
    /// Kachelarchiv
    #[arg(long, default_value = "data/out/fussgaengerzonen.pmtiles")]
    pub out_path: PathBuf,
    #[arg(long, default_value_t = 8)]
    pub min_zoom: i32,
    #[arg(long, default_value_t = 14)]
    pub max_zoom: i32,
    /// Nur die GeoJSONSeq schreiben, tippecanoe nicht aufrufen
    #[arg(long, default_value_t = false)]
    pub no_tiles: bool,
}

/// Ein Way mit `highway=pedestrian`, so wie er aus der PBF kommt.
struct PedWay {
    id: i64,
    refs: Vec<i64>,
    area: bool,
}

/// Eine Multipolygon-Relation mit `highway=pedestrian`: Way-Mitglieder mit
/// Rolle (`inner` oder nicht -- alles andere zaehlt als aussen, wie in
/// `extract::assemble_relation`). Knoten- und Relationsmitglieder entfallen.
struct PedRel {
    id: i64,
    members: Vec<(i64, bool)>,
}

fn par_blocks<T: Send>(
    path: &Path,
    f: impl Fn(&osmpbf::PrimitiveBlock) -> T + Sync,
) -> Result<Vec<T>> {
    let reader =
        BlobReader::from_path(path).with_context(|| format!("{} oeffnen", path.display()))?;
    let mut parts: Vec<(usize, T)> = reader
        .enumerate()
        .par_bridge()
        .map(|(seq, blob)| -> Result<Option<(usize, T)>> {
            match blob?.decode()? {
                BlobDecode::OsmData(block) => Ok(Some((seq, f(&block)))),
                _ => Ok(None),
            }
        })
        .filter_map(|r| r.transpose())
        .collect::<Result<_>>()?;
    parts.sort_by_key(|p| p.0);
    Ok(parts.into_iter().map(|p| p.1).collect())
}

/// Durchgang 1: Ways und Multipolygon-Relationen mit `highway=pedestrian`.
fn read_candidates(path: &Path) -> Result<(Vec<PedWay>, Vec<PedRel>)> {
    let parts = par_blocks(path, |block| {
        let mut ways = Vec::new();
        let mut rels = Vec::new();
        for el in block.elements() {
            match el {
                Element::Way(w) => {
                    let mut is_ped = false;
                    let mut area = false;
                    for (k, v) in w.tags() {
                        match k {
                            "highway" if v == "pedestrian" => is_ped = true,
                            "area" if v == "yes" => area = true,
                            _ => {}
                        }
                    }
                    if is_ped {
                        ways.push(PedWay {
                            id: w.id(),
                            refs: w.refs().collect(),
                            area,
                        });
                    }
                }
                Element::Relation(r) => {
                    let mut is_ped = false;
                    let mut is_mp = false;
                    for (k, v) in r.tags() {
                        match k {
                            "highway" if v == "pedestrian" => is_ped = true,
                            "type" if v == "multipolygon" => is_mp = true,
                            _ => {}
                        }
                    }
                    if is_ped && is_mp {
                        let members = r
                            .members()
                            .filter(|m| m.member_type == RelMemberType::Way)
                            .map(|m| (m.member_id, m.role().unwrap_or("") == "inner"))
                            .collect();
                        rels.push(PedRel {
                            id: r.id(),
                            members,
                        });
                    }
                }
                _ => {}
            }
        }
        (ways, rels)
    })?;
    let mut ways = Vec::new();
    let mut rels = Vec::new();
    for (w, r) in parts {
        ways.extend(w);
        rels.extend(r);
    }
    ways.sort_by_key(|w| w.id);
    rels.sort_by_key(|r| r.id);
    Ok((ways, rels))
}

/// Durchgang 2: Knotenreferenzen der Mitglieds-Ways, die nicht selbst
/// `highway=pedestrian` tragen (die anderen sind schon da).
fn read_member_refs(path: &Path, wanted: &HashSet<i64>) -> Result<HashMap<i64, Vec<i64>>> {
    let parts = par_blocks(path, |block| {
        let mut out: Vec<(i64, Vec<i64>)> = Vec::new();
        for el in block.elements() {
            if let Element::Way(w) = el
                && wanted.contains(&w.id())
            {
                out.push((w.id(), w.refs().collect()));
            }
        }
        out
    })?;
    Ok(parts.into_iter().flatten().collect())
}

/// Durchgang 3: nur die Lagen der gebrauchten Knoten, als Festkomma 1e-7
/// (`assemble::Loc`; die PBF speichert genau das).
fn read_locations(path: &Path, needed: &HashSet<i64>) -> Result<HashMap<i64, Loc>> {
    let parts = par_blocks(path, |block| {
        let mut out: Vec<(i64, Loc)> = Vec::new();
        for el in block.elements() {
            match el {
                Element::Node(n) if needed.contains(&n.id()) => {
                    out.push((n.id(), [n.decimicro_lon(), n.decimicro_lat()]));
                }
                Element::DenseNode(n) if needed.contains(&n.id()) => {
                    out.push((n.id(), [n.decimicro_lon(), n.decimicro_lat()]));
                }
                _ => {}
            }
        }
        out
    })?;
    Ok(parts.into_iter().flatten().collect())
}

/// Sieben Nachkommastellen wie GDALs GeoJSON-Schreiber (und wie `tiles`).
fn coords(line: &[Loc]) -> String {
    let mut s = String::with_capacity(line.len() * 24);
    s.push('[');
    for (i, [x, y]) in line.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let (x, y) = (*x as f64 / 1e7, *y as f64 / 1e7);
        s.push_str(&format!("[{x:.7},{y:.7}]"));
    }
    s.push(']');
    s
}

/// Ein Ring des Assemblers samt Loechern als GeoJSON-Polygon-Koordinaten.
/// Die Ringe kommen geschlossen (Start wiederholt) und richtig orientiert.
fn polygon_coords(ring: &Ring) -> String {
    let mut s = String::from("[");
    s.push_str(&coords(&ring.locs));
    for hole in &ring.inner {
        s.push(',');
        s.push_str(&coords(hole));
    }
    s.push(']');
    s
}

#[derive(Default)]
struct Zahlen {
    ways: usize,
    flaechen: usize,
    linien: usize,
    unvollstaendig: usize,
    relationen: usize,
    rel_flaechen: usize,
    rel_unvollstaendig: usize,
    rel_ungueltig: usize,
}

enum RelOutcome {
    Rings(Vec<Ring>),
    Incomplete,
    Invalid,
}

fn assemble_rel(
    rel: &PedRel,
    refs_of: &HashMap<i64, &[i64]>,
    loc: &HashMap<i64, Loc>,
) -> RelOutcome {
    if rel.members.is_empty() {
        return RelOutcome::Incomplete;
    }
    let mut a = Assembler::new();
    for (i, (way_id, inner)) in rel.members.iter().enumerate() {
        let Some(refs) = refs_of.get(way_id) else {
            return RelOutcome::Incomplete;
        };
        let locs: Vec<Option<Loc>> = refs.iter().map(|r| loc.get(r).copied()).collect();
        // Ein fehlender Knoten reisst den Ring auf; der Assembler meldet das
        // dann als ungueltig. Genauer geht es ohne die Knoten nicht.
        a.add_way(&locs, *inner, i);
    }
    match a.create_rings() {
        Ok(r) if !r.is_empty() => RelOutcome::Rings(r),
        _ => RelOutcome::Invalid,
    }
}

fn write_geojson(
    ways: &[PedWay],
    rels: &[PedRel],
    member_refs: &HashMap<i64, Vec<i64>>,
    loc: &HashMap<i64, Loc>,
    out: &Path,
) -> Result<Zahlen> {
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    let f = std::fs::File::create(out).with_context(|| format!("{} anlegen", out.display()))?;
    let mut w = BufWriter::new(f);
    let mut z = Zahlen {
        ways: ways.len(),
        relationen: rels.len(),
        ..Zahlen::default()
    };
    let mut feature = |geom: String, osm_type: &str, id: i64| -> Result<()> {
        writeln!(
            w,
            "{{\"type\":\"Feature\",\"geometry\":{geom},\"properties\":{{\"osm_id\":{id},\"osm_type\":\"{osm_type}\"}}}}"
        )?;
        Ok(())
    };
    for way in ways {
        // Ein Way am Rand eines Ausschnitts kann Knoten nennen, die nicht in
        // der Datei stehen. Lieber weglassen als eine verkuerzte Geometrie.
        let Some(line): Option<Vec<Loc>> = way.refs.iter().map(|r| loc.get(r).copied()).collect()
        else {
            z.unvollstaendig += 1;
            continue;
        };
        if line.len() < 2 {
            z.unvollstaendig += 1;
            continue;
        }
        let geschlossen = way.refs.first() == way.refs.last();
        // Flaechenregel wie osmium: `highway=pedestrian` ist nur mit `area=yes`
        // eine Flaeche; ein geschlossener Way ohne das ist eine Ringstrasse.
        let geom = if geschlossen && way.area && line.len() >= 4 {
            z.flaechen += 1;
            format!(
                "{{\"type\":\"Polygon\",\"coordinates\":[{}]}}",
                coords(&line)
            )
        } else {
            z.linien += 1;
            format!(
                "{{\"type\":\"LineString\",\"coordinates\":{}}}",
                coords(&line)
            )
        };
        feature(geom, "way", way.id)?;
    }
    // Mitglieds-Ways: erst die eigens gelesenen, dann die Fussgaengerzonen-Ways
    // selbst (ein Mitglied darf auch `highway=pedestrian` tragen).
    let mut refs_of: HashMap<i64, &[i64]> = member_refs
        .iter()
        .map(|(id, refs)| (*id, refs.as_slice()))
        .collect();
    refs_of.extend(ways.iter().map(|w| (w.id, w.refs.as_slice())));
    for rel in rels {
        match assemble_rel(rel, &refs_of, loc) {
            RelOutcome::Rings(rings) => {
                z.rel_flaechen += 1;
                let polys: Vec<String> = rings.iter().map(polygon_coords).collect();
                let geom = format!(
                    "{{\"type\":\"MultiPolygon\",\"coordinates\":[{}]}}",
                    polys.join(",")
                );
                feature(geom, "relation", rel.id)?;
            }
            RelOutcome::Incomplete => z.rel_unvollstaendig += 1,
            RelOutcome::Invalid => z.rel_ungueltig += 1,
        }
    }
    w.flush()?;
    Ok(z)
}

fn tippecanoe(input: &Path, out: &Path, min_zoom: i32, max_zoom: i32) -> Result<()> {
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    let status = Command::new("tippecanoe")
        .args(crate::tiles::TIPPECANOE_FLAGS)
        .arg(format!("--minimum-zoom={min_zoom}"))
        .arg(format!("--maximum-zoom={max_zoom}"))
        .arg(format!("--layer={LAYER_NAME}"))
        .arg(format!("--output={}", out.display()))
        .arg(input)
        .status()
        .context("tippecanoe starten -- nicht installiert?")?;
    if !status.success() {
        bail!("tippecanoe endete mit {status}");
    }
    Ok(())
}

pub fn run(args: PedestrianArgs) -> Result<()> {
    let t0 = Instant::now();
    let (ways, rels) = read_candidates(&args.pbf_path)?;
    let own: HashSet<i64> = ways.iter().map(|w| w.id).collect();
    let wanted: HashSet<i64> = rels
        .iter()
        .flat_map(|r| r.members.iter().map(|m| m.0))
        .filter(|id| !own.contains(id))
        .collect();
    tracing::info!(
        "pedestrian: Durchgang 1: {} Ways, {} Relationen mit {} fremden Mitglieds-Ways ({:.0} s)",
        ways.len(),
        rels.len(),
        wanted.len(),
        t0.elapsed().as_secs_f64()
    );
    let member_refs = if wanted.is_empty() {
        HashMap::new()
    } else {
        let m = read_member_refs(&args.pbf_path, &wanted)?;
        tracing::info!(
            "pedestrian: Durchgang 2: {} Mitglieds-Ways gefunden ({:.0} s)",
            m.len(),
            t0.elapsed().as_secs_f64()
        );
        m
    };
    let needed: HashSet<i64> = ways
        .iter()
        .flat_map(|w| w.refs.iter().copied())
        .chain(member_refs.values().flat_map(|r| r.iter().copied()))
        .collect();
    let loc = read_locations(&args.pbf_path, &needed)?;
    tracing::info!(
        "pedestrian: Durchgang 3: {} von {} Knotenlagen ({:.0} s)",
        loc.len(),
        needed.len(),
        t0.elapsed().as_secs_f64()
    );
    let z = write_geojson(&ways, &rels, &member_refs, &loc, &args.out_geojson)?;
    println!(
        "pedestrian: {} Ways -> {} Flaechen, {} Linien, {} unvollstaendig; {} Relationen -> {} Flaechen, {} unvollstaendig, {} ungueltig -> {}",
        z.ways,
        z.flaechen,
        z.linien,
        z.unvollstaendig,
        z.relationen,
        z.rel_flaechen,
        z.rel_unvollstaendig,
        z.rel_ungueltig,
        args.out_geojson.display()
    );
    if !args.no_tiles {
        tippecanoe(
            &args.out_geojson,
            &args.out_path,
            args.min_zoom,
            args.max_zoom,
        )?;
        crate::tiles::scrub_metadata(&args.out_path, LAYER_NAME, DESCRIPTION)?;
        println!("pedestrian: -> {}", args.out_path.display());
    }
    Ok(())
}
