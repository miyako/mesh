//! Robust format detection without fully parsing the file.
//!
//! Rules:
//! - Binary STL: file length exactly equals `84 + 50 * count` (checked
//!   arithmetic). Count must be plausible and within limits.
//! - ASCII STL: text with recognizable `facet`/`vertex` structure.
//!   A leading `solid` is not sufficient (binary headers may contain it).
//! - PLY: magic `ply` first line; header validated by the PLY parser.
//!   The first three bytes are enough to tentatively identify PLY, but
//!   the header still gets fully validated before parsing.

use std::path::Path;

use crate::diagnostics::error::{Error, Result};
use crate::diagnostics::limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Stl,
    Ply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatVariant {
    StlBinary,
    StlAscii,
    PlyAscii,
    PlyBinaryLe,
    PlyBinaryBe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedFormat {
    pub kind: FormatKind,
    pub variant: FormatVariant,
    /// Same as variant but as a stable string for the JSON protocol.
    pub id: &'static str,
}

impl DetectedFormat {
    fn new(variant: FormatVariant) -> Self {
        let id = match variant {
            FormatVariant::StlBinary => "stl_binary",
            FormatVariant::StlAscii => "stl_ascii",
            FormatVariant::PlyAscii => "ply_ascii",
            FormatVariant::PlyBinaryLe => "ply_binary_le",
            FormatVariant::PlyBinaryBe => "ply_binary_be",
        };
        let kind = match variant {
            FormatVariant::StlBinary | FormatVariant::StlAscii => FormatKind::Stl,
            _ => FormatKind::Ply,
        };
        DetectedFormat { kind, variant, id }
    }
}

/// Check the binary-STL length signature: `84 + 50 * count`.
fn binary_stl_length_matches(len: u64) -> Option<u64> {
    if len < 84 {
        return None;
    }
    let count = (len - 84) / 50;
    let expected = 84u64.checked_add(50u64.checked_mul(count)?)?;
    if expected == len {
        Some(count)
    } else {
        None
    }
}

/// Fast detection from file length + a small header sample.
///
/// `first_line`/`start` should contain the first few hundred bytes.
pub fn detect_from_sample(path: &Path, len: u64, start: &[u8]) -> Result<DetectedFormat> {
    limits::validate_input_size(len)?;

    // PLY magic must be the very first bytes.
    if start.starts_with(b"ply") {
        // Variant resolved by the PLY header parser later; classify here.
        let variant = if is_ply_binary_hint(start) {
            let header = std::str::from_utf8(start).unwrap_or("");
            if header.contains("format binary_big_endian") {
                FormatVariant::PlyBinaryBe
            } else if header.contains("format binary_little_endian") {
                FormatVariant::PlyBinaryLe
            } else {
                FormatVariant::PlyAscii
            }
        } else {
            FormatVariant::PlyAscii
        };
        return Ok(DetectedFormat::new(variant));
    }

    // Binary STL length signature.
    if let Some(count) = binary_stl_length_matches(len) {
        if count <= limits::MAX_TRIANGLES as u64 && count > 0 {
            return Ok(DetectedFormat::new(FormatVariant::StlBinary));
        }
    }

    // ASCII STL structural check: must contain the facet grammar.
    if looks_like_ascii_stl(start) {
        return Ok(DetectedFormat::new(FormatVariant::StlAscii));
    }

    // A file whose length does not match binary STL and whose start is
    // not text is ambiguous or unsupported.
    if len == 0 {
        return Err(Error::unsupported_format(path, "empty file"));
    }
    if !is_probably_text(start) && binary_stl_length_matches(len).is_some() {
        // Length matches binary but count is out of limit -> report size.
        return Err(Error::resource_limit(
            "file length suggests binary STL with triangle count beyond limit".to_string(),
        ));
    }
    Err(Error::unsupported_format(
        path,
        "contents do not match STL (binary length signature or ASCII facet structure) or PLY ('ply' magic)",
    ))
}

fn is_ply_binary_hint(start: &[u8]) -> bool {
    // PLY binary headers have NUL bytes near the start (header padded
    // with spaces or NULs in some files); ascii header is pure ASCII
    // text. We detect binary only if a `format binary` declaration is
    // visible in the first sample.
    let s = match std::str::from_utf8(start) {
        Ok(s) => s,
        Err(_) => return false,
    };
    s.contains("format binary_little_endian") || s.contains("format binary_big_endian")
}

fn looks_like_ascii_stl(start: &[u8]) -> bool {
    if !is_probably_text(start) {
        return false;
    }
    let s = match std::str::from_utf8(start) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let lower = s.to_ascii_lowercase();
    // Typical ASCII STL contains the facet/vertex grammar; require at
    // least two distinct keywords to reduce false positives.
    let has_facet = lower.contains("facet");
    let has_vertex = lower.contains("vertex");
    let has_solid = lower.contains("solid");
    (has_facet && has_vertex) || (has_facet && has_solid)
}

fn is_probably_text(start: &[u8]) -> bool {
    let sample = &start[..start.len().min(1024)];
    if sample.is_empty() {
        return false;
    }
    sample.iter().filter(|&&b| b == 0).count() == 0
        && sample
            .iter()
            .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace())
}

/// Resolve the source file and detect its format. Reads just enough of
/// the file for detection (bounded chunk), not the whole file.
pub fn detect_format(path: &Path) -> Result<DetectedFormat> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| Error::io(path, &e))?;
    let len = f.metadata().map_err(|e| Error::io(path, &e))?.len();
    limits::validate_input_size(len)?;
    let mut start = [0u8; 4096];
    let mut n = 0;
    while n < start.len() {
        let read = f.read(&mut start[n..]).map_err(|e| Error::io(path, &e))?;
        if read == 0 {
            break;
        }
        n += read;
    }
    detect_from_sample(path, len, &start[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_stl_length() {
        assert_eq!(binary_stl_length_matches(84), Some(0));
        assert_eq!(binary_stl_length_matches(84 + 50 * 3), Some(3));
        assert_eq!(binary_stl_length_matches(85 + 50 * 3), None);
        assert_eq!(binary_stl_length_matches(10), None);
    }

    #[test]
    fn ply_magic_detected() {
        let d = detect_from_sample(
            Path::new("x.ply"),
            100,
            b"ply\nformat ascii 1.0\nelement vertex 3\nend_header\n",
        )
        .unwrap();
        assert_eq!(d.variant, FormatVariant::PlyAscii);
    }

    #[test]
    fn binary_stl_detected_by_length() {
        let d = detect_from_sample(Path::new("x.stl"), 84 + 50 * 2, b"solid").unwrap();
        assert_eq!(d.variant, FormatVariant::StlBinary);
    }

    #[test]
    fn ascii_stl_detected_by_structure() {
        let d = detect_from_sample(
            Path::new("x.stl"),
            200,
            b"solid cube\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid cube\n",
        )
        .unwrap();
        assert_eq!(d.variant, FormatVariant::StlAscii);
    }

    #[test]
    fn solid_prefix_not_enough_for_binary() {
        // Binary-like length but text with only `solid`: length decides.
        let d = detect_from_sample(
            Path::new("x.stl"),
            84 + 50,
            b"solid starts a binary header too",
        )
        .unwrap();
        assert_eq!(d.variant, FormatVariant::StlBinary);
    }

    #[test]
    fn empty_rejected() {
        assert!(detect_from_sample(Path::new("x.stl"), 0, b"").is_err());
    }

    #[test]
    fn garbage_rejected() {
        let d = detect_from_sample(Path::new("x.xyz"), 10, b"\x00\x01garbage\xff");
        assert!(d.is_err());
    }
}
