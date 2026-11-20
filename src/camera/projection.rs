//! Orthographic projection and canvas fitting.
//!
//! Normalized geometry (max extent ≈ `NORMALIZE_SIZE`) is projected with
//! the camera, then a single uniform scale fits the projected bounds
//! onto the canvas with a configurable margin, centered. Screen y is
//! flipped to raster rows (top-left origin).

use crate::camera::Camera;
use crate::diagnostics::error::{Error, Result};
use crate::geometry::types::Vec3;

/// Results of projecting and fitting a set of world-space points.
#[derive(Debug, Clone, Copy)]
pub struct Fit {
    /// Uniform screen scale: world units -> device pixels.
    pub scale: f32,
    /// Pixel offset added to screen x.
    pub offset_x: f32,
    /// Pixel offset subtracted from screen y (y is flipped for rows).
    pub offset_y: f32,
    pub width: u32,
    pub height: u32,
}

impl Fit {
    /// Map a camera-space point `(sx, sy)` to device pixel coordinates.
    pub fn to_pixels(&self, sx: f32, sy: f32) -> (f32, f32) {
        let px = self.offset_x + sx * self.scale;
        let py = self.offset_y - sy * self.scale;
        (px, py)
    }
}

/// Compute the orthographic fit for projected points.
///
/// `camera_space` provides projected (sx, sy) per world point; the fit
/// is derived from the projected bounding box of *all* geometry points so
/// the whole model is framed. Handles degenerate projections (flat/point)
/// safely.
pub fn fit_to_canvas(
    camera: &Camera,
    points: &[Vec3],
    width: u32,
    height: u32,
    margin_fraction: f32,
) -> Result<Fit> {
    if width == 0 || height == 0 {
        return Err(Error::invalid_render_option("canvas must be non-empty"));
    }
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for p in points {
        if !p.is_finite() {
            return Err(Error::invalid_geometry(
                "non-finite point during projection fit",
            ));
        }
        let (sx, sy, _) = camera.to_camera_space(*p);
        min_x = min_x.min(sx);
        max_x = max_x.max(sx);
        min_y = min_y.min(sy);
        max_y = max_y.max(sy);
    }
    let span_x = max_x - min_x;
    let span_y = max_y - min_y;
    if !(span_x.is_finite() && span_y.is_finite()) {
        return Err(Error::invalid_geometry("empty projected bounds"));
    }

    let w = width as f32;
    let h = height as f32;
    let margin = margin_fraction.clamp(0.0, 0.45) * w.min(h);
    let avail_x = (w - 2.0 * margin).max(1.0);
    let avail_y = (h - 2.0 * margin).max(1.0);

    // Uniform scale: fit the larger projected span (never divide by zero).
    let scale_x = if span_x > 1e-12 {
        avail_x / span_x
    } else {
        f32::INFINITY
    };
    let scale_y = if span_y > 1e-12 {
        avail_y / span_y
    } else {
        f32::INFINITY
    };
    let scale = if scale_x.is_finite() && scale_y.is_finite() {
        scale_x.min(scale_y)
    } else if scale_x.is_finite() {
        scale_x
    } else if scale_y.is_finite() {
        scale_y
    } else {
        // Degenerate projection (single point on screen): use margin-free
        // half-canvas as a predictable fallback.
        w.min(h) * 0.25
    };
    // Guard against absurdly large upscale of near-point models.
    let scale = scale.min(w.max(h) * 1024.0);

    let cx_screen = (min_x + max_x) * 0.5;
    let cy_screen = (min_y + max_y) * 0.5;
    let offset_x = w * 0.5 - cx_screen * scale;
    let offset_y = h * 0.5 + cy_screen * scale; // row flip

    Ok(Fit {
        scale,
        offset_x,
        offset_y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::presets;

    fn pts(v: &[[f32; 3]]) -> Vec<Vec3> {
        v.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect()
    }

    #[test]
    fn cube_fits_centered() {
        let cam = presets::front();
        let p = pts(&[
            [-0.5, -0.5, -0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, 0.5],
            [-0.5, 0.5, -0.5],
        ]);
        let fit = fit_to_canvas(&cam, &p, 100, 100, 0.1).unwrap();
        let (px0, py0) = fit.to_pixels(-0.5, -0.5);
        let (px1, py1) = fit.to_pixels(0.5, 0.5);
        // Span of 1.0 world unit maps to avail (100 - 2*10) = 80 px.
        assert!((px1 - px0 - 80.0).abs() < 1e-3);
        assert!((py0 - py1 - 80.0).abs() < 1e-3);
        // Centered: model center at canvas center.
        let (cx, cy) = fit.to_pixels(0.0, 0.0);
        assert!((cx - 50.0).abs() < 1e-3);
        assert!((cy - 50.0).abs() < 1e-3);
    }

    #[test]
    fn flat_model_no_div_zero() {
        let cam = presets::front();
        let p = pts(&[[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]); // horizontal line
        let fit = fit_to_canvas(&cam, &p, 64, 64, 0.08).unwrap();
        assert!(fit.scale.is_finite() && fit.scale > 0.0);
        let (x0, _) = fit.to_pixels(-1.0, 0.0);
        let (x1, _) = fit.to_pixels(1.0, 0.0);
        assert!(x1 > x0);
    }

    #[test]
    fn single_point_fallback() {
        let cam = presets::front();
        let p = pts(&[[0.0, 0.0, 0.0]]);
        let fit = fit_to_canvas(&cam, &p, 64, 64, 0.08).unwrap();
        assert!(fit.scale.is_finite() && fit.scale > 0.0);
        let (cx, cy) = fit.to_pixels(0.0, 0.0);
        assert!((cx - 32.0).abs() < 1e-3);
        assert!((cy - 32.0).abs() < 1e-3);
    }

    #[test]
    fn non_finite_rejected() {
        let cam = presets::front();
        let p = pts(&[[0.0, 0.0, f32::NAN]]);
        assert!(fit_to_canvas(&cam, &p, 64, 64, 0.08).is_err());
    }

    #[test]
    fn wide_model_uses_scaled_axis() {
        let cam = presets::front();
        let p = pts(&[[-2.0, -0.5, 0.0], [2.0, 0.5, 0.0]]);
        let fit = fit_to_canvas(&cam, &p, 100, 100, 0.0).unwrap();
        // span_x = 4 -> scale 25; span_y = 1 -> 100; uniform takes min.
        assert!((fit.scale - 25.0).abs() < 1e-3);
    }
}
