//! Brute-Force-Referenz fuer den Sweep (Port von
//! `sampling_ref._ray_hit_distances_batch`): JEDES (Winkel, Kante)-Paar wird
//! ausgewertet, ohne Winkelbereich und ohne Kullung. Nur fuer Tests: der
//! schnelle Kern muss bitgleich dasselbe liefern.

#![allow(clippy::manual_range_contains)]

use crate::scene::Scene;
use crate::sweep::PARALLEL_TOL;

pub fn hit_distances_all_pairs(
    scene: &Scene,
    origin: [f64; 2],
    angles: &[f64],
    max_len: f64,
) -> Vec<f64> {
    let [ox, oy] = origin;
    let mut best = vec![f64::INFINITY; angles.len()];
    if scene.n_segments() == 0 {
        return vec![max_len; angles.len()];
    }
    for (i, &ang) in angles.iter().enumerate() {
        let dx = ang.cos();
        let dy = ang.sin();
        for k in 0..scene.n_segments() {
            let sx = scene.p1x[k] - scene.p0x[k];
            let sy = scene.p1y[k] - scene.p0y[k];
            let qx = scene.p0x[k] - ox;
            let qy = scene.p0y[k] - oy;
            let denom = dx * sy - dy * sx;
            let parallel = denom.abs() <= PARALLEL_TOL;
            let mut t_pair = f64::INFINITY;
            if !parallel {
                let t = (qx * sy - qy * sx) / denom;
                let s = (qx * dy - qy * dx) / denom;
                if t.is_finite() && t >= 0.0 && t <= max_len && s >= -1e-9 && s <= 1.0 + 1e-9 {
                    t_pair = t;
                }
            }
            let q1x = scene.p1x[k] - ox;
            let q1y = scene.p1y[k] - oy;
            let perp0 = qx * dy - qy * dx;
            let perp1 = q1x * dy - q1y * dx;
            let t0 = qx * dx + qy * dy;
            let t1 = q1x * dx + q1y * dy;
            if parallel && perp0.abs() <= PARALLEL_TOL && t0 >= 0.0 && t0 <= max_len {
                t_pair = t_pair.min(t0);
            }
            if parallel && perp1.abs() <= PARALLEL_TOL && t1 >= 0.0 && t1 <= max_len {
                t_pair = t_pair.min(t1);
            }
            if t_pair < best[i] {
                best[i] = t_pair;
            }
        }
    }
    best.iter()
        .map(|&b| if b.is_finite() { b } else { max_len })
        .collect()
}
