//! PBF lesen (osmpbf): drei Durchgaenge, ohne libosmium.
//!
//! 1. Relationen: `type=multipolygon`/`boundary` mit relevanten Schluesseln
//!    (Kategorie-Tags oder `building`) -> Mitglieds-Ways merken.
//! 2. Ways: geschlossene Ways mit relevanten Schluesseln und alle Mitglieds-
//!    Ways der Kandidaten-Relationen -> Knotenreferenzen + Tags; benoetigte
//!    Knoten-IDs sammeln.
//! 3. Knoten: Koordinaten der benoetigten Knoten (int32-Festkomma 1e-7 wie
//!    libosmium: Nanograd / 100, abgeschnitten) plus Knoten mit Kategorie-Tags
//!    (Punkt-Fallback).
//!
//! Koordinaten als `decimicro` (i32) -- `lon = x as f64 / 1e7` reproduziert
//! `osmium::Location::lon()` bitgleich.
//!
//! **Speicher (Ziel 8-GB-Server):** die ~42 Mio. geschlossenen
//! Ways Deutschlands (fast alle reine Gebaeude) liegen nicht mehr als je ein
//! `Way` mit eigener Heap-Liste (56 Byte + 16 Byte je Way + 8 Byte je
//! Referenz, national ~5,5 GB), sondern je PBF-Block als ein Bytestrom:
//! ID relativ zur ersten ID des Blocks, Referenzen als Differenzen, alles als
//! Varint (~20 Byte je Way, national < 1 GB). Nur Relationsmitglieder bleiben
//! als `Way` (Nachschlagen per ID in `assemble_relation` und `clip`). Die
//! benoetigten Knoten-IDs werden eimerweise sortiert (Spitze = Referenzen
//! einmal, nicht Referenzen + Kopie).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use osmpbf::{BlobDecode, BlobReader, Element};
use rayon::prelude::*;

#[derive(Debug, Clone)]
pub struct Tags(pub Vec<(String, String)>);

impl Tags {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    pub fn has(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }
}

