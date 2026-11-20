//! MeshPreview (`mesh-thumb`) — a small, dependable, headless 3D-to-2D
//! thumbnail engine. Reads STL and PLY geometry and produces PNG previews.
//!
//! The crate is organized as a library core (parsers, geometry, camera,
//! renderer, cache, batch) with a thin CLI layer in the `cli` module.

pub mod cache;
pub mod camera;
pub mod cli;
pub mod diagnostics;
pub mod formats;
pub mod geometry;
pub mod render;
pub mod versions;

pub use diagnostics::error::{Error, ErrorCode, Result};
