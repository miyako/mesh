//! Polygonal face triangulation.
//!
//! MVP uses a fan: `(v0, v1, v2), (v0, v2, v3), ...` — correct for
//! convex polygons and most practical files, but can be wrong for
//! concave polygons. Documented limitation (see docs/format-support.md).

use crate::diagnostics::error::{Error, Result};

/// Triangulate a face's vertex index list into index triples using a
/// fan from the first vertex. Indices are validated before use by the
/// caller (or here against `vertex_count` if provided as `Some`).
///
/// Returns an error for lists shorter than 3. Degenerate output
/// triangles (repeated indices) are skipped.
pub fn triangulate_face(face: &[u32], vertex_count: Option<u32>) -> Result<Vec<[u32; 3]>> {
    if face.len() < 3 {
        return Err(Error::new(
            crate::diagnostics::error::ErrorCode::InvalidPlyData,
            format!(
                "face with {} vertices is too short to triangulate",
                face.len()
            ),
        ));
    }
    if let Some(count) = vertex_count {
        for &idx in face {
            if idx >= count {
                return Err(Error::new(
                    crate::diagnostics::error::ErrorCode::InvalidPlyData,
                    format!("face index {idx} out of range (vertex count {count})"),
                ));
            }
        }
    }
    let mut out = Vec::with_capacity(face.len().saturating_sub(2));
    // Fan triangulation using the last index as the shared apex keeps
    // contiguous triangles: (0,1,2),(0,2,3),...
    for i in 1..face.len() - 1 {
        let a = face[0];
        let b = face[i];
        let c = face[i + 1];
        if a == b || b == c || a == c {
            continue; // degenerate; skip
        }
        out.push([a, b, c]);
    }
    if out.is_empty() {
        return Err(Error::new(
            crate::diagnostics::error::ErrorCode::InvalidPlyData,
            "face produced no valid triangles",
        ));
    }
    Ok(out)
}

/// Split a mix of triangle and polygon faces into a flat triangle index
/// list. `faces` are the per-face index lists.
pub fn triangulate_faces(
    faces: &[Vec<u32>],
    vertex_count: u32,
    max_out: usize,
) -> Result<Vec<[u32; 3]>> {
    let mut out = Vec::new();
    for face in faces {
        let tris = triangulate_face(face, Some(vertex_count))?;
        if out.len() + tris.len() > max_out {
            return Err(Error::new(
                crate::diagnostics::error::ErrorCode::ResourceLimitExceeded,
                "triangulated triangle count exceeds limit",
            ));
        }
        out.extend(tris);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_passthrough() {
        let t = triangulate_face(&[0, 1, 2], Some(3)).unwrap();
        assert_eq!(t, vec![[0, 1, 2]]);
    }

    #[test]
    fn quad_fan() {
        let t = triangulate_face(&[0, 1, 2, 3], Some(4)).unwrap();
        assert_eq!(t, vec![[0, 1, 2], [0, 2, 3]]);
    }

    #[test]
    fn pentagon_fan() {
        let t = triangulate_face(&[0, 1, 2, 3, 4], Some(5)).unwrap();
        assert_eq!(t, vec![[0, 1, 2], [0, 2, 3], [0, 3, 4]]);
    }

    #[test]
    fn too_short_rejected() {
        assert!(triangulate_face(&[0, 1], Some(2)).is_err());
        assert!(triangulate_face(&[], Some(0)).is_err());
    }

    #[test]
    fn out_of_range_rejected() {
        assert!(triangulate_face(&[0, 1, 5], Some(3)).is_err());
    }

    #[test]
    fn degenerate_skipped() {
        let t = triangulate_face(&[0, 0, 1, 2], Some(3)).unwrap();
        // (0,0,1) skipped; (0,1,2) kept
        assert_eq!(t, vec![[0, 1, 2]]);
    }

    #[test]
    fn all_degenerate_rejected() {
        assert!(triangulate_face(&[0, 0, 0, 0, 0], Some(1)).is_err());
    }
}
