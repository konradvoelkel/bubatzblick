//! Vereinigung vieler Polygone mit `i_overlay` (Integer-Sweep-Line, robust)
//! statt GEOS. Gemessen beim Umstieg:
//!
//! - `i64`-Kern, nicht `i32`: mit `i32` entstanden an fast deckungsgleichen
//!   Wandkanten benachbarter Sichtbarkeitspolygone Snap-Rounding-Zickzacks
//!   (8 Polygone -> 12.586 Ecken statt 438; 658 Polygone -> OOM).
//! - kaskadiert, nicht als Einmal-Overlay: 658 einander ueberlappende
//!   Konturen in EINEM Overlay brauchten 165 s, paarweise entlang der
//!   Kontur (raeumliche Nachbarschaft) und dann Paare der Ergebnisse
//!   Bruchteile einer Sekunde -- das ist, was GEOS' CascadedPolygonUnion tut.
//!
//! Eingabe/Ausgabe: `Shape` = Aussenring zuerst, dann Loecher, Ringe offen,
//! konsistent orientiert (NonZero-Fuellung). Die Ausgabe von `i_overlay`
//! ist gueltig und konsistent orientiert.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::core::solver::Solver;
use i_overlay::float::overlay::{FloatOverlay, OverlayOptions};
use i_overlay::float::simplify::SimplifyShape;

use crate::geom::{Shape, Shapes};

/// Vereinigung aller Eingabepolygone in EINEM Overlay (NonZero-Fuellung ueber
/// alle Konturen). Fuer wenige, wenig ueberlappende Polygone; sonst
/// [`union_cascade`]. `use_i64 = false` waehlt den `i32`-Kern (nur noch zum
/// Vergleich; siehe Modulkommentar).
/// Overlay-Optionen des Kerns: die Voreinstellung (NonZero-Extraktion).
/// **Nicht `ogc()`:** in i_overlay 8.1.1 fuellt die OGC-Extraktion in einer
/// Kaskade (Zwischenergebnisse mit Loechern als Operanden) umschlossene
/// Loecher -- an 515 Duesseldorfer Gebaeuden 87.111 statt 73.111 m²,
/// reihenfolgeabhaengig (`tests/ogc_repro.rs`). OGC-Gueltigkeit
/// stellt stattdessen [`to_ogc`] als EIN Schlusspass auf dem fertigen
/// Ergebnis her; das ist am selben Datensatz exakt.
pub fn options() -> OverlayOptions<f64, i64> {
    OverlayOptions::<f64, i64>::default()
}

/// Schlusspass: ein fertiges, konsistent gefuelltes Ergebnis in OGC-gueltige
/// Shapes ueberfuehren (Loecher, die den Aussenring beruehren, werden
/// getrennt; sich beruehrende Ringe aufgeteilt), damit Shapely es ohne
/// `make_valid`/`buffer(0)` nimmt und ein anderer Leser dieselben
/// Bytes sieht. Nur fuer Endergebnisse, nie fuer Kaskaden-Operanden.
pub fn to_ogc(shapes: &[Shape]) -> Shapes {
    if shapes.is_empty() {
        return Vec::new();
    }
    shapes.to_vec().simplify_shape_custom_as::<i64>(
        FillRule::NonZero,
        OverlayOptions::<f64, i64>::ogc(),
        Solver::AUTO,
    )
}

pub fn union_all(shapes: &[Shape], use_i64: bool) -> Shapes {
    if use_i64 {
        let mut overlay =
            FloatOverlay::<[f64; 2], i64>::from_subj_custom(shapes, options(), Solver::AUTO);
        overlay.overlay(OverlayRule::Subject, FillRule::NonZero)
    } else {
        let mut overlay = FloatOverlay::with_subj(shapes);
        overlay.overlay(OverlayRule::Subject, FillRule::NonZero)
    }
}

/// Zwei-Operanden-Overlay (`i64`, OGC-Ausgabe) -- der eine Aufruf, ueber den
/// Union, Schnitt und Differenz im Kern laufen.
pub fn overlay_pair(a: &[Shape], b: &[Shape], rule: OverlayRule) -> Shapes {
    let mut overlay =
        FloatOverlay::<[f64; 2], i64>::from_subj_and_clip_custom(a, b, options(), Solver::AUTO);
    overlay.overlay(rule, FillRule::NonZero)
}

fn union_pair(a: &[Shape], b: &[Shape]) -> Shapes {
    overlay_pair(a, b, OverlayRule::Union)
}

