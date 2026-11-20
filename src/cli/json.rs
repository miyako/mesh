//! JSON protocol response types (spec §17/§20). Field names are stable;
//! `JSON_SCHEMA_VERSION` gates incompatibilities (docs/protocol.md).

use std::path::Path;

use serde::Serialize;

use crate::diagnostics::error::Error;
use crate::formats::detect::DetectedFormat;
use crate::geometry::bounds::Bounds3;
use crate::geometry::types::ParseWarnings;
use crate::versions::JSON_SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InputInfo {
    pub path: String,
    pub format: String,
    pub file_size: u64,
}

impl InputInfo {
    pub fn from_loaded(path: &Path, format: &DetectedFormat, file_size: u64) -> InputInfo {
        InputInfo {
            path: path.to_string_lossy().into_owned(),
            format: format.id.to_string(),
            file_size,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct GeometrySummary {
    pub kind: String, // "mesh" | "point_cloud"
    pub vertices: usize,
    pub triangles: usize,
    pub colors: bool,
    pub normals: bool,
    pub bounds: BoundsJson,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BoundsJson {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub valid: bool,
}

impl From<&Bounds3> for BoundsJson {
    fn from(b: &Bounds3) -> Self {
        BoundsJson {
            min: [f64::from(b.min.x), f64::from(b.min.y), f64::from(b.min.z)],
            max: [f64::from(b.max.x), f64::from(b.max.y), f64::from(b.max.z)],
            valid: b.valid,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct WarningsJson {
    pub degenerate_triangles: usize,
    pub non_finite_vertices: usize,
}

impl From<&ParseWarnings> for WarningsJson {
    fn from(w: &ParseWarnings) -> Self {
        WarningsJson {
            degenerate_triangles: w.degenerate_triangles,
            non_finite_vertices: w.non_finite_vertices,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ErrorJson {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub stage: Option<String>,
}

impl From<&Error> for ErrorJson {
    fn from(e: &Error) -> Self {
        ErrorJson {
            code: e.code.as_str().to_string(),
            message: e.message.clone(),
            path: e.input.as_ref().map(|p| p.to_string_lossy().into_owned()),
            stage: e.stage.map(|s| s.to_string()),
        }
    }
}

/// Envelope for *any* top-level response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Response {
    pub schema_version: u32,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorJson>,
}

impl Response {
    pub fn ok(result: impl Serialize) -> Response {
        Response {
            schema_version: JSON_SCHEMA_VERSION,
            success: true,
            result: Some(serde_json::to_value(result).expect("response serialization")),
            error: None,
        }
    }

    pub fn err(error: &Error) -> Response {
        Response {
            schema_version: JSON_SCHEMA_VERSION,
            success: false,
            result: None,
            error: Some(ErrorJson::from(error)),
        }
    }
}
