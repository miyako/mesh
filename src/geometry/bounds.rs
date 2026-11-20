//! Axis-aligned bounding boxes with strict finite-value validation.

use crate::diagnostics::error::{Error, Result};
use crate::geometry::types::Vec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds3 {
    pub min: Vec3,
    pub max: Vec3,
    /// True only if at least one valid point has been included.
    pub valid: bool,
}

impl Bounds3 {
    pub fn empty() -> Self {
        Bounds3 {
            min: Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY),
            max: Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
            valid: false,
        }
    }

    pub fn include(&mut self, p: Vec3) {
        if !p.is_finite() {
            return;
        }
        if !self.valid {
            self.min = p;
            self.max = p;
            self.valid = true;
        } else {
            self.min = self.min.component_min(p);
            self.max = self.max.component_max(p);
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Extent per axis; zero for empty bounds.
    pub fn extent(&self) -> Vec3 {
        if self.valid {
            self.max - self.min
        } else {
            Vec3::ZERO
        }
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn from_points(points: &[Vec3]) -> Result<Bounds3> {
        let mut b = Bounds3::empty();
        for &p in points {
            if !p.is_finite() {
                return Err(Error::invalid_geometry(
                    "non-finite vertex position in bounds computation",
                ));
            }
            b.include(p);
        }
        if !b.valid {
            return Err(Error::invalid_geometry("no valid points for bounds"));
        }
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_basic() {
        let b = Bounds3::from_points(&[
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(-2.0, 0.0, 1.0),
            Vec3::new(0.5, 4.0, -1.0),
        ])
        .unwrap();
        assert_eq!(b.min, Vec3::new(-2.0, 0.0, -1.0));
        assert_eq!(b.max, Vec3::new(1.0, 4.0, 3.0));
        assert_eq!(b.center(), Vec3::new(-0.5, 2.0, 1.0));
        assert_eq!(b.extent(), Vec3::new(3.0, 4.0, 4.0));
    }

    #[test]
    fn bounds_empty_rejected() {
        assert!(Bounds3::from_points(&[]).is_err());
    }

    #[test]
    fn bounds_non_finite_rejected() {
        assert!(Bounds3::from_points(&[Vec3::new(f32::NAN, 0.0, 0.0)]).is_err());
        assert!(Bounds3::from_points(&[Vec3::new(0.0, f32::INFINITY, 0.0)]).is_err());
    }
}
