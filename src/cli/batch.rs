//! `batch` subcommand: render many files to a directory. Every file is
//! processed independently; per-file errors are collected and reported
//! without aborting the rest (unless `--abort-on-error`), per spec §14.

use std::path::{Path, PathBuf};

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

/// Output path for one batch input: `<out_dir>/<stem>.png`, with the
/// same `resolve_output` rules the render path uses.
fn batch_output_path(input: &Path, out_dir: &Path) -> PathBuf {
    let joined = out_dir.join(
        input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
            + ".png",
    );
    resolve_output(input, Some(&joined))
}

/// Render one item. `out` is the precomputed output path (see
/// `batch_output_path`), resolved once up front so the collision check
/// and the write agree.
fn render_one(
    input: &Path,
    out: &Path,
    opts: &RenderOptions,
    cache: &Cache,
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

    write_atomic(out, &png_bytes)?;

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

fn failure_item(input: &Path, e: &Error) -> BatchItemResult {
    BatchItemResult {
        input: input.to_string_lossy().into_owned(),
        ok: false,
        output: None,
        width: None,
        height: None,
        cache: None,
        error: Some(ErrorJson::from(e)),
    }
}

/// Run `batch`. Returns the JSON response when `--json`.
///
/// Files are rendered concurrently on a worker pool sized by the
/// configured limits (default 4, max 16). Per-file errors are collected
/// without aborting the rest, unless `--abort-on-error` is set (in which
/// case the first fatal error is returned and remaining work is stopped).
///
/// Output collisions are rejected up front: two inputs whose stems map to
/// the same `<out_dir>/<stem>.png` (e.g. `a/cube.stl` + `b/cube.stl`) would
/// otherwise overwrite each other with a nondeterministic winner under
/// concurrency, so the whole batch fails with `batch_error` before any
/// file is written.
pub fn execute(args: &BatchArgs) -> Result<Option<Response>> {
    let opts = args
        .render
        .to_options()
        .map_err(|m| Error::new(ErrorCode::InvalidRenderOption, m))?;
    let concurrency = limits::DEFAULT_BATCH_CONCURRENCY.clamp(1, limits::MAX_BATCH_CONCURRENCY);

    let n = args.files.len();
    // Resolve every output path up front; same path twice = collision.
    let mut out_paths: Vec<PathBuf> = Vec::with_capacity(n);
    let mut first_with: std::collections::HashMap<PathBuf, usize> =
        std::collections::HashMap::new();
    let mut collisions: Vec<String> = Vec::new();
    for (i, input) in args.files.iter().enumerate() {
        let out = batch_output_path(input, &args.out);
        match first_with.get(&out) {
            Some(&first) => collisions.push(format!(
                "inputs '{}' and '{}' both map to '{}'",
                args.files[first].display(),
                input.display(),
                out.display()
            )),
            None => {
                first_with.insert(out.clone(), i);
            }
        }
        out_paths.push(out);
    }
    if !collisions.is_empty() {
        return Err(Error::new(
            ErrorCode::BatchError,
            format!(
                "batch output collision: {} (re-run with distinct stems or --out)",
                collisions.join("; ")
            ),
        ));
    }

    std::fs::create_dir_all(&args.out).map_err(|e| Error::io(&args.out, &e))?;

    let cache = if args.no_cache || args.cache_dir.is_none() {
        Cache::disabled()
    } else {
        Cache::open(args.cache_dir.as_deref())
    };

    let next = std::sync::atomic::AtomicUsize::new(0);
    let abort = std::sync::atomic::AtomicBool::new(false);
    let first_error: std::sync::Mutex<Option<Error>> = std::sync::Mutex::new(None);
    let items: std::sync::Mutex<Vec<Option<BatchItemResult>>> =
        std::sync::Mutex::new((0..n).map(|_| None).collect());

    std::thread::scope(|scope| {
        for _ in 0..concurrency.min(n.max(1)) {
            scope.spawn(|| loop {
                use std::sync::atomic::Ordering;
                if abort.load(Ordering::Relaxed) {
                    return;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    return;
                }
                let item = match render_one(&args.files[i], &out_paths[i], &opts, &cache) {
                    Ok(item) => item,
                    Err(e) => {
                        if args.abort_on_error {
                            abort.store(true, Ordering::Relaxed);
                            let mut slot = first_error.lock().unwrap();
                            if slot.is_none() {
                                *slot = Some(e.clone());
                            }
                        }
                        failure_item(&args.files[i], &e)
                    }
                };
                items.lock().unwrap()[i] = Some(item);
            });
        }
    });

    if args.abort_on_error {
        if let Some(e) = first_error.into_inner().unwrap() {
            return Err(e);
        }
    }

    let collected = items.into_inner().unwrap();
    let items: Vec<BatchItemResult> = collected.into_iter().flatten().collect();
    let ok = items.iter().filter(|it| it.ok).count();
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
