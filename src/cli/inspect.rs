//! `inspect` subcommand: report format, counts, and bounds without
//! rendering.

use serde::Serialize;

use crate::cli::args::InspectArgs;
use crate::cli::json::{InputInfo, Response, WarningsJson};
use crate::cli::render::geometry_summary;
use crate::diagnostics::error::Result;
use crate::formats::load::load_geometry;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InspectResult {
    pub input: InputInfo,
    pub geometry: crate::cli::json::GeometrySummary,
    pub warnings: WarningsJson,
}

/// Run `inspect`. Returns the JSON response when `--json`.
pub fn execute(args: &InspectArgs) -> Result<Option<Response>> {
    let loaded = load_geometry(&args.input.input)?;
    let geometry = geometry_summary(&loaded.geometry);
    let result = InspectResult {
        input: InputInfo::from_loaded(&args.input.input, &loaded.format, loaded.file_size),
        geometry,
        warnings: WarningsJson::from(&loaded.warnings),
    };
    if args.json {
        return Ok(Some(Response::ok(result)));
    }
    let g = &result.geometry;
    println!(
        "{}: {} ({} vertices, {} triangles, colors: {}, normals: {})",
        loaded.format.id, g.kind, g.vertices, g.triangles, g.colors, g.normals
    );
    let b = &g.bounds;
    println!(
        "bounds: [{:.6}, {:.6}, {:.6}] .. [{:.6}, {:.6}, {:.6}]",
        b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]
    );
    Ok(None)
}
