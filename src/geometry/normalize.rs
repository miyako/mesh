//! Normalize geometry: translate center to origin, scale uniformly so
//! the maximum extent fits the target unit size. Preserves aspect ratio.
//! The target scale is renderer-versioned (see `versions.rs`).

use crate::diagnostics::error::{Error, Result};
use crate::geometry::types::{Geometry, Mesh, PointCloud, Vec3};
use crate::versions::NORMALIZE_SIZE;

/// Result of normalization: transformed geometry plus the scale applied
/// (useful for diagnostics).
#[derive(Debug)]
pub struct NormalizedGeometry {
    pub geometry: Geometry,
    pub scale: f32,
    /// Model-space size of one unit after normalization.
    pub unit: f32,
}

fn safe_scale(extent: Vec3) -> Result<f32> {
    let max_extent = extent.x.max(extent.y).max(extent.z);
    if !max_extent.is_finite() {
        return Err(Error::invalid_geometry(
            "non-finite model extent during normalization",
        ));
    }
    if max_extent <= 0.0 {
        // Zero-size bounds (single point, line, flat) without any extent:
        // fall back to an arbitrary safe scale rather than dividing by zero.
        return Ok(NORMALIZE_SIZE);
    }
    Ok(NORMALIZE_SIZE / max_extent)
}

fn transform_geometry(geometry: &mut Geometry, center: Vec3, scale: f32) {
    let apply = |p: Vec3| (p - center) * scale;
    match geometry {
        Geometry::Mesh(m) => {
            m.positions.iter_mut().for_each(|p| *p = apply(*p));
            if let Some(n) = &mut m.normals {
                n.iter_mut().for_each(|n| {
                    if let Some(nn) = n.try_normalize() {
                        *n = nn;
                    }
                });
            }
            recompute_bounds_mesh(m);
        }
        Geometry::PointCloud(p) => {
            p.positions.iter_mut().for_each(|p| *p = apply(*p));
            recompute_bounds_points(p);
        }
    }
}

fn recompute_bounds_mesh(m: &mut Mesh) {
    let mut b = crate::geometry::bounds::Bounds3::empty();
    for &p in &m.positions {
        b.include(p);
    }
    m.bounds = b;
}

fn recompute_bounds_points(p: &mut PointCloud) {
    let mut b = crate::geometry::bounds::Bounds3::empty();
    for &v in &p.positions {
        b.include(v);
    }
    p.bounds = b;
}

/// Normalize geometry in place (positions become centered and unit-ish).
pub fn normalize(geometry: Geometry) -> Result<NormalizedGeometry> {
    let bounds = *geometry.bounds();
    if !bounds.is_valid() {
        return Err(Error::invalid_geometry("cannot normalize empty geometry"));
    }
    let center = bounds.center();
    let extent = bounds.extent();
    let scale = safe_scale(extent)?;
    let mut geometry = geometry;
    transform_geometry(&mut geometry, center, scale);
    Ok(NormalizedGeometry {
        geometry,
        scale,
        unit: 1.0 / scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::types::{Color, Vec3};
    use crate::geometry::Bounds3;

    fn unit_cube() -> Geometry {
        // 1x1x1 cube at origin..1
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let triangles = vec![[0u32, 1, 2], [0, 2, 3]];
        let bounds = crate::geometry::bounds::Bounds3::from_points(&positions).unwrap();
        Geometry::Mesh(Mesh {
            positions,
            triangles,
            normals: None,
            colors: Some(vec![
                Color::rgb(1.0, 0.0, 0.0),
                Color::rgb(0.0, 1.0, 0.0),
                Color::rgb(0.0, 0.0, 1.0),
                Color::rgb(1.0, 1.0, 1.0),
            ]),
            bounds,
            warnings: None,
        })
    }

    #[test]
    fn normalize_centers_and_scales() {
        let g = unit_cube();
        let n = normalize(g).unwrap();
        // Max extent 1.0 -> scale 1.0; center (0.5,0.5,0.5) -> origin.
        match n.geometry {
            Geometry::Mesh(m) => {
                assert!(m.positions.iter().all(|p| p.is_finite()));
                // The *bounds center* moves to the origin (not the vertex
                // centroid, which is not the same for asymmetric points).
                let c = m.bounds.center();
                assert!(
                    (c.x).abs() < 1e-6 && (c.y).abs() < 1e-6 && (c.z).abs() < 1e-6,
                    "bounds center {c:?} should be ~0"
                );
                // Extent unchanged (scale 1.0) since size already 1.
                let e = m.bounds.extent();
                assert!((e.x - 1.0).abs() < 1e-6);
                assert!((e.y - 1.0).abs() < 1e-6);
                assert!((e.z - 1.0).abs() < 1e-6);
                // Bounds stay inside the unit cube.
                for p in &m.positions {
                    assert!((-0.5 * 1.01..=0.5 * 1.01).contains(&p.x));
                }
            }
            _ => panic!("expected mesh"),
        }
    }

    #[test]
    fn normalize_flattened_extent() {
        // Cube scaled to 10x0.1x10 -> max extent 10 -> scale 0.1.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.1, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
        ];
        let bounds = Bounds3::from_points(&positions).unwrap();
        let g = Geometry::PointCloud(PointCloud {
            positions,
            colors: None,
            normals: None,
            bounds,
        });
        let n = normalize(g).unwrap();
        match n.geometry {
            Geometry::PointCloud(p) => {
                assert!((n.scale - 0.1).abs() < 1e-6);
                let e = p.bounds.extent();
                assert!((e.x - 1.0).abs() < 1e-5);
                assert!((e.y - 0.01).abs() < 1e-5);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn normalize_zero_extent_no_div_by_zero() {
        // Single point: extent zero -> safe fallback scale, no NaN.
        let positions = vec![Vec3::new(5.0, 5.0, 5.0)];
        let bounds = Bounds3::from_points(&positions).unwrap();
        let g = Geometry::PointCloud(PointCloud {
            positions,
            colors: None,
            normals: None,
            bounds,
        });
        let n = normalize(g).unwrap();
        match n.geometry {
            Geometry::PointCloud(p) => {
                assert!(p.positions[0].is_finite());
                assert_eq!(p.positions[0], Vec3::ZERO);
            }
            _ => panic!(),
        }
        assert!(n.scale.is_finite());
    }
}
