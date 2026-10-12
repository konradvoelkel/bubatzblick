//! Beobachterpunkte entlang der Quellkontur -- bitgleicher Port von
//! `visibility.sample_contour` / `contour_spacing` einschliesslich der GEOS-
//! Arithmetik dahinter (3.13.1): `LineString::getLength` =
//! `Length::ofLine` (Summe `sqrt(dx*dx + dy*dy)` in Kantenreihenfolge),
//! `interpolate(d)` = `LengthIndexedLine::extractPoint` ->
//! `LengthLocationMap::getLocationForward` (erste Kante mit
//! `totalLength + segLen > d`, `frac = (d - totalLength) / segLen`) ->
//! `LinearLocation::pointAlongSegmentByFraction` (`(p1 - p0) * frac + p0`;
//! `frac <= 0` -> p0, `frac >= 1` -> p1). Python: `round()` rundet zur
//! naechsten geraden Zahl.

use bubatzcore::geom::Shape;

/// `LineString.length` eines geschlossenen Rings (offen gespeichert).
pub fn ring_length(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    if n <= 1 {
        return 0.0;
    }
    let mut len = 0.0;
    let (mut x0, mut y0) = (ring[0][0], ring[0][1]);
    for i in 1..=n {
        let p = ring[i % n];
        let dx = p[0] - x0;
        let dy = p[1] - y0;
        len += (dx * dx + dy * dy).sqrt();
        x0 = p[0];
        y0 = p[1];
    }
    len
}

/// `ring.interpolate(length)` (GEOS `LengthIndexedLine::extractPoint`).
pub fn interpolate(ring: &[[f64; 2]], length: f64) -> [f64; 2] {
    let n = ring.len();
    if length <= 0.0 || n == 0 {
        return ring[0];
    }
    let mut total = 0.0;
    for i in 0..n {
        let p0 = ring[i];
        let p1 = ring[(i + 1) % n];
        let dx = p1[0] - p0[0];
        let dy = p1[1] - p0[1];
        let seg = (dx * dx + dy * dy).sqrt();
        if total + seg > length {
            let frac = (length - total) / seg;
            if frac <= 0.0 {
                return p0;
            }
            if frac >= 1.0 {
                return p1;
            }
            return [
                (p1[0] - p0[0]) * frac + p0[0],
                (p1[1] - p0[1]) * frac + p0[1],
            ];
        }
        total += seg;
    }
    // Laenge >= Ringlaenge: Endpunkt (= Startpunkt des geschlossenen Rings)
    ring[0]
}

/// Python `round()` (round half to even) auf f64.
fn py_round(x: f64) -> f64 {
    x.round_ties_even()
}

/// `sample_contour(source, spacing_m, min_points, max_points)`: Punkte in
/// festem Abstand entlang aller Aussenringe.
pub fn sample_contour(
    source: &[Shape],
    spacing_m: f64,
    min_points: usize,
    max_points: usize,
) -> Vec<[f64; 2]> {
    let rings: Vec<&Vec<[f64; 2]>> = source.iter().filter_map(|s| s.first()).collect();
    let lengths: Vec<f64> = rings.iter().map(|r| ring_length(r)).collect();
    let total: f64 = lengths.iter().fold(0.0, |acc, &l| acc + l);
    if total <= 0.0 {
        return vec![crate::grid::centroid(source)];
    }
    let n_total =
        ((total / spacing_m).ceil().max(min_points as f64)).min(max_points as f64) as i64 as f64;
    let mut points = Vec::new();
    for (ring, &len) in rings.iter().zip(&lengths) {
        let n = py_round(n_total * len / total).max(3.0) as i64;
        for i in 0..n {
            points.push(interpolate(ring, len * i as f64 / n as f64));
        }
    }
    points
}

/// `contour_spacing(source, n_points)`.
pub fn contour_spacing(source: &[Shape], n_points: usize) -> f64 {
    let total: f64 = source
        .iter()
        .filter_map(|s| s.first())
        .map(|r| ring_length(r))
        .fold(0.0, |a, l| a + l);
    if n_points > 0 {
        total / n_points as f64
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_is_sampled_evenly_from_the_first_vertex() {
        let sq: Shape = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]];
        assert_eq!(ring_length(&sq[0]), 40.0);
        let pts = sample_contour(std::slice::from_ref(&sq), 5.0, 3, 1000);
        assert_eq!(pts.len(), 8);
        assert_eq!(pts[0], [0.0, 0.0]);
        assert_eq!(pts[1], [5.0, 0.0]);
        assert_eq!(pts[3], [10.0, 5.0]);
        assert_eq!(pts[7], [0.0, 5.0]);
        // Deckel: min 32 -> 32 Punkte, max 4 -> 4 (aber mindestens 3 je Ring)
        assert_eq!(
            sample_contour(std::slice::from_ref(&sq), 5.0, 32, 1000).len(),
            32
        );
        assert_eq!(
            sample_contour(std::slice::from_ref(&sq), 5.0, 1, 4).len(),
            4
        );
        assert!((contour_spacing(&[sq], 8) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn multipolygon_distributes_points_by_length_with_bankers_rounding() {
        let a: Shape = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]]; // 40
        let b: Shape = vec![vec![[50.0, 0.0], [52.0, 0.0], [52.0, 2.0], [50.0, 2.0]]]; // 8
        let pts = sample_contour(&[a, b], 1.0, 32, 1000); // n_total = 48 -> 40 + 8
        assert_eq!(pts.len(), 48);
        assert_eq!(py_round(2.5), 2.0);
        assert_eq!(py_round(3.5), 4.0);
    }
}
