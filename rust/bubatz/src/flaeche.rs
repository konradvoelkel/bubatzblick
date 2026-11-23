//! Zonenflaeche je Verwaltungsgebiet (`bubatz report --areas-pbf`): die
//! Vereinigung aller Baender bis 100 m, geschnitten mit den Gebieten (je
//! Bundesland: admin_level 4, ISO3166-2 "DE-..."). Ueberlappende Zonen zaehlen
//! einmal -- anders als die Summen je Kategorie in `report`, die jedes Band fuer
//! sich zaehlen und deshalb mehr ergeben, als rot auf der Karte ist.
//!
//! Verfahren: Quadrate eines Rasters (Vorgabe 5 km, EPSG:25832) zerlegen die
//! Ebene ueberlappungsfrei. Je Quadrat werden die Baender, die es beruehren,
//! vereinigt und auf das Quadrat geschnitten; die Summe ueber die Quadrate ist
//! die Flaeche der Vereinigung, ohne je alle 500 000 Baender auf einmal zu
//! vereinigen. Gerechnet wird blockweise (Bloecke zu 20 km): je Block nur die
//! Baender, die ihn beruehren, und die Gebiete auf ihn vorgeschnitten, damit der
//! Schnitt je Quadrat nicht die ganze Landesgrenze mitnimmt. Die Baender eines
//! Bundeslaufs (rund 1 Mrd. Punkte) passen nicht auf einmal in den Speicher.

use std::collections::HashMap;

use bubatzcore::geom::{Shape, Shapes};
use i_overlay::core::overlay_rule::OverlayRule;
use rayon::prelude::*;

use crate::geos::shapes_area;
use crate::simplify::union_many;
use bubatzcore::union::overlay_pair;

/// Kantenlaenge eines Blocks (auf ganze Quadrate gerundet). Je Block werden die
/// Baender geladen und die Gebiete vorgeschnitten: 20 km haelt den dichtesten
/// Block klein und die Zahl der Gebietsschnitte gering.
const BLOCK_M: f64 = 20_000.0;

/// Quadrate je Blockkante fuer ein Raster.
fn block_quadrate(raster_m: f64) -> i64 {
    ((BLOCK_M / raster_m).round() as i64).max(1)
}

pub struct Gebiet {
    /// ISO3166-2 oder, wo sie fehlt, die Relations-ID
    pub schluessel: String,
    pub name: String,
    pub form: Shapes,
}

pub struct Ergebnis {
    /// Zonenflaeche je Gebiet (m2), in der Reihenfolge der Gebiete
    pub zone_m2: Vec<f64>,
    /// Flaeche der Gebiete selbst (m2); bei Kuestenlaendern mit Kuestenmeer
    pub gebiet_m2: Vec<f64>,
    /// Zonenflaeche insgesamt, auch ausserhalb aller Gebiete (Zonen ueber die Grenze)
    pub gesamt_m2: f64,
    /// Quadrate mit mindestens einem Band
    pub quadrate: usize,
}

fn bbox(shapes: &[Shape]) -> Option<[f64; 4]> {
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
    (b[0] <= b[2]).then_some(b)
}

fn rechteck(x0: f64, y0: f64, x1: f64, y1: f64) -> Shapes {
    vec![vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]]
}

/// Die Quadrate (Index), die ein Rechteck beruehrt.
fn quadrate_von(b: [f64; 4], raster: f64) -> impl Iterator<Item = (i64, i64)> {
    let (qx0, qy0) = (
        (b[0] / raster).floor() as i64,
        (b[1] / raster).floor() as i64,
    );
    let (qx1, qy1) = (
        (b[2] / raster).floor() as i64,
        (b[3] / raster).floor() as i64,
    );
    (qx0..=qx1).flat_map(move |qx| (qy0..=qy1).map(move |qy| (qx, qy)))
}

/// Wie `flaeche_je_gebiet_mit`, mit allen Baendern schon im Speicher (Tests).
#[cfg(test)]
pub fn flaeche_je_gebiet(baender: &[Shapes], gebiete: &[Gebiet], raster_m: f64) -> Ergebnis {
    let rahmen: Vec<Option<[f64; 4]>> = baender.iter().map(|b| bbox(b)).collect();
    flaeche_je_gebiet_mit(
        &rahmen,
        |idx| Ok(idx.iter().map(|&i| baender[i].clone()).collect()),
        gebiete,
        raster_m,
    )
    .expect("Laden aus dem Speicher scheitert nicht")
}

