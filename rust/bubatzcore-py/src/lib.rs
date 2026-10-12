//! Python-Modul `bubatzcore`: duenne PyO3-Huelle um den Kern `bubatzcore`
//! (Tests und Orakel der Python-Gegenprobe).
//!
//! Eingabe: Liste von Polygonen, je Polygon eine Liste von Ringen
//! (Aussenring zuerst) als (n, 2)-float64-Arrays, offen, konsistent
//! orientiert. Ausgabe: Liste von Shapes, je Shape eine Liste von Ringen
//! (Aussenring zuerst, dann Loecher) als (n, 2)-Arrays.
//!
//! Bauen: `uv run maturin develop --release -m rust/bubatzcore-py/Cargo.toml`.

use bubatzcore::{Ring, Shape};
use numpy::{IntoPyArray, PyArray1, PyArray2, PyArrayMethods, PyReadonlyArray1, PyReadonlyArray2};
use pyo3::prelude::*;

/// Shape-Liste auf der Python-Seite: je Shape eine Liste von (n, 2)-Arrays.
type PyShapes<'py> = Vec<Vec<Bound<'py, PyArray2<f64>>>>;

fn ring_from_array(a: &PyReadonlyArray2<'_, f64>) -> Ring {
    let v = a.as_array();
    (0..v.nrows()).map(|i| [v[[i, 0]], v[[i, 1]]]).collect()
}

fn shapes_from_python(polygons: &[Vec<PyReadonlyArray2<'_, f64>>]) -> Vec<Shape> {
    polygons
        .iter()
        .map(|rings| rings.iter().map(ring_from_array).collect())
        .collect()
}

fn ring_to_array<'py>(py: Python<'py>, ring: &[[f64; 2]]) -> Bound<'py, PyArray2<f64>> {
    let flat: Vec<f64> = ring.iter().flat_map(|p| [p[0], p[1]]).collect();
    let n = ring.len();
    flat.into_pyarray(py)
        .reshape([n, 2])
        .expect("reshape of (n*2,) into (n, 2)")
}

fn shapes_to_python<'py>(py: Python<'py>, shapes: &[Shape]) -> PyShapes<'py> {
    shapes
        .iter()
        .map(|shape| shape.iter().map(|ring| ring_to_array(py, ring)).collect())
        .collect()
}

/// Vereinigung aller Eingabepolygone in EINEM Overlay (NonZero-Fuellung).
#[pyfunction]
#[pyo3(signature = (polygons, use_i64 = false))]
fn union_all<'py>(
    py: Python<'py>,
    polygons: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    use_i64: bool,
) -> PyResult<PyShapes<'py>> {
    let shapes = shapes_from_python(&polygons);
    let result = py.detach(|| bubatzcore::union_all(&shapes, use_i64));
    Ok(shapes_to_python(py, &result))
}

/// Kaskadierte Vereinigung (paarweise entlang der Eingabereihenfolge, dann
/// Paare der Ergebnisse), `i64`-Kern; Ergebnis OGC-gueltig (`to_ogc`).
#[pyfunction]
fn union_cascade<'py>(
    py: Python<'py>,
    polygons: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
) -> PyResult<PyShapes<'py>> {
    let shapes = shapes_from_python(&polygons);
    let result = py.detach(|| bubatzcore::to_ogc(&bubatzcore::union_cascade(shapes)));
    Ok(shapes_to_python(py, &result))
}

/// Versatz um `margin` nach aussen (Aussenringe wachsen, Loecher schrumpfen),
/// runde Ecken mit Sehnenschritt `round_step` = L / R (Shapely `quad_segs=16`
/// entspricht `ROUND_STEP_QUAD_SEGS_16`).
#[pyfunction]
#[pyo3(signature = (shapes, margin, round_step = bubatzcore::ROUND_STEP_QUAD_SEGS_16))]
fn offset_shapes<'py>(
    py: Python<'py>,
    shapes: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    margin: f64,
    round_step: f64,
) -> PyResult<PyShapes<'py>> {
    let shapes = shapes_from_python(&shapes);
    let result = py.detach(|| bubatzcore::offset_shapes(&shapes, margin, round_step));
    Ok(shapes_to_python(py, &result))
}

