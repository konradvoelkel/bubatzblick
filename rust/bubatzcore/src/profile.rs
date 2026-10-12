//! Grobe Phasenzeiten (CPU-Zeit ueber alle Threads summiert) fuer die
//! Laufzeitanalyse von `zones`; Ausgabe mit `BUBATZ_PROFILE=1`.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub static SWEEP_NS: AtomicU64 = AtomicU64::new(0);
pub static CASCADE_NS: AtomicU64 = AtomicU64::new(0);
pub static FINISH_NS: AtomicU64 = AtomicU64::new(0);
pub static BANDS_NS: AtomicU64 = AtomicU64::new(0);
pub static FREE_NS: AtomicU64 = AtomicU64::new(0);
pub static SCENE_NS: AtomicU64 = AtomicU64::new(0);
pub static SWEEP_POINTS: AtomicU64 = AtomicU64::new(0);
pub static SWEEP_CAND_NS: AtomicU64 = AtomicU64::new(0);
pub static SWEEP_HIT_NS: AtomicU64 = AtomicU64::new(0);
pub static SWEEP_RING_NS: AtomicU64 = AtomicU64::new(0);
/// Summe der Kanten je Szene ueber alle Punkte (Mittel = / SWEEP_POINTS)
pub static SWEEP_SEGS: AtomicU64 = AtomicU64::new(0);
/// Summe der Kandidatenwinkel ueber alle Punkte
pub static SWEEP_ANGLES: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn add(counter: &AtomicU64, t0: Instant) {
    counter.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

pub fn snapshot() -> Vec<(&'static str, f64)> {
    let s = |c: &AtomicU64| c.load(Ordering::Relaxed) as f64 / 1e9;
    vec![
        ("scene", s(&SCENE_NS)),
        ("sweep", s(&SWEEP_NS)),
        ("  sweep/candidate_angles", s(&SWEEP_CAND_NS)),
        ("  sweep/hit_distances", s(&SWEEP_HIT_NS)),
        ("  sweep/ring+collinear", s(&SWEEP_RING_NS)),
        ("cascade", s(&CASCADE_NS)),
        ("finish", s(&FINISH_NS)),
        ("free_circle", s(&FREE_NS)),
        ("bands", s(&BANDS_NS)),
    ]
}

pub fn sweep_points() -> u64 {
    SWEEP_POINTS.load(Ordering::Relaxed)
}

pub fn sweep_means() -> (f64, f64) {
    let n = SWEEP_POINTS.load(Ordering::Relaxed).max(1) as f64;
    (
        SWEEP_SEGS.load(Ordering::Relaxed) as f64 / n,
        SWEEP_ANGLES.load(Ordering::Relaxed) as f64 / n,
    )
}
