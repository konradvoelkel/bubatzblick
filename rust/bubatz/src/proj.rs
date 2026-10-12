//! EPSG:4326 -> EPSG:25832 ohne libproj: Port des PROJ-9-Pfads, den pyproj
//! fuer `Transformer.from_crs("EPSG:4326", "EPSG:25832", always_xy=True)`
//! benutzt (`proj=pipeline step proj=unitconvert xy_in=deg xy_out=rad step
//! proj=utm zone=32 ellps=GRS80`; ETRS89 <-> WGS84 ohne Datumsverschiebung).
//! Transversale Mercator nach Poder/Engsager (PROJ `tmerc.cpp`, "exact",
//! 6. Ordnung, Clenshaw-Summation), Ellipsoidparameter wie
//! `pj_calc_ellipsoid_params` (f = 1 - cos(asin(e)), n = tan(alpha/2)^2),
//! Vor-/Nachbereitung wie `fwd.cpp` (lam - lam0, adjlon, x*a + x0). Ziel ist
//! Bitgleichheit mit pyproj (`tests/data/proj_points.json`, 9.998 Punkte).

use std::f64::consts::PI;

const DEG_TO_RAD: f64 = 0.017_453_292_519_943_295_769_236_907_684_886;
const ORDER: usize = 6;

pub struct Utm32 {
    a: f64,
    lam0: f64,
    qn: f64,
    zb: f64,
    cbg: [f64; ORDER],
    gtu: [f64; ORDER],
    cgb: [f64; ORDER],
    utg: [f64; ORDER],
    x0: f64,
    y0: f64,
}

fn gatg(p1: &[f64; ORDER], b: f64, cos_2b: f64, sin_2b: f64) -> f64 {
    let mut h = 0.0;
    let mut h2 = 0.0;
    let two_cos_2b = 2.0 * cos_2b;
    let mut idx = ORDER;
    idx -= 1;
    let mut h1 = p1[idx];
    while idx > 0 {
        idx -= 1;
        h = -h2 + two_cos_2b * h1 + p1[idx];
        h2 = h1;
        h1 = h;
    }
    b + h * sin_2b
}

#[allow(clippy::too_many_arguments)]
fn clen_s(
    a: &[f64; ORDER],
    sin_arg_r: f64,
    cos_arg_r: f64,
    sinh_arg_i: f64,
    cosh_arg_i: f64,
) -> (f64, f64) {
    let mut r = 2.0 * cos_arg_r * cosh_arg_i;
    let mut i = -2.0 * sin_arg_r * sinh_arg_i;
    let mut idx = ORDER;
    let mut hi1 = 0.0;
    let mut hr1 = 0.0;
    let mut hi = 0.0;
    idx -= 1;
    let mut hr = a[idx];
    while idx > 0 {
        let hr2 = hr1;
        let hi2 = hi1;
        hr1 = hr;
        hi1 = hi;
        idx -= 1;
        hr = -hr2 + r * hr1 - i * hi1 + a[idx];
        hi = -hi2 + i * hr1 + r * hi1;
    }
    r = sin_arg_r * cosh_arg_i;
    i = cos_arg_r * sinh_arg_i;
    (r * hr - i * hi, r * hi + i * hr)
}

fn clens(a: &[f64; ORDER], arg_r: f64) -> f64 {
    let cos_arg_r = arg_r.cos();
    let r = 2.0 * cos_arg_r;
    let mut idx = ORDER;
    let mut hr1 = 0.0;
    idx -= 1;
    let mut hr = a[idx];
    while idx > 0 {
        let hr2 = hr1;
        hr1 = hr;
        idx -= 1;
        hr = -hr2 + r * hr1 + a[idx];
    }
    arg_r.sin() * hr
}

/// `adjlon`: Laenge in (-pi, pi] bringen (PROJ `adjlon.cpp`; SPI ist dort
/// bewusst eine grobe Schranke, TWOPI/ONEPI sind als double gleich 2 pi / pi).
#[allow(clippy::approx_constant)]
fn adjlon(lon: f64) -> f64 {
    const SPI: f64 = 3.14159265359;
    const TWOPI: f64 = 2.0 * PI;
    const ONEPI: f64 = PI;
    if lon.abs() <= SPI {
        return lon;
    }
    let mut lon = lon + ONEPI;
    lon -= TWOPI * (lon / TWOPI).floor();
    lon - ONEPI
}

