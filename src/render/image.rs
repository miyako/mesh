//! Framebuffer, supersampled downsampling, and PNG encoding.

use crate::diagnostics::error::{Error, ErrorCode, Result};
use crate::diagnostics::limits;

/// Linear RGBA framebuffer (f32 components in 0..=1) at render
/// resolution, plus a depth buffer.
pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    /// Premultiplied linear RGBA: [r*a, g*a, b*a, a] per pixel.
    pub color: Vec<f32>,
    /// Depth per pixel; larger = closer (matches camera `forward`).
    pub depth: Vec<f32>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Result<Framebuffer> {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > limits::MAX_OUTPUT_PIXELS * 64 {
            return Err(Error::resource_limit("framebuffer too large"));
        }
        let n =
            usize::try_from(pixels).map_err(|_| Error::resource_limit("framebuffer too large"))?;
        Ok(Framebuffer {
            width,
            height,
            color: vec![0.0f32; n * 4],
            depth: vec![f32::NEG_INFINITY; n],
        })
    }

    /// Clear with a background color (premultiplied provided by caller).
    pub fn clear(&mut self, bg: [f32; 4]) {
        for px in self.color.chunks_exact_mut(4) {
            px.copy_from_slice(&bg);
        }
        self.depth.fill(f32::NEG_INFINITY);
    }

    #[inline]
    pub fn write(&mut self, x: u32, y: u32, depth: f32, c: [f32; 4]) {
        let idx = ((y as usize) * self.width as usize + x as usize) * 4;
        // Depth test: larger depth = closer to camera; replace if closer.
        if depth >= self.depth[y as usize * self.width as usize + x as usize] {
            self.depth[y as usize * self.width as usize + x as usize] = depth;
            self.color[idx..idx + 4].copy_from_slice(&c);
        }
    }

    #[inline]
    pub fn depth_at(&self, x: u32, y: u32) -> f32 {
        self.depth[y as usize * self.width as usize + x as usize]
    }
}

/// Downsample a supersampled premultiplied framebuffer to final RGBA8,
/// using a box filter in premultiplied-alpha space (avoids dark/bright
/// fringes at object edges on transparent backgrounds).
pub fn downsample_premul(src: &Framebuffer, out_w: u32, out_h: u32) -> Result<Vec<u8>> {
    let scale_x = src.width / out_w;
    let scale_y = src.height / out_h;
    if scale_x == 0 || scale_y == 0 {
        return Err(Error::invalid_render_option(
            "downsample scale must be >= 1",
        ));
    }
    if scale_x * scale_y != (src.width / out_w) * (src.height / out_h) {
        // Non-integer scale: guaranteed only when quality divides evenly.
        return Err(Error::invalid_render_option(
            "supersampled size must be an integer multiple of output size",
        ));
    }
    let block = (scale_x * scale_y) as f32;
    let mut out = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
    for oy in 0..out_h {
        for ox in 0..out_w {
            let (mut r, mut g, mut b, mut a) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for sy in 0..scale_y {
                for sx in 0..scale_x {
                    let x = ox * scale_x + sx;
                    let y = oy * scale_y + sy;
                    let idx = (y as usize * src.width as usize + x as usize) * 4;
                    r += f64::from(src.color[idx]);
                    g += f64::from(src.color[idx + 1]);
                    b += f64::from(src.color[idx + 2]);
                    a += f64::from(src.color[idx + 3]);
                }
            }
            let (r, g, b, a) = (
                r / f64::from(block),
                g / f64::from(block),
                b / f64::from(block),
                a / f64::from(block),
            );
            let oi = (oy as usize * out_w as usize + ox as usize) * 4;
            if a > 0.0 {
                // Un-premultiply, then encode linear -> sRGB.
                out[oi] = encode_srgb((r / a) as f32);
                out[oi + 1] = encode_srgb((g / a) as f32);
                out[oi + 2] = encode_srgb((b / a) as f32);
            } else {
                out[oi] = 0;
                out[oi + 1] = 0;
                out[oi + 2] = 0;
            }
            out[oi + 3] = ((a as f32).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    Ok(out)
}

/// Linear light -> sRGB-encoded channel in 0..1.
///
/// The renderer works in linear light throughout (framebuffer, shading);
/// sRGB-encoded inputs (hex colors, uchar vertex colors) are converted
/// with [`srgb_to_linear`] at parse time and outputs are re-encoded here.
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB-encoded channel in 0..1 -> linear light.
pub fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear -> sRGB (with linear ramp near zero), quantized to a byte.
pub fn encode_srgb(c: f32) -> u8 {
    ((linear_to_srgb(c) * 255.0) + 0.5).min(255.0) as u8
}

/// Linear -> sRGB byte from a raw sRGB-encoded channel, WITHOUT re-encoding.
/// (Converters use this when the value is already sRGB-encoded.)
pub fn srgb_encode_quantize(c: f32) -> u8 {
    ((c.clamp(0.0, 1.0)) * 255.0 + 0.5).min(255.0) as u8
}

/// Encode final RGBA8 pixels as PNG bytes.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(Error::new(
            ErrorCode::PngEncodeError,
            format!(
                "internal: pixel buffer size {} != expected {}",
                rgba.len(),
                expected
            ),
        ));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual); // declare sRGB output, no embedded profile
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::new(ErrorCode::PngEncodeError, format!("header: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| Error::new(ErrorCode::PngEncodeError, format!("data: {e}")))?;
    }
    Ok(out)
}

