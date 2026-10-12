//! Sichtzone eines Objekts in EINEM Aufruf:
//! Szene einmal, alle Beobachterpunkte, Ringe direkt in die Kaskade,
//! optional der Abschluss (Marge, Quelle, Freikreis).
//!
//! **Kein `buffer(0)` mehr:** ein Sweep-Ring geht ohne Shapely-Umweg in
//! `i_overlay` (NonZero). Ein sich selbst beruehrender oder in seltenen
//! Rundungsfaellen sich kreuzender Ring (Bow-Tie) behaelt dabei BEIDE Lappen
//! -- Shapelys `buffer(0)` hatte einen davon verloren.
//!
//! **Speicher:** die Kaskade laeuft streamend ueber Konturabschnitte von
//! `chunk` Punkten (Default 64): jeder Abschnitt wird sofort vereinigt, die
//! Abschnittsergebnisse danach paarweise. Spitze O(chunk + n/chunk) Ringe
//! statt aller n auf einmal (groesstes Objekt national: 26.369 Punkte).

use crate::finish::finish_region;
use crate::geom::{Shape, Shapes};
use crate::scene::Scene;
use crate::sweep::{Workspace, visibility_ring};
use crate::union::union_cascade;

pub const DEFAULT_CHUNK: usize = 64;

fn cascade_shapes(parts: Vec<Shapes>) -> Shapes {
    // Kaskade ueber bereits vereinigte Teil-Ergebnisse: jedes Teil ist eine
    // Shape-Liste; `union_cascade` erwartet je Eingabe EIN Shape, also die
    // Teile flach in Shapes zerlegen -- benachbarte Teile bleiben benachbart.
    let flat: Vec<Shape> = parts.into_iter().flatten().collect();
    union_cascade(flat)
}

/// Vereinigung der Sichtbarkeitsringe aller `points` gegen `scene`
/// (= `visible_region_from_points` in Python). Leere Punktliste -> leer.
pub fn visible_region_points(scene: &Scene, points: &[[f64; 2]], chunk: usize) -> Shapes {
    if points.is_empty() {
        return Vec::new();
    }
    let chunk = chunk.max(1);
    let mut ws = Workspace::new();
    let mut parts: Vec<Shapes> = Vec::with_capacity(points.len().div_ceil(chunk));
    for block in points.chunks(chunk) {
        let t0 = std::time::Instant::now();
        let rings: Vec<Shape> = block
            .iter()
            .map(|&p| vec![visibility_ring(scene, p, true, &mut ws)])
            .filter(|s| s[0].len() >= 3)
            .collect();
        crate::profile::add(&crate::profile::SWEEP_NS, t0);
        crate::profile::SWEEP_POINTS
            .fetch_add(block.len() as u64, std::sync::atomic::Ordering::Relaxed);
        if !rings.is_empty() {
            let t1 = std::time::Instant::now();
            parts.push(union_cascade(rings));
            crate::profile::add(&crate::profile::CASCADE_NS, t1);
        }
    }
    let t2 = std::time::Instant::now();
    let out = match parts.len() {
        0 => Vec::new(),
        1 => parts.pop().unwrap_or_default(),
        _ => cascade_shapes(parts),
    };
    crate::profile::add(&crate::profile::CASCADE_NS, t2);
    out
}

