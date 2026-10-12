//! Gebaeudespeicher auf Platte (`buildings.bstore`, Ziel 8-GB-Server):
//! die opaken Grundrisse aus `buildings.parquet`, nach 1-km-Kacheln
//! sortiert, je Datensatz Zeilenindex, Bounding-Box und WKB. `zones` und `tiles`
//! lesen je Zelle nur die Kacheln ihres Suchkastens (`pread`, Seitencache) statt
//! alle ~39 Mio. Gebaeude als einzelne `Vec<u8>` samt R-Baum im Speicher zu
//! halten (national ~9,5 GB anonymer Speicher).
//!
//! Die Abfrage liefert genau die Gebaeude, deren Bounding-Box den Suchkasten
//! schneidet (abgeschlossene Intervalle wie rstars `locate_in_envelope_
//! intersecting`), aufsteigend nach Zeilenindex -- dieselbe Menge in derselben
//! Reihenfolge wie bei einem R-Baum, damit Zellen-Fingerabdruck,
//! `cache_key`, Kachel-Cache und Grundrissabzug byte-gleich bleiben.
//!
//! Aufbau: ein Durchgang ueber die Parquet-Datei, Datensaetze in 64
//! Teildateien (Kachel-Koordinaten modulo 8 -- gleichmaessig, unabhaengig von
//! der Siedlungsdichte), je Teildatei stabil nach Kachel sortieren und
//! anhaengen; Spitze im Speicher ~1/64 der Daten. Gebaeude breiter oder hoeher
//! als eine Kachel liegen in einem eigenen Bereich, der bei jeder Abfrage
//! ganz durchgesehen wird (national wenige tausend).
//!
//! Gueltigkeit: Groesse und mtime der Parquet-Datei im Kopf (wie das
//! Zonen-Manifest); passt beides nicht, wird der Speicher neu gebaut.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use tracing::info;

const MAGIC: &[u8; 8] = b"BSTORE01";
const HEADER_LEN: usize = 8 + 8 + 8 + 8 + 8 + 8 + 8 + 8 + 8;
/// Kachelkante; Gebaeude bis zu dieser Ausdehnung liegen in der Kachel ihrer
/// linken unteren Ecke, groessere im Sonderbereich.
pub const BUCKET_M: f64 = 1000.0;
const N_PARTS: usize = 64;
/// Datensatzkopf: Zeilenindex u32, WKB-Laenge u32, Bounding-Box 4 x f64.
const REC_HEAD: usize = 4 + 4 + 32;
/// Versatz der Kachelkoordinaten, damit negative Lagen positiv bleiben.
const GRID_OFFSET: f64 = 32768.0;

fn bucket_coord(v: f64) -> u32 {
    let b = (v / BUCKET_M).floor() + GRID_OFFSET;
    // NaN faellt in Kachel 0 (die Bounding-Box-Pruefung schlaegt dann ohnehin fehl)
    if b.is_nan() || b < 0.0 {
        0
    } else if b > 65535.0 {
        65535
    } else {
        b as u32
    }
}

fn spread(mut v: u32) -> u32 {
    v &= 0xffff;
    v = (v | (v << 8)) & 0x00ff_00ff;
    v = (v | (v << 4)) & 0x0f0f_0f0f;
    v = (v | (v << 2)) & 0x3333_3333;
    v = (v | (v << 1)) & 0x5555_5555;
    v
}

/// Z-Ordnung der Kachel (Spalte, Zeile): raeumlich benachbarte Kacheln liegen
/// meist nahe beieinander in der Datei.
fn zkey(col: u32, row: u32) -> u32 {
    spread(col) | (spread(row) << 1)
}

fn intersects(b: &[f64; 4], q: &[f64; 4]) -> bool {
    !(b[0] > q[2] || b[2] < q[0] || b[1] > q[3] || b[3] < q[1])
}

fn fingerprint(parquet: &Path) -> Result<(u64, u64)> {
    let md = std::fs::metadata(parquet).with_context(|| format!("{} lesen", parquet.display()))?;
    let mtime = md
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Ok((md.len(), mtime))
}

struct Header {
    parquet_size: u64,
    parquet_mtime: u64,
    n: u64,
    n_keys: u64,
    table_off: u64,
    oversize_off: u64,
    oversize_len: u64,
    bucket_m: f64,
}

