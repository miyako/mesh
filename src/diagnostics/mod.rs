//! Diagnostics: the error model, resource limits, and warnings.

pub mod error;
pub mod limits;

pub use error::{Error, ErrorCode, Result};
