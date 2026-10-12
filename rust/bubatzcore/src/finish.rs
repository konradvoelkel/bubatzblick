//! Abschluss einer Sichtzone: Versatz um die
//! Sicherheitsmarge, Vereinigung mit der Quelle, Schnitt mit dem Freikreis.
//! Ersetzt `region.buffer(marge)`, `union_all([region, source])` und
//! `.intersection(free)` (GEOS, zusammen ~19 % der Objektzeit im
//! Bundeslauf 11) durch drei i_overlay-Schritte auf denselben Shapes.
//!
//! **Versatz-Semantik von `i_overlay::mesh::float::outline` (aus der Quelle
//! gelesen, Version 9.0.0):** jeder Ring wird einzeln versetzt, Ringe mit
//! positiver (CCW) Flaeche mit `outer_offset`, Ringe mit negativer (CW)
//! Flaeche mit `inner_offset`; danach werden alle Teile mit
//! `FillRule::Positive` vereinigt. Ein GEOS-`buffer(+m)` (Aussenring
//! waechst, Loecher schrumpfen) entspricht also `outer_offset = inner_offset
//! = m` -- NICHT `inner_offset = 0`, wie der Plan vermutete (das liesse
//! Loecher unveraendert). Voraussetzung: Aussenringe CCW, Loecher CW; siehe
//! [`crate::geom::orient_shapes`].
//!
//! Rundung: `LineJoin::Round(r)` mit `r = L / R` (maximale Sehnenlaenge zu
//! Radius). Shapelys `quad_segs = 16` entspricht Winkelschritt pi/32, also
//! `r = 2 sin(pi/64) ~ 0,0982`; i_overlay klemmt auf `[0,01 pi, 0,25 pi]` und
//! quantisiert den Schritt danach binaer (`ArcOptions::max_step`), wobei die
//! Schlussluecke zum Endpunkt mitzaehlt -- die tatsaechliche Eckenzahl liegt
//! deshalb etwas ueber `2 pi / r`.

use std::f64::consts::PI;

use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};

use crate::geom::{Shape, Shapes, orient_shapes};
use crate::union::{options, overlay_pair, to_ogc};

/// `L / R` fuer 16 Segmente je Viertelkreis (Shapely `quad_segs = 16`).
pub const ROUND_STEP_QUAD_SEGS_16: f64 = 0.098_135_348_654_836_03; // 2 sin(pi/64)

/// `L / R` fuer `quad_segs` Segmente je Viertelkreis.
pub fn round_step_for_quad_segs(quad_segs: u32) -> f64 {
    2.0 * (PI / (4.0 * quad_segs as f64)).sin()
}

/// Versatz aller Shapes um `margin` nach aussen (Aussenringe wachsen, Loecher
/// schrumpfen), runde Ecken mit Sehnenschritt `round_step` (= L / R).
/// `margin <= 0` liefert die Eingabe unveraendert (orientiert).
pub fn offset_shapes(shapes: &[Shape], margin: f64, round_step: f64) -> Shapes {
    let shapes = orient_shapes(shapes);
    if margin <= 0.0 || shapes.is_empty() {
        return shapes;
    }
    let style = OutlineStyle::new(margin).line_join(LineJoin::Round(round_step));
    shapes.outline_custom_as::<i64>(&style, options())
}

fn overlay(a: &[Shape], b: &[Shape], rule: OverlayRule) -> Shapes {
    overlay_pair(a, b, rule)
}

