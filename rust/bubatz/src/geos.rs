//! Zwei GEOS-Bausteine, bitgleich nachgebaut (3.13.1):
//! - `Point.buffer(r, quad_segs)`: `OffsetSegmentGenerator::createCircle` --
//!   Start bei (x + r, y), im Uhrzeigersinn, `nSegs = (int)(2 pi / (pi/2 /
//!   quad_segs) + 0.5)` Punkte auf `p + r (cos a, sin a)`, geschlossen.
//! - `Polygon.area`: `Area::ofRingSigned(CoordinateSequence)` (Schnuerformel
//!   mit `x0`-Verschiebung), Flaeche = |Shell| - Summe |Loecher|.

use std::f64::consts::PI;

use bubatzcore::geom::Shape;

/// Kreisscheibe wie GEOS (Ring geschlossen, CW, Start im Osten).
pub fn point_buffer(x: f64, y: f64, radius: f64, quad_segs: u32) -> Vec<[f64; 2]> {
    let quantum = (PI / 2.0) / quad_segs as f64;
    let total = 2.0 * PI;
    let n_segs = (total / quantum + 0.5) as i64;
    let angle_inc = total / n_segs as f64;
    let mut ring = Vec::with_capacity(n_segs as usize + 1);
    ring.push([x + radius, y]);
    for i in 0..n_segs {
        // GEOS: startAngle + directionFactor * i * angleInc, directionFactor = -1
        let angle = -(i as f64 * angle_inc);
        let pt = [x + radius * angle.cos(), y + radius * angle.sin()];
        // addPt: redundante (praktisch gleiche) Punkte werden verworfen
        if let Some(last) = ring.last()
            && (pt[0] - last[0]).abs() < radius * 1e-6
            && (pt[1] - last[1]).abs() < radius * 1e-6
        {
            continue;
        }
        ring.push(pt);
    }
    // closeRing
    if ring.first() != ring.last() {
        let f = ring[0];
        ring.push(f);
    }
    ring
}

/// `Area::ofRingSigned` fuer einen geschlossenen oder offenen Ring.
pub fn ring_area_signed(ring: &[[f64; 2]]) -> f64 {
    let closed: Vec<[f64; 2]>;
    let r: &[[f64; 2]] = if ring.len() >= 2 && ring.first() != ring.last() {
        closed = ring
            .iter()
            .copied()
            .chain(std::iter::once(ring[0]))
            .collect();
        &closed
    } else {
        ring
    };
    let n = r.len();
    if n < 3 {
        return 0.0;
    }
    let mut p1 = r[0];
    let mut p2 = r[1];
    let x0 = p1[0];
    p2[0] -= x0;
    let mut sum = 0.0;
    let mut p0y;
    for i in 1..n - 1 {
        p0y = p1[1];
        p1[0] = p2[0];
        p1[1] = p2[1];
        p2 = r[i + 1];
        p2[0] -= x0;
        sum += p1[0] * (p0y - p2[1]);
    }
    sum / 2.0
}

/// `Polygon.area` / `MultiPolygon.area` (Summe der Teile).
pub fn shapes_area(shapes: &[Shape]) -> f64 {
    let mut total = 0.0;
    for s in shapes {
        let mut a = ring_area_signed(&s[0]).abs();
        for h in &s[1..] {
            a -= ring_area_signed(h).abs();
        }
        total += a;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_buffer_is_bit_identical_to_shapely() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/point_discs.json"
        ))
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let bits = |u: &serde_json::Value| f64::from_bits(u.as_u64().unwrap());
        let mut exact = 0;
        let n = v["points"].as_array().unwrap().len();
        for i in 0..n {
            let p = &v["points"][i];
            let ring = point_buffer(bits(&p[0]), bits(&p[1]), 15.0, 16);
            let want: Vec<f64> = v["coords"][i]
                .as_array()
                .unwrap()
                .iter()
                .map(bits)
                .collect();
            let got: Vec<f64> = ring.iter().flat_map(|q| [q[0], q[1]]).collect();
            assert_eq!(
                got.len(),
                want.len(),
                "Punkt {i}: {} vs {} Koordinaten",
                got.len(),
                want.len()
            );
            if got
                .iter()
                .zip(&want)
                .all(|(a, b)| a.to_bits() == b.to_bits())
            {
                exact += 1;
            }
            let area = shapes_area(&[vec![ring.clone()]]);
            assert_eq!(
                area.to_bits(),
                bits(&v["areas"][i]).to_bits(),
                "Flaeche Punkt {i}"
            );
        }
        assert_eq!(exact, n, "nur {exact}/{n} Scheiben bitgleich");
    }

    #[test]
    fn area_matches_geos_bit_for_bit() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/areas.json"
        ))
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        for (w, a) in v["wkb"]
            .as_array()
            .unwrap()
            .iter()
            .zip(v["areas"].as_array().unwrap())
        {
            let bytes: Vec<u8> = (0..w.as_str().unwrap().len() / 2)
                .map(|i| u8::from_str_radix(&w.as_str().unwrap()[2 * i..2 * i + 2], 16).unwrap())
                .collect();
            let shapes = crate::wkb::parse(&bytes).unwrap();
            assert_eq!(shapes_area(&shapes).to_bits(), a.as_u64().unwrap());
        }
    }
}
