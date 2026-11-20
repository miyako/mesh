//! CPU rasterizer: triangle coverage with barycentric depth/attribute
//! interpolation, double-sided, plus a depth buffer.
//!
//! Conventions:
//! - Screen space: pixel centers at integer + 0.5, x right, y down
//!   (row 0 = top), converted from camera space via `Fit`.
//! - Depth: larger = closer to the camera (see `camera`).
//! - Double-sided: back-facing triangles are rendered by flipping their
//!   coverage normals toward the camera (spec §9.3).

use crate::camera::projection::Fit;
use crate::geometry::types::{Color, Vec3};
use crate::render::cpu::shading::shade_triangle;
use crate::render::image::Framebuffer;
use crate::render::matcap::Matcap;

/// A raster triangle in device space.
#[derive(Debug, Clone, Copy)]
pub struct ScreenTriangle {
    /// (x, y, depth) per vertex.
    pub v: [(f32, f32, f32); 3],
    /// Per-vertex shading attributes.
    pub shading: TriAttrs,
}

#[derive(Debug, Clone, Copy)]
pub struct TriAttrs {
    /// View-space normals per vertex (or same for all = flat).
    pub normals: [Vec3; 3],
    /// Linear albedo per vertex (or none for neutral material).
    pub colors: Option<[Color; 3]>,
}

/// Rasterize one triangle into the framebuffer.
pub fn rasterize_triangle(fb: &mut Framebuffer, matcap: &Matcap, tri: &ScreenTriangle) {
    // Edge functions; skip degenerate (zero area).
    let (ax, ay, az) = tri.v[0];
    let (bx, by, bz) = tri.v[1];
    let (cx, cy, cz) = tri.v[2];

    let area2 = edge(ax, ay, bx, by, cx, cy);
    if area2.abs() <= 1e-12 {
        return; // degenerate in screen space
    }
    let inv_area = 1.0 / area2;

    // Bounding box with pixel-center convention.
    let min_x = ax.min(bx).min(cx).floor() as i64;
    let max_x = ax.max(bx).max(cx).ceil() as i64;
    let min_y = ay.min(by).min(cy).floor() as i64;
    let max_y = ay.max(by).max(cy).ceil() as i64;

    let w = i64::from(fb.width);
    let h = i64::from(fb.height);
    let x0 = min_x.max(0);
    let x1 = (max_x.max(0)).min(w);
    let y0 = min_y.max(0);
    let y1 = (max_y.max(0)).min(h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    // Precompute shading per vertex (normals oriented, colors).
    let shading = TriAttrs {
        normals: tri.shading.normals,
        colors: tri.shading.colors,
    };

    for py in y0..y1 {
        let y = py as f32 + 0.5;
        for px in x0..x1 {
            let x = px as f32 + 0.5;
            let (mut e0, mut e1, mut e2) = (
                edge(ax, ay, bx, by, x, y),
                edge(bx, by, cx, cy, x, y),
                edge(cx, cy, ax, ay, x, y),
            );
            // Normalize by the signed area; if area negative, negate all.
            if area2 < 0.0 {
                e0 = -e0;
                e1 = -e1;
                e2 = -e2;
            }
            if e0 < 0.0 || e1 < 0.0 || e2 < 0.0 {
                continue;
            }
            let w0 = e0 * inv_area;
            let w1 = e1 * inv_area;
            let w2 = e2 * inv_area;
            let depth = w0 * az + w1 * bz + w2 * cz;
            if !depth.is_finite() {
                continue;
            }

            let col = shade_triangle(matcap, &shading, w0, w1, w2);
            // Premultiplied write.
            let a = col.a;
            fb.write(
                px as u32,
                py as u32,
                depth,
                [col.r * a, col.g * a, col.b * a, a],
            );
        }
    }
}

/// Edge function: twice the signed area of the triangle (a,b,c).
#[inline]
fn edge(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (cx - ax) * (by - ay) - (cy - ay) * (bx - ax)
}

/// Rasterize a point-cloud splat: circular disc of given radius with
/// constant orthographic depth (correct for screen-aligned billboards).
pub fn rasterize_splat(
    fb: &mut Framebuffer,
    cx: f32,
    cy: f32,
    depth: f32,
    radius: f32,
    color: Color,
) {
    if radius <= 0.0 || !depth.is_finite() {
        return;
    }
    let r = radius.ceil() as i64;
    let xc = cx.round() as i64;
    let yc = cy.round() as i64;
    let w = i64::from(fb.width);
    let h = i64::from(fb.height);
    let r2 = radius * radius;
    let a = color.a;
    let premul = [color.r * a, color.g * a, color.b * a, a];

    for py in (yc - r).max(0)..(yc + r + 1).min(h) {
        for px in (xc - r).max(0)..(xc + r + 1).min(w) {
            let dx = (px as f32 + 0.5) - cx;
            let dy = (py as f32 + 0.5) - cy;
            if dx * dx + dy * dy <= r2 {
                fb.write(px as u32, py as u32, depth, premul);
            }
        }
    }
}

/// Compute a pixel-space preview of a Fit (used by projection tests).
#[allow(dead_code)]
pub fn project_point(fit: &Fit, sx: f32, sy: f32) -> (f32, f32) {
    fit.to_pixels(sx, sy)
}
