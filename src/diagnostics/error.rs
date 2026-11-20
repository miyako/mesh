//! Error model with stable, machine-readable error codes.
//!
//! Every error carries a stable `ErrorCode` (used in the JSON protocol),
//! a human-readable message, and optional context (input path, parser
//! stage). The CLI maps errors to exit codes and JSON responses.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

/// Stable error categories. These strings are part of the JSON protocol
/// and must not change once released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    IoError,
    UnsupportedFormat,
    InvalidStl,
    InvalidPlyHeader,
    InvalidPlyData,
    InvalidGeometry,
    ResourceLimitExceeded,
    InvalidRenderOption,
    CacheError,
    PngEncodeError,
    BackendUnavailable,
    OutputError,
    UsageError,
    BatchError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::IoError => "io_error",
            ErrorCode::UnsupportedFormat => "unsupported_format",
            ErrorCode::InvalidStl => "invalid_stl",
            ErrorCode::InvalidPlyHeader => "invalid_ply_header",
            ErrorCode::InvalidPlyData => "invalid_ply_data",
            ErrorCode::InvalidGeometry => "invalid_geometry",
            ErrorCode::ResourceLimitExceeded => "resource_limit_exceeded",
            ErrorCode::InvalidRenderOption => "invalid_render_option",
            ErrorCode::CacheError => "cache_error",
            ErrorCode::PngEncodeError => "png_encode_error",
            ErrorCode::BackendUnavailable => "backend_unavailable",
            ErrorCode::OutputError => "output_error",
            ErrorCode::UsageError => "usage_error",
            ErrorCode::BatchError => "batch_error",
        }
    }
}

/// A user-facing error with a stable code and optional context.
#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub input: Option<PathBuf>,
    pub stage: Option<&'static str>,
    /// Fatal vs. per-item skip. A batch should skip items with
    /// `fatal == false` and continue; fatal errors abort the batch.
    pub fatal: bool,
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            input: None,
            stage: None,
            fatal: false,
        }
    }

    pub fn fatal(mut self) -> Self {
        self.fatal = true;
        self
    }

    pub fn with_input(mut self, path: &Path) -> Self {
        if self.input.is_none() {
            self.input = Some(path.to_path_buf());
        }
        self
    }

    pub fn with_stage(mut self, stage: &'static str) -> Self {
        self.stage = Some(stage);
        self
    }

    /// Append parser-stage context to an existing error.
    pub fn contextualize(mut self, stage: &'static str, detail: impl AsRef<str>) -> Self {
        if self.stage.is_none() {
            self.stage = Some(stage);
        }
        self.message = format!("{} ({})", self.message, detail.as_ref());
        self
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Helper constructors for common error shapes.
impl Error {
    pub fn io(path: &Path, source: &std::io::Error) -> Self {
        Error::new(ErrorCode::IoError, format!("{source}")).with_input(path)
    }

    pub fn unsupported_format(path: &Path, detail: impl Into<String>) -> Self {
        Error::new(
            ErrorCode::UnsupportedFormat,
            format!(
                "unsupported or undetectable input format: {}",
                detail.into()
            ),
        )
        .with_input(path)
    }

    pub fn invalid_geometry(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::InvalidGeometry, message).with_stage("geometry")
    }

    pub fn invalid_render_option(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::InvalidRenderOption, message).with_stage("render")
    }

    pub fn backend_unavailable(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::BackendUnavailable, message).with_stage("render")
    }

    pub fn resource_limit(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::ResourceLimitExceeded, message).with_stage("limits")
    }
}

/// Context for diagnostics that identify where in a pipeline an error
/// occurred without leaking internal paths unnecessarily.
#[derive(Debug, Clone, Serialize)]
pub struct ErrorReport {
    pub code: String,
    pub message: String,
}

impl From<&Error> for ErrorReport {
    fn from(e: &Error) -> Self {
        ErrorReport {
            code: e.code.as_str().to_string(),
            message: e.message.clone(),
        }
    }
}

impl fmt::Display for ErrorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
