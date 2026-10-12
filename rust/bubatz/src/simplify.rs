//! Einseitige Vereinfachung -- der Vertrag:
//!
//! `geom ⊆ Ergebnis ⊆ geom (+) OUTWARD_BOUND_FACTOR * tol`
//!
//! Schritte wie in Python: Douglas-Peucker je Ring (statt GEOS
//! `TopologyPreservingSimplifier`), Enthaltenseins-Pruefung; sonst Versatz der
//! Vereinfachung um `tol` (einmal wiederholt mit `2 tol`); sonst Versatz des
//! Originals um `tol`. Rundung wie Python: `quad_segs = 8` unter
//! `COARSE_TOLERANCE_M`, sonst 2. Enthaltensein = Flaeche von
//! `geom \ Kandidat` ist (bis auf Rundung) null.

use bubatzcore::finish::round_step_for_quad_segs;
use bubatzcore::geom::{Shape, Shapes, shapes_area};
use bubatzcore::union::{overlay_pair, to_ogc};
use i_overlay::core::overlay_rule::OverlayRule;

pub const COARSE_TOLERANCE_M: f64 = 25.0;
const CONTAINS_EPS_M2: f64 = 1e-6;

fn perp_dist2(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let vx = b[0] - a[0];
    let vy = b[1] - a[1];
    let wx = p[0] - a[0];
    let wy = p[1] - a[1];
    let ll = vx * vx + vy * vy;
    if ll == 0.0 {
        return wx * wx + wy * wy;
    }
    let c = (wx * vy - wy * vx).abs();
    c * c / ll
}

fn dp(points: &[[f64; 2]], tol2: f64, keep: &mut [bool], lo: usize, hi: usize) {
    if hi <= lo + 1 {
        return;
    }
    let (a, b) = (points[lo], points[hi]);
    let mut best = 0.0;
    let mut idx = lo;
    for (i, p) in points.iter().enumerate().take(hi).skip(lo + 1) {
        let d = perp_dist2(*p, a, b);
        if d > best {
            best = d;
            idx = i;
        }
    }
    if best > tol2 {
        keep[idx] = true;
        dp(points, tol2, keep, lo, idx);
        dp(points, tol2, keep, idx, hi);
    }
}