impl Header {
    fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[..8].copy_from_slice(MAGIC);
        let fields = [
            self.parquet_size,
            self.parquet_mtime,
            self.n,
            self.n_keys,
            self.table_off,
            self.oversize_off,
            self.oversize_len,
            self.bucket_m.to_bits(),
        ];
        for (i, f) in fields.iter().enumerate() {
            b[8 + 8 * i..16 + 8 * i].copy_from_slice(&f.to_le_bytes());
        }
        b
    }
    fn parse(b: &[u8]) -> Option<Header> {
        if b.len() < HEADER_LEN || &b[..8] != MAGIC {
            return None;
        }
        let f = |i: usize| u64::from_le_bytes(b[8 + 8 * i..16 + 8 * i].try_into().unwrap());
        Some(Header {
            parquet_size: f(0),
            parquet_mtime: f(1),
            n: f(2),
            n_keys: f(3),
            table_off: f(4),
            oversize_off: f(5),
            oversize_len: f(6),
            bucket_m: f64::from_bits(f(7)),
        })
    }
}

/// Ein Treffer der Abfrage: Zeilenindex in `buildings.parquet`, Bounding-Box,
/// WKB-Bereich im Puffer der Abfrage.
#[derive(Clone, Copy)]
struct HitRef {
    idx: u32,
    bounds: [f64; 4],
    start: usize,
    len: usize,
}

/// Ergebnis einer Abfrage: die gelesenen Kachelbytes und die Treffer darin,
/// aufsteigend nach Zeilenindex.
pub struct Hits {
    buf: Vec<u8>,
    hits: Vec<HitRef>,
}

impl Hits {
    pub fn len(&self) -> usize {
        self.hits.len()
    }
    /// Nur fuer die Tests: `idx` belegt, dass die Trefferreihenfolge dem
    /// Zeilenindex folgt.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
    #[cfg(test)]
    pub fn idx(&self, i: usize) -> u32 {
        self.hits[i].idx
    }
    pub fn bounds(&self, i: usize) -> [f64; 4] {
        self.hits[i].bounds
    }
    pub fn wkb(&self, i: usize) -> &[u8] {
        let h = &self.hits[i];
        &self.buf[h.start..h.start + h.len]
    }
    pub fn wkbs(&self) -> Vec<&[u8]> {
        (0..self.hits.len()).map(|i| self.wkb(i)).collect()
    }
}

pub struct BuildingStore {
    path: PathBuf,
    file: File,
    n: u64,
    keys: Vec<u32>,
    offs: Vec<u64>,
    lens: Vec<u64>,
    oversize_off: u64,
    oversize_len: u64,
}

impl BuildingStore {
    /// Speicher neben `buildings.parquet` oeffnen; fehlt er oder passt sein
    /// Kopf nicht zur Parquet-Datei, neu bauen.
    pub fn open_or_build(parquet: &Path) -> Result<Self> {
        let path = parquet.with_extension("bstore");
        let fp = fingerprint(parquet)?;
        if let Ok(s) = Self::open(&path) {
            if s.matches(fp) {
                return Ok(s);
            }
            info!(
                "bstore: {} passt nicht zu {} -- wird neu gebaut",
                path.display(),
                parquet.display()
            );
        }
        build(parquet, &path, fp)?;
        let s = Self::open(&path)?;
        if !s.matches(fp) {
            bail!("bstore: {} nach dem Bau ungueltig", path.display());
        }
        Ok(s)
    }

    fn matches(&self, fp: (u64, u64)) -> bool {
        self.read_header().is_some_and(|h| {
            h.parquet_size == fp.0 && h.parquet_mtime == fp.1 && h.bucket_m == BUCKET_M
        })
    }

