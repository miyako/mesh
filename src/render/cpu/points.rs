//! Point-cloud rendering: projection, deterministic sizing heuristic,
//! and screen-aligned circular splats with depth testing (spec §12).

use crate::camera::Camera;
use crate::diagnostics::error::{Error, ErrorCode, Result};
use crate::geometry::types::{Color, PointCloud};
use crate::render::options::PointRender;

/// Prepared splats: each entry is (screen_x, screen_y, depth, color).
pub struct PointCloudRender {
    pub points: Vec<(f32, f32, f32, Option<Color>)>,
    /// Splat radius in *supersampled* device pixels.
    pub radius: f32,
}

/// Default desired total covered fraction of the canvas for the
/// heuristic when no point size is requested.
const DESIRED_COVERAGE_FRAC: f32 = 0.35;
const MIN_RADIUS_PX: f32 = 0.9;
const MAX_RADIUS_PX: f32 = 14.0;

impl PointCloudRender {
    /// Project and size a point cloud.
    ///
    /// `quality` is the supersampling scale: the heuristic targets final
    /// output pixels and the returned radius is scaled by `quality` so
    /// splats keep the same apparent size after downsampling.
    pub fn prepare(
        camera: &Camera,
        cloud: &PointCloud,
        opts: &PointRender,
        quality: u32,
        fb_width: u32,
        fb_height: u32,
    ) -> Result<PointCloudRender> {
        let n = cloud.positions.len();
        if n == 0 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "cannot render an empty point cloud",
            ));
        }
        // Sizing heuristic in final pixels.
        let final_area =
            f64::from(fb_width) * f64::from(fb_height) / (f64::from(quality) * f64::from(quality));
        let per_point_area = final_area / n as f64;
        let base_radius = match opts.point_size {
            Some(r) => r.clamp(MIN_RADIUS_PX, MAX_RADIUS_PX),
            None => {
                let heuristic =
                    (DESIRED_COVERAGE_FRAC as f64 * per_point_area / std::f64::consts::PI).sqrt();
                heuristic.clamp(MIN_RADIUS_PX as f64, MAX_RADIUS_PX as f64) as f32
            }
        };
        let radius = base_radius * quality as f32;

        let mut points = Vec::with_capacity(n);
        for i in 0..n {
            let p = cloud.positions[i];
            if !p.is_finite() {
                continue;
            }
            let (sx, sy, depth) = camera.to_camera_space(p);
            let color = cloud.colors.as_ref().map(|c| c[i]);
            points.push((sx, sy, depth, color));
        }
        Ok(PointCloudRender { points, radius })
    }
}
