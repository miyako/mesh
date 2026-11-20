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

use mesh_thumb::geometry::types::{Geometry, Mesh, ParseWarnings, Vec3};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// UV-sphere mesh with `2 * rings * segments` triangles. With
/// `smooth: true` every vertex carries its own normal (vertex-normal
/// shading path); otherwise the mesh is flat (STL-like).
fn uv_sphere(rings: u32, segments: u32, smooth: bool) -> Mesh {
    let mut mesh = Mesh::empty();
    let pi = std::f32::consts::PI;
    for r in 0..=rings {
        let phi = pi * r as f32 / rings as f32;
        let (sp, cp) = phi.sin_cos();
        for s in 0..=segments {
            let theta = 2.0 * pi * s as f32 / segments as f32;
            let (st, ct) = theta.sin_cos();
            let p = Vec3::new(sp * ct, cp, sp * st);
            mesh.positions.push(p);
            if smooth {
                mesh.normals
                    .get_or_insert_with(Vec::new)
                    .push(p.try_normalize().unwrap_or(Vec3::new(0.0, 1.0, 0.0)));
            }
        }
    }
    let stride = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * stride + s;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            mesh.triangles.push([a, c, b]);
            mesh.triangles.push([b, c, d]);
        }
    }
    mesh.warnings = Some(ParseWarnings {
        degenerate_triangles: 0,
        non_finite_vertices: 0,
    });
    mesh.finalize().unwrap()
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

fn bench_geo(
    name: &str,
    sphere: &Mesh,
    opts: &mesh_thumb::render::options::RenderOptions,
    iters: u32,
) {
    let geo = Geometry::Mesh(sphere.clone());
    let _ = mesh_thumb::render::render(geo, opts).unwrap(); // warm-up
    let samples: Vec<u64> = (0..iters)
        .map(|_| {
            let start = Instant::now();
            mesh_thumb::render::render(Geometry::Mesh(sphere.clone()), opts).unwrap();
            start.elapsed().as_micros() as u64
        })
        .collect();
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

    // Large-mesh scaling: ~100k triangles, flat (STL-like) and smooth
    // (per-vertex normals) shading.
    let sphere_flat = uv_sphere(225, 223, false);
    let sphere_smooth = uv_sphere(225, 223, true);
    println!(
        "sphere: {} tris, {} verts (flat) / {} verts (smooth)",
        sphere_flat.triangles.len(),
        sphere_flat.positions.len(),
        sphere_smooth.normals.as_ref().map(|n| n.len()).unwrap_or(0),
    );
    bench_geo("sphere_100k_flat 256", &sphere_flat, &base, 10);
    bench_geo("sphere_100k_flat 128", &sphere_flat, &thumb, 10);
    bench_geo("sphere_100k_smooth 256", &sphere_smooth, &base, 10);
    println!("\nnote: includes parse+normalize per iteration (cache bypassed)");
}