/// Douglas-Peucker fuer einen geschlossenen Ring (offen gespeichert): der
/// Ring wird an seinem entferntesten Punkt vom Startpunkt geteilt.
pub fn simplify_ring(ring: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    let n = ring.len();
    if n <= 4 || tol <= 0.0 {
        return ring.to_vec();
    }
    let tol2 = tol * tol;
    let mut pts: Vec<[f64; 2]> = ring.to_vec();
    pts.push(ring[0]);
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[n] = true;
    // Ankerpunkt: entferntester Punkt vom Start
    let far = (1..n)
        .max_by(|&i, &j| {
            let di = (pts[i][0] - pts[0][0]).powi(2) + (pts[i][1] - pts[0][1]).powi(2);
            let dj = (pts[j][0] - pts[0][0]).powi(2) + (pts[j][1] - pts[0][1]).powi(2);
            di.partial_cmp(&dj).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(1);
    keep[far] = true;
    dp(&pts, tol2, &mut keep, 0, far);
    dp(&pts, tol2, &mut keep, far, n);
    let out: Vec<[f64; 2]> = (0..n).filter(|&i| keep[i]).map(|i| pts[i]).collect();
    if out.len() < 3 { ring.to_vec() } else { out }
}

pub fn simplify_shapes(shapes: &[Shape], tol: f64) -> Shapes {
    shapes
        .iter()
        .map(|s| s.iter().map(|r| simplify_ring(r, tol)).collect::<Shape>())
        .filter(|s: &Shape| s.first().is_some_and(|r| r.len() >= 3))
        .collect()
}

/// `geom ⊆ candidate` bis auf Rundung.
pub fn contains(candidate: &[Shape], geom: &[Shape]) -> bool {
    if candidate.is_empty() {
        return geom.is_empty();
    }
    let outside = overlay_pair(geom, candidate, OverlayRule::Difference);
    shapes_area(&outside) <= CONTAINS_EPS_M2
}

pub fn one_sided_simplify(geom: &[Shape], tol: f64) -> Shapes {
    if tol <= 0.0 || geom.is_empty() {
        return to_ogc(geom);
    }
    let quad_segs = if tol >= COARSE_TOLERANCE_M { 2 } else { 8 };
    let step = round_step_for_quad_segs(quad_segs);
    // Selbstueberschneidungen der DP-Ringe auf NonZero-Flaeche bringen
    let simplified = to_ogc(&simplify_shapes(geom, tol));
    if !simplified.is_empty() && contains(&simplified, geom) {
        return simplified;
    }
    let mut margin = tol;
    for _ in 0..2 {
        let candidate = bubatzcore::offset_shapes(&simplified, margin, step);
        if contains(&candidate, geom) {
            return to_ogc(&candidate);
        }
        margin *= 2.0;
    }
    to_ogc(&bubatzcore::offset_shapes(geom, tol, step))
}

/// `merge_bands`: Vereinigung mehrerer Shape-Listen (Baender eines Objekts).
pub fn union_many(parts: &[Shapes]) -> Shapes {
    let flat: Vec<Shape> = parts.iter().flatten().cloned().collect();
    if flat.is_empty() {
        return Vec::new();
    }
    to_ogc(&bubatzcore::union_cascade(flat))
}

/// `subtract_buildings` fuer EIN Merkmal: Differenz mit der Vereinigung der
/// schneidenden Gebaeude; leer, wenn nichts bleibt.
pub fn subtract(geom: &[Shape], buildings: &[Shape]) -> Shapes {
    if buildings.is_empty() {
        return geom.to_vec();
    }
    let out = overlay_pair(geom, buildings, OverlayRule::Difference);
    if shapes_area(&out) <= 0.0 {
        Vec::new()
    } else {
        to_ogc(&out)
    }
}

#[cfg(test)]
mod tests {
    /// Die Schranke aus dem Moduldoc: das Ergebnis liegt hoechstens
    /// `3 * tol` weiter aussen als das Original.
    const OUTWARD_BOUND_FACTOR: f64 = 3.0;

    use super::*;
    use std::f64::consts::PI;

    fn circle(cx: f64, cy: f64, r: f64, n: usize) -> Shape {
        vec![
            (0..n)
                .map(|i| {
                    let a = 2.0 * PI * i as f64 / n as f64;
                    [cx + r * a.cos(), cy + r * a.sin()]
                })
                .collect(),
        ]
    }

    fn max_outside_distance(out: &[Shape], geom: &[Shape], bound: f64) -> f64 {
        // Flaeche ausserhalb geom (+) bound
        let dil = bubatzcore::offset_shapes(geom, bound, round_step_for_quad_segs(64));
        shapes_area(&overlay_pair(out, &dil, OverlayRule::Difference))
    }

    #[test]
    fn always_contains_original_and_stays_within_bound() {
        let star: Shape = vec![
            (0..40)
                .map(|i| {
                    let a = 2.0 * PI * i as f64 / 40.0;
                    let r = if i % 2 == 0 { 100.0 } else { 20.0 };
                    [r * a.cos(), r * a.sin()]
                })
                .collect(),
        ];
        for tol in [0.5, 8.0, 25.0, 80.0, 200.0] {
            for geom in [vec![star.clone()], vec![circle(0.0, 0.0, 25.0, 128)]] {
                let out = one_sided_simplify(&geom, tol);
                assert!(!out.is_empty());
                assert!(contains(&out, &geom), "tol {tol}: enthaelt Original nicht");
                let outside = max_outside_distance(&out, &geom, OUTWARD_BOUND_FACTOR * tol);
                assert!(
                    outside < 1e-6,
                    "tol {tol}: {outside} m2 jenseits der Schranke"
                );
            }
        }
    }

    #[test]
    fn simple_square_is_not_grossly_overcorrected() {
        let sq: Shapes = vec![vec![vec![
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 100.0],
            [0.0, 100.0],
        ]]];
        let out = one_sided_simplify(&sq, 8.0);
        assert!(contains(&out, &sq));
        assert!(
            shapes_area(&out) <= 10_000.0 * 1.05,
            "{}",
            shapes_area(&out)
        );
    }

    #[test]
    fn dp_reduces_a_dense_circle() {
        let c = circle(0.0, 0.0, 100.0, 720);
        let s = simplify_ring(&c[0], 1.0);
        assert!(s.len() < 100 && s.len() >= 8, "{}", s.len());
    }

    #[test]
    fn subtract_removes_building_interior_only() {
        let zone: Shapes = vec![vec![vec![
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 100.0],
            [0.0, 100.0],
        ]]];
        let house: Shapes = vec![vec![vec![
            [40.0, 40.0],
            [60.0, 40.0],
            [60.0, 60.0],
            [40.0, 60.0],
        ]]];
        let out = subtract(&zone, &house);
        assert!((shapes_area(&out) - 9600.0).abs() < 1e-6);
        assert!(subtract(&house, &zone).is_empty());
    }
}
