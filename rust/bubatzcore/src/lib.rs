//! bubatzcore -- der Geometriekern von bubatzblick.
//!
//! Reine Bibliothek: keine I/O, kein PyO3. Die Python-Bindung liegt im
//! Schwester-Crate `bubatzcore-py`, die Pipeline im Binary `bubatz`.

pub mod bands;
pub mod finish;
pub mod geom;
pub mod profile;
#[cfg(test)]
pub mod reference;
pub mod region;
pub mod scene;
pub mod sweep;
pub mod union;

pub use bands::{decompose_into_bands, free_circle};
pub use finish::{ROUND_STEP_QUAD_SEGS_16, finish_region, offset_shapes, round_step_for_quad_segs};
pub use geom::{Point, Ring, Shape, Shapes};
pub use geom::{orient_shape, orient_shapes};
pub use region::{DEFAULT_CHUNK, visible_region, visible_region_points};
pub use scene::Scene;
pub use sweep::{Workspace, visibility_ring};
pub use union::{to_ogc, union_all, union_cascade};
