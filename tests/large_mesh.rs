//! Large-mesh sanity at the public API: a generated UV sphere (~80k
//! triangles) must render successfully, deterministically, and cover a
//! plausible number of pixels. This pins the CPU path's behavior on
//! real-world-size meshes in CI (the 100k-triangle benchmark in
//! benches/render_bench.rs is a perf canary, not a correctness gate).

use mesh_thumb::geometry::types::{Geometry, Mesh, ParseWarnings, Vec3};
use mesh_thumb::render::options::RenderOptions;

/// UV-sphere mesh with `2 * rings * segments` triangles, flat shading
/// (no vertex normals, like an STL source).
fn uv_sphere(rings: u32, segments: u32) -> Mesh {
    let mut mesh = Mesh::empty();
    let pi = std::f32::consts::PI;
    for r in 0..=rings {
        let phi = pi * r as f32 / rings as f32;
        let (sp, cp) = phi.sin_cos();
        for s in 0..=segments {
            let theta = 2.0 * pi * s as f32 / segments as f32;
            let (st, ct) = theta.sin_cos();
            mesh.positions.push(Vec3::new(sp * ct, cp, sp * st));
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

fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    let decoder = png::Decoder::new(bytes);
    let reader = decoder.read_info().expect("valid png");
    (reader.info().width, reader.info().height)
}

/// Count pixels that differ from the default background (#20242b) and
/// are sufficiently opaque.
fn object_pixels(bytes: &[u8]) -> usize {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().expect("valid png");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    reader.next_frame(&mut buf).expect("single frame");
    let samples = reader.info().color_type.samples();
    let mut count = 0;
    for px in buf.chunks_exact(samples) {
        let (r, g, b) = (px[0], px[1], px[2]);
        let a = if samples == 4 { px[3] } else { 255 };
        if (r, g, b) != (32, 36, 43) && a > 200 {
            count += 1;
        }
    }
    count
}

#[test]
fn large_sphere_renders_deterministically_and_cover_pixels() {
    let mesh = uv_sphere(200, 200); // 200*200*2 = 80,000 triangles
    assert_eq!(mesh.triangles.len(), 80_000);

    let opts = RenderOptions {
        width: 128,
        height: 128,
        ..Default::default()
    };
    let a = mesh_thumb::render::render(Geometry::Mesh(mesh.clone()), &opts)
        .expect("large mesh renders");
    let b = mesh_thumb::render::render(Geometry::Mesh(mesh), &opts).expect("rerender");

    assert_eq!(
        a.png, b.png,
        "render must be deterministic for identical inputs"
    );
    assert_eq!(png_dimensions(&a.png), (128, 128));
    let covered = object_pixels(&a.png);
    assert!(
        (2000..16_384).contains(&covered),
        "unexpected object coverage: {covered} pixels"
    );
}
