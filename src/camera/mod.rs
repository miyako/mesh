//! Camera: fixed orthographic views with documented conventions.
//!
//! Conventions (documented in `docs/architecture.md`):
//! - World space is right-handed with +Y up.
//! - A camera is an orthonormal world-space basis `(right, up, forward)`;
//!   `forward` points from the scene toward the camera.
//! - Orthographic projection: screen x = right·p, screen y = up·p,
//!   depth = forward·p (larger depth = closer to the camera).
//! - Screen space has +x right and +y up; the image raster converts to
//!   top-left-origin rows by flipping y.
//!
//! Each preset's exact axes are pinned by tests in `presets.rs` so a
//! thumbnail orientation change fails CI instead of silently shipping.

pub mod presets;
pub mod projection;

use crate::geometry::types::Vec3;

/// A fixed (non-animated) orthographic camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
}

impl Camera {
    /// Construct from explicitly given basis vectors. All three are
    /// normalized; orthonormality is asserted in debug builds.
    pub fn from_basis(right: Vec3, up: Vec3, forward: Vec3) -> Option<Camera> {
        let right = right.try_normalize()?;
        let up = up.try_normalize()?;
        let forward = forward.try_normalize()?;
        debug_assert!(right.dot(up).abs() < 1e-4, "right/up not orthogonal");
        debug_assert!(
            right.dot(forward).abs() < 1e-4,
            "right/forward not orthogonal"
        );
        debug_assert!(up.dot(forward).abs() < 1e-4, "up/forward not orthogonal");
        Some(Camera { right, up, forward })
    }

    /// Project a world point to `(screen_x, screen_y, depth)`.
    pub fn to_camera_space(&self, p: Vec3) -> (f32, f32, f32) {
        (self.right.dot(p), self.up.dot(p), self.forward.dot(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_orthonormal_for_all_presets() {
        for cam in [
            presets::iso(),
            presets::top(),
            presets::front(),
            presets::side(),
        ] {
            assert!((cam.right.length() - 1.0).abs() < 1e-6);
            assert!((cam.up.length() - 1.0).abs() < 1e-6);
            assert!((cam.forward.length() - 1.0).abs() < 1e-6);
            assert!(cam.right.dot(cam.up).abs() < 1e-6);
            assert!(cam.right.dot(cam.forward).abs() < 1e-6);
            assert!(cam.up.dot(cam.forward).abs() < 1e-6);
        }
    }

    #[test]
    fn zero_forward_rejected() {
        assert!(Camera::from_basis(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::ZERO
        )
        .is_none());
    }
}
