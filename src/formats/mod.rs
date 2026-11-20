//! Input format detection. Detection must not trust file extensions;
//! contents decide, with extensions used only as a tie-breaker.

pub mod detect;
pub mod load;
pub mod ply;
pub mod stl;

pub use detect::{detect_format, DetectedFormat, FormatKind, FormatVariant};
