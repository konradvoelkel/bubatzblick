//! WKB (ISO, 2D) lesen und schreiben, byte-identisch zu Shapely (`to_wkb`:
//! Little-Endian, kein SRID, Ringe geschlossen).

use anyhow::{Result, bail};
use bubatzcore::geom::{Ring, Shape, Shapes};

const POLYGON: u32 = 3;
const MULTIPOLYGON: u32 = 6;

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    le: bool,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| anyhow::anyhow!("WKB zu kurz"))?;
        self.pos += 1;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32> {
        let s = self
            .buf
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| anyhow::anyhow!("WKB zu kurz"))?;
        self.pos += 4;
        let a: [u8; 4] = s.try_into().unwrap();
        Ok(if self.le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }
    fn f64(&mut self) -> Result<f64> {
        let s = self
            .buf
            .get(self.pos..self.pos + 8)
            .ok_or_else(|| anyhow::anyhow!("WKB zu kurz"))?;
        self.pos += 8;
        let a: [u8; 8] = s.try_into().unwrap();
        Ok(if self.le {
            f64::from_le_bytes(a)
        } else {
            f64::from_be_bytes(a)
        })
    }
    fn header(&mut self) -> Result<u32> {
        self.le = self.u8()? == 1;
        let t = self.u32()?;
        // Z/M/SRID-Varianten (ISO 1000+, EWKB-Flags) sind hier nicht erlaubt.
        if t & 0xE000_0000 != 0 || t >= 1000 {
            bail!("WKB-Typ {t:#x}: nur 2D ohne SRID");
        }
        Ok(t)
    }
    /// Ring mit wiederholtem Schlusspunkt lesen, offen zurueckgeben.
    fn ring(&mut self) -> Result<Ring> {
        let n = self.u32()? as usize;
        let mut r = Vec::with_capacity(n);
        for _ in 0..n {
            let x = self.f64()?;
            let y = self.f64()?;
            r.push([x, y]);
        }
        if r.len() >= 2 && r.first() == r.last() {
            r.pop();
        }
        Ok(r)
    }
    fn polygon(&mut self) -> Result<Shape> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.ring()).collect()
    }
}

/// Polygon oder MultiPolygon -> Shape-Liste (leere Teile entfallen).
pub fn parse(buf: &[u8]) -> Result<Shapes> {
    let mut r = Reader {
        buf,
        pos: 0,
        le: true,
    };
    let t = r.header()?;
    let shapes = match t {
        POLYGON => vec![r.polygon()?],
        MULTIPOLYGON => {
            let n = r.u32()? as usize;
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                let inner = r.header()?;
                if inner != POLYGON {
                    bail!("MultiPolygon mit Teil vom Typ {inner}");
                }
                out.push(r.polygon()?);
            }
            out
        }
        other => bail!("WKB-Typ {other}: erwartet Polygon (3) oder MultiPolygon (6)"),
    };
    Ok(shapes
        .into_iter()
        .filter(|s| !s.is_empty() && !s[0].is_empty())
        .collect())
}

/// Bounding-Box (minx, miny, maxx, maxy) direkt aus dem WKB, ohne Ringe zu bauen.
pub fn bounds(buf: &[u8]) -> Result<[f64; 4]> {
    let shapes = parse(buf)?;
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in shapes.iter().flatten().flatten() {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    Ok(b)
}

fn write_ring(out: &mut Vec<u8>, ring: &[[f64; 2]]) {
    let closed = !ring.is_empty() && ring.first() != ring.last();
    let n = ring.len() + usize::from(closed);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for p in ring {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
    }
    if closed {
        out.extend_from_slice(&ring[0][0].to_le_bytes());
        out.extend_from_slice(&ring[0][1].to_le_bytes());
    }
}

fn write_polygon(out: &mut Vec<u8>, shape: &Shape) {
    out.push(1);
    out.extend_from_slice(&POLYGON.to_le_bytes());
    out.extend_from_slice(&(shape.len() as u32).to_le_bytes());
    for ring in shape {
        write_ring(out, ring);
    }
}

/// Immer MultiPolygon-WKB (wie osmiums `create_multipolygon`, auch fuer ein Teil).
pub fn write_multipolygon(shapes: &[Shape]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(1);
    out.extend_from_slice(&MULTIPOLYGON.to_le_bytes());
    out.extend_from_slice(&(shapes.len() as u32).to_le_bytes());
    for s in shapes {
        write_polygon(&mut out, s);
    }
    out
}

/// Ein Shape -> Polygon-WKB; mehrere -> MultiPolygon-WKB (wie Shapely aus
/// `Polygon`/`MultiPolygon` schreibt). Leer -> leeres Polygon (0 Ringe).
pub fn write(shapes: &[Shape]) -> Vec<u8> {
    let mut out = Vec::new();
    match shapes.len() {
        0 => {
            out.push(1);
            out.extend_from_slice(&POLYGON.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        1 => write_polygon(&mut out, &shapes[0]),
        n => {
            out.push(1);
            out.extend_from_slice(&MULTIPOLYGON.to_le_bytes());
            out.extend_from_slice(&(n as u32).to_le_bytes());
            for s in shapes {
                write_polygon(&mut out, s);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_polygon_with_hole_and_multipolygon() {
        let shape: Shape = vec![
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            vec![[2.0, 2.0], [2.0, 4.0], [4.0, 4.0], [4.0, 2.0]],
        ];
        let wkb = write(std::slice::from_ref(&shape));
        assert_eq!(wkb[0], 1);
        assert_eq!(u32::from_le_bytes(wkb[1..5].try_into().unwrap()), POLYGON);
        assert_eq!(parse(&wkb).unwrap(), vec![shape.clone()]);
        let multi = write(&[
            shape.clone(),
            vec![vec![[20.0, 20.0], [21.0, 20.0], [21.0, 21.0]]],
        ]);
        let back = parse(&multi).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], shape);
        assert_eq!(bounds(&multi).unwrap(), [0.0, 0.0, 21.0, 21.0]);
    }

    #[test]
    fn big_endian_input_is_accepted() {
        let mut wkb = vec![0u8];
        wkb.extend_from_slice(&POLYGON.to_be_bytes());
        wkb.extend_from_slice(&1u32.to_be_bytes());
        wkb.extend_from_slice(&4u32.to_be_bytes());
        for p in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]] {
            wkb.extend_from_slice(&f64::to_be_bytes(p[0]));
            wkb.extend_from_slice(&f64::to_be_bytes(p[1]));
        }
        assert_eq!(
            parse(&wkb).unwrap(),
            vec![vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]]]
        );
    }
}
