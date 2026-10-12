//! Der Rotationssweep:
//! exaktes Sichtbarkeitspolygon EINES Beobachterpunkts gegen die Kanten
//! einer [`Scene`]. Dieselben Formeln in derselben Reihenfolge wie die
//! numpy-Referenz, damit der Vergleich eng bleibt
//! (Abnahme: Hausdorff < 1e-6 m und symmetrische Differenz < 1e-6 m² je
//! Punktpolygon; Bitgleichheit wird protokolliert, nicht verlangt --
//! `atan2/sin/cos/hypot` kommen aus derselben glibc, das ist eine
//! Plattform-, keine Sprachgarantie).
//!
//! Ablauf je Punkt (`visibility_ring`):
//! 1. Kandidatenwinkel: jede Ecke +/- `EPS_ANGLE` plus `ARC_SEGMENTS`
//!    Bogenstuetzpunkte, sortiert, dedupliziert.
//! 2. Trefferdistanz je Winkel mit Verdeckungs-Kullung: Winkelbereich und
//!    Mindestabstand je Kante, Winkel-Eimer als CSR-Struktur, Kanten je Eimer
//!    nach Mindestabstand; sobald der Mindestabstand den besten Treffer
//!    uebersteigt (abzueglich `CULL_GUARD_*`), kann keine Kante mehr
//!    gewinnen. Ergebnis identisch zur Auswertung aller Paare
//!    (`reference.rs`, proptest).
//! 3. Ringpunkte `origin + min(d, reach) * (cos, sin)`.
//! 4. Kollineare Punkte ausduennen (Abstand zur Sehne der Nachbarn <=
//!    `COLLINEAR_TOL_M`).

// Die Vergleiche `lo <= a && a <= hi` bleiben absichtlich in der Form der
// Referenzimplementierung (Formelreihenfolge, siehe Modulkommentar).
#![allow(clippy::manual_range_contains)]

use std::f64::consts::PI;

use crate::geom::Ring;
use crate::scene::Scene;

pub const ARC_SEGMENTS: usize = 180;
pub const EPS_ANGLE: f64 = 1e-7;
pub const ANGLE_PAD: f64 = 1e-6;
pub const PARALLEL_TOL: f64 = 1e-9;
pub const COLLINEAR_TOL_M: f64 = 1e-9;
pub const N_BUCKETS: usize = 128;
pub const CULL_GUARD_ABS: f64 = 1e-9;
pub const CULL_GUARD_REL: f64 = 1e-12;

/// Bogenstuetzpunkte wie `_ARC_ANGLES` (numpy: `-pi + offset + 2*pi*arange(n)/n`).
pub fn arc_angles() -> Vec<f64> {
    let offset = f64::min(1e-4, 0.1 * (2.0 * PI / ARC_SEGMENTS as f64));
    (0..ARC_SEGMENTS)
        .map(|i| -PI + offset + 2.0 * PI * (i as f64) / ARC_SEGMENTS as f64)
        .collect()
}

/// Wiederverwendbare Puffer fuer viele Punkte gegen dieselbe Szene.
#[derive(Debug, Default)]
pub struct Workspace {
    arc: Vec<f64>,
    angles: Vec<f64>,
    best: Vec<f64>,
    cos_a: Vec<f64>,
    sin_a: Vec<f64>,
    lo: Vec<f64>,
    hi: Vec<f64>,
    dmin: Vec<f64>,
    alive: Vec<bool>,
    counts: Vec<usize>,
    fill: Vec<usize>,
    entries: Vec<usize>,
    order: Vec<usize>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    keep: Vec<bool>,
    /// atan2 jeder Ecke fuer `ang0_origin` (von `visibility_ring` gefuellt,
    /// von `hit_distances_culled` wiederverwendet: `p1[k] == p0[next[k]]`)
    ang0: Vec<f64>,
    ang0_origin: Option<[f64; 2]>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace {
            arc: arc_angles(),
            ..Default::default()
        }
    }

    /// Ergebnis des letzten `hit_distances_culled`.
    pub fn best(&self) -> &[f64] {
        &self.best
    }
}

