//! Content-addressed render cache (spec §19).
//!
//! Design:
//! - The cache key is derived from a BLAKE3 hash of the *raw input
//!   bytes* (never the parsed geometry), the canonical settings string,
//!   and the renderer/cache schema versions.
//! - Directory layout:
//!   ```text
//!   <cache_dir>/
//!     manifest.json          # entries: key -> {input_hash, settings, size, created_ms}
//!     blobs/<key>.png        # cached PNG bytes
//!   ```
//! - Lookup happens **before** parsing (spec §19.2: "cache hit before
//!   parse"). A hit reuses the PNG and skips detection/parse/normalize/
//!   render.
//! - Writes are atomic: write to a temp file, `fsync`, rename.
//! - Any cache I/O problem degrades to a miss (never fails the render);
//!   a corrupted manifest is discarded and rebuilt on the next write.

pub mod key;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::diagnostics::error::{Error, Result};
use crate::versions::CACHE_SCHEMA_VERSION;

/// Single manifest entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// BLAKE3 of the raw input bytes.
    pub input_hash: String,
    /// Canonical settings string that produced this entry.
    pub settings: String,
    /// PNG byte length.
    pub size: u64,
    /// Creation time (epoch milliseconds); informational only.
    pub created_ms: u64,
}

/// Manifest file: `manifest.json` in the cache dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub entries: std::collections::HashMap<String, ManifestEntry>,
}

/// Result of a [`Cache::prune`] pass.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PruneStats {
    /// Manifest entries removed (their blobs were deleted too).
    pub entries_pruned: usize,
    /// Blob files not referenced by any manifest entry (removed).
    pub orphan_blobs_removed: usize,
    /// Bytes freed by deleted blob files.
    pub bytes_freed: u64,
}

impl Manifest {
    pub fn empty() -> Manifest {
        Manifest {
            schema_version: CACHE_SCHEMA_VERSION,
            entries: std::collections::HashMap::new(),
        }
    }
}

/// The render cache handle. `enabled: false` when `--no-cache` is given
/// or no cache dir was provided.
///
/// Thread-safety: [`Cache::get`]/[`Cache::put`] are safe to call from
/// concurrent worker threads (batch mode). Reads/writes of `manifest.json`
/// are serialized through an internal mutex; blob temp files use unique
/// names so concurrent writers never collide.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: Option<PathBuf>,
    enabled: bool,
    manifest_lock: Arc<std::sync::Mutex<()>>,
}

impl Cache {
    /// Build a cache rooted at `dir`, or disable it if `dir` is None.
    pub fn open(dir: Option<&Path>) -> Cache {
        match dir {
            Some(d) => Cache {
                dir: Some(d.to_path_buf()),
                enabled: true,
                manifest_lock: Arc::new(std::sync::Mutex::new(())),
            },
            None => Cache {
                dir: None,
                enabled: false,
                manifest_lock: Arc::new(std::sync::Mutex::new(())),
            },
        }
    }

    pub fn disabled() -> Cache {
        Cache::open(None)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn blob_path(&self, cache_key: &str) -> PathBuf {
        self.dir
            .as_ref()
            .expect("blob_path called on enabled cache")
            .join("blobs")
            .join(format!("{cache_key}.png"))
    }

    fn manifest_path(&self) -> PathBuf {
        self.dir
            .as_ref()
            .expect("manifest_path called on enabled cache")
            .join("manifest.json")
    }

    fn load_manifest(&self) -> Manifest {
        let path = self.manifest_path();
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => return Manifest::empty(),
        };
        match serde_json::from_slice::<Manifest>(&bytes) {
            Ok(m) if m.schema_version == CACHE_SCHEMA_VERSION => m,
            _ => Manifest::empty(), // stale/incompatible manifest: rebuild
        }
    }

    /// Look up a key; returns `Some(bytes)` on a valid hit.
    pub fn get(&self, cache_key: &str) -> Option<Vec<u8>> {
        if !self.enabled {
            return None;
        }
        let manifest = {
            let _guard = self.manifest_lock.lock().ok()?;
            self.load_manifest()
        };
        let entry = manifest.entries.get(cache_key)?;
        let path = self.blob_path(cache_key);
        let bytes = std::fs::read(&path).ok()?;
        if bytes.len() as u64 != entry.size {
            return None; // corrupt/mismatched blob
        }
        Some(bytes)
    }