/// Die ganze Zone: Sweep aller Punkte, Vereinigung, Abschluss
/// (`(region (+) margin) U source`, geschnitten mit `free`).
/// `margin <= 0` heisst ohne Marge. Ohne Kanten in der Szene ist das
/// Ergebnis `free` (wie `visible_region_sampled_safe` bei leeren
/// Hindernissen -- der Aufrufer darf diesen Fall auch selbst abkuerzen).
#[allow(clippy::too_many_arguments)]
pub fn visible_region(
    obstacle_rings: &[Vec<[f64; 2]>],
    reach: f64,
    max_len: f64,
    points: &[[f64; 2]],
    source: &[Shape],
    free: &[Shape],
    margin: f64,
    round_step: f64,
    chunk: usize,
) -> Shapes {
    let t0 = std::time::Instant::now();
    let scene = Scene::from_ring_vecs(obstacle_rings, reach).with_max_len(max_len);
    crate::profile::add(&crate::profile::SCENE_NS, t0);
    if scene.n_segments() == 0 {
        return crate::geom::orient_shapes(free);
    }
    let region = visible_region_points(&scene, points, chunk);
    let t1 = std::time::Instant::now();
    let out = finish_region(&region, source, free, margin, round_step);
    crate::profile::add(&crate::profile::FINISH_NS, t1);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finish::ROUND_STEP_QUAD_SEGS_16;
    use crate::geom::{Ring, shapes_area};
    use std::f64::consts::PI;

    fn square(x: f64, y: f64, s: f64) -> Ring {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    fn circle(cx: f64, cy: f64, r: f64, n: usize) -> Ring {
        (0..n)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / n as f64;
                [cx + r * a.cos(), cy + r * a.sin()]
            })
            .collect()
    }

    #[test]
    fn no_points_is_empty() {
        let scene = Scene::from_rings(&[square(5.0, 5.0, 1.0)], 50.0);
        assert!(visible_region_points(&scene, &[], 64).is_empty());
    }

    #[test]
    fn chunking_does_not_change_the_union() {
        let scene = Scene::from_rings(
            &[
                square(5.0, 5.0, 2.0),
                square(-8.0, 2.0, 3.0),
                square(1.0, -9.0, 4.0),
            ],
            30.0,
        );
        let points: Vec<[f64; 2]> = (0..40).map(|i| [0.1 * i as f64, 0.05 * i as f64]).collect();
        let a = visible_region_points(&scene, &points, 64);
        let b = visible_region_points(&scene, &points, 7);
        let c = visible_region_points(&scene, &points, 1);
        let (aa, ab, ac) = (shapes_area(&a), shapes_area(&b), shapes_area(&c));
        assert!((aa - ab).abs() < 1e-6 * aa, "{aa} vs {ab}");
        assert!((aa - ac).abs() < 1e-6 * aa, "{aa} vs {ac}");
        assert!(
            aa > 0.5 * PI * 30.0 * 30.0,
            "grosser Teil des Kreises sichtbar"
        );
    }

    #[test]
    fn region_of_points_inside_a_wall_ring_is_the_inner_disc() {
        // Beobachter im Inneren eines geschlossenen Kreisrings (Wand) sehen
        // genau die Innenscheibe; die Vereinigung bleibt darin.
        let wall_outer = circle(0.0, 0.0, 12.0, 90);
        let mut wall_inner = circle(0.0, 0.0, 10.0, 90);
        wall_inner.reverse();
        let scene = Scene::from_rings(&[wall_outer, wall_inner], 100.0);
        let points: Vec<[f64; 2]> = (0..16)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / 16.0;
                [3.0 * a.cos(), 3.0 * a.sin()]
            })
            .collect();
        let out = visible_region_points(&scene, &points, 64);
        let a = shapes_area(&out);
        let disc = PI * 100.0;
        assert!(a < disc + 1e-6 && a > 0.99 * disc, "{a} vs {disc}");
        for p in out.iter().flatten().flatten() {
            assert!(p[0].hypot(p[1]) <= 10.0 + 1e-9);
        }
    }

    #[test]
    fn full_region_includes_source_and_stays_in_free() {
        let obstacles = vec![square(20.0, -5.0, 10.0)];
        let source = vec![vec![square(-1.0, -1.0, 2.0)]];
        let free = vec![vec![circle(0.0, 0.0, 50.0, 512)]];
        let points: Vec<[f64; 2]> = (0..8)
            .map(|i| {
                let t = i as f64 / 8.0 * 8.0;
                // Kontur des Quadrats (-1..1)^2 abschreiten
                if t < 2.0 {
                    [-1.0 + t, -1.0]
                } else if t < 4.0 {
                    [1.0, -1.0 + (t - 2.0)]
                } else if t < 6.0 {
                    [1.0 - (t - 4.0), 1.0]
                } else {
                    [-1.0, 1.0 - (t - 6.0)]
                }
            })
            .collect();
        let out = visible_region(
            &obstacles,
            50.0,
            50.0,
            &points,
            &source,
            &free,
            0.5,
            ROUND_STEP_QUAD_SEGS_16,
            64,
        );
        let a = shapes_area(&out);
        let disc = PI * 2500.0;
        assert!(
            a < disc && a > 0.8 * disc,
            "{a} vs {disc}: Schatten hinter dem Quadrat"
        );
        for p in out.iter().flatten().flatten() {
            assert!(p[0].hypot(p[1]) <= 50.0 + 1e-6);
        }
        // Ohne Hindernisse: der Freikreis selbst.
        let free_only = visible_region(
            &[],
            50.0,
            50.0,
            &points,
            &source,
            &free,
            0.5,
            ROUND_STEP_QUAD_SEGS_16,
            64,
        );
        assert!((shapes_area(&free_only) - shapes_area(&free)).abs() < 1e-9);
    }
}
