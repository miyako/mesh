# Architecture

`mesh-thumb` is a headless Rust library + CLI that renders STL and PLY
files to PNG thumbnails entirely on the CPU. It has no GPU and no display
dependency.

## Crate layout

```
src/
  lib.rs                 # public module tree
  main.rs                # thin binary: std::process::exit(cli::main_entry())
  versions.rs            # renderer/cache/json schema versions, tuning constants
  diagnostics/
    error.rs             # Error {code, message, input, stage}, ErrorCode
    limits.rs            # resource limits for untrusted input (docs/security.md)
  formats/
    detect.rs            # STL vs PLY sniffing
    stl.rs               # ASCII state-machine parser + binary parser
    ply.rs               # ascii/binary LE/BE via RowReader, shared parse_body
    load.rs              # load_geometry: detect -> parse -> geometry
  geometry/
    mod.rs               # Geometry enum: Mesh | PointCloud
    bounds.rs            # Bounds3 {min, max, valid}
    normalize.rs         # center + scale to NORMALIZE_SIZE, warnings
    normals.rs           # compute_normals, sanitize_vertex_normals
  camera/
    mod.rs               # Camera, Fit, fit_to_canvas, to_pixels
    presets.rs           # ViewPreset: iso/top/front/side + camera_for
    projection.rs        # orthographic projection helpers
  render/
    mod.rs               # render(): the pipeline driver
    options.rs           # RenderOptions, Background, PointRender, settings_string
    image.rs             # Framebuffer, downsample_premul, encode_srgb, encode_png
    matcap.rs            # 64x64 procedural matcap + shading
    cpu/
      mod.rs             # Backend, Scene, render_geometry (mesh + points)
      points.rs          # point-cloud splat rasterization
  cli/
    mod.rs               # dispatch, exit codes, JSON error envelope
    args.rs              # clap derive definitions
    render.rs            # render subcommand + write_atomic
    inspect.rs           # inspect subcommand
    capabilities.rs      # capabilities subcommand
    batch.rs             # batch subcommand (threaded runner)
    json.rs              # Response/ErrorJson/InputInfo/GeometrySummary
  cache/
    mod.rs               # Cache, Manifest; atomic, thread-safe
    key.rs               # cache_key, content_hash (BLAKE3)
tests/
  cli.rs                 # bin-level integration tests
  golden_images.rs       # golden-image tests vs tests/golden/ref/
fixtures/                # small STL/PLY files used by tests
docs/                    # format-support.md, decisions.md, security.md,
                         # protocol.md, architecture.md (this file)
```

## Render pipeline

`render()` in `src/render/mod.rs`:

1. **Validate output** — `limits::validate_output_dimensions(w, h, quality)`
   bounds dimensions, pixel count, supersampling and framebuffer size.
2. **Resolve backend** — `Backend::resolve()`; `gpu` fails with
   `backend_unavailable` (MVP is CPU-only).
3. **Normalize** — geometry is centered on the origin and scaled so its
   largest extent is `NORMALIZE_SIZE` (1.0), producing
   `normalize_scale` for the JSON result.
4. **Camera** — `camera_for(view)` builds the orthographic frame; the
   iso preset is pinned (see `docs/decisions.md`).
5. **Project** — geometry positions are projected to screen space and
   `fit_to_canvas` computes the uniform scale + centered offset with an
   8% margin.
6. **Framebuffer** — supersampled `(w*q) × (h*q)` buffer, premultiplied
   linear RGBA f32 with a depth channel; cleared to the background
   (solid linear color or transparent).
7. **Shade** — triangles are rasterized per-pixel with depth tests and
   lit by the procedural matcap; point clouds are splatted.
8. **Downsample** — box-filter in premultiplied space (quality 1 = no-op).
9. **Encode** — linear → sRGB → PNG bytes with an sRGB chunk; validated
   by `validate_png_bytes` before an atomic write.

### Color model

All shading happens in **linear light** (`render/image.rs`). sRGB-encoded
inputs — hex backgrounds and PLY uchar/float vertex colors — are
linearized at parse time; the final `encode_srgb` step reproduces the
intended sRGB colors. See `docs/decisions.md` for the rationale.

### Cache

See `src/cache/mod.rs` and `docs/decisions.md`. Lookup happens **before
parsing**: the key is BLAKE3 of the raw input bytes + canonical settings
string + version tags. Any cache I/O failure degrades to a miss.

### Error model

`Error` carries a stable `ErrorCode` (snake_case, `docs/protocol.md`),
a human message, the offending input path, and a stage tag. Human output
is `code: message` on stderr; `--json` additionally emits a structured
envelope on stdout from the same `Error`.