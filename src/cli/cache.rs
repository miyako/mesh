//! `cache` subcommand: cache maintenance (currently: prune).

use serde::Serialize;

use crate::cache::Cache;
use crate::cli::args::PruneArgs;
use crate::cli::json::Response;
use crate::diagnostics::error::Result;

/// Result payload of `cache prune` (also the JSON body with --json).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PruneResult {
    pub cache_dir: String,
    #[serde(flatten)]
    pub stats: crate::cache::PruneStats,
}

/// Run `cache prune`. Returns the JSON response when `--json`.
pub fn execute_prune(args: &PruneArgs) -> Result<Option<Response>> {
    let cutoff = match (args.older_than, args.all) {
        (Some(days), false) => {
            // Whole days into the past; everything before this instant.
            crate::versions::now_epoch_ms().saturating_sub(days.saturating_mul(86_400_000))
        }
        (None, true) => u64::MAX,
        _ => unreachable!("clap ArgGroup enforces exactly one of --all / --older-than"),
    };

    let cache = Cache::open(Some(args.dir.as_path()));
    let stats = cache.prune(cutoff)?;

    let result = PruneResult {
        cache_dir: args.dir.to_string_lossy().into_owned(),
        stats,
    };
    if args.json {
        return Ok(Some(Response::ok(result)));
    }
    println!(
        "pruned {} entries, removed {} orphan blobs, freed {} bytes (cache: {})",
        stats.entries_pruned,
        stats.orphan_blobs_removed,
        stats.bytes_freed,
        args.dir.display()
    );
    Ok(None)
}