    fn read_header(&self) -> Option<Header> {
        let mut b = [0u8; HEADER_LEN];
        self.file.read_exact_at(&mut b, 0).ok()?;
        Header::parse(&b)
    }

    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("{} oeffnen", path.display()))?;
        let mut hb = [0u8; HEADER_LEN];
        file.read_exact_at(&mut hb, 0)?;
        let h = Header::parse(&hb).context("bstore: kein gueltiger Kopf")?;
        let nk = h.n_keys as usize;
        let mut tb = vec![0u8; nk * (4 + 8 + 8)];
        file.read_exact_at(&mut tb, h.table_off)?;
        let mut keys = Vec::with_capacity(nk);
        let mut offs = Vec::with_capacity(nk);
        let mut lens = Vec::with_capacity(nk);
        let (kb, rest) = tb.split_at(nk * 4);
        let (ob, lb) = rest.split_at(nk * 8);
        for i in 0..nk {
            keys.push(u32::from_le_bytes(kb[4 * i..4 * i + 4].try_into().unwrap()));
            offs.push(u64::from_le_bytes(ob[8 * i..8 * i + 8].try_into().unwrap()));
            lens.push(u64::from_le_bytes(lb[8 * i..8 * i + 8].try_into().unwrap()));
        }
        if keys.windows(2).any(|w| w[0] >= w[1]) {
            bail!("bstore: Kacheltabelle nicht sortiert");
        }
        Ok(BuildingStore {
            path: path.to_path_buf(),
            file,
            n: h.n,
            keys,
            offs,
            lens,
            oversize_off: h.oversize_off,
            oversize_len: h.oversize_len,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Anzahl gespeicherter (opaker) Gebaeude.
    pub fn len(&self) -> usize {
        self.n as usize
    }

    fn read_range(&self, buf: &mut Vec<u8>, off: u64, len: u64) -> Result<usize> {
        let start = buf.len();
        buf.resize(start + len as usize, 0);
        self.file
            .read_exact_at(&mut buf[start..], off)
            .with_context(|| format!("bstore {}: lesen bei {off}+{len}", self.path.display()))?;
        Ok(start)
    }

    /// Alle Gebaeude, deren Bounding-Box `q` schneidet, aufsteigend nach
    /// Zeilenindex.
    pub fn query(&self, q: [f64; 4]) -> Result<Hits> {
        let mut out = Hits {
            buf: Vec::new(),
            hits: Vec::new(),
        };
        let mut ranges: Vec<(u64, u64)> = Vec::new();
        if q[0] <= q[2] && q[1] <= q[3] {
            let c0 = bucket_coord(q[0] - BUCKET_M);
            let c1 = bucket_coord(q[2]);
            let r0 = bucket_coord(q[1] - BUCKET_M);
            let r1 = bucket_coord(q[3]);
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if let Ok(i) = self.keys.binary_search(&zkey(c, r)) {
                        ranges.push((self.offs[i], self.lens[i]));
                    }
                }
            }
        }
        if self.oversize_len > 0 {
            ranges.push((self.oversize_off, self.oversize_len));
        }
        for (off, len) in ranges {
            let start = self.read_range(&mut out.buf, off, len)?;
            scan(&out.buf, start, start + len as usize, &q, &mut out.hits)?;
        }
        out.hits.sort_unstable_by_key(|h| h.idx);
        Ok(out)
    }
}

fn scan(
    buf: &[u8],
    mut pos: usize,
    end: usize,
    q: &[f64; 4],
    hits: &mut Vec<HitRef>,
) -> Result<()> {
    while pos < end {
        if pos + REC_HEAD > end {
            bail!("bstore: Datensatz ueber das Kachelende hinaus");
        }
        let idx = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
        let wlen = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let mut b = [0f64; 4];
        for (k, v) in b.iter_mut().enumerate() {
            let o = pos + 8 + 8 * k;
            *v = f64::from_le_bytes(buf[o..o + 8].try_into().unwrap());
        }
        let start = pos + REC_HEAD;
        if start + wlen > end {
            bail!("bstore: WKB ueber das Kachelende hinaus");
        }
        if intersects(&b, q) {
            hits.push(HitRef {
                idx,
                bounds: b,
                start,
                len: wlen,
            });
        }
        pos = start + wlen;
    }
    Ok(())
}

fn write_record(
    w: &mut impl Write,
    idx: u32,
    bounds: &[f64; 4],
    wkb: &[u8],
) -> std::io::Result<()> {
    w.write_all(&idx.to_le_bytes())?;
    w.write_all(&(wkb.len() as u32).to_le_bytes())?;
    for v in bounds {
        w.write_all(&v.to_le_bytes())?;
    }
    w.write_all(wkb)
}

/// Speicher aus `buildings.parquet` bauen (atomar ueber Geschwisterdatei).
fn build(parquet: &Path, path: &Path, fp: (u64, u64)) -> Result<()> {
    let t0 = Instant::now();
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = path.with_extension(format!("bstore.tmp{}", std::process::id()));
    let part_dir = path.with_extension(format!("bstore.parts{}", std::process::id()));
    std::fs::create_dir_all(&part_dir)?;
    let result = build_inner(parquet, &tmp, &part_dir, fp);
    let _ = std::fs::remove_dir_all(&part_dir);
    match result {
        Ok(n) => {
            std::fs::rename(&tmp, path)
                .with_context(|| format!("{} -> {}", tmp.display(), path.display()))?;
            let size = std::fs::metadata(path)?.len();
            info!(
                "bstore: {} Gebaeude -> {} ({:.1} GB, {:.0} s)",
                n,
                path.display(),
                size as f64 / 1e9,
                t0.elapsed().as_secs_f64()
            );
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e).with_context(|| format!("bstore in {} bauen", dir.display()))
        }
    }
}