/// Kaskadierte Vereinigung: paarweise entlang der Eingabereihenfolge
/// (= Konturreihenfolge, also raeumliche Nachbarschaft), dann Paare der
/// Ergebnisse, bis eines uebrig ist. Jeder Schritt ist ein Zwei-Operanden-
/// Overlay (`i64`) auf bereits zusammengefassten Formen.
pub fn union_cascade(polygons: Vec<Shape>) -> Shapes {
    let mut level: Vec<Shapes> = polygons.into_iter().map(|shape| vec![shape]).collect();
    while level.len() > 1 {
        let mut next: Vec<Shapes> = Vec::with_capacity(level.len().div_ceil(2));
        let mut it = level.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(union_pair(&a, &b)),
                None => next.push(a),
            }
        }
        level = next;
    }
    level.pop().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Ring, is_ccw, shapes_area};
    use proptest::prelude::*;

    fn square(x: f64, y: f64, s: f64) -> Shape {
        vec![vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]]
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert!(union_cascade(vec![]).is_empty());
        assert!(union_all(&[], true).is_empty());
    }

    #[test]
    fn single_polygon_passes_through() {
        let out = union_cascade(vec![square(0.0, 0.0, 2.0)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 1);
        assert_eq!(out[0][0].len(), 4);
        assert!((shapes_area(&out) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn overlapping_squares_merge() {
        // 2x2 und 2x2, Ueberlappung 1x1 -> Flaeche 7, ein Shape ohne Loch.
        let out = union_cascade(vec![square(0.0, 0.0, 2.0), square(1.0, 1.0, 2.0)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 1);
        assert!((shapes_area(&out) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn disjoint_squares_stay_apart() {
        let out = union_cascade(vec![square(0.0, 0.0, 1.0), square(5.0, 5.0, 1.0)]);
        assert_eq!(out.len(), 2);
        assert!((shapes_area(&out) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn ring_of_squares_produces_a_hole_with_opposite_orientation() {
        // Acht 2x2-Quadrate um ein leeres 2x2-Feld in der Mitte.
        let mut polys = Vec::new();
        for (i, j) in [
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (2, 1),
            (0, 2),
            (1, 2),
            (2, 2),
        ] {
            polys.push(square(2.0 * i as f64, 2.0 * j as f64, 2.0));
        }
        let out = union_cascade(polys);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 2, "Aussenring plus ein Loch");
        assert!((shapes_area(&out) - 32.0).abs() < 1e-9);
        assert_ne!(is_ccw(&out[0][0]), is_ccw(&out[0][1]));
    }

    #[test]
    fn cascade_result_has_consistent_orientation() {
        let out = union_cascade(vec![square(0.0, 0.0, 2.0), square(1.0, 1.0, 2.0)]);
        let outer_ccw = is_ccw(&out[0][0]);
        for shape in &out {
            assert_eq!(is_ccw(&shape[0]), outer_ccw);
            for hole in &shape[1..] {
                assert_ne!(is_ccw(hole), outer_ccw);
            }
        }
    }

    fn rect_strategy() -> impl Strategy<Value = Shape> {
        (0.0f64..100.0, 0.0f64..100.0, 0.5f64..30.0, 0.5f64..30.0)
            .prop_map(|(x, y, w, h)| vec![vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]])
    }

    fn area_of_union_by_inclusion_exclusion_is_hard(rects: &[Shape]) -> f64 {
        // Brute-Force-Referenz: Flaeche der Vereinigung achsenparalleler
        // Rechtecke per Rasterung ueber die Kantenkoordinaten (exakt).
        let mut xs: Vec<f64> = rects
            .iter()
            .flat_map(|r| [r[0][0][0], r[0][2][0]])
            .collect();
        let mut ys: Vec<f64> = rects
            .iter()
            .flat_map(|r| [r[0][0][1], r[0][2][1]])
            .collect();
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        ys.sort_by(f64::total_cmp);
        ys.dedup();
        let mut area = 0.0;
        for xw in xs.windows(2) {
            for yw in ys.windows(2) {
                let (cx, cy) = (0.5 * (xw[0] + xw[1]), 0.5 * (yw[0] + yw[1]));
                let covered = rects.iter().any(|r| {
                    let (x0, y0) = (r[0][0][0], r[0][0][1]);
                    let (x1, y1) = (r[0][2][0], r[0][2][1]);
                    x0 < cx && cx < x1 && y0 < cy && cy < y1
                });
                if covered {
                    area += (xw[1] - xw[0]) * (yw[1] - yw[0]);
                }
            }
        }
        area
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

        /// Kaskade und Einmal-Overlay meinen dieselbe Flaeche wie die
        /// Brute-Force-Referenz (bis auf die 1-mm-aehnliche Rundung des
        /// Integer-Gitters, hier grosszuegig 1e-6 relativ).
        #[test]
        fn cascade_matches_reference_area(rects in prop::collection::vec(rect_strategy(), 1..40)) {
            let reference = area_of_union_by_inclusion_exclusion_is_hard(&rects);
            let cascade = shapes_area(&union_cascade(rects.clone()));
            let once = shapes_area(&union_all(&rects, true));
            prop_assert!((cascade - reference).abs() <= 1e-6 * reference.max(1.0), "cascade {cascade} vs {reference}");
            prop_assert!((once - reference).abs() <= 1e-6 * reference.max(1.0), "union_all {once} vs {reference}");
        }

        /// Orientierung der Ausgabe ist konsistent: alle Aussenringe gleich,
        /// Loecher entgegengesetzt.
        #[test]
        fn cascade_output_is_consistently_oriented(rects in prop::collection::vec(rect_strategy(), 1..40)) {
            let out = union_cascade(rects);
            prop_assert!(!out.is_empty());
            let outer_ccw = is_ccw(&out[0][0]);
            for shape in &out {
                prop_assert!(is_ccw(&shape[0]) == outer_ccw);
                for hole in &shape[1..] {
                    prop_assert!(is_ccw(hole) != outer_ccw);
                }
            }
        }
    }

    #[allow(dead_code)]
    fn _ring_type_is_public(_: Ring) {}
}