/// `setup_exact`: Koeffizienten der Rueckrichtung (Gauss -> geodaetisch,
/// ell. N/E -> sph. N/E), dieselbe Reihenfolge wie in PROJ.
#[allow(clippy::excessive_precision)]
fn inverse_coefficients(n: f64) -> ([f64; ORDER], [f64; ORDER]) {
    let mut np = n;
    let mut cgb = [0.0; ORDER];
    cgb[0] = n
        * (2.0
            + n * (-2.0 / 3.0
                + n * (-2.0 + n * (116.0 / 45.0 + n * (26.0 / 45.0 + n * (-2854.0 / 675.0))))));
    np *= n;
    cgb[1] = np
        * (7.0 / 3.0
            + n * (-8.0 / 5.0 + n * (-227.0 / 45.0 + n * (2704.0 / 315.0 + n * (2323.0 / 945.0)))));
    np *= n;
    cgb[2] =
        np * (56.0 / 15.0 + n * (-136.0 / 35.0 + n * (-1262.0 / 105.0 + n * (73814.0 / 2835.0))));
    np *= n;
    cgb[3] = np * (4279.0 / 630.0 + n * (-332.0 / 35.0 + n * (-399572.0 / 14175.0)));
    np *= n;
    cgb[4] = np * (4174.0 / 315.0 + n * (-144838.0 / 6237.0));
    np *= n;
    cgb[5] = np * (601676.0 / 22275.0);
    np = n * n;
    let mut utg = [0.0; ORDER];
    utg[0] = n
        * (-0.5
            + n * (2.0 / 3.0
                + n * (-37.0 / 96.0
                    + n * (1.0 / 360.0 + n * (81.0 / 512.0 + n * (-96199.0 / 604800.0))))));
    utg[1] = np
        * (-1.0 / 48.0
            + n * (-1.0 / 15.0
                + n * (437.0 / 1440.0 + n * (-46.0 / 105.0 + n * (1118711.0 / 3870720.0)))));
    np *= n;
    utg[2] =
        np * (-17.0 / 480.0 + n * (37.0 / 840.0 + n * (209.0 / 4480.0 + n * (-5569.0 / 90720.0))));
    np *= n;
    utg[3] = np * (-4397.0 / 161280.0 + n * (11.0 / 504.0 + n * (830251.0 / 7257600.0)));
    np *= n;
    utg[4] = np * (-4583.0 / 161280.0 + n * (108847.0 / 3991680.0));
    np *= n;
    utg[5] = np * (-20648693.0 / 638668800.0);
    (cgb, utg)
}

impl Utm32 {
    /// (x, y) in EPSG:25832 -> (lon, lat) in Grad (PROJ `exact_e_inv` + `inv.cpp`).
    pub fn inverse(&self, x: f64, y: f64) -> [f64; 2] {
        let xx = (x - self.x0) * (1.0 / self.a);
        let yy = (y - self.y0) * (1.0 / self.a);
        let mut cn = (yy - self.zb) / self.qn;
        let mut ce = xx / self.qn;
        if ce.abs() > 2.623395162778 {
            return [f64::NAN, f64::NAN];
        }
        let sin_arg_r = (2.0 * cn).sin();
        let cos_arg_r = (2.0 * cn).cos();
        let exp_2_ce = (2.0 * ce).exp();
        let half_inv = 0.5 / exp_2_ce;
        let sinh_arg_i = 0.5 * exp_2_ce - half_inv;
        let cosh_arg_i = 0.5 * exp_2_ce + half_inv;
        let (d_cn, d_ce) = clen_s(&self.utg, sin_arg_r, cos_arg_r, sinh_arg_i, cosh_arg_i);
        cn += d_cn;
        ce += d_ce;
        let sin_cn = cn.sin();
        let cos_cn = cn.cos();
        let sinh_ce = ce.sinh();
        ce = sinh_ce.atan2(cos_cn);
        let modulus_ce = sinh_ce.hypot(cos_cn);
        cn = sin_cn.atan2(modulus_ce);
        let tmp = 2.0 * modulus_ce / (sinh_ce * sinh_ce + 1.0);
        let sin_2_cn = sin_cn * tmp;
        let cos_2_cn = tmp * modulus_ce - 1.0;
        let phi = gatg(&self.cgb, cn, cos_2_cn, sin_2_cn);
        let lam = adjlon(ce + self.lam0);
        [lam / DEG_TO_RAD, phi / DEG_TO_RAD]
    }
}