    /// Store bytes under a key (atomic write + manifest update).
    pub fn put(
        &self,
        cache_key: &str,
        input_hash: &str,
        settings: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let dir = self.dir.as_ref().expect("enabled cache has dir");
        let blobs_dir = dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir).map_err(|e| {
            Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                format!("create cache dir: {e}"),
            )
        })?;

        // Atomic blob write: temp -> rename. Unique temp name so concurrent
        // writers (batch mode) never share a temp path.
        let blob = self.blob_path(cache_key);
        static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join("blobs").join(format!(
            ".{}.{}.{:x}.tmp",
            cache_key,
            std::process::id(),
            seq
        ));
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| {
                Error::new(
                    crate::diagnostics::error::ErrorCode::CacheError,
                    format!("open tmp: {e}"),
                )
            })?;
            use std::io::Write;
            f.write_all(bytes).map_err(|e| {
                Error::new(
                    crate::diagnostics::error::ErrorCode::CacheError,
                    format!("write tmp: {e}"),
                )
            })?;
            f.sync_all().ok();
        }
        std::fs::rename(&tmp, &blob).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                format!("publish blob: {e}"),
            )
        })?;

        // Manifest update is serialized so no concurrent reader sees a
        // torn/half-updated manifest.
        let _guard = self.manifest_lock.lock().map_err(|_| {
            Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                "manifest lock poisoned",
            )
        })?;
        let mut manifest = self.load_manifest();
        manifest.entries.insert(
            cache_key.to_string(),
            ManifestEntry {
                input_hash: input_hash.to_string(),
                settings: settings.to_string(),
                size: bytes.len() as u64,
                created_ms: crate::versions::now_epoch_ms(),
            },
        );
        self.replace_manifest(&manifest)
    }

    /// Atomically replace `manifest.json` (temp file + rename).
    fn replace_manifest(&self, manifest: &Manifest) -> Result<()> {
        let dir = self.dir.as_ref().expect("enabled cache has dir");
        let json = serde_json::to_vec_pretty(manifest).map_err(|e| {
            Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                format!("serialize manifest: {e}"),
            )
        })?;
        let manifest_path = self.manifest_path();
        let tmp = dir.join(".manifest.json.tmp");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| {
                Error::new(
                    crate::diagnostics::error::ErrorCode::CacheError,
                    format!("open manifest tmp: {e}"),
                )
            })?;
            use std::io::Write;
            f.write_all(&json).map_err(|e| {
                Error::new(
                    crate::diagnostics::error::ErrorCode::CacheError,
                    format!("write manifest tmp: {e}"),
                )
            })?;
            f.sync_all().ok();
        }
        std::fs::rename(&tmp, &manifest_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                format!("publish manifest: {e}"),
            )
        })?;
        Ok(())
    }

    /// Delete every manifest entry older than `cutoff_ms` (epoch
    /// milliseconds, exclusive) and every blob file not referenced by the
    /// manifest. The manifest is rewritten atomically when anything was
    /// removed. Unlike lookups/writes (which degrade to a miss), cache
    /// problems are surfaced as errors here because pruning is an
    /// explicit user command.
    pub fn prune(&self, cutoff_ms: u64) -> Result<PruneStats> {
        if !self.enabled {
            return Ok(PruneStats::default());
        }
        let dir = self.dir.as_ref().expect("enabled cache has dir");
        if !dir.is_dir() {
            return Err(Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                format!("cache directory does not exist: {}", dir.display()),
            ));
        }
        // Serialize against concurrent render writes so we never prune a
        // half-written state.
        let _guard = self.manifest_lock.lock().map_err(|_| {
            Error::new(
                crate::diagnostics::error::ErrorCode::CacheError,
                "manifest lock poisoned",
            )
        })?;
        let mut manifest = self.load_manifest();
        let mut stats = PruneStats::default();

        // 1. Expired entries (blob + manifest row).
        let expired: Vec<String> = manifest
            .entries
            .iter()
            .filter(|(_, e)| e.created_ms < cutoff_ms)
            .map(|(k, _)| k.clone())
            .collect();
        for key in &expired {
            if let Ok(meta) = std::fs::metadata(self.blob_path(key)) {
                stats.bytes_freed += meta.len();
            }
            let _ = std::fs::remove_file(self.blob_path(key));
            manifest.entries.remove(key);
            stats.entries_pruned += 1;
        }

        // 2. Orphan blobs (files not referenced by any entry).
        let blobs_dir = dir.join("blobs");
        if let Ok(entries) = std::fs::read_dir(&blobs_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                // Skip temp files (our own atomic-write staging).
                if name.starts_with('.') || !name.ends_with(".png") {
                    continue;
                }
                let key = name.trim_end_matches(".png").to_string();
                if manifest.entries.contains_key(&key) {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    stats.bytes_freed += meta.len();
                }
                let _ = std::fs::remove_file(entry.path());
                stats.orphan_blobs_removed += 1;
            }
        }

        if stats.entries_pruned > 0 || stats.orphan_blobs_removed > 0 {
            self.replace_manifest(&manifest)?;
        }
        Ok(stats)
    }
}

