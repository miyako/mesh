//! Deterministic "fuzz" smoke test.
//!
//! `cargo-fuzz` is not installed in this environment, so instead of a
//! libFuzzer harness this integration test performs a *deterministic
//! mutation walk* over the fixtures: for fixed seeds it flips bits,
//! truncates, duplicates and ASCII-mangles bytes, then pushes every
//! mutant through the full pipeline (parse -> normalize -> render ->
//! encode).
//!
//! Invariants asserted for every mutant:
//!   - no panic (catch_unwind);
//!   - a parse/render error is fine, but must be a *typed* `Error`
//!     (never poisoned state, never a panic);
//!   - if the pipeline succeeds, the output PNG must validate.
//!
//! The same corpus is the basis of the real `fuzz/` target
//! (libFuzzer) which can be run where `cargo-fuzz` is available.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// Small deterministic xorshift64 RNG (fixed seeds -> reproducible runs).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(seed: u64, base: &[u8], steps: usize) -> Vec<u8> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut data = base.to_vec();
    for _ in 0..steps {
        if data.is_empty() {
            // Refill from a random tail so mutation stays productive.
            data.extend_from_slice(&base[rng.below(base.len())..]);
            continue;
        }
        match rng.below(4) {
            0 => {
                // Bit flip at a random offset.
                let i = rng.below(data.len());
                data[i] ^= 1 << rng.below(8);
            }
            1 => {
                // Random byte overwrite.
                let i = rng.below(data.len());
                data[i] = (rng.next() & 0xff) as u8;
            }
            2 => {
                // Truncate (or grow by duplicating a tail).
                if data.len() > 16 && rng.below(2) == 0 {
                    data.truncate(rng.below(data.len()));
                } else {
                    let take = rng.below(data.len()).min(64);
                    let tail = data[data.len().saturating_sub(take)..].to_vec();
                    data.extend_from_slice(&tail);
                }
            }
            _ => {
                // ASCII-ish corruption: overwrite a run with printable text.
                let start = rng.below(data.len());
                let run = 1 + rng.below(48);
                for k in 0..run {
                    let i = start + k;
                    if i < data.len() {
                        data[i] = b' ' + (rng.next() % 94) as u8;
                    }
                }
            }
        }
    }
    data
}

fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("mesh_fuzz_smoke_{}", std::process::id()))
}

fn run_mutant(name: &str, data: &[u8]) -> Result<(), String> {
    // Write the mutant to a temp file and feed it through the exact same
    // file-based path the CLI uses (detect -> parse -> render -> encode).
    let dir = work_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("mutant.bin");
    std::fs::write(&input, data).map_err(|e| format!("[{name}] write: {e}"))?;

    let loaded = catch_unwind(AssertUnwindSafe(|| {
        mesh_thumb::formats::load::load_geometry(&input)
    }))
    .map_err(|_| format!("[{name}] PANIC during load_geometry"))?;

    let loaded = match loaded {
        Ok(l) => l,
        Err(e) => {
            // Typed parse errors are acceptable; assert they carry a code.
            let _ = e.code.as_str();
            return Ok(());
        }
    };

    let opts = mesh_thumb::render::options::RenderOptions {
        width: 32,
        height: 32,
        ..Default::default()
    };
    let rendered = catch_unwind(AssertUnwindSafe(|| {
        mesh_thumb::render::render(loaded.geometry, &opts)
    }))
    .map_err(|_| format!("[{name}] PANIC during render"))?;
    let rendered = match rendered {
        Ok(r) => r,
        Err(e) => {
            let _ = e.code.as_str();
            return Ok(());
        }
    };

    // If everything succeeded, the PNG must be valid and finite-sized.
    mesh_thumb::render::image::validate_png_bytes(&rendered.png)
        .map_err(|e| format!("[{name}] rendered PNG failed validation: {e}"))
}

#[test]
fn mutated_inputs_never_panic() {
    let corpus = [
        ("cube_ascii.stl", fixture("cube_ascii.stl")),
        ("tri_binary.stl", fixture("tri_binary.stl")),
        ("ring_ascii.ply", fixture("ring_ascii.ply")),
    ];
    let mut runs = 0usize;
    let mut failures = Vec::new();
    for (name, path) in &corpus {
        let base = std::fs::read(path).unwrap();
        for seed in 0..80u64 {
            let mutant = mutate(seed, &base, 3 + (seed % 6) as usize);
            if let Err(msg) = run_mutant(name, &mutant) {
                failures.push((name.to_string(), seed, msg));
                if failures.len() >= 5 {
                    break;
                }
            }
            runs += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "mutation smoke failed after {runs} runs: {failures:?}"
    );
    // Sanity guard so the test does not silently shrink to nothing.
    assert!(runs >= 200, "mutation corpus too small: {runs}");
}

#[test]
fn empty_and_tiny_inputs_are_typed_errors() {
    // Degenerate inputs must produce typed errors, never panics.
    std::fs::create_dir_all(work_dir()).unwrap();
    for data in [&[][..], &[0u8][..], b"ply", b"solid", b"xyz"] {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let dir = work_dir();
            let input = dir.join("tiny.bin");
            std::fs::write(&input, data).unwrap();
            mesh_thumb::formats::load::load_geometry(&input)
        }));
        match result {
            Err(_) => panic!("panic on tiny input {:?}", &data[..data.len().min(8)]),
            Ok(Ok(_)) => {} // some tiny inputs may parse as valid minimal meshes
            Ok(Err(e)) => {
                let _ = e.code.as_str(); // typed error with a stable code
            }
        }
    }
}
