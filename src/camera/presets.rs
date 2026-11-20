//! Named camera presets. Preset behavior is part of the stable visual
//! contract and must not change across versions without bumping the
//! renderer version ($RENDERER_VERSION).

use super::Camera;
use crate::geometry::types::Vec3;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    /// Isometric three-quarter view (default): direction (1,1,1).
    Iso,
    /// View from +Y down; on screen, +X right and +Z up ("top plan").
    Top,
    /// View from +Z toward -Z; on screen, +X right and +Y up.
    Front,
    /// View from +X toward -X; on screen, +Z right and +Y up.
    Side,
}

pub const ALL_PRESETS: [ViewPreset; 4] = [
    ViewPreset::Iso,
    ViewPreset::Top,
    ViewPreset::Front,
    ViewPreset::Side,
];

impl ViewPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewPreset::Iso => "iso",
            ViewPreset::Top => "top",
            ViewPreset::Front => "front",
            ViewPreset::Side => "side",
        }
    }
}

impl FromStr for ViewPreset {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "iso" | "isometric" => Ok(ViewPreset::Iso),
            "top" => Ok(ViewPreset::Top),
            "front" => Ok(ViewPreset::Front),
            "side" | "right" | "left" => Ok(ViewPreset::Side),
            other => Err(format!(
                "unknown view '{other}' (expected one of: iso, top, front, side)"
            )),
        }
    }
}

/// Isometric view along direction (1,1,1): elevation ≈35.264°, yaw 45°.
///
/// Basis (pinned by `iso_basis_pinned`): with the world +Y axis as the
/// "top", the model's top face must appear at the top of the image:
/// ```text
/// right   = normalize(cross(world_up, forward)) = ( .7071, 0, -.7071)
/// up      = forward x right                     = (-.4082, .8165, -.4082)
/// forward = normalize(1, 1, 1)                  = ( .5774, .5774,  .5774)
/// ```
/// Equivalently `up = right x forward` would place the top face at the
/// BOTTOM, which reads as a bottom view; we deliberately use the
/// top-upment orientation.
pub fn iso() -> Camera {
    let forward = Vec3::new(1.0, 1.0, 1.0).try_normalize().unwrap();
    // right = normalize(cross(world_up, forward))
    let right = Vec3::new(0.0, 1.0, 0.0)
        .cross(forward)
        .try_normalize()
        .unwrap();
    let up = forward.cross(right).try_normalize().unwrap();
    Camera::from_basis(right, up, forward).expect("iso basis")
}

/// Top view: forward +Y, right +X, up +Z.
pub fn top() -> Camera {
    Camera::from_basis(
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 1.0, 0.0),
    )
    .expect("top basis")
}

/// Front view: forward +Z, right +X, up +Y.
pub fn front() -> Camera {
    Camera::from_basis(
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    )
    .expect("front basis")
}

/// Side view: forward +X, right +Z, up +Y.
pub fn side() -> Camera {
    Camera::from_basis(
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    )
    .expect("side basis")
}

pub fn camera_for(preset: ViewPreset) -> Camera {
    match preset {
        ViewPreset::Iso => iso(),
        ViewPreset::Top => top(),
        ViewPreset::Front => front(),
        ViewPreset::Side => side(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse() {
        assert_eq!("iso".parse::<ViewPreset>().unwrap(), ViewPreset::Iso);
        assert_eq!("TOP".parse::<ViewPreset>().unwrap(), ViewPreset::Top);
        assert!("diagonal".parse::<ViewPreset>().is_err());
    }

    #[test]
    fn front_view_axes() {
        let cam = front();
        assert_eq!(cam.right, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(cam.up, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(cam.forward, Vec3::new(0.0, 0.0, 1.0));
    }

    #[test]
    fn top_view_axes() {
        let cam = top();
        assert_eq!(cam.right, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(cam.up, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(cam.forward, Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn side_view_axes() {
        let cam = side();
        assert_eq!(cam.right, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(cam.up, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(cam.forward, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn iso_positive_octant_visibility() {
        // From (1,1,1) the camera sees the +X, +Y, +Z faces of a cube;
        // the classic look shows the top (+Y) face at the top of the
        // image with the two side faces below it.
        let cam = iso();
        let (x0, y0, _) = cam.to_camera_space(Vec3::ZERO);
        let (xp, yp, _) = cam.to_camera_space(Vec3::new(1.0, 0.0, 0.0));
        let (_, yz, _) = cam.to_camera_space(Vec3::new(0.0, 0.0, 1.0));
        let (_, yy_up, _) = cam.to_camera_space(Vec3::new(0.0, 1.0, 0.0));
        assert!(xp > x0, "+X should appear to the right");
        assert!(yy_up > y0, "+Y (top) should appear above the origin");
        assert!(yy_up > yp, "world +Y should map above +X direction");
        assert!(yy_up > yz, "top face should be the highest on screen");
        assert!(yz < y0, "+Z should appear below the origin (side face)");
        // depth: closer along forward = larger depth; the corner nearest
        // the camera must have larger depth.
        let front_corner_depth = cam.to_camera_space(Vec3::new(1.0, 1.0, 1.0)).2;
        let back_corner_depth = cam.to_camera_space(Vec3::new(0.0, 0.0, 0.0)).2;
        assert!(front_corner_depth > back_corner_depth);
    }

    #[test]
    fn iso_basis_pinned() {
        // Exact basis values are part of the stable visual contract.
        let cam = iso();
        let assert_close = |v: Vec3, e: [f32; 3]| {
            assert!((v.x - e[0]).abs() < 1e-5, "x: {} vs {}", v.x, e[0]);
            assert!((v.y - e[1]).abs() < 1e-5, "y: {} vs {}", v.y, e[1]);
            assert!((v.z - e[2]).abs() < 1e-5, "z: {} vs {}", v.z, e[2]);
        };
        let inv_sqrt3 = 1.0f32 / 3.0f32.sqrt();
        let inv_sqrt6 = 1.0f32 / 6.0f32.sqrt();
        assert_close(cam.forward, [inv_sqrt3, inv_sqrt3, inv_sqrt3]);
        assert_close(
            cam.right,
            [
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                -std::f32::consts::FRAC_1_SQRT_2,
            ],
        );
        assert_close(cam.up, [-inv_sqrt6, 2.0 * inv_sqrt6, -inv_sqrt6]);
    }
}
