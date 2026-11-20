//! Canonical geometry representation shared by all parsers and
//! consumed by the renderer. See `docs/architecture.md`.

pub mod bounds;
pub mod normalize;
pub mod normals;
pub mod triangulate;
pub mod types;

pub use bounds::Bounds3;
pub use types::{Color, Geometry, Mesh, PointCloud, Vec3};
