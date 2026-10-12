//! Gitter und Zellenzuordnung -- Port von `bubatzblick.grid` (`build_grid`,
//! `assign_to_cells`) und des GEOS-Flaechenschwerpunkts, den Shapely fuer
//! `centroid` benutzt (die Zuordnung geht ueber den Schwerpunkt).

use bubatzcore::geom::{Shape, is_ccw};

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub col: i64,
    pub row: i64,
    pub bounds: [f64; 4],
}

impl Cell {
    pub fn id(&self) -> String {
        format!("x{}_y{}", self.col, self.row)
    }
}

/// `build_grid`: Zellen an ganzzahligen Vielfachen von `cell_size` ab (0,0),
/// Spalte aussen, Zeile innen (dieselbe Reihenfolge wie Python).
pub fn build_grid(total_bounds: [f64; 4], cell_size: f64) -> Vec<Cell> {
    let [minx, miny, maxx, maxy] = total_bounds;
    let col0 = (minx / cell_size).floor() as i64;
    let row0 = (miny / cell_size).floor() as i64;
    let col1 = (maxx / cell_size).floor() as i64;
    let row1 = (maxy / cell_size).floor() as i64;
    let mut cells = Vec::new();
    for col in col0..=col1 {
        for row in row0..=row1 {
            let cx0 = col as f64 * cell_size;
            let cy0 = row as f64 * cell_size;
            cells.push(Cell {
                col,
                row,
                bounds: [cx0, cy0, cx0 + cell_size, cy0 + cell_size],
            });
        }
    }
    cells
}

/// Zellindex eines Punkts wie `assign_to_cells`: Ganzzahldivision, dann
/// Nachjustierung gegen die exakten Zellgrenzen `col * cell_size`.
pub fn cell_index(x: f64, y: f64, cell_size: f64) -> (i64, i64) {
    let mut col = (x / cell_size).floor() as i64;
    let mut row = (y / cell_size).floor() as i64;
    if x < col as f64 * cell_size {
        col -= 1;
    }
    if x >= (col + 1) as f64 * cell_size {
        col += 1;
    }
    if y < row as f64 * cell_size {
        row -= 1;
    }
    if y >= (row + 1) as f64 * cell_size {
        row += 1;
    }
    (col, row)
}

/// Flaechenschwerpunkt nach GEOS `algorithm::Centroid` (JTS): Dreiecke
/// (Basispunkt, p_i, p_{i+1}) mit vorzeichenbehafteter doppelter Flaeche,
/// Basispunkt = erster Punkt des ersten Aussenrings; Vorzeichen je Ring nach
/// Orientierung (Aussenring positiv, wenn CW; Loch positiv, wenn CCW -- so
/// dass Loecher abgezogen werden). Ergebnis `cg3 / 3 / areasum2`. Bei
/// Flaeche 0 (entartet) der Schwerpunkt der Kanten (Laengen-gewichtet).
pub fn centroid(shapes: &[Shape]) -> [f64; 2] {
    let mut base: Option<[f64; 2]> = None;
    let mut cg3 = [0.0f64, 0.0];
    let mut areasum2 = 0.0f64;
    let mut line_cent = [0.0f64, 0.0];
    let mut line_len = 0.0f64;
    for shape in shapes {
        for (i, ring) in shape.iter().enumerate() {
            if ring.is_empty() {
                continue;
            }
            let b = *base.get_or_insert(ring[0]);
            // JTS: Aussenring "isPositiveArea = !isCCW", Loch "isPositiveArea = isCCW"
            let ccw = is_ccw(ring);
            let positive = if i == 0 { !ccw } else { ccw };
            let sign = if positive { 1.0 } else { -1.0 };
            let n = ring.len();
            for k in 0..n {
                let p1 = ring[k];
                let p2 = ring[(k + 1) % n];
                let area2 = (p1[0] - b[0]) * (p2[1] - b[1]) - (p2[0] - b[0]) * (p1[1] - b[1]);
                cg3[0] += sign * area2 * (b[0] + p1[0] + p2[0]);
                cg3[1] += sign * area2 * (b[1] + p1[1] + p2[1]);
                areasum2 += sign * area2;
                // Linienschwerpunkt als Rueckfall
                let seg = (p2[0] - p1[0]).hypot(p2[1] - p1[1]);
                line_cent[0] += seg * (p1[0] + p2[0]) / 2.0;
                line_cent[1] += seg * (p1[1] + p2[1]) / 2.0;
                line_len += seg;
            }
        }
    }
    if areasum2.abs() > 0.0 {
        [cg3[0] / 3.0 / areasum2, cg3[1] / 3.0 / areasum2]
    } else if line_len > 0.0 {
        [line_cent[0] / line_len, line_cent[1] / line_len]
    } else {
        base.unwrap_or([f64::NAN, f64::NAN])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_covers_bounds_and_ids_match_python_format() {
        let cells = build_grid([-1.0, 1999.0, 4000.0, 4001.0], 2000.0);
        let ids: Vec<String> = cells.iter().map(Cell::id).collect();
        assert_eq!(
            ids,
            vec![
                "x-1_y0", "x-1_y1", "x-1_y2", "x0_y0", "x0_y1", "x0_y2", "x1_y0", "x1_y1", "x1_y2",
                "x2_y0", "x2_y1", "x2_y2"
            ]
        );
        assert_eq!(cell_index(3999.999, 2000.0, 2000.0), (1, 1));
        assert_eq!(cell_index(4000.0, 1999.999, 2000.0), (2, 0));
    }

    #[test]
    fn centroid_of_square_with_hole_and_multipolygon() {
        let sq = vec![vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]];
        let c = centroid(std::slice::from_ref(&sq));
        assert!((c[0] - 2.0).abs() < 1e-12 && (c[1] - 2.0).abs() < 1e-12);
        // Loch rechts oben verschiebt den Schwerpunkt nach links unten.
        let with_hole = vec![
            vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
            vec![[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]],
        ];
        let c = centroid(&[with_hole]);
        // Flaeche 16 - 4 = 12: (16*2 - 4*3)/12 = 5/3
        assert!((c[0] - 5.0 / 3.0).abs() < 1e-12 && (c[1] - 5.0 / 3.0).abs() < 1e-12);
        let two = [
            sq.clone(),
            vec![vec![[10.0, 0.0], [14.0, 0.0], [14.0, 4.0], [10.0, 4.0]]],
        ];
        let c = centroid(&two);
        assert!((c[0] - 7.0).abs() < 1e-12 && (c[1] - 2.0).abs() < 1e-12);
    }
}
