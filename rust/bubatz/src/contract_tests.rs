//! Der Vertrag der Zone eines Objekts (`zones::visible_zone`, Parameterversion
//! v7) auf den eingefrorenen Fixtures `tests/fixtures/duesseldorf_regbez_*.parquet`
//! (Spalte `role`: eine "source"-Zeile, sonst "obstacle"; Hindernisse bereits
//! verschmolzen). Die einzige Python-Gegenprobe ist der Sweep gegen
//! `sampling_ref.py` (`tests/test_rust_vs_reference.py`).
//!
//! Fixtures: `mittel`, `gross` (zwei youth-Objekte, Golden Master), `anker`
//! (oeffentlicher Spielplatz Duesseldorf-Stadtmitte, osm_id 29359108, Golden
//! Master unten), `schluesselloch` (osm_id 437966337,
//! 6.852 m Umfang -- nur im `#[ignore]`-Test, `cargo test --release -- --ignored`).

use std::path::PathBuf;

use bubatzcore::geom::{Shape, Shapes, shapes_area};
use bubatzcore::union::overlay_pair;
use i_overlay::core::overlay_rule::OverlayRule;

use crate::config::{self, Config};
use crate::zones::visible_zone;

const FAST: [&str; 3] = ["mittel", "gross", "anker"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cfg() -> Config {
    config::load(&root().join("config/pipeline.toml")).expect("config/pipeline.toml")
}

/// (Quelle, Hindernisse) eines Fixtures.
fn fixture(name: &str) -> (Shapes, Vec<Shape>) {
    use arrow::array::Array;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let path = root().join(format!("tests/fixtures/duesseldorf_regbez_{name}.parquet"));
    let f = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let reader = ParquetRecordBatchReaderBuilder::try_new(f)
        .unwrap()
        .build()
        .unwrap();
    let mut source: Option<Shapes> = None;
    let mut obstacles: Vec<Shape> = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let role = batch.column_by_name("role").unwrap().clone();
        let geo = batch.column_by_name("geometry").unwrap().clone();
        for i in 0..batch.num_rows() {
            if geo.is_null(i) {
                continue;
            }
            let shapes =
                crate::wkb::parse(&crate::parquet_io::binary_at(&geo, i).unwrap()).unwrap();
            if crate::parquet_io::string_at(&role, i).unwrap() == "source" {
                assert!(source.is_none(), "{name}: mehr als eine Quelle");
                source = Some(shapes);
            } else {
                obstacles.extend(shapes);
            }
        }
    }
    (source.expect("Quelle"), obstacles)
}

fn bounds(shapes: &[Shape]) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in shapes.iter().flatten().flatten() {
        b = [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ];
    }
    b
}

fn zone(cfg: &Config, source: &[Shape], obstacles: &[Shape]) -> Shapes {
    visible_zone(cfg, source, bounds(source), obstacles)
}

fn area_outside(a: &[Shape], b: &[Shape]) -> f64 {
    shapes_area(&overlay_pair(a, b, OverlayRule::Difference))
}

fn sym_diff_area(a: &[Shape], b: &[Shape]) -> f64 {
    area_outside(a, b) + area_outside(b, a)
}

fn free(cfg: &Config, source: &[Shape]) -> Shapes {
    bubatzcore::free_circle(source, cfg.effective_reach_m())
}

fn square(c: [f64; 2], half: f64) -> Shape {
    vec![vec![
        [c[0] - half, c[1] - half],
        [c[0] + half, c[1] - half],
        [c[0] + half, c[1] + half],
        [c[0] - half, c[1] + half],
    ]]
}

fn transform(shapes: &[Shape], f: &dyn Fn([f64; 2]) -> [f64; 2]) -> Shapes {
    shapes
        .iter()
        .map(|s| {
            s.iter()
                .map(|r| r.iter().map(|&p| f(p)).collect())
                .collect()
        })
        .collect()
}

// --- P1/P8: Quelle drin, im Freikreis, nicht leer, konsistent ---------------------

#[test]
fn zone_contains_source_and_stays_inside_the_free_circle() {
    let cfg = cfg();
    for name in FAST {
        let (source, obstacles) = fixture(name);
        let z = zone(&cfg, &source, &obstacles);
        assert!(!z.is_empty(), "{name}: leer");
        assert!(
            shapes_area(&z) > shapes_area(&source),
            "{name}: nicht groesser als die Quelle"
        );
        // Toleranzen wie im Python-Vertrag (GEOS_TOL): Overlay-Rundung im
        // Mikrometerbereich, nicht 0.0.
        let src_out = area_outside(&source, &z);
        assert!(
            src_out < 1e-3,
            "{name}: Quelle ragt {src_out:.2e} m2 heraus"
        );
        let reach_out = area_outside(&z, &free(&cfg, &source));
        assert!(
            reach_out < 1e-3,
            "{name}: {reach_out:.2e} m2 ueber die Reichweite hinaus"
        );
        for p in z.iter().flatten().flatten() {
            assert!(
                p[0].is_finite() && p[1].is_finite(),
                "{name}: Koordinate nicht endlich"
            );
        }
        // Gueltigkeit: der OGC-Schlusspass aendert eine gueltige Flaeche nicht.
        let ogc = bubatzcore::to_ogc(&z);
        assert!(
            (shapes_area(&ogc) - shapes_area(&z)).abs() < 1e-6 * shapes_area(&z),
            "{name}: OGC-Pass veraendert die Flaeche"
        );
    }
}