/// Zonenflaeche je Gebiet, blockweise: `rahmen` sind die Rechtecke aller Baender
/// (ein Durchgang vorab, wenige MB), `laden` liefert zu Band-Indizes die Formen.
/// Je Block (`BLOCK_M`, auf ganze Quadrate gerundet) werden nur die Baender geladen, die ihn
/// beruehren, und danach wieder freigegeben: der Speicher richtet sich nach dem
/// dichtesten Block, nicht nach allen Baendern (bundesweit rund 1 Mrd. Punkte,
/// 17 GB -- daran starb der erste Bundeslauf mit MemoryMax=7G). Jedes
/// Quadrat zaehlt nur in seinem eigenen Block, mit allen Baendern, die es
/// beruehren; deshalb gleicht das Ergebnis dem einer Rechnung in einem Stueck.
pub fn flaeche_je_gebiet_mit<F>(
    rahmen: &[Option<[f64; 4]>],
    laden: F,
    gebiete: &[Gebiet],
    raster_m: f64,
) -> anyhow::Result<Ergebnis>
where
    F: Fn(&[usize]) -> anyhow::Result<Vec<Shapes>>,
{
    assert!(raster_m > 0.0, "Rasterweite muss positiv sein");
    let block = block_quadrate(raster_m);
    let block_m = raster_m * block as f64;
    // Baender je Block (ein Band an einer Blockkante gehoert zu bis zu vier).
    let mut je_block: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, bb) in rahmen.iter().enumerate() {
        if let Some(bb) = bb {
            for b in quadrate_von(*bb, block_m) {
                je_block.entry(b).or_default().push(i);
            }
        }
    }
    let mut bloecke: Vec<(i64, i64)> = je_block.keys().copied().collect();
    bloecke.sort_unstable();
    let gebiet_bb: Vec<Option<[f64; 4]>> = gebiete.iter().map(|g| bbox(&g.form)).collect();
    let mut zone_m2 = vec![0.0; gebiete.len()];
    let mut gesamt_m2 = 0.0;
    let mut quadrate_gesamt = 0usize;
    for (n, &(bx, by)) in bloecke.iter().enumerate() {
        let idx = &je_block[&(bx, by)];
        let formen = laden(idx)?;
        let (x0, y0) = (bx as f64 * block_m, by as f64 * block_m);
        // Gebiete auf diesen Block schneiden (je Gebiet parallel).
        let kasten = rechteck(x0, y0, x0 + block_m, y0 + block_m);
        let stuecke: Vec<(usize, Shapes)> = (0..gebiete.len())
            .into_par_iter()
            .filter_map(|gi| {
                let b = gebiet_bb[gi]?;
                if b[2] < x0 || b[0] > x0 + block_m || b[3] < y0 || b[1] > y0 + block_m {
                    return None;
                }
                let teil = overlay_pair(&gebiete[gi].form, &kasten, OverlayRule::Intersect);
                (!teil.is_empty()).then_some((gi, teil))
            })
            .collect();
        // Quadrate dieses Blocks mit den Baendern, die sie beruehren.
        let mut je_quadrat: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (k, &i) in idx.iter().enumerate() {
            for q in quadrate_von(rahmen[i].expect("nur Baender mit Rahmen"), raster_m) {
                if (q.0.div_euclid(block), q.1.div_euclid(block)) == (bx, by) {
                    je_quadrat.entry(q).or_default().push(k);
                }
            }
        }
        quadrate_gesamt += je_quadrat.len();
        let quadrate: Vec<(&(i64, i64), &Vec<usize>)> = je_quadrat.iter().collect();
        let teile: Vec<(f64, Vec<(usize, f64)>)> = quadrate
            .par_iter()
            .map(|&(&(qx, qy), ks)| {
                let (x0, y0) = (qx as f64 * raster_m, qy as f64 * raster_m);
                let quadrat = rechteck(x0, y0, x0 + raster_m, y0 + raster_m);
                let teile: Vec<Shapes> = ks.iter().map(|&k| formen[k].clone()).collect();
                let u = overlay_pair(&union_many(&teile), &quadrat, OverlayRule::Intersect);
                let mut je = Vec::new();
                if u.is_empty() {
                    return (0.0, je);
                }
                for (gi, stueck) in &stuecke {
                    let im_quadrat = overlay_pair(stueck, &quadrat, OverlayRule::Intersect);
                    if im_quadrat.is_empty() {
                        continue;
                    }
                    let a = shapes_area(&overlay_pair(&u, &im_quadrat, OverlayRule::Intersect));
                    if a > 0.0 {
                        je.push((*gi, a));
                    }
                }
                (shapes_area(&u), je)
            })
            .collect();
        for (a, je) in &teile {
            gesamt_m2 += a;
            for &(gi, x) in je {
                zone_m2[gi] += x;
            }
        }
        if (n + 1) % 50 == 0 || n + 1 == bloecke.len() {
            tracing::info!(
                "flaeche: Block {}/{} ({} Baender), bisher {:.1} km2",
                n + 1,
                bloecke.len(),
                idx.len(),
                gesamt_m2 / 1e6
            );
        }
    }
    Ok(Ergebnis {
        zone_m2,
        gebiet_m2: gebiete.iter().map(|g| shapes_area(&g.form)).collect(),
        gesamt_m2,
        quadrate: quadrate_gesamt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quadrat(x: f64, y: f64, a: f64) -> Shapes {
        rechteck(x, y, x + a, y + a)
    }

    fn gebiet(name: &str, form: Shapes) -> Gebiet {
        Gebiet {
            schluessel: name.into(),
            name: name.into(),
            form,
        }
    }

    #[test]
    fn ueberlappung_zaehlt_einmal_auch_ueber_rasterkanten() {
        // Zwei 200-m-Quadrate, die sich zur Haelfte ueberlappen, genau ueber einer
        // Rasterkante (x = 1000): Vereinigung 300 x 200 = 60 000 m2.
        let b = vec![quadrat(800.0, 0.0, 200.0), quadrat(900.0, 0.0, 200.0)];
        let g = vec![gebiet("alles", rechteck(-5000.0, -5000.0, 5000.0, 5000.0))];
        let e = flaeche_je_gebiet(&b, &g, 1000.0);
        assert!((e.gesamt_m2 - 60_000.0).abs() < 1e-6, "{}", e.gesamt_m2);
        assert!((e.zone_m2[0] - 60_000.0).abs() < 1e-6, "{}", e.zone_m2[0]);
        assert_eq!(e.quadrate, 2);
    }

    #[test]
    fn grenze_teilt_die_flaeche_und_ausserhalb_zaehlt_nur_gesamt() {
        // Ein Band 0..400 x 0..100 ueber die Grenze x = 250 zweier Gebiete, das
        // bei x < 100 ueber beide hinausragt: West 100..250, Ost 250..400.
        let b = vec![rechteck(0.0, 0.0, 400.0, 100.0)];
        let g = vec![
            gebiet("west", rechteck(100.0, -1000.0, 250.0, 1000.0)),
            gebiet("ost", rechteck(250.0, -1000.0, 9000.0, 1000.0)),
        ];
        let e = flaeche_je_gebiet(&b, &g, 5000.0);
        assert!((e.zone_m2[0] - 15_000.0).abs() < 1e-6, "{}", e.zone_m2[0]);
        assert!((e.zone_m2[1] - 15_000.0).abs() < 1e-6, "{}", e.zone_m2[1]);
        assert!((e.gesamt_m2 - 40_000.0).abs() < 1e-6, "{}", e.gesamt_m2);
    }

    #[test]
    fn laedt_je_block_nur_dessen_baender() {
        // Zwei Baender 50 km auseinander (Raster 5 km, Bloecke 20 km): zwei Ladeaufrufe
        // mit je einem Band, nie beide zugleich -- der Speicher folgt dem Block.
        let b = [
            quadrat(1000.0, 1000.0, 100.0),
            quadrat(51_000.0, 1000.0, 100.0),
        ];
        let rahmen: Vec<Option<[f64; 4]>> = b.iter().map(|s| bbox(s)).collect();
        let aufrufe = std::sync::Mutex::new(Vec::new());
        let g = vec![gebiet("alles", rechteck(-1e6, -1e6, 1e6, 1e6))];
        let e = flaeche_je_gebiet_mit(
            &rahmen,
            |idx| {
                aufrufe.lock().unwrap().push(idx.to_vec());
                Ok(idx.iter().map(|&i| b[i].clone()).collect())
            },
            &g,
            5000.0,
        )
        .unwrap();
        assert_eq!(*aufrufe.lock().unwrap(), vec![vec![0], vec![1]]);
        assert!((e.gesamt_m2 - 20_000.0).abs() < 1e-6, "{}", e.gesamt_m2);
    }

    #[test]
    fn gebiet_ueber_blockgrenzen_und_negative_koordinaten() {
        // Ein Gebiet, das mehrere Bloecke (10 x 10 Quadrate zu 100 m) quert, und
        // Baender beidseits des Ursprungs: jedes Band zaehlt genau einmal.
        let b: Vec<Shapes> = (-30..30)
            .map(|i| quadrat(i as f64 * 97.0, i as f64 * 53.0, 50.0))
            .collect();
        let g = vec![gebiet(
            "gross",
            rechteck(-10_000.0, -10_000.0, 10_000.0, 10_000.0),
        )];
        let e = flaeche_je_gebiet(&b, &g, 100.0);
        assert!(
            (e.zone_m2[0] - 60.0 * 2500.0).abs() < 1e-6,
            "{}",
            e.zone_m2[0]
        );
        assert!(
            (e.gesamt_m2 - 60.0 * 2500.0).abs() < 1e-6,
            "{}",
            e.gesamt_m2
        );
    }
}
