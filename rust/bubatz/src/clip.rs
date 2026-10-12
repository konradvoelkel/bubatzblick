//! `bubatz clip`: eine `boundary=administrative`-Relation (per ID oder
//! Name + admin_level) aus der PBF assemblieren und `protected.parquet` auf
//! die Grenze plus Marge filtern (`within(boundary.buffer(margin))`);
//! `buildings.parquet` wird bewusst NICHT gefiltert (Hindernisse jenseits der
//! Grenze bleiben Hindernisse).
//!
//! **Grenzcache:** `--boundary-cache` legt neben
//! der Geometrie (`<cache>.wkb`) die Relation-, Way- und Knoten-IDs der
//! Grenze und die PBF-Folge ab. Beim naechsten Lauf wird der PBF-Pass
//! uebersprungen, wenn die PBF auf derselben Replikationsquelle steht, alle
//! Diffs seit der Cache-Folge unter `--diffs-dir` liegen (`update-pbf
//! --diff-dir`) und keines davon eine dieser IDs enthaelt. Sonst laeuft der
//! volle PBF-Pass.
//!
//! Unterschied zu Python: der Marge-Puffer ist der exakte Versatz mit 200
//! Segmenten je Vollkreis (i_overlay) statt GEOS `buffer` (512 Segmente, aber
//! Eingabe um 1 % der Distanz vereinfacht -- bei 252 m bis 2,5 m). Objekte,
//! die den Rand des Puffers auf Meter genau beruehren, koennen anders
//! entschieden werden; das sind Objekte 250 m jenseits der Grenze.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use bubatzcore::geom::{Shape, Shapes};
use clap::Args;
use i_overlay::core::overlay_rule::OverlayRule;
use tracing::info;

use crate::extract::{assemble_relation, project_rings};
use crate::pbf::Tags;
use crate::proj::Utm32;

#[derive(Args, Debug)]
pub struct ClipArgs {
    #[arg(long)]
    pub pbf: PathBuf,
    /// Bekannte Relation-ID (z. B. Duesseldorf: 62539)
    #[arg(long)]
    pub admin_relation_id: Option<i64>,
    /// Alternative: Name der Verwaltungseinheit (mit --admin-level)
    #[arg(long)]
    pub admin_name: Option<String>,
    #[arg(long)]
    pub admin_level: Option<u32>,
    #[arg(long, default_value = "data/interim/protected.parquet")]
    pub protected_in: PathBuf,
    /// Default: config.effective_reach_m
    #[arg(long)]
    pub margin_m: Option<f64>,
    #[arg(long)]
    pub out_dir: PathBuf,
    /// Grenzcache (JSON + .wkb daneben): Grenze ohne PBF-Pass uebernehmen, wenn die Diffs sie nicht beruehren
    #[arg(long)]
    pub boundary_cache: Option<PathBuf>,
    /// Verzeichnis mit den angewandten Diffs `<folge>.osc.gz` (update-pbf --diff-dir)
    #[arg(long)]
    pub diffs_dir: Option<PathBuf>,
}

/// IDs, von denen die Grenzgeometrie abhaengt.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct BoundaryIds {
    pub relation: i64,
    pub ways: Vec<i64>,
    pub nodes: Vec<i64>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct BoundaryCache {
    selector: String,
    base_url: String,
    sequence: i64,
    n_parts: usize,
    ids: BoundaryIds,
}

fn selector(args: &ClipArgs) -> String {
    format!(
        "id={:?} name={:?} level={:?}",
        args.admin_relation_id, args.admin_name, args.admin_level
    )
}

/// Folgen in (from, to], fuer die keine Diff-Datei vorliegt.
pub fn missing_sequences(dir: &Path, from: i64, to: i64) -> Vec<i64> {
    (from + 1..=to)
        .filter(|s| !dir.join(format!("{s}.osc.gz")).is_file())
        .collect()
}

