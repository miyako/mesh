//! Version constants that participate in cache identity and the JSON
//! protocol. See `docs/decisions.md` for change policy.

/// Application version, from `Cargo.toml`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Renderer version. Increment whenever pixel output can change:
/// normalization, camera, projection, rasterization, shading, color
/// interpretation, point rendering, supersampling, or defaults that
/// affect output.
pub const RENDERER_VERSION: u32 = 1;

/// Cache schema version. Increment when cache layout or manifest
/// compatibility changes.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// JSON protocol schema version. Increment when externally visible
/// response structure changes incompatibly.
pub const JSON_SCHEMA_VERSION: u32 = 1;

/// Normalization target: the max extent of the model fits this size
/// after centering. Renderer-versioned because changing it changes
/// output composition and therefore cache identity.
pub const NORMALIZE_SIZE: f32 = 1.0;

/// Orthographic projection: models are fitted onto the canvas with this
/// fraction of the smaller canvas dimension reserved as margin.
pub const DEFAULT_MARGIN_FRACTION: f32 = 0.08;

/// Current wall-clock time in epoch milliseconds (used for cache manifest
/// metadata; not part of any cache key).
pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
