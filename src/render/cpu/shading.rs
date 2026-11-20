//! Per-pixel shading: barycentric interpolation of view-space normals
//! and albedo, then a matcap lookup (spec §11.4). The same conceptual
//! model is used for meshes and cloud splats.

use crate::geometry::types::{Color, Vec3};
use crate::render::cpu::raster::TriAttrs;
use crate::render::matcap::{shade_with_albedo, Matcap, NEUTRAL_ALBEDO};

/// Shade a fragment given barycentric weights. Flat normals (all three
/// identical) fall out naturally.
pub fn shade_triangle(matcap: &Matcap, shading: &TriAttrs, w0: f32, w1: f32, w2: f32) -> Color {
    let n = Vec3::new(
        w0 * shading.normals[0].x + w1 * shading.normals[1].x + w2 * shading.normals[2].x,
        w0 * shading.normals[0].y + w1 * shading.normals[1].y + w2 * shading.normals[2].y,
        w0 * shading.normals[0].z + w1 * shading.normals[1].z + w2 * shading.normals[2].z,
    )
    .try_normalize()
    .unwrap_or(Vec3::new(0.0, 0.0, 1.0));

    match shading.colors {
        Some(cs) => {
            let c = Color::new(
                w0 * cs[0].r + w1 * cs[1].r + w2 * cs[2].r,
                w0 * cs[0].g + w1 * cs[1].g + w2 * cs[2].g,
                w0 * cs[0].b + w1 * cs[1].b + w2 * cs[2].b,
                w0 * cs[0].a + w1 * cs[1].a + w2 * cs[2].a,
            );
            shade_with_albedo(matcap, n, c)
        }
        None => {
            let shade = matcap.sample(n);
            Color::new(shade[0], shade[1], shade[2], 1.0)
        }
    }
}

/// Shade a point-cloud splat (constant normal per point).
pub fn shade_point(matcap: &Matcap, n: Vec3, color: Option<Color>) -> Color {
    match color {
        Some(c) => shade_with_albedo(matcap, n, c),
        None => {
            let shade = matcap.sample(n);
            Color::new(shade[0], shade[1], shade[2], 1.0)
        }
    }
}

/// Neutral albedo exposed for tests/point clouds without colors.
pub fn neutral_albedo() -> Color {
    Color::new(NEUTRAL_ALBEDO[0], NEUTRAL_ALBEDO[1], NEUTRAL_ALBEDO[2], 1.0)
}