/// Sortierte, deduplizierte Kandidatenwinkel (`_candidate_angles_nb` + `np.unique`).
pub fn candidate_angles(scene: &Scene, origin: [f64; 2], arc: &[f64], out: &mut Vec<f64>) {
    let mut ang0 = Vec::with_capacity(scene.n_segments());
    corner_angles(scene, origin, &mut ang0);
    candidate_angles_from(&ang0, arc, out);
}

/// atan2 jeder Ecke (= Startpunkt jeder Kante) vom Beobachter aus -- dieselbe
/// Rechnung wie `b0` in `hit_distances_culled`, dort wiederverwendet.
pub fn corner_angles(scene: &Scene, origin: [f64; 2], ang0: &mut Vec<f64>) {
    ang0.clear();
    let [ox, oy] = origin;
    ang0.extend(scene.corners().map(|c| (c[1] - oy).atan2(c[0] - ox)));
}

/// Kandidatenwinkel aus den Eckwinkeln: jede Ecke +/- `EPS_ANGLE` plus Bogen,
/// sortiert, dedupliziert.
pub fn candidate_angles_from(ang0: &[f64], arc: &[f64], out: &mut Vec<f64>) {
    out.clear();
    out.reserve(3 * ang0.len() + arc.len());
    for &b in ang0 {
        out.push(b);
        out.push(b + EPS_ANGLE);
        out.push(b - EPS_ANGLE);
    }
    out.extend_from_slice(arc);
    // Radix-Sortierung (radsort: dieselbe Ordnung wie `total_cmp`, also
    // dieselbe Folge wie `sort_unstable_by(total_cmp)`), ~3x schneller bei
    // ~5.000 Winkeln je Punkt.
    radsort::sort(out);
    out.dedup();
}

#[inline]
fn point_segment_distance(qx: f64, qy: f64, sx: f64, sy: f64) -> f64 {
    let ll = sx * sx + sy * sy;
    if ll == 0.0 {
        return qx.hypot(qy);
    }
    let u = -(qx * sx + qy * sy) / ll;
    if u <= 0.0 {
        return qx.hypot(qy);
    }
    if u >= 1.0 {
        return (qx + sx).hypot(qy + sy);
    }
    (qx + u * sx).hypot(qy + u * sy)
}

#[inline]
fn bucket_range(lo: f64, hi: f64, scale: f64) -> (usize, usize) {
    if lo < -PI || hi > PI {
        return (0, N_BUCKETS - 1);
    }
    let blo = ((lo.max(-PI) + PI) * scale).floor();
    let bhi = ((hi.min(PI) + PI) * scale).floor();
    let clamp = |b: f64| -> usize { (b.max(0.0) as usize).min(N_BUCKETS - 1) };
    (clamp(blo), clamp(bhi))
}

/// Python-`%` fuer positiven Divisor.
#[inline]
fn pymod(x: f64, m: f64) -> f64 {
    let r = x % m;
    if r < 0.0 { r + m } else { r }
}

