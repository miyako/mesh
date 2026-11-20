//! Simple render benchmarks.
//!
//! `cargo bench` on stable Rust compiles this as a standalone binary that
//! times the full pipeline (load -> parse -> normalize -> render ->
//! encode) on the bundled fixtures and prints per-render averages.
//! Run with `--release` for meaningful numbers:
//!
//!   cargo bench -- --nocapture
//!   # or directly:
//!   cargo run --release --bench render_bench

use std::path::{Path, PathBuf};
use std::time::Instant;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn time_render(path: &Path, opts: &mesh_thumb::render::options::RenderOptions) -> u64 {
    let loaded = mesh_thumb::formats::load::load_geometry(path).unwrap();
    let start = Instant::now();
    mesh_thumb::render::render(loaded.geometry, opts).unwrap();
    start.elapsed().as_micros() as u64
}

fn bench(name: &str, path: &Path, opts: &mesh_thumb::render::options::RenderOptions, iters: u32) {
    // Warm-up (first render pays for allocator/TLS warmup).
    let _ = time_render(path, opts);
    let samples: Vec<u64> = (0..iters).map(|_| time_render(path, opts)).collect();
    let total: u64 = samples.iter().sum();
    let avg = total as f64 / samples.len() as f64;
    let min = samples.iter().min().copied().unwrap_or(0);
    println!(
        "{name:<24} {w}x{h} q{q}  n={iters:<3}  avg {avg:>9.1} us  min {min:>7} us",
        w = opts.width,
        h = opts.height,
        q = opts.quality,
    );
}

fn main() {
    let base = mesh_thumb::render::options::RenderOptions {
        width: 256,
        height: 256,
        ..Default::default()
    };
    let thumb = mesh_thumb::render::options::RenderOptions {
        width: 128,
        height: 128,
        ..Default::default()
    };
    println!("mesh-thumb render benchmarks (release build recommended)");
    bench("cube_ascii 256", &fixture("cube_ascii.stl"), &base, 20);
    bench("cube_ascii 128", &fixture("cube_ascii.stl"), &thumb, 40);
    bench("tri_binary 256", &fixture("tri_binary.stl"), &base, 20);
    bench("ring_ascii 256", &fixture("ring_ascii.ply"), &base, 20);
    println!("\nnote: includes parse+normalize per iteration (cache bypassed)");
}