/// Abschluss einer Sichtzone: `(region (+) margin) U source`, geschnitten mit
/// `free`. Alle drei Eingaben sind Shape-Listen.
#[pyfunction]
#[pyo3(signature = (region, source, free, margin, round_step = bubatzcore::ROUND_STEP_QUAD_SEGS_16))]
fn finish_region<'py>(
    py: Python<'py>,
    region: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    source: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    free: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    margin: f64,
    round_step: f64,
) -> PyResult<PyShapes<'py>> {
    let region = shapes_from_python(&region);
    let source = shapes_from_python(&source);
    let free = shapes_from_python(&free);
    let result =
        py.detach(|| bubatzcore::finish_region(&region, &source, &free, margin, round_step));
    Ok(shapes_to_python(py, &result))
}

/// Hindernisszene eines Objekts: Ringe einmal aufbereiten,
/// dann je Beobachterpunkt den Sichtbarkeitsring rechnen; das Polygon baut
/// die Python-Seite.
#[pyclass(name = "Scene", frozen)]
struct PyScene {
    inner: bubatzcore::Scene,
    ws: std::sync::Mutex<bubatzcore::Workspace>,
}

#[pymethods]
impl PyScene {
    #[new]
    #[pyo3(signature = (rings, reach, max_len=None))]
    fn new(rings: Vec<PyReadonlyArray2<'_, f64>>, reach: f64, max_len: Option<f64>) -> Self {
        let rings: Vec<Ring> = rings.iter().map(ring_from_array).collect();
        PyScene {
            inner: bubatzcore::Scene::from_ring_vecs(&rings, reach)
                .with_max_len(max_len.unwrap_or(reach)),
            ws: std::sync::Mutex::new(bubatzcore::Workspace::new()),
        }
    }

    #[getter]
    fn n_segments(&self) -> usize {
        self.inner.n_segments()
    }

    #[getter]
    fn reach(&self) -> f64 {
        self.inner.reach
    }

    /// Sichtbarkeitsring von (ox, oy) als (n, 2)-Array (offen).
    #[pyo3(signature = (ox, oy, drop_collinear = true))]
    fn ring<'py>(
        &self,
        py: Python<'py>,
        ox: f64,
        oy: f64,
        drop_collinear: bool,
    ) -> Bound<'py, PyArray2<f64>> {
        let ring = py.detach(|| {
            let mut ws = self.ws.lock().expect("Workspace-Mutex");
            bubatzcore::visibility_ring(&self.inner, [ox, oy], drop_collinear, &mut ws)
        });
        ring_to_array(py, &ring)
    }

    /// Trefferdistanz je Winkel (sortiert, in (-pi, pi]); `max_len`, wo nichts
    /// getroffen wird -- fuer den Vergleich mit `VisibilityScene.hit_distances`.
    fn hit_distances<'py>(
        &self,
        py: Python<'py>,
        ox: f64,
        oy: f64,
        angles: PyReadonlyArray1<'py, f64>,
        max_len: f64,
    ) -> Bound<'py, PyArray1<f64>> {
        let angles: Vec<f64> = angles
            .as_slice()
            .expect("zusammenhaengendes Array")
            .to_vec();
        let best = py.detach(|| {
            let mut ws = self.ws.lock().expect("Workspace-Mutex");
            bubatzcore::sweep::hit_distances_culled(
                &self.inner,
                [ox, oy],
                &angles,
                max_len,
                &mut ws,
            );
            ws.best().to_vec()
        });
        best.into_pyarray(py)
    }
}