/// Short hex format helper for keys.
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miss_then_hit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(Some(dir.path()));
        assert_eq!(cache.get("abc"), None);
        cache
            .put("abc", "hash1", "settings1", b"png-bytes")
            .unwrap();
        assert_eq!(cache.get("abc"), Some(b"png-bytes".to_vec()));
        // Different dir = miss
        let cache2 = Cache::open(Some(dir.path()));
        assert_eq!(cache2.get("abc"), Some(b"png-bytes".to_vec()));
    }

    #[test]
    fn disabled_never_hits() {
        let cache = Cache::disabled();
        assert!(!cache.is_enabled());
        cache.put("k", "h", "s", b"data").unwrap();
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn corrupt_manifest_degrades_to_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(Some(dir.path()));
        cache.put("k", "h", "s", b"data").unwrap();
        // Corrupt the manifest file.
        std::fs::write(dir.path().join("manifest.json"), b"not json{").unwrap();
        assert_eq!(cache.get("k"), None);
        // A subsequent put rebuilds it.
        cache.put("k2", "h", "s", b"more").unwrap();
        assert_eq!(cache.get("k2"), Some(b"more".to_vec()));
    }

    #[test]
    fn corrupted_blob_is_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(Some(dir.path()));
        cache.put("k", "h", "s", b"12345678").unwrap();
        std::fs::write(cache.blob_path("k"), b"SHORT").unwrap();
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn prune_drops_expired_keeps_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(Some(dir.path()));
        cache.put("old", "h1", "s1", b"aaaa").unwrap();
        cache.put("new", "h2", "s2", b"bbbb").unwrap();
        // Backdate "old" so it is clearly expired.
        {
            let path = cache.manifest_path();
            let mut m: Manifest = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            m.entries.get_mut("old").unwrap().created_ms = 1000;
            std::fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
        }
        // Cutoff one hour in the future: "old" (ms 1000) is pruned,
        // "new" (created moments ago) survives deterministically.
        // Cutoff at the fresh entry's own timestamp: "old" (ms 1000) is
        // strictly below it and pruned; "new" is not and survives. Uses
        // only manifest data, so it is independent of wall-clock skew.
        let fresh_ms = {
            let m: Manifest =
                serde_json::from_slice(&std::fs::read(cache.manifest_path()).unwrap()).unwrap();
            m.entries.get("new").unwrap().created_ms
        };
        let stats = cache.prune(fresh_ms).unwrap();
        assert_eq!(stats.entries_pruned, 1);
        assert_eq!(stats.orphan_blobs_removed, 0);
        assert_eq!(stats.bytes_freed, 4);
        assert_eq!(cache.get("old"), None);
        assert_eq!(cache.get("new"), Some(b"bbbb".to_vec()));
        // The manifest no longer lists "old".
        let m: Manifest =
            serde_json::from_slice(&std::fs::read(cache.manifest_path()).unwrap()).unwrap();
        assert!(!m.entries.contains_key("old"));
    }

    #[test]
    fn prune_removes_orphans_and_keeps_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(Some(dir.path()));
        cache.put("k", "h", "s", b"data").unwrap();
        std::fs::write(dir.path().join("blobs").join("orphan.png"), b"stray").unwrap();
        std::fs::write(dir.path().join("blobs").join(".tmpstaging"), b"x").unwrap();
        let stats = cache.prune(u64::MAX).unwrap();
        assert_eq!(stats.entries_pruned, 1);
        assert_eq!(stats.orphan_blobs_removed, 1);
        assert!(!dir.path().join("blobs").join("orphan.png").exists());
        assert!(dir.path().join("blobs").join(".tmpstaging").exists());
        // Second pass is a no-op.
        let stats = cache.prune(u64::MAX).unwrap();
        assert_eq!(stats.entries_pruned, 0);
        assert_eq!(stats.orphan_blobs_removed, 0);
    }

    #[test]
    fn prune_missing_dir_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let cache = Cache::open(Some(&missing));
        assert!(cache.prune(u64::MAX).is_err());
    }
}
