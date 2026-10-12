//! Grundtypen und elementare Masse.
//!
//! Konvention im ganzen Kern: ein `Ring` ist offen (der Schlusspunkt wird
//! NICHT wiederholt), ein `Shape` ist Aussenring zuerst, dann Loecher.
//! `i_overlay` fuellt NonZero; Ringe muessen deshalb konsistent orientiert
//! sein (Aussenringe gleich, Loecher entgegengesetzt).

pub type Point = [f64; 2];
pub type Ring = Vec<Point>;
pub type Shape = Vec<Ring>;
pub type Shapes = Vec<Shape>;

/// Vorzeichenbehaftete Flaeche nach der Schnuerformel: positiv bei
/// Gegenuhrzeigersinn (CCW), negativ bei Uhrzeigersinn.
pub fn signed_area(ring: &[Point]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let mut twice = 0.0;
    for i in 0..n {
        let [x0, y0] = ring[i];
        let [x1, y1] = ring[(i + 1) % n];
        twice += x0 * y1 - x1 * y0;
    }
    0.5 * twice
}

pub fn is_ccw(ring: &[Point]) -> bool {
    signed_area(ring) > 0.0
}

/// Flaeche eines Shapes: |Aussenring| minus Summe |Loecher|, unabhaengig
/// von der Orientierung.
pub fn shape_area(shape: &[Ring]) -> f64 {
    let mut it = shape.iter();
    let outer = it.next().map_or(0.0, |r| signed_area(r).abs());
    outer - it.map(|r| signed_area(r).abs()).sum::<f64>()
}

/// Summe der Shape-Flaechen.
pub fn shapes_area(shapes: &[Shape]) -> f64 {
    shapes.iter().map(|s| shape_area(s)).sum()
}

/// Aussenring CCW, Loecher CW -- die Konvention von `i_overlay` (NonZero)
/// und die Voraussetzung des Versatzes in [`crate::finish`]. Leere oder
/// entartete Ringe bleiben, wie sie sind.
pub fn orient_shape(shape: &[Ring]) -> Shape {
    shape
        .iter()
        .enumerate()
        .map(|(i, ring)| {
            let want_ccw = i == 0;
            if ring.len() >= 3 && is_ccw(ring) != want_ccw && signed_area(ring) != 0.0 {
                ring.iter().rev().copied().collect()
            } else {
                ring.clone()
            }
        })
        .collect()
}

pub fn orient_shapes(shapes: &[Shape]) -> Shapes {
    shapes.iter().map(|s| orient_shape(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Ring {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn signed_area_sign_follows_orientation() {
        let ccw = square(0.0, 0.0, 2.0);
        let mut cw = ccw.clone();
        cw.reverse();
        assert_eq!(signed_area(&ccw), 4.0);
        assert_eq!(signed_area(&cw), -4.0);
        assert!(is_ccw(&ccw));
        assert!(!is_ccw(&cw));
    }

    #[test]
    fn degenerate_rings_have_zero_area() {
        assert_eq!(signed_area(&[]), 0.0);
        assert_eq!(signed_area(&[[0.0, 0.0], [1.0, 1.0]]), 0.0);
        assert_eq!(signed_area(&[[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]), 0.0);
    }

    #[test]
    fn shape_area_subtracts_holes_regardless_of_orientation() {
        let outer = square(0.0, 0.0, 10.0);
        let mut hole = square(1.0, 1.0, 2.0);
        assert_eq!(shape_area(&[outer.clone(), hole.clone()]), 96.0);
        hole.reverse();
        assert_eq!(shape_area(&[outer, hole]), 96.0);
        assert_eq!(shape_area(&[]), 0.0);
    }

    #[test]
    fn orient_shape_makes_outer_ccw_and_holes_cw() {
        let mut outer = square(0.0, 0.0, 10.0);
        outer.reverse();
        let hole = square(1.0, 1.0, 2.0);
        let s = orient_shape(&[outer, hole]);
        assert!(is_ccw(&s[0]));
        assert!(!is_ccw(&s[1]));
        assert_eq!(shape_area(&s), 96.0);
        assert_eq!(orient_shape(&s), s, "idempotent");
    }
}
