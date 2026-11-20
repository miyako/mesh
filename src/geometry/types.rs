//! Core geometry types: vectors, colors, mesh, point cloud.

use crate::diagnostics::error::{Error, Result};
use crate::geometry::bounds::Bounds3;

/// 3D vector of `f32`, renderer storage precision.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3 {
            x: self.y * o.z - self.z * o.y,
            y: self.z * o.x - self.x * o.z,
            z: self.x * o.y - self.y * o.x,
        }
    }

    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Normalize; returns None for the zero vector (never NaN).
    pub fn try_normalize(self) -> Option<Vec3> {
        let len = self.length();
        if len > 0.0 && len.is_finite() {
            Some(self * (1.0 / len))
        } else {
            None
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn component_min(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }

    pub fn component_max(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// Linear RGBA color, components in 0..=1 (clamped on construction and
/// when parsed). Rendered via sRGB output curve.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// Premultiplied-alpha multiply (used for downsampling).
    pub fn premultiplied(self) -> [f32; 4] {
        [self.r * self.a, self.g * self.a, self.b * self.a, self.a]
    }

    pub fn is_opaque(self) -> bool {
        self.a >= 1.0
    }
}

/// Collected parse-time warnings attached to geometry for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParseWarnings {
    /// Degenerate triangles dropped during parsing.
    pub degenerate_triangles: usize,
    /// Non-finite vertices skipped during parsing.
    pub non_finite_vertices: usize,
}

/// Triangle mesh: vertex positions, triangle index triples, optional
/// per-vertex normals and colors.
#[derive(Debug, Clone)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub normals: Option<Vec<Vec3>>,
    pub colors: Option<Vec<Color>>,
    pub bounds: Bounds3,
    pub warnings: Option<ParseWarnings>,
}

/// Point cloud: first-class geometry kind, not degenerate triangles.
#[derive(Debug, Clone)]
pub struct PointCloud {
    pub positions: Vec<Vec3>,
    pub colors: Option<Vec<Color>>,
    pub normals: Option<Vec<Vec3>>,
    pub bounds: Bounds3,
}

/// Geometry enum; renderers dispatch on this.
#[derive(Debug, Clone)]
pub enum Geometry {
    Mesh(Mesh),
    PointCloud(PointCloud),
}

impl Geometry {
    pub fn bounds(&self) -> &Bounds3 {
        match self {
            Geometry::Mesh(m) => &m.bounds,
            Geometry::PointCloud(p) => &p.bounds,
        }
    }

    pub fn vertex_count(&self) -> usize {
        match self {
            Geometry::Mesh(m) => m.positions.len(),
            Geometry::PointCloud(p) => p.positions.len(),
        }
    }

    pub fn triangle_count(&self) -> usize {
        match self {
            Geometry::Mesh(m) => m.triangles.len(),
            Geometry::PointCloud(_) => 0,
        }
    }

    pub fn is_point_cloud(&self) -> bool {
        matches!(self, Geometry::PointCloud(_))
    }
}

/// Builders used by parsers for incremental/validated construction.
impl Mesh {
    /// Empty constructor; parsers should use this and fill arrays, then
    /// finalize (compute bounds, validate).
    pub fn empty() -> Self {
        Mesh {
            positions: Vec::new(),
            triangles: Vec::new(),
            normals: None,
            colors: None,
            bounds: Bounds3::empty(),
            warnings: None,
        }
    }

    pub fn finalize(mut self) -> Result<Self> {
        self.bounds = Bounds3::from_points(&self.positions)?;
        if self.positions.is_empty() {
            return Err(Error::invalid_geometry("mesh has no vertices"));
        }
        if self.triangles.is_empty() {
            return Err(Error::invalid_geometry("mesh has no triangles"));
        }
        if let Some(n) = &self.normals {
            if n.len() != self.positions.len() {
                return Err(Error::invalid_geometry(
                    "vertex normal count does not match vertex count",
                ));
            }
        }
        if let Some(c) = &self.colors {
            if c.len() != self.positions.len() {
                return Err(Error::invalid_geometry(
                    "vertex color count does not match vertex count",
                ));
            }
        }
        Ok(self)
    }
}

impl PointCloud {
    pub fn empty() -> Self {
        PointCloud {
            positions: Vec::new(),
            colors: None,
            normals: None,
            bounds: Bounds3::empty(),
        }
    }

    pub fn finalize(mut self) -> Result<Self> {
        self.bounds = Bounds3::from_points(&self.positions)?;
        if self.positions.is_empty() {
            return Err(Error::invalid_geometry("point cloud has no vertices"));
        }
        Ok(self)
    }
}