// --- P2: die Sicherheitszusicherung des Verfahrens -----------------------------------

#[test]
fn zone_covers_four_times_denser_sampling() {
    // Das Ergebnis bei Produktionsdichte (0,5 m + Marge) deckt die Vereinigung
    // der Punktpolygone bei vierfacher Dichte OHNE Marge ab. Schranke relativ
    // (1e-4), weil dichtere Punkte eine NEUE Punktmenge sind, keine Verfeinerung.
    let cfg = cfg();
    for name in ["mittel", "gross"] {
        let (source, obstacles) = fixture(name);
        let z = zone(&cfg, &source, &obstacles);
        let dense_points = crate::sample::sample_contour(&source, 0.125, 128, 4096);
        let rings: Vec<Vec<[f64; 2]>> = obstacles.iter().flatten().cloned().collect();
        let b = bounds(&source);
        let reach = cfg.effective_reach_m();
        let dense = bubatzcore::visible_region(
            &rings,
            reach,
            reach + (b[2] - b[0]).hypot(b[3] - b[1]),
            &dense_points,
            &source,
            &free(&cfg, &source),
            0.0,
            bubatzcore::ROUND_STEP_QUAD_SEGS_16,
            bubatzcore::DEFAULT_CHUNK,
        );
        let missed = area_outside(&dense, &z);
        let limit = (1e-4 * shapes_area(&dense)).max(1.0);
        assert!(
            missed < limit,
            "{name}: {missed:.3} m2 der dichteren Rechnung fehlen (Grenze {limit:.1})"
        );
    }
}

#[test]
fn margin_enlarges_and_zero_margin_does_not() {
    let cfg = cfg();
    let mut cfg0 = cfg.clone();
    cfg0.visibility.margin_m = 0.0;
    let (source, obstacles) = fixture("mittel");
    let z = zone(&cfg, &source, &obstacles);
    let z0 = zone(&cfg0, &source, &obstacles);
    assert!(
        area_outside(&z0, &z) < 1e-6,
        "ohne Marge ragt etwas ueber die Zone mit Marge hinaus"
    );
    assert!(
        shapes_area(&z) > shapes_area(&z0) + 1.0,
        "die Marge vergroessert nicht"
    );
}

// --- P6: monoton in den Hindernissen -----------------------------------------------

#[test]
fn more_obstacles_never_enlarge_the_zone() {
    let cfg = cfg();
    for name in ["mittel", "anker"] {
        let (source, obstacles) = fixture(name);
        let z_less = zone(&cfg, &source, &obstacles);
        // Eine 6 x 6 m grosse Wand an einer Stelle, die Z als sichtbar ausweist
        // (40-80 m vom Quellzentrum), ausserhalb von Quelle und Hindernissen.
        let b = bounds(&source);
        let c = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
        let mut wall: Option<Shape> = None;
        'search: for ring in z_less.iter().flatten() {
            for &v in ring {
                let d = (v[0] - c[0]).hypot(v[1] - c[1]);
                if !(40.0..=80.0).contains(&d) {
                    continue;
                }
                // 4 m nach innen (zur Quelle hin), damit die Wand in Z liegt
                let inward = [
                    c[0] + (v[0] - c[0]) * (d - 4.0) / d,
                    c[1] + (v[1] - c[1]) * (d - 4.0) / d,
                ];
                let w = square(inward, 3.0);
                let ws = std::slice::from_ref(&w);
                let inside = shapes_area(&overlay_pair(ws, &z_less, OverlayRule::Intersect));
                if inside > 30.0
                    && shapes_area(&overlay_pair(ws, &source, OverlayRule::Intersect)) == 0.0
                    && shapes_area(&overlay_pair(ws, &obstacles, OverlayRule::Intersect)) == 0.0
                {
                    wall = Some(w);
                    break 'search;
                }
            }
        }
        let wall =
            wall.unwrap_or_else(|| panic!("{name}: keine Stelle fuer die Testwand gefunden"));
        let mut more = obstacles.clone();
        more.push(wall);
        let z_more = zone(&cfg, &source, &more);
        let grown = area_outside(&z_more, &z_less);
        assert!(
            grown < 1e-2,
            "{name}: Z ist mit einem Hindernis mehr um {grown:.2e} m2 gewachsen"
        );
        assert!(
            shapes_area(&z_more) < shapes_area(&z_less) - 1.0,
            "{name}: die Wand wirft keinen Schatten"
        );
    }
}

