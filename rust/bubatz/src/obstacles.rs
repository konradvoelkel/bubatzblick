//! Hindernisse je Verbotsobjekt -- Port von `grid.ObstacleIndex`,
//! `geometry.obstacles_within` (v6: exakter Abstand `dwithin`),
//! `geometry.exclude_self` (Ueberlappung >= 99 % der Gebaeudeflaeche) und
//! der Vorbereitung in `zones.obstacle_context` (nach WKB sortieren,
//! verschmelzen, `cache_key`).

use anyhow::Result;
use bubatzcore::geom::{Shape, Shapes, shapes_area};
use bubatzcore::union::{overlay_pair, to_ogc};
use bubatzcore::union_cascade;
use i_overlay::core::overlay_rule::OverlayRule;

use crate::bstore::Hits;

/// Gebaeude einer Zelle: WKB + Ringe, einmal je Zelle dekodiert. Die
/// Gebaeude des Suchkastens kommen aus dem Gebaeudespeicher (`bstore`,
/// statt eines R-Baums ueber alle Gebaeude im Speicher), aufsteigend nach
/// Zeilenindex.
pub struct CellBuilding {
    pub wkb: Vec<u8>,
    pub shapes: Shapes,
    pub bounds: [f64; 4],
}

pub fn decode_cell_buildings(hits: &Hits) -> Result<Vec<CellBuilding>> {
    (0..hits.len())
        .map(|i| {
            let wkb = hits.wkb(i).to_vec();
            let shapes = crate::wkb::parse(&wkb)?;
            Ok(CellBuilding {
                wkb,
                shapes,
                bounds: hits.bounds(i),
            })
        })
        .collect()
}

// --- Abstand (GEOS DistanceOp fuer Polygon/Polygon: 0 bei Schnitt/Enthalten,
// sonst kleinster Kanten-Kanten-Abstand) --------------------------------------

fn seg_seg_dist2(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    if segments_intersect(a, b, c, d) {
        return 0.0;
    }
    let m = point_seg_dist2(a, c, d).min(point_seg_dist2(b, c, d));
    m.min(point_seg_dist2(c, a, b))
        .min(point_seg_dist2(d, a, b))
}

fn point_seg_dist2(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let vx = b[0] - a[0];
    let vy = b[1] - a[1];
    let wx = p[0] - a[0];
    let wy = p[1] - a[1];
    let ll = vx * vx + vy * vy;
    let t = if ll > 0.0 {
        ((wx * vx + wy * vy) / ll).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = wx - t * vx;
    let dy = wy - t * vy;
    dx * dx + dy * dy
}

fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
        && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
    {
        return true;
    }
    let on = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| -> bool {
        orient(p, q, r) == 0.0
            && r[0] >= p[0].min(q[0])
            && r[0] <= p[0].max(q[0])
            && r[1] >= p[1].min(q[1])
            && r[1] <= p[1].max(q[1])
    };
    on(a, b, c) || on(a, b, d) || on(c, d, a) || on(c, d, b)
}

