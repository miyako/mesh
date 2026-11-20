//! Input loading pipeline: read a file (bounded), detect its format,
//! parse it into canonical geometry. Kept separate from the CLI so both
//! the binary and library consumers share one code path.

use std::path::Path;

use crate::diagnostics::error::{Error, Result};
use crate::diagnostics::limits;
use crate::formats::detect::{detect_format, DetectedFormat};
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

/// Load and parse an input file into canonical geometry.
pub fn load_geometry(path: &Path) -> Result<LoadedGeometry> {
    let data = read_file_limited(path)?;
    let format = detect_format(path)?;
    let loaded = match format.kind {
        crate::formats::detect::FormatKind::Stl => {
            let mesh = crate::formats::stl::parse(&data, format.variant)?;
            let warnings = mesh.warnings.clone().unwrap_or_default();
            LoadedGeometry {
                geometry: Geometry::Mesh(mesh),
                format,
                warnings,
                file_size: data.len() as u64,
            }
        }
        crate::formats::detect::FormatKind::Ply => {
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