/// Trefferdistanz je Winkel mit Verdeckungs-Kullung (`_hit_distances_culled_nb`).
/// `angles` sortiert aufsteigend in (-pi, pi]; Ergebnis in `ws.best`.
pub fn hit_distances_culled(
    scene: &Scene,
    origin: [f64; 2],
    angles: &[f64],
    max_len: f64,
    ws: &mut Workspace,
) {
    let n_ang = angles.len();
    let n_seg = scene.n_segments();
    ws.best.clear();
    ws.best.resize(n_ang, f64::INFINITY);
    ws.cos_a.clear();
    ws.sin_a.clear();
    ws.cos_a.extend(angles.iter().map(|a| a.cos()));
    ws.sin_a.extend(angles.iter().map(|a| a.sin()));
    if n_seg == 0 || n_ang == 0 {
        ws.best.iter_mut().for_each(|b| *b = max_len);
        return;
    }
    let [ox, oy] = origin;
    let two_pi = 2.0 * PI;

    ws.lo.clear();
    ws.lo.resize(n_seg, 0.0);
    ws.hi.clear();
    ws.hi.resize(n_seg, 0.0);
    ws.dmin.clear();
    ws.dmin.resize(n_seg, f64::INFINITY);
    ws.alive.clear();
    ws.alive.resize(n_seg, false);
    ws.counts.clear();
    ws.counts.resize(N_BUCKETS + 1, 0);
    let scale = N_BUCKETS as f64 / two_pi;
    let cached = ws.ang0_origin == Some(origin) && ws.ang0.len() == n_seg;

    for k in 0..n_seg {
        let q0x = scene.p0x[k] - ox;
        let q0y = scene.p0y[k] - oy;
        let q1x = scene.p1x[k] - ox;
        let q1y = scene.p1y[k] - oy;
        if f64::min(q0x.hypot(q0y), q1x.hypot(q1y)) - scene.seg_len[k] > max_len {
            continue;
        }
        let (b0, b1) = if cached {
            (ws.ang0[k], ws.ang0[scene.next[k]])
        } else {
            (q0y.atan2(q0x), q1y.atan2(q1x))
        };
        let mut delta = b1 - b0;
        delta = pymod(delta + PI, two_pi) - PI;
        let mut lo = f64::min(b0, b0 + delta) - ANGLE_PAD;
        let mut hi = f64::max(b0, b0 + delta) + ANGLE_PAD;
        if (delta.abs() - PI).abs() < 1e-6 {
            lo = -PI - 1.0;
            hi = PI + 1.0;
        }
        ws.lo[k] = lo;
        ws.hi[k] = hi;
        ws.dmin[k] = point_segment_distance(q0x, q0y, q1x - q0x, q1y - q0y);
        ws.alive[k] = true;
        let (blo, bhi) = bucket_range(lo, hi, scale);
        for bkt in blo..=bhi {
            ws.counts[bkt + 1] += 1;
        }
    }
    for bkt in 0..N_BUCKETS {
        ws.counts[bkt + 1] += ws.counts[bkt];
    }
    ws.entries.clear();
    ws.entries.resize(ws.counts[N_BUCKETS], 0);
    ws.fill.clear();
    ws.fill.extend_from_slice(&ws.counts[..N_BUCKETS]);
    // Kanten einmal nach Mindestabstand sortieren, dann in dieser Reihenfolge
    // in die Eimer fuellen -- jeder Eimer ist damit sortiert.
    ws.order.clear();
    ws.order.extend((0..n_seg).filter(|&k| ws.alive[k]));
    let dmin = &ws.dmin;
    ws.order
        .sort_unstable_by(|&a, &b| dmin[a].total_cmp(&dmin[b]));
    for &k in &ws.order {
        let (blo, bhi) = bucket_range(ws.lo[k], ws.hi[k], scale);
        for bkt in blo..=bhi {
            ws.entries[ws.fill[bkt]] = k;
            ws.fill[bkt] += 1;
        }
    }
    for (i, &ang) in angles.iter().enumerate() {
        let bkt = (((ang + PI) * scale).floor().max(0.0) as usize).min(N_BUCKETS - 1);
        let dx = ws.cos_a[i];
        let dy = ws.sin_a[i];
        let mut cur = f64::INFINITY;
        for e in ws.counts[bkt]..ws.counts[bkt + 1] {
            let k = ws.entries[e];
            let d = ws.dmin[k];
            if d - CULL_GUARD_ABS - d * CULL_GUARD_REL > cur {
                break;
            }
            let lo = ws.lo[k];
            let hi = ws.hi[k];
            let mut inside = lo <= ang && ang <= hi;
            if !inside && lo < -PI {
                inside = ang >= lo + two_pi;
            }
            if !inside && hi > PI {
                inside = ang <= hi - two_pi;
            }
            if !inside {
                continue;
            }
            let q0x = scene.p0x[k] - ox;
            let q0y = scene.p0y[k] - oy;
            let sx = scene.p1x[k] - scene.p0x[k];
            let sy = scene.p1y[k] - scene.p0y[k];
            let denom = dx * sy - dy * sx;
            let mut t_best = f64::INFINITY;
            if denom.abs() > PARALLEL_TOL {
                let t = (q0x * sy - q0y * sx) / denom;
                let s = (q0x * dy - q0y * dx) / denom;
                if t >= 0.0 && t <= max_len && s >= -1e-9 && s <= 1.0 + 1e-9 {
                    t_best = t;
                }
            } else {
                let q1x = scene.p1x[k] - ox;
                let q1y = scene.p1y[k] - oy;
                let perp0 = q0x * dy - q0y * dx;
                let perp1 = q1x * dy - q1y * dx;
                let t0 = q0x * dx + q0y * dy;
                let t1 = q1x * dx + q1y * dy;
                if perp0.abs() <= PARALLEL_TOL && t0 >= 0.0 && t0 <= max_len {
                    t_best = t_best.min(t0);
                }
                if perp1.abs() <= PARALLEL_TOL && t1 >= 0.0 && t1 <= max_len {
                    t_best = t_best.min(t1);
                }
            }
            if t_best < cur {
                cur = t_best;
            }
        }
        ws.best[i] = if cur.is_finite() { cur } else { max_len };
    }
}