fn point_in_ring(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = ring.len();
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

fn point_in_shapes(p: [f64; 2], shapes: &[Shape]) -> bool {
    shapes
        .iter()
        .any(|s| point_in_ring(p, &s[0]) && !s[1..].iter().any(|h| point_in_ring(p, h)))
}

fn rings(shapes: &[Shape]) -> impl Iterator<Item = &Vec<[f64; 2]>> {
    shapes.iter().flatten()
}

/// Ist `b` hoechstens `d` von `a` entfernt? (GEOS `dwithin`: Abstand <= d;
/// Abstand 0 bei Schnitt oder Enthaltensein.)
pub fn dwithin(a: &[Shape], b: &[Shape], d: f64) -> bool {
    let d2 = d * d;
    let mut best = f64::INFINITY;
    for ra in rings(a) {
        for rb in rings(b) {
            let na = ra.len();
            let nb = rb.len();
            for i in 0..na {
                let p = ra[i];
                let q = ra[(i + 1) % na];
                for j in 0..nb {
                    let s = seg_seg_dist2(p, q, rb[j], rb[(j + 1) % nb]);
                    if s < best {
                        best = s;
                        if best == 0.0 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    if best <= d2 {
        return true;
    }
    // Kein Kantenkontakt: eines koennte im anderen liegen.
    let pa = a.iter().flatten().next().and_then(|r| r.first()).copied();
    let pb = b.iter().flatten().next().and_then(|r| r.first()).copied();
    matches!(pb, Some(p) if point_in_shapes(p, a)) || matches!(pa, Some(p) if point_in_shapes(p, b))
}

/// `exclude_self`: Gebaeude, dessen Flaeche zu >= 99 % in der Quelle liegt.
pub fn is_self(building: &CellBuilding, source: &[Shape], source_bounds: [f64; 4]) -> bool {
    let b = building.bounds;
    if b[0] > source_bounds[2]
        || b[2] < source_bounds[0]
        || b[1] > source_bounds[3]
        || b[3] < source_bounds[1]
    {
        return false;
    }
    let area = shapes_area(&building.shapes);
    if area <= 0.0 {
        return false;
    }
    let overlap = shapes_area(&overlay_pair(
        &building.shapes,
        source,
        OverlayRule::Intersect,
    ));
    overlap >= 0.99 * area
}

/// Hindernisse eines Objekts wie `obstacle_context`: in Reichweite,
/// nicht die Quelle selbst, nach WKB sortiert, verschmolzen (OGC-Schlusspass),
/// als einzelne Shapes (wie `list(merged.geoms)`).
pub fn merged_obstacles(
    cell_buildings: &[CellBuilding],
    source: &[Shape],
    source_bounds: [f64; 4],
    reach: f64,
) -> Shapes {
    let sb = [
        source_bounds[0] - reach,
        source_bounds[1] - reach,
        source_bounds[2] + reach,
        source_bounds[3] + reach,
    ];
    let mut selected: Vec<&CellBuilding> = cell_buildings
        .iter()
        .filter(|b| {
            !(b.bounds[0] > sb[2]
                || b.bounds[2] < sb[0]
                || b.bounds[1] > sb[3]
                || b.bounds[3] < sb[1])
        })
        .filter(|b| dwithin(source, &b.shapes, reach))
        .filter(|b| !is_self(b, source, source_bounds))
        .collect();
    selected.sort_by(|x, y| x.wkb.cmp(&y.wkb));
    let shapes: Vec<Shape> = selected
        .iter()
        .flat_map(|b| b.shapes.iter().cloned())
        .collect();
    if shapes.is_empty() {
        return Vec::new();
    }
    to_ogc(&union_cascade(shapes))
}

/// `blake2b(WKB(Quelle) ‖ sorted(WKB(Hindernisse)) ‖ Parameterversion)`,
/// hex (128 Zeichen) -- wie `zones._cache_key`.
pub fn cache_key(source_wkb: &[u8], merged: &[Shape], param_version: &str) -> String {
    use blake2::{Blake2b512, Digest};
    let mut wkbs: Vec<Vec<u8>> = merged
        .iter()
        .map(|s| crate::wkb::write(std::slice::from_ref(s)))
        .collect();
    wkbs.sort();
    let mut h = Blake2b512::new();
    h.update(source_wkb);
    for w in &wkbs {
        h.update(w);
    }
    h.update(param_version.as_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(128);
    for b in out {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Shape {
        vec![vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]]
    }

    #[test]
    fn dwithin_handles_gap_touch_overlap_and_containment() {
        let a = vec![square(0.0, 0.0, 10.0)];
        assert!(dwithin(&a, &[square(12.0, 0.0, 5.0)], 2.0));
        assert!(!dwithin(&a, &[square(12.1, 0.0, 5.0)], 2.0));
        assert!(dwithin(&a, &[square(10.0, 0.0, 5.0)], 0.0));
        assert!(dwithin(&a, &[square(5.0, 5.0, 10.0)], 0.0));
        assert!(dwithin(&a, &[square(2.0, 2.0, 1.0)], 0.0), "enthalten");
        assert!(dwithin(&[square(2.0, 2.0, 1.0)], &a, 0.0), "umschliessend");
        assert!(
            dwithin(&a, &[square(13.0, 13.0, 1.0)], 4.3),
            "diagonal 3*sqrt2 = 4,24"
        );
        assert!(!dwithin(&a, &[square(13.0, 13.0, 1.0)], 4.2));
    }

    #[test]
    fn self_exclusion_uses_99_percent_overlap() {
        let source: Shapes = vec![square(0.0, 0.0, 10.0)];
        let sb = [0.0, 0.0, 10.0, 10.0];
        let same = CellBuilding {
            wkb: vec![],
            shapes: vec![square(0.0, 0.0, 10.0)],
            bounds: sb,
        };
        let shifted = CellBuilding {
            wkb: vec![],
            shapes: vec![square(0.05, 0.0, 10.0)],
            bounds: [0.05, 0.0, 10.05, 10.0],
        };
        let half = CellBuilding {
            wkb: vec![],
            shapes: vec![square(5.0, 0.0, 10.0)],
            bounds: [5.0, 0.0, 15.0, 10.0],
        };
        assert!(is_self(&same, &source, sb));
        assert!(is_self(&shifted, &source, sb), "99,5 % Ueberlappung");
        assert!(!is_self(&half, &source, sb));
    }

    #[test]
    fn cache_key_is_blake2b_hex_and_order_independent() {
        let k1 = cache_key(
            b"src",
            &[square(0.0, 0.0, 1.0), square(5.0, 5.0, 1.0)],
            "v1",
        );
        let k2 = cache_key(
            b"src",
            &[square(5.0, 5.0, 1.0), square(0.0, 0.0, 1.0)],
            "v1",
        );
        assert_eq!(k1.len(), 128);
        assert_eq!(k1, k2);
        assert_ne!(k1, cache_key(b"src", &[square(0.0, 0.0, 1.0)], "v1"));
    }

    /// Der `cache_key` steht in jeder Zonen-Zelle auf Platte: aendert sich sein
    /// Wert fuer dieselbe Eingabe, uebernimmt kein Lauf mehr etwas von seinem
    /// Vorlauf, und ein Bundeslauf rechnet ~500 000 Objekte neu. Deshalb ist er
    /// hier festgenagelt -- die Erwartung bricht, wenn eine neue Fassung der
    /// Hash-Kiste andere Bytes liefert, und nicht erst auf dem Server.
    #[test]
    fn cache_key_bytes_are_pinned() {
        assert_eq!(
            cache_key(b"src", &[square(0.0, 0.0, 1.0)], "v1"),
            "d9a25ca22064b368701ef666f145d3ef2a45e3f720c3c5da3425b17b5c9ad78c\
             aac830b7ef14b9e942c70e8d8abaa7b2bf5d18404f2bb163133ee8b41d3307da"
                .replace([' ', '\n'], "")
        );
    }
}