fn build_inner(parquet: &Path, tmp: &Path, part_dir: &Path, fp: (u64, u64)) -> Result<u64> {
    // 1. Ein Durchgang: Datensaetze mit Kachelschluessel in 64 Teildateien
    //    (Spalte mod 8, Zeile mod 8) bzw. den Sonderbereich.
    let mut parts: Vec<BufWriter<File>> = (0..=N_PARTS)
        .map(|p| -> Result<_> {
            Ok(BufWriter::with_capacity(
                1 << 20,
                File::create(part_dir.join(format!("p{p:02}")))?,
            ))
        })
        .collect::<Result<_>>()?;
    let mut n = 0u64;
    let mut n_over = 0u64;
    crate::parquet_io::for_each_opaque_building(parquet, |idx, wkb| {
        let b = crate::wkb::bounds(wkb)?;
        let oversize = !(b[2] - b[0] <= BUCKET_M && b[3] - b[1] <= BUCKET_M);
        let (p, key) = if oversize {
            n_over += 1;
            (N_PARTS, 0u32)
        } else {
            let (c, r) = (bucket_coord(b[0]), bucket_coord(b[1]));
            (((c & 7) | ((r & 7) << 3)) as usize, zkey(c, r))
        };
        let w = &mut parts[p];
        w.write_all(&key.to_le_bytes())?;
        write_record(w, idx, &b, wkb)?;
        n += 1;
        Ok(())
    })?;
    for w in &mut parts {
        w.flush()?;
    }
    drop(parts);

    // 2. Je Teildatei: Datensaetze stabil nach Kachel sortieren (die
    //    Zeilenreihenfolge bleibt je Kachel erhalten) und anhaengen.
    let mut out = BufWriter::with_capacity(1 << 20, File::create(tmp)?);
    out.write_all(&[0u8; HEADER_LEN])?;
    let mut pos = HEADER_LEN as u64;
    let mut table: Vec<(u32, u64, u64)> = Vec::new();
    let (mut oversize_off, mut oversize_len) = (0u64, 0u64);
    for p in 0..=N_PARTS {
        let data = std::fs::read(part_dir.join(format!("p{p:02}")))?;
        let mut recs: Vec<(u32, usize, usize)> = Vec::new();
        let mut i = 0usize;
        while i < data.len() {
            let key = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
            let wlen = u32::from_le_bytes(data[i + 8..i + 12].try_into().unwrap()) as usize;
            let len = REC_HEAD + wlen;
            recs.push((key, i + 4, len));
            i += 4 + len;
        }
        if p == N_PARTS {
            oversize_off = pos;
            for (_, start, len) in &recs {
                out.write_all(&data[*start..*start + *len])?;
                pos += *len as u64;
            }
            oversize_len = pos - oversize_off;
            continue;
        }
        recs.sort_by_key(|r| r.0);
        for (key, start, len) in recs {
            match table.last_mut() {
                Some(last) if last.0 == key => last.2 += len as u64,
                _ => table.push((key, pos, len as u64)),
            }
            out.write_all(&data[start..start + len])?;
            pos += len as u64;
        }
    }
    table.sort_unstable_by_key(|t| t.0);
    let table_off = pos;
    for (k, _, _) in &table {
        out.write_all(&k.to_le_bytes())?;
    }
    for (_, o, _) in &table {
        out.write_all(&o.to_le_bytes())?;
    }
    for (_, _, l) in &table {
        out.write_all(&l.to_le_bytes())?;
    }
    out.flush()?;
    let mut f = out.into_inner().map_err(|e| e.into_error())?;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(
        &Header {
            parquet_size: fp.0,
            parquet_mtime: fp.1,
            n,
            n_keys: table.len() as u64,
            table_off,
            oversize_off,
            oversize_len,
            bucket_m: BUCKET_M,
        }
        .to_bytes(),
    )?;
    f.sync_all()?;
    info!(
        "bstore: {n} Gebaeude in {} Kacheln, {n_over} uebergross",
        table.len()
    );
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geoparquet::{BuildingOut, StreamWriter};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<u8> {
        let ring = vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        crate::wkb::write_multipolygon(&[vec![ring]])
    }

    /// Einfacher deterministischer Zufall (kein Crate noetig).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn query_matches_brute_force_and_row_order() {
        let d = std::env::temp_dir().join(format!("bubatz-bstore-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let parquet = d.join("buildings.parquet");
        let mut rng = Lcg(7);
        let mut rows: Vec<BuildingOut> = Vec::new();
        for i in 0..3000i64 {
            let (x, y) = (
                300_000.0 + rng.next() * 8000.0,
                5_600_000.0 + rng.next() * 8000.0,
            );
            let (w, h) = (2.0 + rng.next() * 60.0, 2.0 + rng.next() * 60.0);
            rows.push(BuildingOut {
                osm_id: i,
                is_opaque: i % 5 != 0,
                building_tag: "yes".into(),
                wkb: rect(x, y, w, h),
            });
        }
        for i in 0..25i64 {
            // uebergross (> 1 km), teils negativ gelegen
            let (x, y) = (
                -2000.0 + rng.next() * 12000.0,
                5_598_000.0 + rng.next() * 12000.0,
            );
            rows.push(BuildingOut {
                osm_id: 10_000 + i,
                is_opaque: true,
                building_tag: "industrial".into(),
                wkb: rect(
                    x,
                    y,
                    1200.0 + rng.next() * 3000.0,
                    400.0 + rng.next() * 2000.0,
                ),
            });
        }
        let mut w = StreamWriter::buildings(&parquet).unwrap();
        w.write_buildings(&rows[..1500]).unwrap();
        w.write_buildings(&rows[1500..]).unwrap();
        w.close().unwrap();

        let store = BuildingStore::open_or_build(&parquet).unwrap();
        let opaque: Vec<(u32, [f64; 4], &[u8])> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.is_opaque)
            .map(|(i, r)| {
                (
                    i as u32,
                    crate::wkb::bounds(&r.wkb).unwrap(),
                    r.wkb.as_slice(),
                )
            })
            .collect();
        assert_eq!(store.len(), opaque.len());
        for _ in 0..300 {
            let (x, y) = (
                298_000.0 + rng.next() * 12000.0,
                5_598_000.0 + rng.next() * 12000.0,
            );
            let (w, h) = (rng.next() * 2500.0, rng.next() * 2500.0);
            let q = [x, y, x + w, y + h];
            let hits = store.query(q).unwrap();
            let expect: Vec<(u32, [f64; 4], &[u8])> = opaque
                .iter()
                .filter(|(_, b, _)| intersects(b, &q))
                .cloned()
                .collect();
            assert_eq!(hits.len(), expect.len(), "Treffermenge fuer {q:?}");
            for (i, (idx, b, wkb)) in expect.iter().enumerate() {
                assert_eq!(hits.idx(i), *idx, "Reihenfolge = Zeilenindex");
                assert_eq!(hits.bounds(i), *b);
                assert_eq!(hits.wkb(i), *wkb);
            }
        }
        // Kanten: Beruehrung zaehlt (abgeschlossene Intervalle), leere Abfrage
        let b0 = opaque[0].1;
        let touch = store
            .query([b0[2], b0[3], b0[2] + 1.0, b0[3] + 1.0])
            .unwrap();
        assert!((0..touch.len()).any(|i| touch.idx(i) == opaque[0].0));
        assert!(store.query([0.0, 0.0, 1.0, 1.0]).unwrap().is_empty());

        // Wiederverwendung bei gleichem Kopf, Neubau bei geaenderter Parquet-Datei
        let mtime = std::fs::metadata(parquet.with_extension("bstore"))
            .unwrap()
            .modified()
            .unwrap();
        let again = BuildingStore::open_or_build(&parquet).unwrap();
        assert_eq!(again.len(), store.len());
        assert_eq!(
            std::fs::metadata(parquet.with_extension("bstore"))
                .unwrap()
                .modified()
                .unwrap(),
            mtime,
            "unveraendert wiederverwendet"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut w = StreamWriter::buildings(&parquet).unwrap();
        w.write_buildings(&rows[..10]).unwrap();
        w.close().unwrap();
        let rebuilt = BuildingStore::open_or_build(&parquet).unwrap();
        assert_eq!(
            rebuilt.len(),
            rows[..10].iter().filter(|r| r.is_opaque).count()
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn zkey_is_injective_on_grid_and_offset_handles_negatives() {
        assert_eq!(zkey(0, 0), 0);
        assert_eq!(zkey(1, 0), 1);
        assert_eq!(zkey(0, 1), 2);
        assert_eq!(zkey(65535, 65535), u32::MAX);
        assert_eq!(bucket_coord(-1.0), 32767);
        assert_eq!(bucket_coord(0.0), 32768);
        assert_eq!(bucket_coord(999.9), 32768);
        assert_eq!(bucket_coord(1000.0), 32769);
        assert_eq!(bucket_coord(f64::NAN), 0);
        assert_eq!(bucket_coord(1e12), 65535);
    }
}
