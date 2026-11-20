//! Input loading pipeline: read a file (bounded), detect its format,
//! parse it into canonical geometry. Kept separate from the CLI so both
//! the binary and library consumers share one code path.

use std::path::Path;

use crate::diagnostics::error::{Error, Result};
use crate::diagnostics::limits;
use crate::formats::detect::{DetectedFormat, FormatKind};
use crate::geometry::types::{Geometry, ParseWarnings};

/// Result of loading an input file.
#[derive(Debug)]
pub struct LoadedGeometry {
    pub geometry: Geometry,
    pub format: DetectedFormat,
    pub warnings: ParseWarnings,
    /// File size in bytes (used for diagnostics/cache).
    pub file_size: u64,
}

/// Read a file fully with a size limit; used by loaders and hashing.
pub fn read_file_limited(path: &Path) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path).map_err(|e| Error::io(path, &e))?;
    if !meta.is_file() {
        return Err(Error::new(
            crate::diagnostics::error::ErrorCode::IoError,
            "input is not a regular file",
        )
        .with_input(path));
    }
    limits::validate_input_size(meta.len())?;
    let mut f = std::fs::File::open(path).map_err(|e| Error::io(path, &e))?;
    let mut data = Vec::with_capacity(meta.len() as usize);
    std::io::Read::read_to_end(&mut f, &mut data).map_err(|e| Error::io(path, &e))?;
    Ok(data)
}

/// Whether an input path refers to stdin (`-`).
pub fn is_stdin_path(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// Read all of stdin into memory, bounded by the input size limit so a
/// hostile/endless stream cannot exhaust memory.
pub fn read_stdin_limited() -> Result<Vec<u8>> {
    use std::io::Read;
    let mut data = Vec::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = handle
            .read(&mut buf)
            .map_err(|e| Error::io(Path::new("-"), &e))?;
        if n == 0 {
            break;
        }
        limits::validate_input_size(data.len() as u64 + n as u64)?;
        data.extend_from_slice(&buf[..n]);
    }
    Ok(data)
}

/// Load and parse an input file into canonical geometry.
pub fn load_geometry(path: &Path) -> Result<LoadedGeometry> {
    let data = read_file_limited(path)?;
    load_bytes(data, path)
}

/// Parse geometry from in-memory bytes (stdin or other byte sources).
/// `source` is used only for diagnostics/error messages; detection runs
/// on the same 4096-byte sample the file path uses.
pub fn load_bytes(data: Vec<u8>, source: &Path) -> Result<LoadedGeometry> {
    limits::validate_input_size(data.len() as u64)?;
    let sample_len = data.len().min(4096);
    let format =
        crate::formats::detect::detect_from_sample(source, data.len() as u64, &data[..sample_len])?;
    let loaded = match format.kind {
        FormatKind::Stl => {
            let mesh = crate::formats::stl::parse(&data, format.variant)?;
            let warnings = mesh.warnings.clone().unwrap_or_default();
            LoadedGeometry {
                geometry: Geometry::Mesh(mesh),
                format,
                warnings,
                file_size: data.len() as u64,
            }
        }
        FormatKind::Ply => {
            let (geometry, warnings) = crate::formats::ply::parse(&data, format.variant)?;
            LoadedGeometry {
                geometry,
                format,
                warnings,
                file_size: data.len() as u64,
            }
        }
    };
    Ok(loaded)
}
