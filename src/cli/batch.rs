//! `batch` subcommand: render many files to a directory. Every file is
//! processed independently; per-file errors are collected and reported
//! without aborting the rest (unless `--abort-on-error`), per spec §14.

use std::path::Path;

use serde::Serialize;

use crate::cache::key::{cache_key, content_hash};
use crate::cache::Cache;
use crate::cli::args::BatchArgs;
use crate::cli::json::{ErrorJson, Response};
use crate::cli::render::{resolve_output, write_atomic};
use crate::diagnostics::error::{Error, ErrorCode, Result};
use crate::diagnostics::limits;
use crate::formats::load::{load_geometry, read_file_limited};
use crate::render::options::RenderOptions;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchItemResult {
    pub input: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorJson>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BatchResult {
    pub items: Vec<BatchItemResult>,
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
}

/// Render one item (also used by the future threaded runner).
fn render_one(
    input: &Path,
    opts: &RenderOptions,
    cache: &Cache,
    out_dir: &Path,
) -> std::result::Result<BatchItemResult, Error> {
    let data = read_file_limited(input)?;
    let input_hash = content_hash(&data);
    let settings = opts.settings_string();
    let key = cache_key(&input_hash, &settings);

    let mut cache_status = if cache.is_enabled() { "miss" } else { "off" };
    let png_bytes = if let Some(hit) = cache.get(&key) {
        cache_status = "hit";
        hit
    } else {
        let loaded = load_geometry(input)?;
        let output = crate::render::render(loaded.geometry, opts)?;
        if cache.is_enabled() {
            let _ = cache.put(&key, &input_hash, &settings, &output.png);
        }
        output.png
    };
    crate::render::image::validate_png_bytes(&png_bytes)?;

    let out = resolve_output(
        input,
        Some(
            &out_dir.join(
                input
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
                    + ".png",
            ),
        ),
    );
    write_atomic(&out, &png_bytes)?;

    Ok(BatchItemResult {
        input: input.to_string_lossy().into_owned(),
        ok: true,
        output: Some(out.to_string_lossy().into_owned()),
        width: Some(opts.width),
        height: Some(opts.height),
        cache: Some(cache_status.to_string()),
        error: None,
    })
}

/// Run `batch`. Returns the JSON response when `--json`.
pub fn execute(args: &BatchArgs) -> Result<Option<Response>> {
    let opts = args
        .render
        .to_options()
        .map_err(|m| Error::new(ErrorCode::InvalidRenderOption, m))?;
    let concurrency = limits::DEFAULT_BATCH_CONCURRENCY.min(limits::MAX_BATCH_CONCURRENCY);
    let _ = concurrency; // sequential MVP; threaded runner in a later phase

    std::fs::create_dir_all(&args.out).map_err(|e| Error::io(&args.out, &e))?;

    let cache = if args.no_cache || args.cache_dir.is_none() {
        Cache::disabled()
    } else {
        Cache::open(args.cache_dir.as_deref())
    };

    let mut items = Vec::with_capacity(args.files.len());
    let mut ok = 0usize;
    for input in &args.files {
        match render_one(input, &opts, &cache, &args.out) {
            Ok(item) => {
                if item.ok {
                    ok += 1;
                }
                items.push(item);
            }
            Err(e) => {
                if args.abort_on_error {
                    return Err(e);
                }
                items.push(BatchItemResult {
                    input: input.to_string_lossy().into_owned(),
                    ok: false,
                    output: None,
                    width: None,
                    height: None,
                    cache: None,
                    error: Some(ErrorJson::from(&e)),
                });
            }
        }
    }

    let failed = items.len() - ok;
    let result = BatchResult {
        items,
        total: args.files.len(),
        ok,
        failed,
    };

    if args.json {
        return Ok(Some(Response::ok(result)));
    }
    println!(
        "batch: {} ok, {} failed (of {}) -> {}",
        ok,
        failed,
        args.files.len(),
        args.out.display()
    );
    Ok(None)
}