/// Ringpunkte aus Winkeln und Distanzen (`_ring_coords_nb`).
pub fn ring_coords(
    origin: [f64; 2],
    angles: &[f64],
    distances: &[f64],
    reach: f64,
    xs: &mut Vec<f64>,
    ys: &mut Vec<f64>,
) {
    xs.clear();
    ys.clear();
    for (&a, &d0) in angles.iter().zip(distances) {
        let d = if d0 < reach { d0 } else { reach };
        xs.push(origin[0] + d * a.cos());
        ys.push(origin[1] + d * a.sin());
    }
}

/// Wie `ring_coords`, aber mit bereits berechneten `cos`/`sin` der Winkel
/// (aus `hit_distances_culled`) -- bitgleich, spart zwei Winkelfunktionen je Punkt.
pub fn ring_coords_cs(
    origin: [f64; 2],
    cos_a: &[f64],
    sin_a: &[f64],
    distances: &[f64],
    reach: f64,
    xs: &mut Vec<f64>,
    ys: &mut Vec<f64>,
) {
    xs.clear();
    ys.clear();
    for ((&c, &s), &d0) in cos_a.iter().zip(sin_a).zip(distances) {
        let d = if d0 < reach { d0 } else { reach };
        xs.push(origin[0] + d * c);
        ys.push(origin[1] + d * s);
    }
}

/// Kollineare Punkte markieren (`_collinear_keep_nb`, zyklisch): behalten,
/// wenn der Abstand zur Sehne Vorgaenger->Nachfolger > `tol`; bleiben
/// weniger als drei Punkte, werden alle behalten.
pub fn collinear_keep(xs: &[f64], ys: &[f64], tol: f64, keep: &mut Vec<bool>) {
    let n = xs.len();
    keep.clear();
    keep.resize(n, false);
    // 1. Aufeinanderfolgende (fast) gleiche Punkte zu EINEM zusammenfassen
    //    (zyklisch). Ohne diesen Schritt loeschte der Kollinearitaetstest
    //    einen ganzen Block von Treffern in Distanz 0 (Beobachter auf einer
    //    mit dem Hindernis geteilten Wand): jeder Punkt des Blocks war
    //    "kollinear" zu seinen gleichen Nachbarn, der Scheitel verschwand,
    //    und der Ring sprang als Sehne ueber den verdeckten Sektor --
    //    Flaeche hinter der Wand (Befund an osm_id 143704845).
    let mut idx: Vec<usize> = Vec::with_capacity(n);
    for i in 0..n {
        match idx.last() {
            Some(&p) if (xs[i] - xs[p]).hypot(ys[i] - ys[p]) <= tol => {}
            _ => idx.push(i),
        }
    }
    while idx.len() > 1 {
        let (f, l) = (idx[0], *idx.last().unwrap());
        if (xs[l] - xs[f]).hypot(ys[l] - ys[f]) <= tol {
            idx.pop();
        } else {
            break;
        }
    }
    if idx.len() < 3 {
        keep.iter_mut().for_each(|k| *k = true);
        return;
    }
    // 2. Kollinearitaet auf dem bereinigten Ring
    let m = idx.len();
    let mut kept = 0usize;
    for j in 0..m {
        let i = idx[j];
        let ip = idx[if j > 0 { j - 1 } else { m - 1 }];
        let ino = idx[if j < m - 1 { j + 1 } else { 0 }];
        let ax = xs[i] - xs[ip];
        let ay = ys[i] - ys[ip];
        let bx = xs[ino] - xs[i];
        let by = ys[ino] - ys[i];
        let cross = ax * by - ay * bx;
        let chord = (xs[ino] - xs[ip]).hypot(ys[ino] - ys[ip]);
        let deviation = if chord > 0.0 {
            cross.abs() / chord
        } else {
            0.0
        };
        keep[i] = deviation > tol;
        if keep[i] {
            kept += 1;
        }
    }
    if kept < 3 {
        for &i in &idx {
            keep[i] = true;
        }
    }
}