/// Validate a cached PNG: correct length, PNG signature, decodable.
pub fn validate_png_bytes(data: &[u8]) -> Result<()> {
    if data.len() < 8 || data[..8] != [137, 80, 78, 71, 13, 10, 26, 10] {
        return Err(Error::new(
            ErrorCode::PngEncodeError,
            "invalid PNG signature",
        ));
    }
    let decoder = png::Decoder::new(data);
    let mut reader = decoder
        .read_info()
        .map_err(|e| Error::new(ErrorCode::PngEncodeError, format!("invalid PNG: {e}")))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| Error::new(ErrorCode::PngEncodeError, format!("invalid PNG: {e}")))?;
    if info.width == 0 || info.height == 0 {
        return Err(Error::new(ErrorCode::PngEncodeError, "PNG has zero size"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_roundtrip() {
        for v in [0.0f32, 0.1, 0.17, 0.5, 0.75, 0.95, 1.0] {
            let back = srgb_to_linear(linear_to_srgb(v));
            assert!((back - v).abs() < 0.01, "v={v} back={back}");
        }
        // Known reference: sRGB 0.1255 (32/255) -> linear ~0.0145.
        let lin = srgb_to_linear(32.0 / 255.0);
        assert!((lin - 0.01446).abs() < 0.0005, "lin={lin}");
    }

    #[test]
    fn srgb_encode_ramps() {
        assert_eq!(encode_srgb(0.0), 0);
        assert_eq!(encode_srgb(1.0), 255);
        let mid = f32::from(encode_srgb(0.5));
        assert!(mid > 100.0 && mid < 200.0, "mid=0.5 -> {mid}");
    }

    #[test]
    fn downsample_keeps_alpha() {
        // 2x2 supersample of one opaque red sample over transparency.
        let mut fb = Framebuffer::new(2, 2).unwrap();
        fb.clear([0.0, 0.0, 0.0, 0.0]);
        fb.write(0, 0, 0.0, [1.0, 0.0, 0.0, 1.0]); // (0,0) covered red
        let out = downsample_premul(&fb, 1, 1).unwrap();
        // Averaged premultiplied color = [0.25,0,0,0.25]; un-premultiplied
        // color stays pure red (coverage changes alpha, not hue).
        assert_eq!(out[0], 255, "visible color = pure red");
        assert_eq!(out[1], 0);
        assert_eq!(out[2], 0);
        assert_eq!(out[3], (0.25 * 255.0f32).round() as u8, "25% coverage");
    }

    #[test]
    fn png_roundtrip() {
        let rgba = vec![0u8; 16]; // 2x2
        let png = encode_png(2, 2, &rgba).unwrap();
        assert!(validate_png_bytes(&png).is_ok());
        assert!(validate_png_bytes(b"not a png").is_err());
    }
}