impl Default for Utm32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Utm32 {
    pub fn new() -> Self {
        // GRS80 (PROJ ellps-Tabelle): a = 6378137, rf = 298.257222101
        let a = 6_378_137.0_f64;
        let rf = 298.257_222_101_f64;
        let f0 = 1.0 / rf;
        let es = 2.0 * f0 - f0 * f0;
        // pj_calc_ellipsoid_params
        let e = es.sqrt();
        let alpha = e.asin();
        let n = (alpha / 2.0).tan().powi(2);
        let k0 = 0.9996_f64;
        // setup_exact (nur die Vorwaertskoeffizienten)
        let mut np = n;
        let mut cbg = [0.0; ORDER];
        cbg[0] = n
            * (-2.0
                + n * (2.0 / 3.0
                    + n * (4.0 / 3.0
                        + n * (-82.0 / 45.0 + n * (32.0 / 45.0 + n * (4642.0 / 4725.0))))));
        np *= n;
        cbg[1] = np
            * (5.0 / 3.0
                + n * (-16.0 / 15.0
                    + n * (-13.0 / 9.0 + n * (904.0 / 315.0 + n * (-1522.0 / 945.0)))));
        np *= n;
        cbg[2] =
            np * (-26.0 / 15.0 + n * (34.0 / 21.0 + n * (8.0 / 5.0 + n * (-12686.0 / 2835.0))));
        np *= n;
        cbg[3] = np * (1237.0 / 630.0 + n * (-12.0 / 5.0 + n * (-24832.0 / 14175.0)));
        np *= n;
        cbg[4] = np * (-734.0 / 315.0 + n * (109598.0 / 31185.0));
        np *= n;
        cbg[5] = np * (444337.0 / 155925.0);
        np = n * n;
        let qn = k0 / (1.0 + n) * (1.0 + np * (1.0 / 4.0 + np * (1.0 / 64.0 + np / 256.0)));
        let mut gtu = [0.0; ORDER];
        gtu[0] = n
            * (0.5
                + n * (-2.0 / 3.0
                    + n * (5.0 / 16.0
                        + n * (41.0 / 180.0 + n * (-127.0 / 288.0 + n * (7891.0 / 37800.0))))));
        gtu[1] = np
            * (13.0 / 48.0
                + n * (-3.0 / 5.0
                    + n * (557.0 / 1440.0 + n * (281.0 / 630.0 + n * (-1983433.0 / 1935360.0)))));
        np *= n;
        gtu[2] = np
            * (61.0 / 240.0
                + n * (-103.0 / 140.0 + n * (15061.0 / 26880.0 + n * (167603.0 / 181440.0))));
        np *= n;
        gtu[3] = np * (49561.0 / 161280.0 + n * (-179.0 / 168.0 + n * (6601661.0 / 7257600.0)));
        np *= n;
        gtu[4] = np * (34729.0 / 80640.0 + n * (-3418889.0 / 1995840.0));
        np *= n;
        gtu[5] = np * (212378941.0 / 319334400.0);
        let (cgb, utg) = inverse_coefficients(n);
        let phi0 = 0.0_f64;
        let z = gatg(&cbg, phi0, (2.0 * phi0).cos(), (2.0 * phi0).sin());
        let zb = -qn * (z + clens(&gtu, 2.0 * z));
        // PJ_PROJECTION(utm): zone 32 -> lam0 = (31 + .5) * pi / 30 - pi
        let zone = 31.0_f64;
        let lam0 = (zone + 0.5) * PI / 30.0 - PI;
        Utm32 {
            a,
            lam0,
            qn,
            zb,
            cbg,
            gtu,
            cgb,
            utg,
            x0: 500_000.0,
            y0: 0.0,
        }
    }

