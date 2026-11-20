//! `render` subcommand: one input file -> one PNG (with optional
//! content-addressed cache; cache lookup happens before parsing).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cache::key::{cache_key, content_hash};
use crate::cache::Cache;
use crate::cli::args::RenderArgs;
use crate::cli::json::{self, GeometrySummary, InputInfo, Response, WarningsJson};
use crate::diagnostics::error::{Error, ErrorCode, Result};
use crate::formats::load::{load_geometry, read_file_limited};

/// Result payload of a render.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RenderResult {
    pub input: InputInfo,
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub render_ms: u64,
    pub cache: String, // "hit" | "miss" | "off"
    pub geometry: GeometrySummary,
    pub warnings: WarningsJson,
    /// Normalized scale applied to the model (relative to source units).
    pub normalize_scale: f32,
}

/// Default output path: input with `.png` extension.
pub fn resolve_output(input: &Path, given: Option<&Path>) -> PathBuf {
    match given {
        Some(p) => p.to_path_buf(),
        None => input.with_extension("png"),
    }
}

/// Geometry summary used by inspect and render JSON.
pub fn geometry_summary(geometry: &crate::geometry::types::Geometry) -> GeometrySummary {
    match geometry {
        crate::geometry::types::Geometry::Mesh(m) => GeometrySummary {
            kind: "mesh".to_string(),
            vertices: m.positions.len(),
            triangles: m.triangles.len(),
            colors: m.colors.is_some(),
            normals: m.normals.is_some(),
            bounds: json::BoundsJson::from(&m.bounds),
        },
        crate::geometry::types::Geometry::PointCloud(p) => GeometrySummary {
            kind: "point_cloud".to_string(),
            vertices: p.positions.len(),
            triangles: 0,
            colors: p.colors.is_some(),
            normals: p.normals.is_some(),
            bounds: json::BoundsJson::from(&p.bounds),
        },
    }
}

/// Write bytes atomically: temp file in the same directory, then rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = match path.parent() {
        Some(d) if d.as_os_str().is_empty() => PathBuf::from("."),
        Some(d) => d.to_path_buf(),
        None => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(path, &e))?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".to_string());
    let tmp = dir.join(format!(".{stem}.tmp{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| Error::io(&tmp, &e))?;
        f.write_all(bytes).map_err(|e| Error::io(&tmp, &e))?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::new(
            ErrorCode::OutputError,
            format!("cannot write output '{}': {e}", path.display()),
        )
    })?;
    Ok(())
}

/// Top-level entry for the render subcommand. Returns the JSON response
/// when `--json` is set (also written as a separate step by the caller).
pub fn execute(args: &RenderArgs) -> Result<Option<Response>> {
    let started = std::time::Instant::now();
    let opts = args
        .render
        .to_options()
        .map_err(|m| Error::new(ErrorCode::InvalidRenderOption, m))?;
    let out_path = resolve_output(&args.input.input, args.output.as_deref());

    let cache = if args.no_cache || args.cache_dir.is_none() {
        Cache::disabled()
    } else {
        Cache::open(args.cache_dir.as_deref())
    };

    // 1. Read raw bytes (bounded) and hash for the cache key.
    let data = read_file_limited(&args.input.input)?;
    let input_hash = content_hash(&data);
    let settings = opts.settings_string();
    let key = cache_key(&input_hash, &settings);

    // 2. Cache lookup before parsing (spec §19.2).
    let mut cache_status = if cache.is_enabled() { "miss" } else { "off" };
    let png_bytes = if let Some(hit) = cache.get(&key) {
        cache_status = "hit";
        hit
    } else {
        let loaded = load_geometry(&args.input.input)?;
        let output = crate::render::render(loaded.geometry, &opts)?;
        if cache.is_enabled() {
            // Best-effort store; a cache failure is never fatal.
            let _ = cache.put(&key, &input_hash, &settings, &output.png);
        }
        output.png
    };

    // 3. Validate PNG bytes, then publish atomically.
    crate::render::image::validate_png_bytes(&png_bytes)?;
    write_atomic(&out_path, &png_bytes)?;

    // 4. Diagnostics summary (a light parse for counts; cheap relative
    //    to rendering, and required for inspect-style output).
    let loaded = load_geometry(&args.input.input)?;
    let geometry = geometry_summary(&loaded.geometry);
    let render_ms = started.elapsed().as_millis() as u64;

    let result = RenderResult {
        input: InputInfo::from_loaded(&args.input.input, &loaded.format, loaded.file_size),
        output: out_path.to_string_lossy().into_owned(),
        width: opts.width,
        height: opts.height,
        render_ms,
        cache: cache_status.to_string(),
        geometry,
        warnings: WarningsJson::from(&loaded.warnings),
        normalize_scale: crate::versions::NORMALIZE_SIZE,
    };

    if args.json {
        return Ok(Some(Response::ok(result)));
    }
    println!(
        "rendered {} -> {} ({}x{}, {} vertices, cache: {})",
        args.input.input.display(),
        out_path.display(),
        opts.width,
        opts.height,
        result.geometry.vertices,
        cache_status
    );
    Ok(None)
}