/// Abschluss: `(region (+) margin) U source` geschnitten mit `free`.
/// Alle Eingaben sind Shape-Listen (Aussenring zuerst, dann Loecher); die
/// Orientierung wird normalisiert. Ausgabe: gueltige, konsistent
/// orientierte Shapes (Aussen CCW, Loecher CW).
pub fn finish_region(
    region: &[Shape],
    source: &[Shape],
    free: &[Shape],
    margin: f64,
    round_step: f64,
) -> Shapes {
    let widened = offset_shapes(region, margin, round_step);
    let source = orient_shapes(source);
    let free = orient_shapes(free);
    let with_source = if source.is_empty() {
        widened
    } else if widened.is_empty() {
        source
    } else {
        overlay(&widened, &source, OverlayRule::Union)
    };
    if with_source.is_empty() {
        return with_source;
    }
    to_ogc(&overlay(&with_source, &free, OverlayRule::Intersect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Ring, is_ccw, shapes_area};

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
    fn round_step_matches_quad_segs_16() {
        assert!((round_step_for_quad_segs(16) - ROUND_STEP_QUAD_SEGS_16).abs() < 1e-15);
    }

    #[test]
    fn offset_of_square_has_area_of_rounded_rectangle() {
        // 10x10 um 1 versetzt: 100 + 4*10*1 + pi*1^2 (Rundungen etwas kleiner,
        // Sehnen liegen innen).
        let out = offset_shapes(
            &[vec![square(0.0, 0.0, 10.0)]],
            1.0,
            ROUND_STEP_QUAD_SEGS_16,
        );
        assert_eq!(out.len(), 1);
        let a = shapes_area(&out);
        let exact = 100.0 + 40.0 + PI;
        assert!(a < exact && a > exact - 0.02, "{a} vs {exact}");
        assert!(is_ccw(&out[0][0]));
    }

    #[test]
    fn zero_margin_is_identity_up_to_orientation() {
        let mut cw = square(0.0, 0.0, 3.0);
        cw.reverse();
        let out = offset_shapes(&[vec![cw]], 0.0, ROUND_STEP_QUAD_SEGS_16);
        assert_eq!(out.len(), 1);
        assert!(is_ccw(&out[0][0]));
        assert_eq!(shapes_area(&out), 9.0);
    }

    #[test]
    fn offset_grows_exterior_and_shrinks_holes_like_a_positive_buffer() {
        // Ring R=10, r=4, um m=1: Flaeche pi((R+m)^2 - (r-m)^2) = pi(121 - 9).
        let outer = circle(0.0, 0.0, 10.0, 720);
        let mut hole = circle(0.0, 0.0, 4.0, 720);
        hole.reverse();
        let out = offset_shapes(&[vec![outer, hole]], 1.0, round_step_for_quad_segs(64));
        assert_eq!(out.len(), 1, "ein Shape");
        assert_eq!(out[0].len(), 2, "mit genau einem Loch");
        let a = shapes_area(&out);
        let exact = PI * (121.0 - 9.0);
        assert!((a - exact).abs() < 0.05 * exact.sqrt(), "{a} vs {exact}");
        assert!(is_ccw(&out[0][0]));
        assert!(!is_ccw(&out[0][1]));
        // Das Loch ist geschrumpft, nicht gewachsen oder gleich geblieben.
        let hole_area = crate::geom::signed_area(&out[0][1]).abs();
        assert!(
            (hole_area - 9.0 * PI).abs() < 0.1,
            "Lochflaeche {hole_area}"
        );
    }

    #[test]
    fn offset_keeps_a_four_millimetre_nub_that_geos_would_flatten() {
        // GEOS vereinfacht die Puffer-EINGABE um 1 % der Pufferdistanz (bei
        // 0,5 m: 5 mm) und entfernt dabei nur konkave Ecken -- sein Ergebnis
        // ist an feinen Kerben/Nasen bis zu 5 mm GROESSER als die exakte
        // Minkowski-Summe. i_overlay vereinfacht nicht: die 4-mm-Nase bleibt
        // und die Oberkante liegt exakt bei 10 + 0,004 + 0,5.
        let h = 0.004;
        let nub: Shape = vec![vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [6.0, 10.0],
            [6.0, 10.0 + h],
            [4.0, 10.0 + h],
            [4.0, 10.0],
            [0.0, 10.0],
        ]];
        let out = offset_shapes(&[nub], 0.5, ROUND_STEP_QUAD_SEGS_16);
        let max_y = out
            .iter()
            .flatten()
            .flatten()
            .map(|p| p[1])
            .fold(f64::MIN, f64::max);
        assert!((max_y - (10.5 + h)).abs() < 1e-9, "{max_y}");
        // Flaeche: exakt 100 + 2h + 0,5 (40 + 2h) + pi/4, minus Sehnenfehler
        // von 540 Grad Boegen (96 Segmente): 0,125 (r - sin r) 96 ~ 0,002.
        let exact = 100.0 + 2.0 * h + 0.5 * (40.0 + 2.0 * h) + PI / 4.0;
        let a = shapes_area(&out);
        assert!(a < exact && a > exact - 0.01, "{a} vs {exact}");
    }

    #[test]
    fn offset_closes_gaps_narrower_than_twice_the_margin() {
        // Zwei Quadrate mit 1 m Luecke, Marge 0,6 -> ein Shape.
        let out = offset_shapes(
            &[vec![square(0.0, 0.0, 5.0)], vec![square(6.0, 0.0, 5.0)]],
            0.6,
            ROUND_STEP_QUAD_SEGS_16,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn finish_region_adds_source_and_clips_to_free() {
        // Region: Quadrat 10x10 bei (0,0); Quelle: Quadrat 2x2 bei (20,20)
        // (disjunkt); Freikreis: Quadrat 0..15 -> Quelle faellt weg, Region
        // (versetzt um 1) wird bei 15 abgeschnitten.
        let region = vec![vec![square(0.0, 0.0, 10.0)]];
        let source = vec![vec![square(20.0, 20.0, 2.0)]];
        let free = vec![vec![square(0.0, 0.0, 15.0)]];
        let out = finish_region(&region, &source, &free, 1.0, ROUND_STEP_QUAD_SEGS_16);
        assert_eq!(out.len(), 1);
        let a = shapes_area(&out);
        // (-1..11)^2 mit runden Ecken, geschnitten mit (0..15)^2: 11x11,
        // die Ecke bei (11,11) bleibt rund (Defizit 1 - pi/4), Sehnen innen.
        let exact = 121.0 - (1.0 - PI / 4.0);
        assert!(a < exact + 1e-9 && a > exact - 0.01, "{a} vs {exact}");
    }

    #[test]
    fn finish_region_keeps_source_inside_free() {
        let region = vec![vec![square(0.0, 0.0, 4.0)]];
        let source = vec![vec![square(10.0, 10.0, 2.0)]];
        let free = vec![vec![square(-5.0, -5.0, 30.0)]];
        let out = finish_region(&region, &source, &free, 0.0, ROUND_STEP_QUAD_SEGS_16);
        assert_eq!(out.len(), 2);
        assert!((shapes_area(&out) - 20.0).abs() < 1e-6);
    }

    #[test]
    fn finish_region_with_empty_region_is_source_within_free() {
        let source = vec![vec![square(0.0, 0.0, 2.0)]];
        let free = vec![vec![square(1.0, 1.0, 10.0)]];
        let out = finish_region(&[], &source, &free, 0.5, ROUND_STEP_QUAD_SEGS_16);
        assert_eq!(out.len(), 1);
        assert!((shapes_area(&out) - 1.0).abs() < 1e-6);
    }
}
