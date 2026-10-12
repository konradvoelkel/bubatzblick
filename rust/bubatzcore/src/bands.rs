//! Ringzerlegung einer Zone:
//! `D(source, r_k) \ D(source, r_{k-1})` geschnitten mit `v_hat`, fuer
//! aufsteigende Radien. Der Kreis-Puffer `D(source, r)` ist der Versatz aus
//! [`crate::finish::offset_shapes`] mit dem feinsten Sehnenschritt, den
//! i_overlay zulaesst: `LineJoin::Round` klemmt den Schritt auf `0,01 pi`.
//! Diese Klemmung ist die **Schranke** -- je Segment hoechstens 0,01 pi, also
//! bei 250 m Radius hoechstens 3,1 cm Sehnenhoehe. Herausgezaehlt werden
//! **216 Segmente je Vollkreis** (i_overlay quantisiert den Schritt danach
//! binaer und zaehlt die Schlussluecke mit), gemessener relativer
//! Flaechenfehler 1,5e-4. Shapelys `quad_segs = 128` (512 Ecken, 1,9 mm) ist
//! damit nicht erreichbar; wer es braucht, muss den Bogen selbst
//! konstruieren.
//!
//! Unterschied zu GEOS, absichtlich: GEOS `buffer(r)` vereinfacht die
//! Eingabe um 1 % von `r` (bei 250 m: 2,5 m, nur konkave Ecken) und ist an
//! einspringenden Ecken der Quelle bis 2,5 m GROESSER als die exakte
//! Minkowski-Summe; i_overlay versetzt exakt. Dasselbe gilt fuer den
//! Freikreis (`free_circle`), der die Zone nach aussen begrenzt.

use i_overlay::core::overlay_rule::OverlayRule;

use crate::finish::{offset_shapes, round_step_for_quad_segs};
use crate::geom::{Shape, Shapes, orient_shapes};
use crate::union::{overlay_pair, to_ogc};

/// Sehnenschritt der Kreis-Puffer: `quad_segs = 128` angefordert, von
/// i_overlay auf `0,01 pi` geklemmt; heraus kommen 216 Segmente je Vollkreis.
pub fn circle_round_step() -> f64 {
    round_step_for_quad_segs(128)
}

/// Relativer Flaechenfehler des Freikreises, **gemessen**: 1,487e-4.
///
/// Das Modell ist `1 - (n / 2 pi) sin(2 pi / n)`, der Flaechenfehler eines
/// regelmaessigen n-Ecks. Es passt hier nur ungefaehr: i_overlay klemmt den
/// angeforderten Schritt auf `0,01 pi`, quantisiert ihn binaer und zaehlt die
/// Schlussluecke zum Endpunkt mit, die Ecken sitzen also nicht gleichmaessig.
/// Gezaehlt werden 216 Ecken; das gemessene Defizit entspricht einem
/// gleichmaessigen 210-Eck. Deshalb steht hier der gemessene Wert und nicht
/// der aus der Formel -- wer die Zahl prueft, vergleicht mit dem Test
/// `free_circle_of_a_point_like_square_has_disc_area`.
///
/// Das Polygon ist dem Kreis **einbeschrieben**: der Freikreis ist kleiner
/// als die exakte Scheibe, nie groesser.
pub const CIRCLE_AREA_DEFICIT_REL: f64 = 1.487e-4;

/// `D(source, r)`: die Quelle um `r` versetzt (216 Segmente je Vollkreis).
pub fn free_circle(source: &[Shape], r: f64) -> Shapes {
    to_ogc(&offset_shapes(source, r, circle_round_step()))
}

fn overlay(a: &[Shape], b: &[Shape], rule: OverlayRule) -> Shapes {
    if a.is_empty() {
        return Vec::new();
    }
    if b.is_empty() {
        return match rule {
            OverlayRule::Difference => a.to_vec(),
            _ => Vec::new(),
        };
    }
    overlay_pair(a, b, rule)
}

