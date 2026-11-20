//! `capabilities` subcommand: what this build supports.

use serde::Serialize;

use crate::camera::presets::ALL_PRESETS;
use crate::cli::args::CapabilitiesArgs;
use crate::cli::json::Response;
use crate::diagnostics::error::Result;
use crate::diagnostics::limits;
use crate::render::cpu::Backend;
use crate::versions::APP_VERSION;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CapabilitiesResult {
    pub app: &'static str,
    pub version: &'static str,
    pub formats: Vec<&'static str>,
    pub views: Vec<&'static str>,
    pub backends: Vec<&'static str>,
    pub max_output_dimension: u32,
    pub max_output_pixels: u64,
    pub max_quality: u32,
    pub max_input_bytes: u64,
    pub gpu_backend_available: bool,
}

/// Run `capabilities`. Returns the JSON response when `--json`.
pub fn execute(args: &CapabilitiesArgs) -> Result<Option<Response>> {
    let result = CapabilitiesResult {
        app: "mesh-thumb",
        version: APP_VERSION,
        formats: vec!["stl", "ply"],
        views: ALL_PRESETS.iter().map(|v| v.as_str()).collect(),
        backends: vec![Backend::Auto.as_str(), Backend::Cpu.as_str()],
        max_output_dimension: limits::MAX_OUTPUT_DIMENSION,
        max_output_pixels: limits::MAX_OUTPUT_PIXELS,
        max_quality: limits::MAX_SUPERSAMPLE,
        max_input_bytes: limits::MAX_INPUT_FILE_BYTES,
        gpu_backend_available: false,
    };
    if args.json {
        return Ok(Some(Response::ok(result)));
    }
    println!("mesh-thumb {}", APP_VERSION);
    println!("formats: {}", result.formats.join(", "));
    println!("views: {}", result.views.join(", "));
    println!("backends: {}", result.backends.join(", "));
    println!(
        "limits: max {}x{} px, {} MP output, quality <= {}, input <= {} bytes",
        limits::MAX_OUTPUT_DIMENSION,
        limits::MAX_OUTPUT_DIMENSION,
        limits::MAX_OUTPUT_PIXELS / (1024 * 1024),
        limits::MAX_SUPERSAMPLE,
        limits::MAX_INPUT_FILE_BYTES
    );
    println!("gpu backend: unavailable (MVP is CPU-only)");
    Ok(None)
}