// --- P7: deterministisch -------------------------------------------------------------

#[test]
fn zone_is_deterministic_to_the_byte() {
    let cfg = cfg();
    for name in ["mittel", "anker"] {
        let (source, obstacles) = fixture(name);
        let a = crate::wkb::write(&zone(&cfg, &source, &obstacles));
        let b = crate::wkb::write(&zone(&cfg, &source, &obstacles));
        assert_eq!(a, b, "{name}");
    }
}

// --- P9: invariant unter Verschiebung und Drehung -------------------------------------

#[test]
fn zone_is_invariant_under_rigid_motion() {
    // Drehung aendert Z nur um die Bogen-Diskretisierung (Stuetzpunkte der
    // Kreisboegen liegen in absoluten Winkeln fest): relativ 1e-4; reine
    // Verschiebung bleibt unter 0,01 m2.
    let cfg = cfg();
    let (source, obstacles) = fixture("mittel");
    let z = zone(&cfg, &source, &obstacles);
    let b = bounds(&source);
    let c = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    let cases: [(f64, f64, f64); 3] = [
        (1000.0, -2500.0, 0.0),
        (0.0, 0.0, 90.0),
        (-300.0, 700.0, 37.0),
    ];
    for (dx, dy, deg) in cases {
        let (s, co) = (deg.to_radians().sin(), deg.to_radians().cos());
        let fwd = move |p: [f64; 2]| -> [f64; 2] {
            let (x, y) = (p[0] - c[0], p[1] - c[1]);
            [c[0] + x * co - y * s + dx, c[1] + x * s + y * co + dy]
        };
        let back = move |p: [f64; 2]| -> [f64; 2] {
            let (x, y) = (p[0] - dx - c[0], p[1] - dy - c[1]);
            [c[0] + x * co + y * s, c[1] - x * s + y * co]
        };
        let moved = zone(
            &cfg,
            &transform(&source, &fwd),
            &transform(&obstacles, &fwd),
        );
        let diff = sym_diff_area(&z, &transform(&moved, &back));
        let limit = if deg == 0.0 {
            0.01
        } else {
            1e-4 * shapes_area(&z)
        };
        assert!(
            diff < limit,
            "({dx}, {dy}, {deg} Grad): symmetrische Differenz {diff:.4} m2"
        );
    }
}

// --- Ankerobjekt als Golden Master ---------------------------------------------------
// Oeffentlicher Spielplatz in Duesseldorf-Stadtmitte (osm_id 29359108, 1.058 m2,
// 140 m Umfang), dichte Blockrandbebauung: die Zone ist ein Stern aus
// Strassenkorridoren. Sollwerte aus einem Bundeslauf
// (`bubatz verify anchor`: 8.461 m2 bis 100 m).
const ANCHOR_POINTS: usize = 281;
const ANCHOR_M2: f64 = 8461.0;

#[test]
fn anchor_object_has_expected_observer_points_and_area_within_100_m() {
    let cfg = cfg();
    let (source, obstacles) = fixture("anker");
    let v = &cfg.visibility;
    let n = crate::sample::sample_contour(&source, v.spacing_m, v.min_points, v.max_points).len();
    assert_eq!(n, ANCHOR_POINTS, "Anker: {n} Beobachterpunkte");
    let z = zone(&cfg, &source, &obstacles);
    let bands = bubatzcore::decompose_into_bands(&z, &source, &[100.0]);
    let area = shapes_area(&bands[0]);
    assert!(
        (area - ANCHOR_M2).abs() <= 1.5,
        "Anker: {area:.1} m2 statt {ANCHOR_M2} m2"
    );
}
// --- Schluesselloch: der Fall, an dem der Deckel 4096 scheiterte (langsam) -----------

#[test]
#[ignore = "13.704 Punkte; cargo test --release -- --ignored"]
fn keyhole_object_covers_denser_sampling_at_production_spacing() {
    let cfg = cfg();
    let (source, obstacles) = fixture("schluesselloch");
    let z = zone(&cfg, &source, &obstacles);
    let v = &cfg.visibility;
    let dense_points =
        crate::sample::sample_contour(&source, v.spacing_m / 2.0, v.min_points, 2 * v.max_points);
    let rings: Vec<Vec<[f64; 2]>> = obstacles.iter().flatten().cloned().collect();
    let b = bounds(&source);
    let reach = cfg.effective_reach_m();
    let dense = bubatzcore::visible_region(
        &rings,
        reach,
        reach + (b[2] - b[0]).hypot(b[3] - b[1]),
        &dense_points,
        &source,
        &free(&cfg, &source),
        0.0,
        bubatzcore::ROUND_STEP_QUAD_SEGS_16,
        bubatzcore::DEFAULT_CHUNK,
    );
    let missed = area_outside(&dense, &z);
    assert!(
        missed < (1e-4 * shapes_area(&dense)).max(1.0),
        "{missed:.1} m2 fehlen"
    );
}