/// Baender zu `radii` (werden aufsteigend sortiert; Rueckgabe in dieser
/// Reihenfolge). Jeder Puffer wird genau einmal gerechnet: der aeussere von
/// Band k ist der innere von Band k+1.
pub fn decompose_into_bands(v_hat: &[Shape], source: &[Shape], radii: &[f64]) -> Vec<Shapes> {
    let v_hat = orient_shapes(v_hat);
    let source = orient_shapes(source);
    let mut radii: Vec<f64> = radii.to_vec();
    radii.sort_by(f64::total_cmp);
    let mut out = Vec::with_capacity(radii.len());
    let mut prev: Option<Shapes> = None;
    for r in radii {
        let outer = free_circle(&source, r);
        let ring = match &prev {
            None => outer.clone(),
            Some(p) => overlay(&outer, p, OverlayRule::Difference),
        };
        out.push(to_ogc(&overlay(&v_hat, &ring, OverlayRule::Intersect)));
        prev = Some(outer);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Ring, shapes_area};
    use std::f64::consts::PI;

    fn square(x: f64, y: f64, s: f64) -> Ring {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn circle_step_is_128_segments_per_quarter() {
        let step = circle_round_step();
        assert!((step - 2.0 * (PI / 512.0).sin()).abs() < 1e-15);
    }

    #[test]
    fn free_circle_of_a_point_like_square_has_disc_area() {
        // 0,2-m-Quadrat um 100 m: Flaeche ~ pi 100^2 (+ 4*0,2*100 + 0,04),
        // Sehnen innen. i_overlay klemmt den Schritt auf 0,01 pi, quantisiert
        // ihn binaer und zaehlt die Schlussluecke mit -> 216 Ecken.
        let src = vec![vec![square(-0.1, -0.1, 0.2)]];
        let out = free_circle(&src, 100.0);
        let a = shapes_area(&out);
        let exact = PI * 1e4 + 80.0 + 0.04;
        let deficit = (exact - a) / (PI * 1e4);
        // Einbeschrieben: der Freikreis ist kleiner als die exakte Scheibe.
        // Diese Richtung ist die Zusage, nicht die genaue Eckenzahl.
        assert!(a < exact, "{a} vs {exact}");
        assert!(
            (deficit - CIRCLE_AREA_DEFICIT_REL).abs() < 1e-5,
            "Defizit {deficit:.3e}"
        );
        let n = out[0][0].len();
        assert!((210..=225).contains(&n), "~216 Ecken erwartet, {n}");
    }

    #[test]
    fn bands_partition_v_hat_within_the_outermost_radius() {
        let source = vec![vec![square(-5.0, -5.0, 10.0)]];
        // v_hat: grosses Quadrat mit Loch (ausserhalb der ersten beiden
        // Baender), ragt ueber 60 m hinaus.
        let mut hole = square(40.0, 40.0, 10.0);
        hole.reverse();
        let v_hat = vec![vec![square(-80.0, -80.0, 160.0), hole]];
        let radii = [25.0, 50.0, 60.0];
        let bands = decompose_into_bands(&v_hat, &source, &radii);
        assert_eq!(bands.len(), 3);
        let total: f64 = bands.iter().map(|b| shapes_area(b)).sum();
        let clipped = overlay(&v_hat, &free_circle(&source, 60.0), OverlayRule::Intersect);
        assert!(
            (total - shapes_area(&clipped)).abs() < 1e-6,
            "{total} vs {}",
            shapes_area(&clipped)
        );
        // Paarweise disjunkt.
        for i in 0..3 {
            for j in (i + 1)..3 {
                let x = overlay(&bands[i], &bands[j], OverlayRule::Intersect);
                assert!(shapes_area(&x) < 1e-9, "Baender {i}/{j} ueberlappen");
            }
        }
        // Erstes Band = Quelle-Puffer 25 (ganz in v_hat): 100 + 4*10*25 + pi 25^2
        let b0 = shapes_area(&bands[0]);
        let exact0 = 100.0 + 1000.0 + PI * 625.0;
        let tol = 2.0 * CIRCLE_AREA_DEFICIT_REL * PI * 625.0;
        assert!(b0 < exact0 && b0 > exact0 - tol, "{b0} vs {exact0}");
    }

    #[test]
    fn empty_v_hat_gives_empty_bands() {
        let source = vec![vec![square(0.0, 0.0, 1.0)]];
        let bands = decompose_into_bands(&[], &source, &[10.0, 20.0]);
        assert_eq!(bands.len(), 2);
        assert!(bands.iter().all(|b| b.is_empty()));
    }
}
