//! Reproduktion: i_overlay 8.1.1 `OverlayOptions::ogc()` fuellt bei der
//! Vereinigung sich beruehrender Gebaeude umschlossene Hoefe. Eingabe:
//! scratchpad/ogc_repro.json (Duesseldorfer Gebaeude um zwei Objekte).
use bubatzcore::geom::{Shape, Shapes, shapes_area};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::core::solver::Solver;
use i_overlay::float::overlay::{FloatOverlay, OverlayOptions};
use i_overlay::float::simplify::SimplifyShape;

fn load() -> Vec<(String, f64, Vec<Shape>)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ogc_repro.json.gz");
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap()),
        &mut text,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, o)| {
            let shapes: Vec<Shape> = o["shapes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| {
                    s.as_array()
                        .unwrap()
                        .iter()
                        .map(|r| {
                            r.as_array()
                                .unwrap()
                                .iter()
                                .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
                                .collect()
                        })
                        .collect()
                })
                .collect();
            (k.clone(), o["expected_area"].as_f64().unwrap(), shapes)
        })
        .collect()
}

fn pair(a: &[Shape], b: &[Shape], opt: OverlayOptions<f64, i64>, fill: FillRule) -> Shapes {
    let mut ov = FloatOverlay::<[f64; 2], i64>::from_subj_and_clip_custom(a, b, opt, Solver::AUTO);
    ov.overlay(OverlayRule::Union, fill)
}

fn cascade(shapes: &[Shape], opt: OverlayOptions<f64, i64>, fill: FillRule) -> Shapes {
    let mut level: Vec<Shapes> = shapes.iter().map(|s| vec![s.clone()]).collect();
    while level.len() > 1 {
        let mut next = Vec::new();
        let mut it = level.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(pair(&a, &b, opt, fill)),
                None => next.push(a),
            }
        }
        level = next;
    }
    level.pop().unwrap_or_default()
}

fn one_shot(shapes: &[Shape], opt: OverlayOptions<f64, i64>, fill: FillRule) -> Shapes {
    let mut ov = FloatOverlay::<[f64; 2], i64>::from_subj_custom(shapes, opt, Solver::AUTO);
    ov.overlay(OverlayRule::Subject, fill)
}

/// Dokumentiert den Fehler und prueft den Workaround: Kaskade mit
/// Voreinstellung + `to_ogc`-Schlusspass = GEOS-Flaeche (Gebaeude ueberlappen
/// nicht, also Summe der Einzelflaechen).
#[test]
fn ogc_in_cascade_fills_courtyards_but_final_pass_is_exact() {
    for (name, expected, shapes) in load() {
        let dflt = OverlayOptions::<f64, i64>::default();
        let ogc = OverlayOptions::<f64, i64>::ogc();
        let a_default = cascade(&shapes, dflt, FillRule::NonZero);
        let a_ogc = cascade(&shapes, ogc, FillRule::NonZero);
        let a_ogc_pos = cascade(&shapes, ogc, FillRule::Positive);
        let a_one_ogc = one_shot(&shapes, ogc, FillRule::NonZero);
        let a_one_dflt = one_shot(&shapes, dflt, FillRule::NonZero);
        // OGC nur als letzter Schritt auf dem sauberen Default-Ergebnis:
        let final_pass =
            a_default.simplify_shape_custom_as::<i64>(FillRule::NonZero, ogc, Solver::AUTO);
        let workaround = shapes_area(&bubatzcore::to_ogc(&a_default));
        assert!(
            (shapes_area(&a_default) - expected).abs() < 1e-3,
            "{name}: Kaskade default {}",
            shapes_area(&a_default)
        );
        assert!(
            (workaround - expected).abs() < 1e-3,
            "{name}: Workaround {workaround}"
        );
        assert!(
            (shapes_area(&a_one_ogc) - expected).abs() < 1e-3,
            "{name}: Einmal-Overlay ogc"
        );
        // Der Fehler selbst (informativ, nicht verlangt -- eine korrigierte
        // i_overlay-Version darf hier gleich werden):
        if (shapes_area(&a_ogc) - expected).abs() > 1.0 {
            eprintln!(
                "{name}: i_overlay ogc() in der Kaskade weicht ab: {:.1} statt {expected:.1}",
                shapes_area(&a_ogc)
            );
        }
        println!(
            "{name}: GEOS {expected:.1} | Kaskade default {:.1} | Kaskade ogc {:.1} | Kaskade ogc+Positive {:.1} | Einmal-Overlay default {:.1} | Einmal-Overlay ogc {:.1} | default + OGC-Schlusspass {:.1} (Shapes {} -> {})",
            shapes_area(&a_default),
            shapes_area(&a_ogc),
            shapes_area(&a_ogc_pos),
            shapes_area(&a_one_dflt),
            shapes_area(&a_one_ogc),
            shapes_area(&final_pass),
            a_default.len(),
            final_pass.len()
        );
    }
}
