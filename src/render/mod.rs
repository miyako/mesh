//! Renderer: pipeline orchestration (normalize -> project -> rasterize
//! -> downsample -> encode) and the settings that define output.

pub mod cpu;
pub mod image;
pub mod matcap;
pub mod options;

use crate::camera::presets::camera_for;
use crate::camera::projection::fit_to_canvas;
use crate::diagnostics::error::{Error, Result};
use crate::geometry::normalize::normalize;
use crate::geometry::types::Geometry;
use crate::render::image::Framebuffer;
use crate::render::options::{Background, RenderOptions};
use crate::versions::{DEFAULT_MARGIN_FRACTION, NORMALIZE_SIZE};

pub use options::PointRender;

/// Result of a render pass.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    pub width: u32,
    pub height: u32,
    /// Final RGBA8 pixels (top-left origin), sRGB-encoded.
    pub rgba: Vec<u8>,
    /// Encoded PNG bytes.
    pub png: Vec<u8>,
    /// Wall-clock time of the render pass, in milliseconds.
    pub render_ms: u64,
    /// Normalized scale applied to the model (diagnostics).
    pub normalize_scale: f32,
}

/// Render normalized+projected geometry: full pipeline to PNG bytes.
///
/// `geometry` must already be validated and may be normalized here (this
/// function normalizes then renders). Callers wanting the cached
/// normalized stage can use `render_geometry_raw` instead.
pub fn render(geometry: Geometry, opts: &RenderOptions) -> Result<RenderOutput> {
    // Validate dimensions before allocating anything.
    crate::diagnostics::limits::validate_output_dimensions(opts.width, opts.height, opts.quality)?;
    let _backend = opts.backend.resolve()?; // validates backend selection (gpu -> clear error)

    let started = std::time::Instant::now();
    let normalized = normalize(geometry)?;
    let geometry = &normalized.geometry;
    let camera = camera_for(opts.view);
    let positions = geometry_positions(geometry);
    // Fit at supersampled resolution so margins/quality scale together.
    let fb_w = opts.width * opts.quality;
    let fb_h = opts.height * opts.quality;
    let fit = fit_to_canvas(&camera, positions, fb_w, fb_h, DEFAULT_MARGIN_FRACTION)?;

    let mut fb = Framebuffer::new(fb_w, fb_h)?;
    let bg: [f32; 4] = match opts.background {
        Background::Solid(c) => [c.r, c.g, c.b, 1.0],
        Background::Transparent => [0.0, 0.0, 0.0, 0.0],
    };
    fb.clear(bg);

    let scene = cpu::Scene {
        camera,
        fit,
        view_forward: camera.forward,
    };
    let matcap = matcap::Matcap::default_matcap();
    cpu::render_geometry(&mut fb, matcap, &scene, geometry, &opts.point, opts.quality)?;

    let rgba = image::downsample_premul(&fb, opts.width, opts.height)?;
    let png = image::encode_png(opts.width, opts.height, &rgba)?;
    let render_ms = started.elapsed().as_millis() as u64;

    Ok(RenderOutput {
        width: opts.width,
        height: opts.height,
        rgba,
        png,
        render_ms,
        normalize_scale: normalized.scale,
    })
}

fn geometry_positions(g: &Geometry) -> &[crate::geometry::types::Vec3] {
    match g {
        Geometry::Mesh(m) => &m.positions,
        Geometry::PointCloud(p) => &p.positions,
    }
}

/// Convenience for library users: render with defaults. Errors on
/// unsupported input; see `versions` for stability guarantees.
pub fn render_default(geometry: Geometry) -> Result<RenderOutput> {
    render(geometry, &RenderOptions::default())
}

/// Expose the normalization target for diagnostics/tests.
pub fn normalize_size() -> f32 {
    NORMALIZE_SIZE
}

/// Small helper error for callers that pass degenerate options.
pub fn invalid_options(message: impl Into<String>) -> Error {
    Error::invalid_render_option(message)
}