#[derive(Debug, Clone)]
pub struct RelMember {
    pub way_id: i64,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct Relation {
    pub id: i64,
    pub tags: Tags,
    pub members: Vec<RelMember>,
}

/// Tags eines Ways -- vollstaendig nur, wo sie gebraucht werden. Von den
/// ~37 Mio. geschlossenen Ways Deutschlands sind fast alle reine Gebaeude
/// (Adress-, Hoehen-, Dachtags ...); mit vollstaendigen Tags brauchte
/// `extract` national > 16 GB (OOM auf einem kleinen Server).
/// Ein Way ohne Kategorie-Schluessel kann keine Kategorie treffen; von ihm
/// liest `extract` nur `building` (Wert), `building:levels=0` (Opazitaet)
/// und `area=no` -- genau das bleibt, der Rest der Tags entfaellt.
#[derive(Debug, Clone)]
pub enum WayTags {
    /// Kandidat fuer eine Kategorie oder Relationsmitglied: alle Tags.
    /// Reine Gebaeude liegen nicht hier, sondern kompakt als
    /// `PackedTags::Building`.
    Full(Tags),
}

/// Mitglieds-Way einer Kandidaten-Relation (alle Tags, Referenzen als Liste).
#[derive(Debug, Clone)]
pub struct Way {
    pub id: i64,
    pub tags: WayTags,
    /// Knotenreferenzen; `Box<[i64]>` statt `Vec` spart 8 Byte je Way
    pub refs: Box<[i64]>,
}

fn building_tags(values: &[String], value: u32, levels_zero: bool, area_no: bool) -> Tags {
    let mut v = vec![("building".to_string(), values[value as usize].clone())];
    if levels_zero {
        v.push(("building:levels".to_string(), "0".to_string()));
    }
    if area_no {
        v.push(("area".to_string(), "no".to_string()));
    }
    Tags(v)
}

impl Way {
    /// Tags in der Form, die `TagSpec` liest; fuer reine Gebaeude nur die
    /// drei ausgewerteten Schluessel (in Dateireihenfolge nicht
    /// rekonstruierbar, aber `Tags::get` ist reihenfolgeunabhaengig).
    pub fn tags<'a>(&'a self, data: &'a PbfData) -> Cow<'a, Tags> {
        let _ = data;
        match &self.tags {
            WayTags::Full(t) => Cow::Borrowed(t),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaggedNode {
    pub id: i64,
    pub tags: Tags,
    pub loc: [i32; 2],
}

// --- kompakte geschlossene Ways ----------------------------------------------

fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn get_varint(buf: &[u8], pos: &mut usize) -> u64 {
    let mut v = 0u64;
    let mut shift = 0u32;
    loop {
        let b = buf[*pos];
        *pos += 1;
        v |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return v;
        }
        shift += 7;
    }
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Tags eines kompakt gespeicherten Ways: Gebaeude-Kurzform oder die
/// behaltenen Tags als Paare von Indizes in die Zeichenkettentabelle.
pub enum PackedTags {
    Full(Vec<(u32, u32)>),
    Building {
        value: u32,
        levels_zero: bool,
        area_no: bool,
    },
}

impl PackedTags {
    fn tags(self, strings: &[String]) -> Tags {
        match self {
            PackedTags::Full(pairs) => Tags(
                pairs
                    .into_iter()
                    .map(|(k, v)| (strings[k as usize].clone(), strings[v as usize].clone()))
                    .collect(),
            ),
            PackedTags::Building {
                value,
                levels_zero,
                area_no,
            } => building_tags(strings, value, levels_zero, area_no),
        }
    }
}

/// Geschlossene Nicht-Mitglieds-Ways EINES PBF-Blocks als Bytestrom.
/// Eintrag: zigzag(id - first_id) | n_refs | zigzag(ref_0) | zigzag(ref_k -
/// ref_{k-1}) ... | Art (0 = Gebaeude: value, Flags; 1 = Tags: n, n x
/// (Schluessel, Wert) als Indizes der Zeichenkettentabelle). Jeder Eintrag
/// ist ab seinem Offset allein dekodierbar.
#[derive(Debug, Default)]
pub struct ClosedBlock {
    first_id: i64,
    last_id: i64,
    min_id: i64,
    max_id: i64,
    /// IDs im Block streng aufsteigend
    sorted: bool,
    n: u32,
    n_full: u32,
    bytes: Vec<u8>,
}

impl ClosedBlock {
    fn push(&mut self, id: i64, refs: &[i64], tags: PackedTags) {
        if self.n == 0 {
            self.first_id = id;
            self.min_id = id;
            self.max_id = id;
            self.sorted = true;
        } else {
            if id <= self.last_id {
                self.sorted = false;
            }
            self.min_id = self.min_id.min(id);
            self.max_id = self.max_id.max(id);
        }
        self.last_id = id;
        put_varint(&mut self.bytes, zigzag(id - self.first_id));
        put_varint(&mut self.bytes, refs.len() as u64);
        let mut prev = 0i64;
        for &r in refs {
            put_varint(&mut self.bytes, zigzag(r.wrapping_sub(prev)));
            prev = r;
        }
        match tags {
            PackedTags::Building {
                value,
                levels_zero,
                area_no,
            } => {
                self.bytes.push(0);
                put_varint(&mut self.bytes, value as u64);
                self.bytes
                    .push(u8::from(levels_zero) | (u8::from(area_no) << 1));
            }
            PackedTags::Full(pairs) => {
                self.bytes.push(1);
                put_varint(&mut self.bytes, pairs.len() as u64);
                for (k, v) in pairs {
                    put_varint(&mut self.bytes, k as u64);
                    put_varint(&mut self.bytes, v as u64);
                }
                self.n_full += 1;
            }
        }
        self.n += 1;
    }

    /// Eintrag bei `pos` lesen; `refs` wird ueberschrieben.
    fn decode(&self, pos: &mut usize, refs: &mut Vec<i64>) -> (i64, PackedTags) {
        let id = self.first_id + unzigzag(get_varint(&self.bytes, pos));
        let n = get_varint(&self.bytes, pos) as usize;
        refs.clear();
        refs.reserve(n);
        let mut prev = 0i64;
        for _ in 0..n {
            prev = prev.wrapping_add(unzigzag(get_varint(&self.bytes, pos)));
            refs.push(prev);
        }
        let kind = self.bytes[*pos];
        *pos += 1;
        let tags = if kind == 0 {
            let value = get_varint(&self.bytes, pos) as u32;
            let flags = self.bytes[*pos];
            *pos += 1;
            PackedTags::Building {
                value,
                levels_zero: flags & 1 != 0,
                area_no: flags & 2 != 0,
            }
        } else {
            let n = get_varint(&self.bytes, pos) as usize;
            let pairs = (0..n)
                .map(|_| {
                    let k = get_varint(&self.bytes, pos) as u32;
                    let v = get_varint(&self.bytes, pos) as u32;
                    (k, v)
                })
                .collect();
            PackedTags::Full(pairs)
        };
        (id, tags)
    }
}

/// Besucher fuer Ways in ID-Reihenfolge: (ID, Referenzen, Tags) -> weiter?
pub type WayVisitor<'a> = dyn FnMut(i64, &[i64], Cow<'_, Tags>) -> Result<bool> + 'a;

/// Alle kompakten Bloecke in Dateireihenfolge. Liegen die Ways in der PBF
/// nach ID sortiert (osmium/Geofabrik: immer), laufen die Bloecke direkt in
/// ID-Reihenfolge durch; sonst ueber einen nach ID sortierten Index
/// (id, Block, Offset).
#[derive(Debug, Default)]
pub struct ClosedWays {
    blocks: Vec<ClosedBlock>,
    n: usize,
    ordered: bool,
    index: Option<Vec<(i64, u32, u32)>>,
}

impl ClosedWays {
    fn new(blocks: Vec<ClosedBlock>) -> Self {
        let n = blocks.iter().map(|b| b.n as usize).sum();
        let mut ordered = true;
        let mut prev_max: Option<i64> = None;
        for b in blocks.iter().filter(|b| b.n > 0) {
            if !b.sorted || prev_max.is_some_and(|p| p >= b.min_id) {
                ordered = false;
                break;
            }
            prev_max = Some(b.max_id);
        }
        let index = if ordered {
            None
        } else {
            let mut idx: Vec<(i64, u32, u32)> = Vec::with_capacity(n);
            let mut refs = Vec::new();
            for (bi, b) in blocks.iter().enumerate() {
                let mut pos = 0usize;
                for _ in 0..b.n {
                    let off = pos as u32;
                    let (id, _) = b.decode(&mut pos, &mut refs);
                    idx.push((id, bi as u32, off));
                }
            }
            idx.sort_unstable_by_key(|e| e.0);
            Some(idx)
        };
        ClosedWays {
            blocks,
            n,
            ordered,
            index,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn bytes(&self) -> usize {
        self.blocks.iter().map(|b| b.bytes.len()).sum()
    }

    /// Ways mit behaltenen Tags (Kategorie-Kandidaten).
    pub fn n_full(&self) -> usize {
        self.blocks.iter().map(|b| b.n_full as usize).sum()
    }

    /// Alle Ways aufsteigend nach ID; `f` gibt `false` zurueck, um abzubrechen.
    /// Rueckgabe: `false`, wenn abgebrochen.
    fn for_each(&self, values: &[String], f: &mut WayVisitor<'_>) -> Result<bool> {
        let mut refs: Vec<i64> = Vec::new();
        if self.ordered {
            for b in &self.blocks {
                let mut pos = 0usize;
                for _ in 0..b.n {
                    let (id, tags) = b.decode(&mut pos, &mut refs);
                    if !f(id, &refs, Cow::Owned(tags.tags(values)))? {
                        return Ok(false);
                    }
                }
            }
        } else {
            for &(_, bi, off) in self.index.as_ref().unwrap() {
                let b = &self.blocks[bi as usize];
                let mut pos = off as usize;
                let (id, tags) = b.decode(&mut pos, &mut refs);
                if !f(id, &refs, Cow::Owned(tags.tags(values)))? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Alle Knotenreferenzen (in Dateireihenfolge, mit Wiederholungen).
    fn for_each_ref(&self, f: &mut dyn FnMut(i64)) {
        let mut refs: Vec<i64> = Vec::new();
        for b in &self.blocks {
            let mut pos = 0usize;
            for _ in 0..b.n {
                b.decode(&mut pos, &mut refs);
                for &r in &refs {
                    f(r);
                }
            }
        }
    }
}

pub struct PbfData {
    pub relations: Vec<Relation>,
    /// Mitglieds-Ways der Kandidaten-Relationen (alle Tags), nach ID sortiert.
    pub ways: Vec<Way>,
    /// Way-ID -> Index in `ways` (Nachschlagen in `assemble_relation`, `clip`).
    pub way_index: HashMap<i64, usize>,
    /// Geschlossene Ways mit relevanten Schluesseln, die KEINE Mitglieder sind, kompakt.
    pub closed: ClosedWays,
    /// Zeichenkettentabelle der kompakten Ways: `building`-Werte
    /// (`WayTags::Building`), Schluessel und Werte behaltener Tags.
    pub strings: Vec<String>,
    /// Benoetigte Knoten-IDs (sortiert, eindeutig) und ihre Lage an derselben
    /// Position, gepackt `x | y << 32` (decimicro); `u64::MAX` = nicht in der
    /// PBF. Atomar, weil Durchgang 3 die Lagen parallel an Ort und Stelle
    /// eintraegt -- keine Zwischenliste (id, Lage), kein Umsortieren, kein
    /// Entpacken: national 213 Mio. Knoten, 1,7 GB statt > 10 GB Spitze.
    pub node_ids: Vec<i64>,
    pub node_locs: Vec<AtomicU64>,
    pub tagged_nodes: Vec<TaggedNode>,
}

const NO_LOC: u64 = u64::MAX;

fn pack_loc(x: i32, y: i32) -> u64 {
    (x as u32 as u64) | ((y as u32 as u64) << 32)
}

impl PbfData {
    pub fn location(&self, id: i64) -> Option<[i32; 2]> {
        let i = self.node_ids.binary_search(&id).ok()?;
        let v = self.node_locs[i].load(Ordering::Relaxed);
        if v == NO_LOC {
            return None;
        }
        Some([v as u32 as i32, (v >> 32) as u32 as i32])
    }

    /// Alle Ways (Mitglieder und kompakte geschlossene) aufsteigend nach ID,
    /// statt einer einzigen sortierten Liste; `f` gibt `false` zurueck, um
    /// abzubrechen (`extract --limit`).
    pub fn for_each_way_by_id(
        &self,
        mut f: impl FnMut(i64, &[i64], Cow<'_, Tags>) -> Result<bool>,
    ) -> Result<()> {
        let mut mi = 0usize;
        // Mitglieder mit ID unter `limit` (bzw. alle) ausgeben.
        fn members_below(
            data: &PbfData,
            mi: &mut usize,
            limit: Option<i64>,
            f: &mut WayVisitor<'_>,
        ) -> Result<bool> {
            while *mi < data.ways.len() && limit.is_none_or(|l| data.ways[*mi].id < l) {
                let w = &data.ways[*mi];
                *mi += 1;
                if !f(w.id, &w.refs, w.tags(data))? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        let go_on = self.closed.for_each(&self.strings, &mut |id, refs, tags| {
            if !members_below(self, &mut mi, Some(id), &mut f)? {
                return Ok(false);
            }
            f(id, refs, tags)
        })?;
        if go_on {
            members_below(self, &mut mi, None, &mut f)?;
        }
        Ok(())
    }
}

/// Zeichenkettentabelle fuer die kompakten Ways waehrend Durchgang 2:
/// `building`-Werte, Schluessel und Werte der behaltenen Tags (ein Mutex;
/// fast immer `yes`/`parking`/`pitch`: der Griff ist kurz).
#[derive(Default)]
struct Interner {
    map: HashMap<String, u32>,
    strings: Vec<String>,
}

impl Interner {
    fn intern(&mut self, v: &str) -> u32 {
        if let Some(&i) = self.map.get(v) {
            return i;
        }
        let i = self.strings.len() as u32;
        self.map.insert(v.to_string(), i);
        self.strings.push(v.to_string());
        i
    }
}

/// Bloecke der PBF parallel lesen, je Block EIN Ergebnis in Dateireihenfolge
/// (statt osmpbfs `par_map_reduce`, das die Blockergebnisse paarweise per
/// `extend` zusammenfuehrt und damit die Spitze national verdoppelt).
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

/// Teillisten einmal in eine passend grosse Liste verschieben.
fn flatten<T>(parts: Vec<Vec<T>>) -> Vec<T> {
    let n: usize = parts.iter().map(Vec::len).sum();
    let mut out = Vec::with_capacity(n);
    for p in parts {
        out.extend(p);
    }
    out
}

/// Tags einsammeln: `relevant` = einer der Schluessel `keys` kommt vor;
/// behalten werden nur Tags mit Schluessel in `keep` (`None` = alle) --
/// mehr liest die Tag-Spezifikation nicht, und die vollen Tag-Listen von
/// Millionen Parkplaetzen und Sportplaetzen kosteten national Gigabytes.
fn collect_tags<'a>(
    it: impl Iterator<Item = (&'a str, &'a str)>,
    keys: &HashSet<&str>,
    keep: Option<&HashSet<&str>>,
) -> (Tags, bool) {
    let mut tags = Vec::new();
    let mut relevant = false;
    for (k, v) in it {
        if keys.contains(k) {
            relevant = true;
        }
        if keep.is_none_or(|s| s.contains(k)) {
            tags.push((k.to_string(), v.to_string()));
        }
    }
    (Tags(tags), relevant)
}

/// Benoetigte Knoten-IDs sortiert und eindeutig, eimerweise (32 Mio. IDs je
/// Eimer): jede Referenz liegt einmal im Speicher, die Eimer werden nach dem
/// Sortieren einzeln in die Ergebnisliste verschoben und freigegeben.
fn needed_node_ids(data_ways: &[Way], closed: &ClosedWays) -> Vec<i64> {
    const SHIFT: u32 = 25;
    let bucket = |id: i64| (id.max(0) >> SHIFT) as usize;
    let mut max_b = 0usize;
    let mut n_refs = 0usize;
    let mut visit = |id: i64| {
        max_b = max_b.max(bucket(id));
        n_refs += 1;
    };
    for w in data_ways {
        w.refs.iter().for_each(|&r| visit(r));
    }
    closed.for_each_ref(&mut visit);
    let mut counts = vec![0usize; max_b + 1];
    for w in data_ways {
        w.refs.iter().for_each(|&r| counts[bucket(r)] += 1);
    }
    closed.for_each_ref(&mut |r| counts[bucket(r)] += 1);
    let mut buckets: Vec<Vec<i64>> = counts.iter().map(|&c| Vec::with_capacity(c)).collect();
    for w in data_ways {
        w.refs.iter().for_each(|&r| buckets[bucket(r)].push(r));
    }
    closed.for_each_ref(&mut |r| buckets[bucket(r)].push(r));
    buckets.par_iter_mut().for_each(|b| {
        b.sort_unstable();
        b.dedup();
    });
    let total: usize = buckets.iter().map(Vec::len).sum();
    debug_assert!(total <= n_refs);
    let mut out: Vec<i64> = Vec::with_capacity(total);
    for b in buckets.iter_mut() {
        out.extend_from_slice(b);
        *b = Vec::new();
    }
    out
}

/// Resident Set des Prozesses in MB (Linux /proc), fuer die Phasenmeldungen.
pub fn rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map(|pages| pages * 4096 / 1_000_000)
        .unwrap_or(0)
}

/// Alle drei Durchgaenge; `keys` = relevante Tag-Schluessel (`building` plus
/// die Kategorie-Schluessel), `keep` = alle Schluessel, die der Aufrufer an
/// Ways und Knoten liest (nur diese Tags werden gespeichert; Relationen
/// behalten alle Tags), `relation_ok` erlaubt weitere Relationen (z. B.
/// `boundary=administrative` fuer `clip`), `node_ok` entscheidet ueber die
/// behaltenen Knoten mit Tags (Punkt-Fallback: nur Kategorie-Treffer; von
/// 3,8 Mio. Knoten mit relevantem Schluessel bleiben so wenige).
pub fn read(
    path: &Path,
    keys: &HashSet<&str>,
    keep: &HashSet<&str>,
    relation_ok: &(dyn Fn(&Tags) -> bool + Sync),
    node_ok: &(dyn Fn(&Tags) -> bool + Sync),
) -> Result<PbfData> {
    let t0 = std::time::Instant::now();
    // Durchgang 1: Relationen
    let parts = par_blocks(path, |block| {
        let mut out = Vec::new();
        for el in block.elements() {
            if let Element::Relation(r) = el {
                let (tags, relevant) = collect_tags(r.tags(), keys, None);
                let t = tags.get("type");
                let is_mp = matches!(t, Some("multipolygon") | Some("boundary"));
                if is_mp && (relevant || relation_ok(&tags)) {
                    let members = r
                        .members()
                        .filter(|m| m.member_type == osmpbf::RelMemberType::Way)
                        .map(|m| RelMember {
                            way_id: m.member_id,
                            role: m.role().unwrap_or("").to_string(),
                        })
                        .collect();
                    out.push(Relation {
                        id: r.id(),
                        tags,
                        members,
                    });
                }
            }
        }
        out
    })?;
    let mut relations = flatten(parts);
    relations.sort_by_key(|r| r.id);
    let member_ways: HashSet<i64> = relations
        .iter()
        .flat_map(|r| r.members.iter().map(|m| m.way_id))
        .collect();
    tracing::info!(
        "pbf: Durchgang 1: {} Relationen, {} Mitglieds-Ways ({:.0} s, RSS {} MB)",
        relations.len(),
        member_ways.len(),
        t0.elapsed().as_secs_f64(),
        rss_mb()
    );

    // Durchgang 2: Ways -- Mitglieder als `Way`, alle anderen kompakt je Block
    let interner = Mutex::new(Interner::default());
    let parts: Vec<(Vec<Way>, ClosedBlock)> = par_blocks(path, |block| {
        let mut members = Vec::new();
        let mut closed_block = ClosedBlock::default();
        let mut refs: Vec<i64> = Vec::new();
        for el in block.elements() {
            let Element::Way(w) = el else { continue };
            refs.clear();
            refs.extend(w.refs());
            let member = member_ways.contains(&w.id());
            let closed = refs.len() >= 4 && refs.first() == refs.last();
            if !member && !closed {
                continue;
            }
            let (tags, relevant) = collect_tags(w.tags(), keys, Some(keep));
            if !(member || relevant) {
                continue;
            }
            if member {
                members.push(Way {
                    id: w.id(),
                    tags: WayTags::Full(tags),
                    refs: refs.as_slice().into(),
                });
                continue;
            }
            let building_only = !tags
                .0
                .iter()
                .any(|(k, _)| k != "building" && keys.contains(k.as_str()));
            let tags = if building_only {
                let value = interner
                    .lock()
                    .unwrap()
                    .intern(tags.get("building").unwrap_or(""));
                PackedTags::Building {
                    value,
                    levels_zero: tags.get("building:levels") == Some("0"),
                    area_no: tags.get("area") == Some("no"),
                }
            } else {
                let mut it = interner.lock().unwrap();
                PackedTags::Full(
                    tags.0
                        .iter()
                        .map(|(k, v)| (it.intern(k), it.intern(v)))
                        .collect(),
                )
            };
            closed_block.push(w.id(), &refs, tags);
        }
        closed_block.bytes.shrink_to_fit();
        (members, closed_block)
    })?;
    let (member_parts, blocks): (Vec<Vec<Way>>, Vec<ClosedBlock>) = parts.into_iter().unzip();
    let mut ways = flatten(member_parts);
    let strings = interner.into_inner().unwrap().strings;
    // IDs sind eindeutig: instabil sortieren (kein n/2-Puffer)
    ways.sort_unstable_by_key(|w| w.id);
    let way_index: HashMap<i64, usize> = ways.iter().enumerate().map(|(i, w)| (w.id, i)).collect();
    let closed = ClosedWays::new(blocks);
    tracing::info!(
        "pbf: Durchgang 2: {} Mitglieds-Ways, {} geschlossene Ways kompakt ({:.0} MB Strom, {} mit vollen Tags, sortiert={}) ({:.0} s, RSS {} MB)",
        ways.len(),
        closed.len(),
        closed.bytes() as f64 / 1e6,
        closed.n_full(),
        closed.ordered,
        t0.elapsed().as_secs_f64(),
        rss_mb()
    );
    let needed = needed_node_ids(&ways, &closed);
    let node_locs: Vec<AtomicU64> = (0..needed.len()).map(|_| AtomicU64::new(NO_LOC)).collect();
    tracing::info!(
        "pbf: {} benoetigte Knoten ({:.0} s, RSS {} MB)",
        needed.len(),
        t0.elapsed().as_secs_f64(),
        rss_mb()
    );

    // Durchgang 3: Knoten -- Lagen direkt an die Position der ID schreiben
    let tagged_parts: Vec<Vec<TaggedNode>> = par_blocks(path, |block| {
        let mut tagged = Vec::new();
        for el in block.elements() {
            let mut handle = |id: i64, x: i32, y: i32, tags: (Tags, bool)| {
                if let Ok(i) = needed.binary_search(&id) {
                    node_locs[i].store(pack_loc(x, y), Ordering::Relaxed);
                }
                if tags.1 && node_ok(&tags.0) {
                    tagged.push(TaggedNode {
                        id,
                        tags: tags.0,
                        loc: [x, y],
                    });
                }
            };
            match el {
                Element::Node(n) => {
                    let t = collect_tags(n.tags(), keys, Some(keep));
                    handle(n.id(), n.decimicro_lon(), n.decimicro_lat(), t);
                }
                Element::DenseNode(n) => {
                    let t = collect_tags(n.tags(), keys, Some(keep));
                    handle(n.id(), n.decimicro_lon(), n.decimicro_lat(), t);
                }
                _ => {}
            }
        }
        tagged
    })?;
    let mut tagged = flatten(tagged_parts);
    tagged.sort_by_key(|n| n.id);
    tracing::info!(
        "pbf: Durchgang 3: {} Knoten mit Tags ({:.0} s, RSS {} MB)",
        tagged.len(),
        t0.elapsed().as_secs_f64(),
        rss_mb()
    );
    Ok(PbfData {
        relations,
        ways,
        way_index,
        closed,
        strings,
        node_ids: needed,
        node_locs,
        tagged_nodes: tagged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        Tags(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn varint_zigzag_roundtrip() {
        for v in [
            0i64,
            1,
            -1,
            63,
            -64,
            64,
            1 << 40,
            -(1 << 40),
            i64::MAX,
            i64::MIN,
        ] {
            let mut b = Vec::new();
            put_varint(&mut b, zigzag(v));
            let mut pos = 0;
            assert_eq!(unzigzag(get_varint(&b, &mut pos)), v);
            assert_eq!(pos, b.len());
        }
    }

    /// Zeichenkettentabelle der Beispiele: 0 yes, 1 house, 2 roof, 3 leisure,
    /// 4 park, 5 name, 6 P
    fn strings() -> Vec<String> {
        ["yes", "house", "roof", "leisure", "park", "name", "P"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn sample_ways() -> Vec<(i64, Vec<i64>, PackedTags)> {
        vec![
            (
                100,
                vec![5_000_000_001, 5_000_000_002, 5_000_000_003, 5_000_000_001],
                PackedTags::Building {
                    value: 0,
                    levels_zero: false,
                    area_no: false,
                },
            ),
            (
                101,
                vec![7, 3, 9, 7],
                PackedTags::Full(vec![(3, 4), (5, 6)]),
            ),
            (
                250,
                vec![1, 2, 3, 4, 1],
                PackedTags::Building {
                    value: 2,
                    levels_zero: true,
                    area_no: true,
                },
            ),
        ]
    }

    #[test]
    fn closed_block_roundtrip_in_order_and_via_index() {
        let values = strings();
        let mut b = ClosedBlock::default();
        for (id, refs, t) in sample_ways() {
            b.push(id, &refs, t);
        }
        assert_eq!(b.n_full, 1);
        assert!(b.sorted);
        let cw = ClosedWays::new(vec![b]);
        assert!(cw.ordered && cw.index.is_none());
        let mut seen = Vec::new();
        cw.for_each(&values, &mut |id, refs, t| {
            seen.push((id, refs.to_vec(), t.into_owned().0));
            Ok(true)
        })
        .unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].0, 100);
        assert_eq!(seen[0].1, sample_ways()[0].1);
        assert_eq!(seen[0].2, vec![("building".to_string(), "yes".to_string())]);
        assert_eq!(
            seen[1].2,
            vec![
                ("leisure".to_string(), "park".to_string()),
                ("name".to_string(), "P".to_string())
            ],
            "behaltene Tags"
        );
        assert_eq!(
            seen[2].2,
            vec![
                ("building".to_string(), "roof".to_string()),
                ("building:levels".to_string(), "0".to_string()),
                ("area".to_string(), "no".to_string())
            ]
        );

        // Unsortiert (zwei Bloecke, ueberlappende IDs): Index sortiert nach ID
        let mut b1 = ClosedBlock::default();
        let mut b2 = ClosedBlock::default();
        let mut w = sample_ways();
        let (w2, w1, w0) = (w.pop().unwrap(), w.pop().unwrap(), w.pop().unwrap());
        b1.push(w2.0, &w2.1, w2.2);
        b1.push(w0.0, &w0.1, w0.2);
        b2.push(w1.0, &w1.1, w1.2);
        assert!(!b1.sorted);
        let cw = ClosedWays::new(vec![b1, b2]);
        assert!(!cw.ordered && cw.index.is_some());
        let mut ids = Vec::new();
        let mut n_refs = 0;
        cw.for_each_ref(&mut |_| n_refs += 1);
        assert_eq!(n_refs, 4 + 4 + 5);
        cw.for_each(&values, &mut |id, _, _| {
            ids.push(id);
            Ok(id != 101)
        })
        .unwrap();
        assert_eq!(ids, vec![100, 101], "sortiert und abgebrochen");
    }

    #[test]
    fn needed_ids_are_sorted_unique() {
        let mut b = ClosedBlock::default();
        for (id, refs, t) in sample_ways() {
            b.push(id, &refs, t);
        }
        let member = Way {
            id: 5,
            tags: WayTags::Full(tags(&[])),
            refs: vec![9, 8, 5_000_000_001, 40_000_000_000].into(),
        };
        let needed = needed_node_ids(std::slice::from_ref(&member), &ClosedWays::new(vec![b]));
        assert_eq!(
            needed,
            vec![
                1,
                2,
                3,
                4,
                7,
                8,
                9,
                5_000_000_001,
                5_000_000_002,
                5_000_000_003,
                40_000_000_000
            ]
        );
    }
}