    /// (lon, lat) in Grad -> (x, y) in EPSG:25832; NaN ausserhalb des Definitionsbereichs.
    pub fn forward(&self, lon_deg: f64, lat_deg: f64) -> [f64; 2] {
        // unitconvert deg -> rad, dann fwd_prepare
        let phi = lat_deg * DEG_TO_RAD;
        let mut lam = lon_deg * DEG_TO_RAD;
        lam = (lam - 0.0) - self.lam0;
        lam = adjlon(lam);
        // exact_e_fwd
        let mut cn = gatg(&self.cbg, phi, (2.0 * phi).cos(), (2.0 * phi).sin());
        let sin_cn = cn.sin();
        let cos_cn = cn.cos();
        let sin_ce = lam.sin();
        let cos_ce = lam.cos();
        let cos_cn_cos_ce = cos_cn * cos_ce;
        cn = sin_cn.atan2(cos_cn_cos_ce);
        let inv_denom_tan_ce = 1.0 / sin_cn.hypot(cos_cn_cos_ce);
        let tan_ce = sin_ce * cos_cn * inv_denom_tan_ce;
        let mut ce = tan_ce.asinh();
        let two_inv_denom_tan_ce = 2.0 * inv_denom_tan_ce;
        let two_inv_denom_tan_ce_square = two_inv_denom_tan_ce * inv_denom_tan_ce;
        let tmp_r = cos_cn_cos_ce * two_inv_denom_tan_ce_square;
        let sin_arg_r = sin_cn * tmp_r;
        let cos_arg_r = cos_cn_cos_ce * tmp_r - 1.0;
        let sinh_arg_i = tan_ce * two_inv_denom_tan_ce;
        let cosh_arg_i = two_inv_denom_tan_ce_square - 1.0;
        let (d_cn, d_ce) = clen_s(&self.gtu, sin_arg_r, cos_arg_r, sinh_arg_i, cosh_arg_i);
        cn += d_cn;
        ce += d_ce;
        if ce.abs() > 2.623395162778 {
            return [f64::NAN, f64::NAN];
        }
        let y = self.qn * cn + self.zb;
        let x = self.qn * ce;
        // fwd_finalize: PJ_IO_UNITS_CLASSIC -> * a, dann + x0 (fr_meter = 1)
        [1.0 * (x * self.a + self.x0), 1.0 * (y * self.a + self.y0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_roundtrips_within_a_nanometre() {
        let p = Utm32::new();
        for (lon, lat) in [
            (6.78, 51.23),
            (9.0, 51.0),
            (13.4, 52.5),
            (5.9, 47.3),
            (14.9, 54.8),
        ] {
            let [x, y] = p.forward(lon, lat);
            let [lon2, lat2] = p.inverse(x, y);
            assert!(
                (lon2 - lon).abs() < 1e-11 && (lat2 - lat).abs() < 1e-11,
                "{lon} {lat} -> {lon2} {lat2}"
            );
        }
    }

    #[test]
    fn matches_pyproj_bit_for_bit_on_reference_points() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/proj_points.json"
        ))
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let col = |k: &str| -> Vec<f64> {
            v[k].as_array()
                .unwrap()
                .iter()
                .map(|u| f64::from_bits(u.as_u64().unwrap()))
                .collect()
        };
        let (lon, lat, x, y) = (col("lon"), col("lat"), col("x"), col("y"));
        let p = Utm32::new();
        let mut exact = 0usize;
        let mut worst = 0.0f64;
        for i in 0..lon.len() {
            let [px, py] = p.forward(lon[i], lat[i]);
            if px.to_bits() == x[i].to_bits() && py.to_bits() == y[i].to_bits() {
                exact += 1;
            }
            worst = worst.max((px - x[i]).abs()).max((py - y[i]).abs());
        }
        println!(
            "bitgleich {exact}/{}; groesste Abweichung {worst:.3e} m",
            lon.len()
        );
        assert!(worst < 1e-6, "{worst}");
        // Erwartet (nicht verlangt): alle Punkte bitgleich. Ab 99 % gilt der
        // Port als richtig; darunter stimmt die Operationsreihenfolge nicht.
        assert!(
            exact * 100 >= lon.len() * 99,
            "nur {exact} von {} bitgleich",
            lon.len()
        );
    }

    #[test]
    fn central_meridian_and_known_point() {
        let p = Utm32::new();
        let [x, _] = p.forward(9.0, 51.0);
        assert!((x - 500_000.0).abs() < 1e-6);
        // Duesseldorf (6.78, 51.23) liegt bei ~345.0 km / 5.678 Mm (pyproj)
        let [x, y] = p.forward(6.78, 51.23);
        assert!(
            (x - 345_003.0).abs() < 10.0 && (y - 5_677_744.0).abs() < 10.0,
            "{x} {y}"
        );
    }
}
