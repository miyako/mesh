//! Cache key derivation (spec §19.1).

use crate::versions::{CACHE_SCHEMA_VERSION, JSON_SCHEMA_VERSION, RENDERER_VERSION};

/// The fixed prefix that namespaces all cache keys. Any change to the
/// version constants or key shape invalidates old entries (they are
/// simply never looked up).
const KEY_PREFIX: &str = "mesh-thumb-cache";

/// Compute a cache key from raw input bytes + canonical settings.
///
/// `input_hash` is the BLAKE3 hex digest of the raw input bytes.
pub fn cache_key(input_hash: &str, settings: &str) -> String {
    let settings_hash = blake3::hash(settings.as_bytes());
    let settings_hex = crate::cache::hex(settings_hash.as_bytes());
    format!(
        "{KEY_PREFIX}.v{CACHE_SCHEMA_VERSION}.r{RENDERER_VERSION}.j{JSON_SCHEMA_VERSION}.{settings_hex}.{input_hash}"
    )
}

/// BLAKE3 hex digest of raw bytes (the content-addressing primitive).
pub fn content_hash(bytes: &[u8]) -> String {
    let h = blake3::hash(bytes);
    crate::cache::hex(h.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_deterministic_and_sensitive() {
        let s = "v1:256x256:iso:02:#20242bpointauto:auto";
        let k1 = cache_key("aaaa", s);
        let k2 = cache_key("aaaa", s);
        assert_eq!(k1, k2);
        assert_ne!(k1, cache_key("aaab", s), "content hash must matter");
        let mut s2 = s.to_string();
        s2.push('x');
        assert_ne!(k1, cache_key("aaaa", &s2), "settings must matter");
    }

    #[test]
    fn content_hash_stable() {
        let direct = blake3::hash(b"hello").to_hex().to_string();
        assert_eq!(content_hash(b"hello"), direct);
        assert_eq!(content_hash(b"hello"), direct, "deterministic");
        assert_ne!(content_hash(b"hello"), content_hash(b"hellp"));
    }
}
