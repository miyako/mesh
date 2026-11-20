//! Resource limits for untrusted input. All limits are documented in
//! `docs/security.md`. Limits exist to keep memory and CPU bounded on
//! arbitrary (possibly malicious) files; they are intentionally
//! conservative for a thumbnail workload.

use crate::diagnostics::error::{Error, ErrorCode, Result};

/// Safe upper bound on a single input file, in bytes.
pub const MAX_INPUT_FILE_BYTES: u64 = 1 << 30; // 1 GiB
/// Maximum size of a PLY header in bytes.
pub const MAX_PLY_HEADER_BYTES: usize = 64 * 1024;
/// Maximum PLY header line length.
pub const MAX_PLY_LINE_LEN: usize = 4096;
/// Maximum STL ASCII line length.
pub const MAX_STL_LINE_LEN: usize = 256;
/// Maximum vertices per file.
pub const MAX_VERTICES: usize = 20_000_000;
/// Maximum triangles per file.
pub const MAX_TRIANGLES: usize = 10_000_000;
/// Maximum point count (point clouds).
pub const MAX_POINTS: usize = 20_000_000;
/// Maximum face vertex-list length before triangulation.
pub const MAX_FACE_LIST_LEN: usize = 1_000_000;
/// Maximum single output dimension (width or height).
pub const MAX_OUTPUT_DIMENSION: u32 = 8192;
/// Maximum total output pixels.
pub const MAX_OUTPUT_PIXELS: u64 = 64 * 1024 * 1024; // 64 MP
/// Maximum supersampling scale.
pub const MAX_SUPERSAMPLE: u32 = 8;
/// Maximum render framebuffer bytes (supersampled RGBA8).
pub const MAX_FRAMEBUFFER_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
/// Maximum number of concurrent batch jobs.
pub const MAX_BATCH_CONCURRENCY: usize = 16;
/// Default batch concurrency.
pub const DEFAULT_BATCH_CONCURRENCY: usize = 4;

fn checked_framebuffer_bytes(width: u32, height: u32, quality: u32) -> Result<u64> {
    let w = u64::from(width);
    let h = u64::from(height);
    let q = u64::from(quality);
    w.checked_mul(h)
        .and_then(|p| p.checked_mul(q))
        .and_then(|p| p.checked_mul(q))
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| Error::resource_limit("framebuffer size overflow"))
}

/// Validate output dimensions (before supersampling) and compute the
/// supersampled framebuffer size.
pub fn validate_output_dimensions(width: u32, height: u32, quality: u32) -> Result<(u64, u64)> {
    if width == 0 || height == 0 {
        return Err(Error::invalid_render_option(
            "output width and height must be >= 1",
        ));
    }
    if width > MAX_OUTPUT_DIMENSION || height > MAX_OUTPUT_DIMENSION {
        return Err(Error::resource_limit(format!(
            "output dimension {width}x{height} exceeds maximum {}",
            MAX_OUTPUT_DIMENSION
        )));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_OUTPUT_PIXELS {
        return Err(Error::resource_limit(format!(
            "output pixel count {pixels} exceeds maximum {}",
            MAX_OUTPUT_PIXELS
        )));
    }
    if quality == 0 {
        return Err(Error::invalid_render_option(
            "quality (supersampling scale) must be >= 1",
        ));
    }
    if quality > MAX_SUPERSAMPLE {
        return Err(Error::invalid_render_option(format!(
            "quality {quality} exceeds maximum supersampling scale {MAX_SUPERSAMPLE}"
        )));
    }
    let fb = checked_framebuffer_bytes(width, height, quality)?;
    if fb > MAX_FRAMEBUFFER_BYTES {
        return Err(Error::resource_limit(format!(
            "framebuffer would use {fb} bytes, exceeding limit {}",
            MAX_FRAMEBUFFER_BYTES
        )));
    }
    Ok((
        u64::from(width) * u64::from(quality),
        u64::from(height) * u64::from(quality),
    ))
}

/// Validate that counts from a parsed file stay within limits.
pub fn validate_counts(vertices: usize, triangles: usize, points: usize) -> Result<()> {
    if vertices > MAX_VERTICES {
        return Err(Error::resource_limit(format!(
            "vertex count {vertices} exceeds maximum {}",
            MAX_VERTICES
        )));
    }
    if triangles > MAX_TRIANGLES {
        return Err(Error::resource_limit(format!(
            "triangle count {triangles} exceeds maximum {}",
            MAX_TRIANGLES
        )));
    }
    if points > MAX_POINTS {
        return Err(Error::resource_limit(format!(
            "point count {points} exceeds maximum {}",
            MAX_POINTS
        )));
    }
    Ok(())
}

/// Validate the input file size against the configured limit.
pub fn validate_input_size(len: u64) -> Result<()> {
    if len > MAX_INPUT_FILE_BYTES {
        return Err(Error::resource_limit(format!(
            "input file size {len} bytes exceeds maximum {}",
            MAX_INPUT_FILE_BYTES
        )));
    }
    Ok(())
}

/// Translate an overflow-producing checked conversion into a clean error.
pub fn checked_usize(name: &str, v: u64) -> Result<usize> {
    usize::try_from(v).map_err(|_| {
        Error::new(
            ErrorCode::ResourceLimitExceeded,
            format!("{name} too large"),
        )
    })
}
