//! Mesh/point normal handling: recompute face normals from positions,
//! never trust source STL normals. Safe normalization throughout.

use crate::diagnostics::error::Result;
use crate::geometry::types::{Mesh, Vec3};

/// Per-triangle face normal (before normalization sign handling) — flat
/// shading reference. Double-sided rendering flips back faces toward the
/// camera, so this returns the raw geometric normal.
pub fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Option<Vec3> {
    let ab = b - a;
    let ac = c - a;
    ab.cross(ac).try_normalize()
}

/// Recompute all face normals from positions. Returns per-triangle
/// normals aligned with `triangles`; degenerate triangles yield `None`.
pub fn recompute_face_normals(mesh: &Mesh) -> Vec<Option<Vec3>> {
    mesh.triangles
        .iter()
        .map(|t| {
            let a = mesh.positions[t[0] as usize];
            let b = mesh.positions[t[1] as usize];
            let c = mesh.positions[t[2] as usize];
            face_normal(a, b, c)
        })
        .collect()
}

/// Count degenerate triangles (zero area after finite check).
pub fn count_degenerate(mesh: &Mesh) -> usize {
    mesh.triangles
        .iter()
        .filter(|t| {
            let a = mesh.positions[t[0] as usize];
            let b = mesh.positions[t[1] as usize];
            let c = mesh.positions[t[2] as usize];
            face_normal(a, b, c).is_none()
        })
        .count()
}

/// Validate and normalize optional vertex normals in place. Returns the
/// number of entries that were dropped (invalid) — callers may warn.
pub fn sanitize_vertex_normals(normals: &mut [Vec3]) -> usize {
    let mut dropped = 0;
    for n in normals.iter_mut() {
        match n.try_normalize() {
            Some(nn) => *n = nn,
            None => {
                dropped += 1;
                *n = Vec3::ZERO;
            }
        }
    }
    dropped
}

/// Result of an adjustment so reversed windings do not go dark.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadingNormal {
    pub normal: Vec3,
    pub flipped: bool,
}

/// Given a face normal and the view-space forward direction (toward the
/// camera), return a normal oriented toward the camera (double-sided
/// rendering).
pub fn orient_toward_camera(normal: Vec3, view_forward: Vec3) -> ShadingNormal {
    if normal.dot(view_forward) < 0.0 {
        ShadingNormal {
            normal: -normal,
            flipped: true,
        }
    } else {
        ShadingNormal {
            normal,
            flipped: false,
        }
    }
}

/// Validate mesh topology: all triangle indices in range.
pub fn validate_indices(mesh: &Mesh) -> Result<()> {
    let n = mesh.positions.len() as u32;
    for t in &mesh.triangles {
        for &i in t {
            if i >= n {
                return Err(crate::diagnostics::error::Error::new(
                    crate::diagnostics::error::ErrorCode::InvalidGeometry,
                    format!("triangle index {i} out of range ({n} vertices)"),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_normal_basic() {
        let n = face_normal(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        assert!((n.z - 1.0).abs() < 1e-6, "n = {n:?}");
    }

    #[test]
    fn degenerate_none() {
        assert!(face_normal(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO).is_none());
        assert!(face_normal(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
        )
        .is_none());
    }

    #[test]
    fn orient_flips_backface() {
        let forward = Vec3::new(0.0, 0.0, 1.0);
        // Normal pointing away from camera
        let n = Vec3::new(0.0, 0.0, -1.0);
        let r = orient_toward_camera(n, forward);
        assert!(r.flipped);
        assert!((r.normal.z - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sanitize_normals() {
        let mut ns = vec![
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let dropped = sanitize_vertex_normals(&mut ns);
        assert_eq!(dropped, 1);
        assert!((ns[0].z - 1.0).abs() < 1e-6);
        assert_eq!(ns[1], Vec3::ZERO);
    }
}