/// Sichtbarkeitsring von `origin` (offen, Reihenfolge nach Winkel) -- die
/// Koordinaten vor dem `Polygon(...)`/`buffer(0)`-Schritt der Python-Seite.
pub fn visibility_ring(
    scene: &Scene,
    origin: [f64; 2],
    drop_collinear: bool,
    ws: &mut Workspace,
) -> Ring {
    if ws.arc.is_empty() {
        ws.arc = arc_angles();
    }
    let arc = std::mem::take(&mut ws.arc);
    let mut angles = std::mem::take(&mut ws.angles);
    let t0 = std::time::Instant::now();
    let mut ang0 = std::mem::take(&mut ws.ang0);
    corner_angles(scene, origin, &mut ang0);
    candidate_angles_from(&ang0, &arc, &mut angles);
    ws.ang0 = ang0;
    ws.ang0_origin = Some(origin);
    crate::profile::add(&crate::profile::SWEEP_CAND_NS, t0);
    let max_len = scene.max_len();
    let t1 = std::time::Instant::now();
    hit_distances_culled(scene, origin, &angles, max_len, ws);
    crate::profile::add(&crate::profile::SWEEP_HIT_NS, t1);
    {
        use std::sync::atomic::Ordering::Relaxed;
        crate::profile::SWEEP_SEGS.fetch_add(scene.n_segments() as u64, Relaxed);
        crate::profile::SWEEP_ANGLES.fetch_add(angles.len() as u64, Relaxed);
    }
    let t2 = std::time::Instant::now();
    let mut xs = std::mem::take(&mut ws.xs);
    let mut ys = std::mem::take(&mut ws.ys);
    ring_coords_cs(
        origin,
        &ws.cos_a,
        &ws.sin_a,
        &ws.best,
        scene.max_len(),
        &mut xs,
        &mut ys,
    );
    let ring: Ring = if drop_collinear && xs.len() > 3 {
        collinear_keep(&xs, &ys, COLLINEAR_TOL_M, &mut ws.keep);
        xs.iter()
            .zip(&ys)
            .zip(&ws.keep)
            .filter(|&(_, &k)| k)
            .map(|((&x, &y), _)| [x, y])
            .collect()
    } else {
        xs.iter().zip(&ys).map(|(&x, &y)| [x, y]).collect()
    };
    crate::profile::add(&crate::profile::SWEEP_RING_NS, t2);
    ws.arc = arc;
    ws.angles = angles;
    ws.xs = xs;
    ws.ys = ys;
    ring
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::signed_area;
    use crate::reference::hit_distances_all_pairs;
    use proptest::prelude::*;

    fn square(x: f64, y: f64, s: f64) -> Ring {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn arc_angles_match_numpy_definition() {
        let arc = arc_angles();
        assert_eq!(arc.len(), 180);
        assert_eq!(arc[0], -PI + 1e-4);
        assert!(arc.windows(2).all(|w| w[0] < w[1]));
        assert!(*arc.last().unwrap() < PI);
    }

    #[test]
    fn empty_scene_gives_a_circle_polygon_of_reach() {
        let scene = Scene::from_rings::<Ring>(&[], 100.0);
        let mut ws = Workspace::new();
        let ring = visibility_ring(&scene, [10.0, 20.0], true, &mut ws);
        // 180 Bogenpunkte, alle bei Abstand reach -> Flaeche ~ pi r^2 (Sehnen innen).
        assert_eq!(ring.len(), 180);
        let a = signed_area(&ring);
        let exact = PI * 100.0 * 100.0;
        assert!(a > 0.0 && a < exact && a > 0.999 * exact, "{a}");
        for p in &ring {
            assert!(((p[0] - 10.0).hypot(p[1] - 20.0) - 100.0).abs() < 1e-9);
        }
    }

    #[test]
    fn wall_blocks_half_plane() {
        // Beobachter im Ursprung, lange duenne Wand bei x = 5: rechts davon nichts sichtbar.
        let wall = vec![[5.0, -1000.0], [5.1, -1000.0], [5.1, 1000.0], [5.0, 1000.0]];
        let scene = Scene::from_rings(&[wall], 100.0);
        let mut ws = Workspace::new();
        let ring = visibility_ring(&scene, [0.0, 0.0], true, &mut ws);
        assert!(
            ring.iter().all(|p| p[0] <= 5.0 + 1e-9),
            "Punkt hinter der Wand"
        );
        assert!(
            ring.iter().any(|p| p[0] < -99.0),
            "links frei bis zur Reichweite"
        );
        assert!(
            ring.iter().any(|p| (p[0] - 5.0).abs() < 1e-9),
            "Treffer auf der Wand"
        );
    }

    #[test]
    fn culled_matches_all_pairs_on_a_small_scene() {
        let scene = Scene::from_rings(
            &[
                square(3.0, 3.0, 2.0),
                square(-6.0, 1.0, 1.5),
                square(0.5, -7.0, 3.0),
            ],
            50.0,
        );
        let mut ws = Workspace::new();
        let origin = [0.1, -0.2];
        let arc = arc_angles();
        let mut angles = Vec::new();
        candidate_angles(&scene, origin, &arc, &mut angles);
        hit_distances_culled(&scene, origin, &angles, scene.max_len(), &mut ws);
        let reference = hit_distances_all_pairs(&scene, origin, &angles, scene.max_len());
        assert_eq!(ws.best, reference);
        assert!(
            reference.iter().any(|&d| d < 50.0),
            "mindestens ein Treffer"
        );
    }

    #[test]
    fn collinear_keep_removes_middle_of_straight_run_and_keeps_triangle() {
        let xs = [0.0, 1.0, 2.0, 2.0, 0.0];
        let ys = [0.0, 0.0, 0.0, 2.0, 2.0];
        let mut keep = Vec::new();
        collinear_keep(&xs, &ys, COLLINEAR_TOL_M, &mut keep);
        assert_eq!(keep, vec![true, false, true, true, true]);
        let mut keep = Vec::new();
        collinear_keep(
            &[0.0, 1.0, 2.0],
            &[0.0, 0.0, 0.0],
            COLLINEAR_TOL_M,
            &mut keep,
        );
        assert_eq!(
            keep,
            vec![true, true, true],
            "weniger als drei -> alle behalten"
        );
    }

    fn rect_or_triangle() -> impl Strategy<Value = Ring> {
        (
            -80.0f64..80.0,
            -80.0f64..80.0,
            0.5f64..25.0,
            0.5f64..25.0,
            prop::bool::ANY,
        )
            .prop_map(|(x, y, w, h, tri)| {
                if tri {
                    vec![[x, y], [x + w, y], [x + 0.3 * w, y + h]]
                } else {
                    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
                }
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 300, ..ProptestConfig::default() })]

        /// Kullung ist eine reine Beschleunigung: bitgleich zur Auswertung
        /// aller Paare, fuer Beobachter innerhalb und ausserhalb von Hindernissen.
        #[test]
        fn culled_equals_all_pairs(rings in prop::collection::vec(rect_or_triangle(), 1..50),
                                   ox in -90.0f64..90.0, oy in -90.0f64..90.0,
                                   reach in prop::sample::select(vec![20.0, 100.0, 252.0])) {
            let scene = Scene::from_rings(&rings, reach);
            let mut ws = Workspace::new();
            let arc = arc_angles();
            let mut angles = Vec::new();
            candidate_angles(&scene, [ox, oy], &arc, &mut angles);
            hit_distances_culled(&scene, [ox, oy], &angles, scene.max_len(), &mut ws);
            let reference = hit_distances_all_pairs(&scene, [ox, oy], &angles, scene.max_len());
            prop_assert_eq!(&ws.best, &reference);
        }

        /// Der Ring liegt nie ausserhalb der Reichweite und hat mindestens
        /// drei Punkte.
        #[test]
        fn ring_is_within_reach(rings in prop::collection::vec(rect_or_triangle(), 0..20),
                                ox in -90.0f64..90.0, oy in -90.0f64..90.0) {
            let scene = Scene::from_rings(&rings, 60.0);
            let mut ws = Workspace::new();
            let ring = visibility_ring(&scene, [ox, oy], true, &mut ws);
            prop_assert!(ring.len() >= 3);
            for p in &ring {
                prop_assert!((p[0] - ox).hypot(p[1] - oy) <= 60.0 + 1e-9);
            }
        }
    }
}
