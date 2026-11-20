//! Render options: the stable, versioned set of knobs that influence
//! output pixels and therefore cache identity.

use std::str::FromStr;

use crate::camera::presets::ViewPreset;
use crate::diagnostics::error::Error;
use crate::geometry::types::Color;
use crate::render::cpu::Backend;

/// Background: solid RGB or fully transparent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    Solid(Color),
    Transparent,
}

impl Background {
    /// Display the background as an sRGB hex string. `Color` values in the
    /// pipeline are linear; the hex form is the *encoded* (sRGB) color the
    /// user typed, which also keeps cache settings stable across versions.
    pub fn as_str(&self) -> String {
        match self {
            Background::Solid(c) => format!(
                "#{:02X}{:02X}{:02X}",
                crate::render::image::srgb_encode_quantize(crate::render::image::linear_to_srgb(
                    c.r
                )),
                crate::render::image::srgb_encode_quantize(crate::render::image::linear_to_srgb(
                    c.g
                )),
                crate::render::image::srgb_encode_quantize(crate::render::image::linear_to_srgb(
                    c.b
                )),
            ),
            Background::Transparent => "transparent".to_string(),
        }
    }
}

impl FromStr for Background {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.eq_ignore_ascii_case("transparent") || t.eq_ignore_ascii_case("none") {
            return Ok(Background::Transparent);
        }
        let hex = t.strip_prefix('#').unwrap_or(t);
        if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
            // Hex colors are sRGB-encoded; linearize so the render
            // pipeline (which works in linear light) reproduces the
            // exact color after the final sRGB re-encode.
            return Ok(Background::Solid(Color::new(
                crate::render::image::srgb_to_linear(f32::from(r) / 255.0),
                crate::render::image::srgb_to_linear(f32::from(g) / 255.0),
                crate::render::image::srgb_to_linear(f32::from(b) / 255.0),
                1.0,
            )));
        }
        Err(Error::invalid_render_option(format!(
            "invalid background '{s}' (use \"transparent\" or a hex color like #20242b)"
        )))
    }
}

/// Point-cloud rendering options (subset of `RenderOptions` that affects
/// the point path).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PointRender {
    /// Manual splat radius in final pixels; None = heuristic.
    pub point_size: Option<f32>,
}

/// The full set of render options; participates in cache identity via
/// its canonical serialization (`RenderOptions::settings_string`).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub view: ViewPreset,
    pub background: Background,
    /// Supersampling scale (1 = none).
    pub quality: u32,
    pub point: PointRender,
    pub backend: Backend,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            width: 256,
            height: 256,
            view: ViewPreset::Iso,
            background: "#20242b".parse().expect("valid default background"),
            quality: 2,
            point: PointRender::default(),
            backend: Backend::Auto,
        }
    }
}

impl RenderOptions {
    /// Canonical, stable string encoding of every output-affecting
    /// option. Part of the cache key: any change here changes cache
    /// identity (see docs/decisions.md).
    pub fn settings_string(&self) -> String {
        let point = match self.point.point_size {
            Some(r) => format!("point{r:.4}"),
            None => "pointauto".to_string(),
        };
        format!(
            "v{}:{}x{}:{}:{:02}:{}{}:{}",
            crate::versions::RENDERER_VERSION,
            self.width,
            self.height,
            self.view.as_str(),
            self.quality,
            self.background.as_str(),
            point,
            self.backend.as_str(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_parses() {
        assert!(matches!(
            "transparent".parse::<Background>().unwrap(),
            Background::Transparent
        ));
        let c = "#ff0000".parse::<Background>().unwrap();
        match c {
            Background::Solid(c) => {
                assert!((c.r - 1.0).abs() < 0.01 && c.g == 0.0 && c.b == 0.0)
            }
            _ => panic!(),
        }
        assert!("#12345".parse::<Background>().is_err());
        assert!("orange".parse::<Background>().is_err());
    }

    #[test]
    fn settings_string_stable() {
        let a = RenderOptions::default();
        let b = RenderOptions::default();
        assert_eq!(a.settings_string(), b.settings_string());
        let mut c = a.clone();
        c.quality = 3;
        assert_ne!(a.settings_string(), c.settings_string());
        let mut d = a.clone();
        d.background = Background::Transparent;
        assert_ne!(a.settings_string(), d.settings_string());
    }
}
