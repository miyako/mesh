//! Matcap shading: a compact normal-based lighting lookup.
//!
//! The MVP generates the matcap procedurally (deterministic, embedded in
//! the executable — no runtime download) as a small RGBA texture indexed
//! by the view-space normal. The interface is designed so a texture
//! asset can replace the generated lookup without touching callers.

use crate::geometry::types::{Color, Vec3};

/// Size of the generated matcap texture (square, power of two).
pub const MATCAP_SIZE: u32 = 64;

/// View-space light direction for the neutral matcap.
const LIGHT_DIR: Vec3 = Vec3 {
    x: 0.45,
    y: 0.55,
    z: 0.97,
};

/// Neutral material albedo (linear sRGB).
pub const NEUTRAL_ALBEDO: [f32; 3] = [0.62, 0.66, 0.74];

/// A matcap texture stored as linear RGBA `f32` in [0,1].
pub struct Matcap {
    size: u32,
    tex: Vec<[f32; 3]>, // RGB only; alpha always 1
}

/// Process-wide default matcap, generated once (deterministic).
static DEFAULT: std::sync::OnceLock<Matcap> = std::sync::OnceLock::new();

impl Matcap {
    /// The shared default matcap (kept distinct from
    /// `std::default::Default`).
    pub fn default_matcap() -> &'static Matcap {
        DEFAULT.get_or_init(|| Matcap::generate(MATCAP_SIZE))
    }

    /// Generate the deterministic matcap. Deterministic across runs and
    /// platforms (pure floating-point math; see docs).
    pub fn generate(size: u32) -> Matcap {
        let mut tex = Vec::with_capacity((size * size) as usize);
        for v in 0..size {
            for u in 0..size {
                // texel -> view-space normal over the unit disc:
                // +x right, +y up (matching screen).
                let nx = (2.0 * (u as f32 + 0.5) / size as f32) - 1.0;
                let ny = (2.0 * (v as f32 + 0.5) / size as f32) - 1.0;
                let r2 = nx * nx + ny * ny;
                let nz = if r2 <= 1.0 {
                    (1.0 - r2).max(0.0).sqrt()
                } else {
                    0.0
                };
                let n = Vec3::new(nx, ny, nz);
                let mut rgb = shade_neutral(n);
                // slight blue tint at grazing angles
                let rim = (1.0 - nz).powi(2) * 0.10;
                rgb[0] -= rim * 0.6;
                rgb[1] -= rim * 0.3;
                rgb[2] += rim * 0.2;
                tex.push(rgb.map(|c| c.clamp(0.0, 1.0)));
            }
        }
        Matcap { size, tex }
    }

    /// Sample the matcap for a view-space normal; bilinear filtering.
    pub fn sample(&self, n: Vec3) -> [f32; 3] {
        if !n.is_finite() {
            return NEUTRAL_ALBEDO;
        }
        let u = 0.5 + 0.5 * n.x;
        let v = 0.5 + 0.5 * n.y;
        let size = self.size as f32;
        let fx = (u * size - 0.5).clamp(0.0, size - 1.0 - 1e-4);
        let fy = (v * size - 0.5).clamp(0.0, size - 1.0 - 1e-4);
        let x0 = fx.floor() as u32;
        let y0 = fy.floor() as u32;
        let x1 = (x0 + 1).min(self.size - 1);
        let y1 = (y0 + 1).min(self.size - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let c00 = self.tex[(y0 * self.size + x0) as usize];
        let c10 = self.tex[(y0 * self.size + x1) as usize];
        let c01 = self.tex[(y1 * self.size + x0) as usize];
        let c11 = self.tex[(y1 * self.size + x1) as usize];
        let mut out = [0.0f32; 3];
        for (i, o) in out.iter_mut().enumerate() {
            let a = c00[i] * (1.0 - tx) + c10[i] * tx;
            let b = c01[i] * (1.0 - tx) + c11[i] * tx;
            *o = a * (1.0 - ty) + b * ty;
        }
        out
    }

    /// Luminance of the neutral-material shading for a normal; used to
    /// modulate vertex colors while preserving hue.
    pub fn shade_factor(&self, n: Vec3) -> f32 {
        let c = self.sample(n);
        luminance(c)
    }
}

/// Deterministic neutral shading: ambient + diffuse + specular.
fn shade_neutral(n: Vec3) -> [f32; 3] {
    let light = LIGHT_DIR
        .try_normalize()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let diffuse = n.dot(light).max(0.0);
    // Blinn-Phong-ish specular toward camera (view = +Z in view space).
    let half = (light + Vec3::new(0.0, 0.0, 1.0))
        .try_normalize()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let spec = n.dot(half).max(0.0).powf(28.0) * 0.35;
    let ambient = 0.34 + 0.06 * n.z.clamp(0.0, 1.0);

    let t = ambient + 0.78 * diffuse;
    let base = NEUTRAL_ALBEDO;
    [base[0] * t + spec, base[1] * t + spec, base[2] * t + spec]
}

/// Rec.709-style luminance.
pub fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Combine the neutral matcap shade with an albedo color: the matcap
/// modulates both value and tint; a colored albedo is multiplied and
/// re-normalized against the neutral luminance so hue is preserved.
pub fn shade_with_albedo(matcap: &Matcap, n: Vec3, albedo: Color) -> Color {
    let factor = matcap.shade_factor(n);
    let neutral_lum = luminance(NEUTRAL_ALBEDO);
    let scale = if neutral_lum > 0.0 {
        factor / neutral_lum
    } else {
        factor
    };
    Color::new(
        albedo.r * scale,
        albedo.g * scale,
        albedo.b * scale,
        albedo.a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let m = Matcap::generate(MATCAP_SIZE);
        let m2 = Matcap::generate(MATCAP_SIZE);
        let n = Vec3::new(0.0, 0.0, 1.0);
        assert_eq!(m.sample(n), m2.sample(n));
    }

    #[test]
    fn front_normal_lit() {
        let m = Matcap::generate(MATCAP_SIZE);
        let n = Vec3::new(0.0, 0.0, 1.0);
        let c = m.sample(n);
        let lum = luminance(c);
        assert!(lum > 0.4, "front-facing should be well lit: {lum}");
    }

    #[test]
    fn normals_finite() {
        let m = Matcap::generate(8);
        for n in [
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ] {
            let c = m.sample(n);
            assert!(c.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn colored_albedo_keeps_hue() {
        let m = Matcap::generate(MATCAP_SIZE);
        let red = Color::rgb(1.0, 0.0, 0.0);
        let out = shade_with_albedo(&m, Vec3::new(0.0, 0.0, 1.0), red);
        assert!(out.r > out.g && out.r > out.b, "red stays red: {out:?}");
        assert!(out.r <= 1.0);
    }

    #[test]
    fn degenerate_normal_safe() {
        let m = Matcap::generate(8);
        let c = m.sample(Vec3::new(f32::NAN, 0.0, 0.0));
        assert!(
            c.iter().all(|v| v.is_finite()),
            "NaN normal must not poison"
        );
    }
}