/// Liest eine `.osc.gz` und meldet, ob sie eine der IDs beruehrt (create,
/// modify oder delete -- alle drei enthalten `<node id=".."`,
/// `<way id=".."`, `<relation id=".."`). Byteweise Suche nach den drei
/// Praefixen, kein XML-Parser noetig.
pub fn diff_touches(path: &Path, ids: &BoundaryIds) -> Result<bool> {
    use std::collections::HashSet;
    use std::io::Read;
    let f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(std::io::BufReader::new(f))
        .read_to_end(&mut text)
        .with_context(|| format!("{} entpacken", path.display()))?;
    let ways: HashSet<i64> = ids.ways.iter().copied().collect();
    let nodes: HashSet<i64> = ids.nodes.iter().copied().collect();
    Ok(scan_ids(&text, |kind, id| match kind {
        b'n' => nodes.contains(&id),
        b'w' => ways.contains(&id),
        _ => id == ids.relation,
    }))
}

/// Ruft `hit(kind, id)` fuer jedes `<node|way|relation id="N"` auf
/// (kind = erstes Zeichen des Elementnamens) und bricht beim ersten Treffer ab.
fn scan_ids(text: &[u8], mut hit: impl FnMut(u8, i64) -> bool) -> bool {
    let mut i = 0;
    while let Some(off) = text[i..].iter().position(|&b| b == b'<') {
        let p = i + off + 1;
        let rest = &text[p..];
        let kind_len = if rest.starts_with(b"node id=\"") {
            Some(9)
        } else if rest.starts_with(b"way id=\"") {
            Some(8)
        } else if rest.starts_with(b"relation id=\"") {
            Some(13)
        } else {
            None
        };
        if let Some(n) = kind_len {
            let digits: Vec<u8> = rest[n..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .copied()
                .collect();
            if let Ok(id) = std::str::from_utf8(&digits).unwrap_or("").parse::<i64>()
                && hit(rest[0], id)
            {
                return true;
            }
        }
        i = p;
    }
    false
}

/// Grenze aus dem Cache, wenn er zur PBF passt und die Diffs sie nicht
/// beruehren; `None` = PBF-Pass noetig (Grund im Log).
fn boundary_from_cache(args: &ClipArgs, cache_path: &Path) -> Result<Option<(i64, Shapes)>> {
    if !cache_path.is_file() || !cache_path.with_extension("wkb").is_file() {
        info!("clip: kein Grenzcache unter {}", cache_path.display());
        return Ok(None);
    }
    let c: BoundaryCache = serde_json::from_str(&std::fs::read_to_string(cache_path)?)
        .with_context(|| cache_path.display().to_string())?;
    if c.selector != selector(args) {
        info!(
            "clip: Grenzcache fuer andere Auswahl ({}) -- PBF-Pass",
            c.selector
        );
        return Ok(None);
    }
    let h = crate::update_pbf::read_header(&args.pbf)?;
    if h.base_url != c.base_url || h.sequence < c.sequence {
        info!(
            "clip: Grenzcache von {} Folge {}, PBF {} Folge {} -- PBF-Pass",
            c.base_url, c.sequence, h.base_url, h.sequence
        );
        return Ok(None);
    }
    if h.sequence > c.sequence {
        let Some(dir) = &args.diffs_dir else {
            info!(
                "clip: Grenzcache Folge {} < PBF Folge {} und kein --diffs-dir -- PBF-Pass",
                c.sequence, h.sequence
            );
            return Ok(None);
        };
        let missing = missing_sequences(dir, c.sequence, h.sequence);
        if !missing.is_empty() {
            info!(
                "clip: Diffs fehlen unter {} (z. B. Folge {}) -- PBF-Pass",
                dir.display(),
                missing[0]
            );
            return Ok(None);
        }
        for seq in c.sequence + 1..=h.sequence {
            let p = dir.join(format!("{seq}.osc.gz"));
            if diff_touches(&p, &c.ids)? {
                info!(
                    "clip: Diff Folge {seq} beruehrt die Grenze (Relation {}) -- PBF-Pass",
                    c.ids.relation
                );
                return Ok(None);
            }
        }
    }
    let wkb = std::fs::read(cache_path.with_extension("wkb"))?;
    let shapes = crate::wkb::parse(&wkb).context("Grenzcache-WKB")?;
    if shapes.len() != c.n_parts {
        bail!("Grenzcache: {} Teile, erwartet {}", shapes.len(), c.n_parts);
    }
    info!(
        "clip: Grenze Relation {} aus dem Cache (Folge {} -> {}, {} Ways, {} Knoten geprueft)",
        c.ids.relation,
        c.sequence,
        h.sequence,
        c.ids.ways.len(),
        c.ids.nodes.len()
    );
    let relation = c.ids.relation;
    if h.sequence > c.sequence {
        write_cache(
            cache_path,
            &BoundaryCache {
                sequence: h.sequence,
                ..c
            },
            None,
        )?;
    }
    Ok(Some((relation, shapes)))
}

fn write_cache(path: &Path, c: &BoundaryCache, wkb: Option<&[u8]>) -> Result<()> {
    if let Some(w) = wkb {
        let tmp = path.with_extension("wkb.tmp");
        std::fs::write(&tmp, w)?;
        std::fs::rename(&tmp, path.with_extension("wkb"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(c)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Grenzgeometrie (EPSG:25832) der gesuchten Relation, dazu die IDs, von
/// denen sie abhaengt (fuer den Grenzcache).
pub fn admin_boundary(pbf: &Path, args: &ClipArgs) -> Result<(i64, Shapes, BoundaryIds)> {
    let keys: HashSet<&str> = HashSet::new();
    let ok = |t: &Tags| t.get("boundary") == Some("administrative");
    let data = crate::pbf::read(pbf, &keys, &keys, &ok, &|_t: &Tags| false)?;
    let proj = Utm32::new();
    let mut matches: Vec<(i64, Shapes, BoundaryIds)> = Vec::new();
    for rel in &data.relations {
        if rel.tags.get("boundary") != Some("administrative") {
            continue;
        }
        let hit = match (args.admin_relation_id, &args.admin_name, args.admin_level) {
            (Some(id), _, _) => rel.id == id,
            (None, Some(name), Some(level)) => {
                rel.tags.get("name") == Some(name.as_str())
                    && rel.tags.get("admin_level") == Some(level.to_string().as_str())
            }
            _ => bail!(
                "genau eines von --admin-relation-id oder --admin-name (+--admin-level) angeben"
            ),
        };
        if !hit {
            continue;
        }
        let rings = match assemble_relation(&data, rel) {
            crate::extract::RelationOutcome::Rings(r) => r,
            crate::extract::RelationOutcome::Incomplete => bail!(
                "Relation {} ist in {} unvollstaendig (Mitglieds-Ways fehlen)",
                rel.id,
                pbf.display()
            ),
            crate::extract::RelationOutcome::Invalid => {
                bail!(
                    "Relation {} laesst sich nicht zu einer Flaeche assemblieren",
                    rel.id
                )
            }
        };
        let mut ways: Vec<i64> = rel.members.iter().map(|m| m.way_id).collect();
        ways.sort_unstable();
        ways.dedup();
        let mut nodes: Vec<i64> = ways
            .iter()
            .filter_map(|w| data.way_index.get(w).map(|&i| &data.ways[i]))
            .flat_map(|w| w.refs.iter().copied())
            .collect();
        nodes.sort_unstable();
        nodes.dedup();
        matches.push((
            rel.id,
            project_rings(&rings, &proj),
            BoundaryIds {
                relation: rel.id,
                ways,
                nodes,
            },
        ));
    }
    match matches.len() {
        0 => bail!(
            "keine passende boundary=administrative-Relation in {} gefunden",
            pbf.display()
        ),
        1 => Ok(matches.pop().unwrap()),
        n => bail!(
            "mehrdeutig: {n} Treffer: {:?}",
            matches.iter().map(|m| m.0).collect::<Vec<_>>()
        ),
    }
}

fn bounds_of(shapes: &[Shape]) -> [f64; 4] {
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

fn boxed(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
    vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]
}

/// Die Suchflaeche (Grenze + Marge) in Kacheln zerlegt: `within` rechnet je
/// Objekt nur gegen die Stuecke unter seinem Rechteck statt gegen die ganze
/// Landesgrenze (NRW: 110.000 Objekte gegen eine Grenze mit Hunderttausenden
/// Stuetzpunkten dauerte sonst Stunden). Zwei Stufen (Bloecke aus 10 x 10
/// Kacheln, dann Kacheln), damit auch die Bundesgrenze schnell zerlegt ist.
/// Die Stuecke partitionieren die Suchflaeche; die Differenz eines Objekts
/// gegen die Vereinigung seiner Stuecke ist die Differenz gegen die Flaeche.
struct TiledArea {
    bounds: [f64; 4],
    size: f64,
    nx: usize,
    ny: usize,
    tiles: Vec<Shapes>,
}

impl TiledArea {
    fn new(search: &[Shape], size: f64) -> Self {
        use rayon::prelude::*;
        let bounds = bounds_of(search);
        let nx = ((bounds[2] - bounds[0]) / size).ceil().max(1.0) as usize;
        let ny = ((bounds[3] - bounds[1]) / size).ceil().max(1.0) as usize;
        let per_block = 10usize;
        let nbx = nx.div_ceil(per_block);
        let nby = ny.div_ceil(per_block);
        let block_size = size * per_block as f64;
        let blocks: Vec<(usize, usize, Shapes)> = (0..nbx * nby)
            .into_par_iter()
            .filter_map(|k| {
                let (bx, by) = (k % nbx, k / nbx);
                let x0 = bounds[0] + bx as f64 * block_size;
                let y0 = bounds[1] + by as f64 * block_size;
                let piece = bubatzcore::union::overlay_pair(
                    search,
                    &[boxed(x0, y0, x0 + block_size, y0 + block_size)],
                    OverlayRule::Intersect,
                );
                (!piece.is_empty()).then_some((bx, by, piece))
            })
            .collect();
        let pieces: Vec<(usize, Shapes)> = blocks
            .par_iter()
            .flat_map_iter(|(bx, by, piece)| {
                let mut out = Vec::new();
                for ty in by * per_block..((by + 1) * per_block).min(ny) {
                    for tx in bx * per_block..((bx + 1) * per_block).min(nx) {
                        let x0 = bounds[0] + tx as f64 * size;
                        let y0 = bounds[1] + ty as f64 * size;
                        let t = bubatzcore::union::overlay_pair(
                            piece,
                            &[boxed(x0, y0, x0 + size, y0 + size)],
                            OverlayRule::Intersect,
                        );
                        if !t.is_empty() {
                            out.push((ty * nx + tx, t));
                        }
                    }
                }
                out
            })
            .collect();
        let mut tiles = vec![Vec::new(); nx * ny];
        for (i, t) in pieces {
            tiles[i] = t;
        }
        Self {
            bounds,
            size,
            nx,
            ny,
            tiles,
        }
    }

    /// `within(search_area)`: nichts vom Objekt liegt ausserhalb.
    fn contains(&self, object: &[Shape]) -> bool {
        let b = bounds_of(object);
        if b[0] < self.bounds[0]
            || b[1] < self.bounds[1]
            || b[2] > self.bounds[2]
            || b[3] > self.bounds[3]
        {
            return false;
        }
        let ix = |x: f64| (((x - self.bounds[0]) / self.size) as usize).min(self.nx - 1);
        let iy = |y: f64| (((y - self.bounds[1]) / self.size) as usize).min(self.ny - 1);
        let mut pieces: Vec<Shape> = Vec::new();
        for ty in iy(b[1])..=iy(b[3]) {
            for tx in ix(b[0])..=ix(b[2]) {
                pieces.extend(self.tiles[ty * self.nx + tx].iter().cloned());
            }
        }
        if pieces.is_empty() {
            return false;
        }
        let outside = bubatzcore::union::overlay_pair(object, &pieces, OverlayRule::Difference);
        bubatzcore::geom::shapes_area(&outside) <= 0.0
    }
}

pub fn run(config_path: &Path, args: ClipArgs) -> Result<()> {
    let t0 = Instant::now();
    let cfg = crate::config::load(config_path)?;
    let margin = args.margin_m.unwrap_or(cfg.effective_reach_m());
    let cached = match &args.boundary_cache {
        Some(p) => boundary_from_cache(&args, p)?,
        None => None,
    };
    let (rel_id, boundary) = match cached {
        Some(c) => c,
        None => {
            let (rel_id, boundary, ids) = admin_boundary(&args.pbf, &args)?;
            if let Some(p) = &args.boundary_cache {
                let h = crate::update_pbf::read_header(&args.pbf)?;
                write_cache(
                    p,
                    &BoundaryCache {
                        selector: selector(&args),
                        base_url: h.base_url,
                        sequence: h.sequence,
                        n_parts: boundary.len(),
                        ids,
                    },
                    Some(&crate::wkb::write(&boundary)),
                )?;
                info!("clip: Grenzcache -> {}", p.display());
            }
            (rel_id, boundary)
        }
    };
    if args.admin_name.is_some() {
        println!(
            "Relation-ID fuer {:?} (admin_level={:?}): {rel_id}",
            args.admin_name, args.admin_level
        );
    }
    let area = bubatzcore::geom::shapes_area(&boundary);
    info!(
        "clip: Grenze Relation {rel_id}, {} Teile, {:.4} km2 ({:.0} s)",
        boundary.len(),
        area / 1e6,
        t0.elapsed().as_secs_f64()
    );
    let search = bubatzcore::free_circle(&boundary, margin);
    let area = TiledArea::new(&search, 10_000.0);
    info!(
        "clip: Suchflaeche in {} x {} Kacheln a 10 km zerlegt ({:.0} s)",
        area.nx,
        area.ny,
        t0.elapsed().as_secs_f64()
    );
    // protected.parquet vollstaendig lesen (alle Spalten) und gefiltert schreiben
    let rows = crate::parquet_io::read_protected_full(&args.protected_in)?;
    // Reihenfolge der Eingabe bleibt erhalten (Vec<Result<bool>> ist geordnet,
    // Result<Vec<_>> aus rayon nicht).
    let flags: Vec<Result<bool>> = {
        use rayon::prelude::*;
        rows.par_iter()
            .map(|r| Ok(area.contains(&crate::wkb::parse(&r.wkb)?)))
            .collect()
    };
    let mut kept = Vec::new();
    for (r, f) in rows.into_iter().zip(flags) {
        if f? {
            kept.push(r);
        }
    }
    std::fs::create_dir_all(&args.out_dir)?;
    let mut w =
        crate::geoparquet::StreamWriter::protected(&args.out_dir.join("protected.parquet"))?;
    w.write_protected(&kept)?;
    w.close()?;
    println!(
        "clip: {} von {} Verbotsobjekten innerhalb Grenze + {margin:.0} m -> {} ({:.0} s)",
        kept.len(),
        crate::parquet_io::count_rows(&args.protected_in)?,
        args.out_dir.join("protected.parquet").display(),
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn gz(text: &str) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(text.as_bytes()).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn scan_finds_ids_of_all_three_kinds() {
        let x = br#"<osmChange><modify><node id="12" lat="1" lon="2"/><way id="34"><nd ref="12"/></way></modify><delete><relation id="56"/></delete></osmChange>"#;
        let mut seen = Vec::new();
        assert!(!scan_ids(x, |k, id| {
            seen.push((k, id));
            false
        }));
        assert_eq!(seen, vec![(b'n', 12), (b'w', 34), (b'r', 56)]);
        assert!(scan_ids(x, |k, id| k == b'w' && id == 34));
        assert!(!scan_ids(x, |_, id| id == 99), "nd ref zaehlt nicht");
    }

    #[test]
    fn diff_touches_boundary_ids_only() {
        let d = std::env::temp_dir().join(format!("bubatz-clip-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let ids = BoundaryIds {
            relation: 51477,
            ways: vec![100, 200],
            nodes: vec![7, 8, 9],
        };
        let a = d.join("1.osc.gz");
        std::fs::write(
            &a,
            gz(r#"<modify><node id="5"/><way id="300"/><relation id="1"/></modify>"#),
        )
        .unwrap();
        assert!(!diff_touches(&a, &ids).unwrap());
        let b = d.join("2.osc.gz");
        std::fs::write(&b, gz(r#"<delete><node id="8"/></delete>"#)).unwrap();
        assert!(diff_touches(&b, &ids).unwrap());
        let c = d.join("3.osc.gz");
        std::fs::write(&c, gz(r#"<create><relation id="51477"/></create>"#)).unwrap();
        assert!(diff_touches(&c, &ids).unwrap());
        assert_eq!(missing_sequences(&d, 0, 4), vec![4]);
        assert_eq!(missing_sequences(&d, 3, 3), Vec::<i64>::new());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