/// Vereinigung der Sichtbarkeitsringe aller Punkte gegen die Szene.
#[pyfunction]
#[pyo3(signature = (scene, points, chunk = bubatzcore::DEFAULT_CHUNK))]
fn visible_region_points<'py>(
    py: Python<'py>,
    scene: &PyScene,
    points: PyReadonlyArray2<'py, f64>,
    chunk: usize,
) -> PyResult<PyShapes<'py>> {
    let points = ring_from_array(&points);
    let result = py.detach(|| bubatzcore::visible_region_points(&scene.inner, &points, chunk));
    Ok(shapes_to_python(py, &result))
}

/// Die ganze Zone in einem Aufruf: Sweep aller Punkte, Kaskade,
/// Abschluss. `obstacle_rings` wie fuer `Scene`, `source`/`free` als
/// Shape-Listen, `margin <= 0` ohne Marge.
#[pyfunction]
#[pyo3(signature = (obstacle_rings, reach, max_len, points, source, free, margin, round_step = bubatzcore::ROUND_STEP_QUAD_SEGS_16, chunk = bubatzcore::DEFAULT_CHUNK))]
#[allow(clippy::too_many_arguments)]
fn visible_region<'py>(
    py: Python<'py>,
    obstacle_rings: Vec<PyReadonlyArray2<'py, f64>>,
    reach: f64,
    max_len: f64,
    points: PyReadonlyArray2<'py, f64>,
    source: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    free: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    margin: f64,
    round_step: f64,
    chunk: usize,
) -> PyResult<PyShapes<'py>> {
    let rings: Vec<Ring> = obstacle_rings.iter().map(ring_from_array).collect();
    let points = ring_from_array(&points);
    let source = shapes_from_python(&source);
    let free = shapes_from_python(&free);
    let result = py.detach(|| {
        bubatzcore::visible_region(
            &rings, reach, max_len, &points, &source, &free, margin, round_step, chunk,
        )
    });
    Ok(shapes_to_python(py, &result))
}

/// Kreis-Puffer `D(source, r)` mit 512 Ecken je Vollkreis (Shapely `quad_segs=128`).
#[pyfunction]
fn free_circle<'py>(
    py: Python<'py>,
    source: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    r: f64,
) -> PyResult<PyShapes<'py>> {
    let source = shapes_from_python(&source);
    let result = py.detach(|| bubatzcore::free_circle(&source, r));
    Ok(shapes_to_python(py, &result))
}

/// Ringzerlegung: je Radius (aufsteigend) `v_hat ∩ (D(source, r_k) \ D(source, r_{k-1}))`.
#[pyfunction]
fn decompose_into_bands<'py>(
    py: Python<'py>,
    v_hat: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    source: Vec<Vec<PyReadonlyArray2<'py, f64>>>,
    radii: Vec<f64>,
) -> PyResult<Vec<PyShapes<'py>>> {
    let v_hat = shapes_from_python(&v_hat);
    let source = shapes_from_python(&source);
    let result = py.detach(|| bubatzcore::decompose_into_bands(&v_hat, &source, &radii));
    Ok(result
        .iter()
        .map(|band| shapes_to_python(py, band))
        .collect())
}

#[pymodule]
#[pyo3(name = "bubatzcore")]
fn bubatzcore_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(union_all, m)?)?;
    m.add_function(wrap_pyfunction!(union_cascade, m)?)?;
    m.add_function(wrap_pyfunction!(offset_shapes, m)?)?;
    m.add_function(wrap_pyfunction!(finish_region, m)?)?;
    m.add(
        "ROUND_STEP_QUAD_SEGS_16",
        bubatzcore::ROUND_STEP_QUAD_SEGS_16,
    )?;
    m.add_class::<PyScene>()?;
    m.add_function(wrap_pyfunction!(visible_region_points, m)?)?;
    m.add_function(wrap_pyfunction!(visible_region, m)?)?;
    m.add_function(wrap_pyfunction!(free_circle, m)?)?;
    m.add_function(wrap_pyfunction!(decompose_into_bands, m)?)?;
    Ok(())
}
