# MeshPreview — Project Specification and Autonomous Implementation Guide

**Document type:** Engineering specification / agent handoff  
**Project:** MeshPreview (`mesh-thumb`)  
**Status:** Implementation-ready specification  
**Primary goal:** Produce deterministic, compact PNG thumbnails from STL and PLY geometry without requiring a GUI, display server, or heavyweight 3D application.

---

## 1. Mission

Build a small, dependable, headless 3D-to-2D thumbnail engine. It accepts STL or PLY files and produces useful, attractive PNG previews suitable for file lists, grids, catalogs, inspectors, and UI components.

The product is intentionally narrow. It should be a reliable geometry thumbnail utility—not a general-purpose 3D viewer, CAD application, model repair tool, renderer, or asset-conversion suite.

The initial implementation must work on a machine without a GPU or graphics context. It should be easy to bundle and invoke from another application, particularly a future 4D component.

### Product principle

> A thumbnail is a predictable visual summary of geometry, not a simulation of a complete 3D rendering pipeline.

Prioritize:
1. Reliability on arbitrary input files.
2. Consistent composition across a collection of models.
3. Small deployment footprint and straightforward embedding.
4. Fast cold rendering and very fast cache hits.
5. Clear errors and safe handling of malformed/untrusted files.
6. A stable CLI and machine-readable protocol.

Do not sacrifice these goals to implement unnecessary rendering features.

---

## 2. Goals and non-goals

### 2.1 MVP goals

The MVP must:

- Be implemented in Rust.
- Provide a native command-line executable named `mesh-thumb`.
- Run headlessly; no window, display server, GPU, or external runtime is required.
- Read STL (ASCII and binary).
- Read PLY (ASCII, binary little-endian, binary big-endian).
- Support triangle meshes and PLY point clouds.
- Parse PLY vertex coordinates, optional vertex normals, optional vertex colors, and faces.
- Accept PLY face lists with 3 or more vertices and triangulate polygons.
- Recompute mesh face normals rather than trusting STL normals.
- Render a stable orthographic thumbnail using a fixed default camera.
- Use a simple embedded matcap or equivalent compact normal-based shading.
- Preserve PLY vertex colors where available.
- Render point clouds as colored screen-space splats.
- Produce PNG output.
- Provide configurable output dimensions, background, view, point size, and quality where applicable.
- Provide a CPU reference renderer as the default and guaranteed renderer.
- Use an input-content hash cache so cache hits can happen before parsing.
- Store cache metadata in a sidecar manifest.
- Write outputs atomically.
- Provide single-file and batch processing.
- Provide JSON output for automation.
- Report per-file errors without aborting an entire batch.
- Enforce limits for input size, geometry counts, memory, and output dimensions.
- Include comprehensive unit, integration, fuzz, and golden-image tests.
- Be portable enough to bundle into a host application without runtime downloads.

### 2.2 Explicit non-goals

Do **not** implement in the MVP:

- A GUI or interactive viewer.
- CAD editing, mesh repair, remeshing, or boolean operations.
- Photorealistic rendering, physically based materials, material systems, texture maps, or scene lighting editors.
- Animation, skeletons, or rigging.
- Importing every 3D format. Only STL and PLY are in scope.
- Automatic semantic orientation inference (“this is a car, orient it upright”).
- A sophisticated camera fitting or composition editor.
- Mesh decimation/simplification as a mandatory step.
- GPU rendering as a requirement.
- Multiple GPU APIs or a collection of platform-specific renderers.
- Runtime downloading of dependencies, assets, binaries, or shaders.
- Pixel-identical CPU and GPU rendering.
- A service/daemon, network API, or cloud component.
- A general image-processing library or image conversion suite.

When a feature is not needed to meet the thumbnail goal, defer it.

---

## 3. Technology and implementation constraints

### 3.1 Language and packaging

- Rust stable.
- Use a conventional Cargo workspace only if it meaningfully separates the CLI, core library, and renderer. Avoid workspace complexity for its own sake.
- Prefer a library core with a thin CLI layer so parser and renderer logic can be tested without invoking the executable.
- Keep dependencies limited and well justified.
- Pin or lock dependency versions in the committed lockfile for reproducible builds.
- No network access at runtime.
- Embed the matcap asset(s) in the executable.
- Do not rely on external system graphics libraries for the CPU MVP.

### 3.2 Suggested internal crate/module organization

The exact names can change, but responsibilities should remain separated:

```text
mesh-thumb/
  Cargo.toml
  Cargo.lock
  README.md
  LICENSE
  docs/
    architecture.md
    format-support.md
    cli.md
    security.md
  src/
    main.rs                 # CLI entry
    lib.rs                 # public/core entry points
    cli/
      mod.rs
      args.rs
      output.rs
    formats/
      mod.rs
      stl.rs
      ply.rs
      detect.rs
    geometry/
      mod.rs
      types.rs
      bounds.rs
      normalize.rs
      triangulate.rs
      normals.rs
      sampling.rs
    camera/
      mod.rs
      presets.rs
      projection.rs
    render/
      mod.rs
      cpu/
        mod.rs
        raster.rs
        depth.rs
        shading.rs
        points.rs
      matcap.rs
      image.rs
    cache/
      mod.rs
      key.rs
      manifest.rs
      store.rs
    batch/
      mod.rs
    diagnostics/
      mod.rs
      error.rs
      limits.rs
  tests/
    fixtures/
    parser/
    cli/
    golden/
  benches/
```

Do not feel compelled to create every module at the start. Introduce boundaries when they make the code clearer.

### 3.3 Core dependency choices

Choose stable, maintained crates for:
- PNG encoding.
- CLI parsing.
- JSON serialization/deserialization.
- Hashing (BLAKE3 is the preferred input hash).
- Error context.
- Optional parallelism, only when justified.

Do not add a 2D drawing library as a substitute for a 3D rasterizer. A lightweight image crate can own the framebuffer and PNG encoding; triangle rasterization and depth testing are project responsibilities.

The agent must document important dependency choices in a short ADR (Architecture Decision Record) or `docs/decisions.md`, including why the selected crate is appropriate and any portability implications.

---

## 4. Architecture overview

The pipeline is:

```text
CLI / JSON request
        |
        v
Request validation + resource limits
        |
        v
Resolve source file + cache key
        |
        +---- Cache hit? ----> validate cached artifact ----> return
        |
        v
Format detection
        |
        v
STL / PLY parser
        |
        v
Canonical geometry representation
        |
        +---- Mesh path -------------------------------+
        |                                            |
        |    Bounds -> normalize -> camera transform  |
        |    -> triangle rasterization -> depth       |
        |    -> normal shading / matcap              |
        |                                            |
        +---- Point cloud path -----------------------+
             Bounds -> normalize -> camera transform
             -> splat rasterization -> depth
             -> point color / simple shading
        |
        v
Framebuffer + PNG encoding
        |
        v
Atomic output and cache writes
        |
        v
CLI result / JSON response
```

### 4.1 Architectural boundaries

Keep these boundaries explicit:

1. **CLI/request layer:** parses options and validates requested operations.
2. **Format detection:** identifies supported input type without fully parsing.
3. **Format parsers:** convert format-specific data into canonical geometry.
4. **Canonical geometry:** renderer-independent model representation.
5. **Camera and projection:** transforms geometry into screen coordinates.
6. **Renderer:** rasterizes meshes or point clouds.
7. **Image output:** owns framebuffer, supersampling, and PNG encoding.
8. **Cache:** maps source content and render settings to output artifacts.
9. **Batch coordinator:** manages independent jobs, concurrency, and error reporting.

The parser must not know about the cache. The renderer must not know whether geometry came from STL or PLY. The CLI must not contain rasterization logic.

---

## 5. Canonical geometry model

Create a compact geometry representation that both parsers can produce and both renderer paths can consume.

### 5.1 Mesh

A mesh should contain:

- A vertex array of 3D positions.
- Triangle indices or triangle records.
- Optional per-vertex normals.
- Optional per-vertex colors.
- Bounds or a means to compute bounds.
- Optional metadata useful for diagnostics (source format, counts).

Suggested conceptual types:

```rust
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

struct Mesh {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    normals: Option<Vec<Vec3>>,
    colors: Option<Vec<Color>>,
    bounds: Bounds3,
}
```

The final representation may be optimized. Do not duplicate large arrays unnecessarily.

### 5.2 Point cloud

Point clouds should be a first-class geometry kind, not represented as degenerate triangles.

```rust
struct PointCloud {
    positions: Vec<Vec3>,
    colors: Option<Vec<Color>>,
    normals: Option<Vec<Vec3>>,
    bounds: Bounds3,
}
```

A PLY file can contain vertices without faces. This is a valid point-cloud input and must not be rejected merely because there are no polygons.

### 5.3 Geometry enum

```rust
enum Geometry {
    Mesh(Mesh),
    PointCloud(PointCloud),
}
```

A PLY file may contain both vertices and faces; if faces are valid, render it as a mesh. If it contains vertices and no faces, render it as a point cloud. If the file has faces but all faces are invalid or degenerate, report a clear failure or deliberately fall back to points according to a documented policy. Do not silently turn malformed data into a misleading result.

### 5.4 Coordinate and numeric rules

- Use `f32` for rendering storage unless parser precision or overflow analysis justifies `f64`.
- Validate that every position, normal, and color component used by rendering is finite.
- Reject or safely skip non-finite vertices according to a documented policy. Prefer rejecting malformed geometry with a diagnostic rather than allowing NaNs to poison bounds and transforms.
- Avoid integer overflow when converting counts, indices, dimensions, byte lengths, and allocation sizes.
- Indices must be validated before dereferencing.
- Bounds must be valid and finite.
- Empty geometry is an explicit error.
- Zero-size bounds must be handled (e.g. a point, line-like model, or flat geometry), not divided by zero.

---

## 6. Format detection

Format detection should be robust and must not rely solely on the STL ASCII `solid` prefix.

### 6.1 STL detection

Binary STL contains an 80-byte header, a 4-byte triangle count, and 50 bytes per triangle. If the file length exactly matches:

```text
84 + 50 * triangle_count
```

then binary STL is a strong candidate. Calculate this with checked arithmetic.

ASCII STL is text containing recognizable STL structure, but do not classify a file as ASCII solely because its first word is `solid`; binary STL headers may also begin with that text.

Detection should:
1. Check file size and binary-length consistency.
2. Attempt a conservative ASCII structural check when appropriate.
3. Produce a clear unsupported/invalid-format error when ambiguous or malformed.
4. Avoid reading an entire huge file into memory just to detect it if streaming is practical.

### 6.2 PLY detection

Use the PLY magic header (`ply`) and validate the header before choosing a parser. The header determines encoding and element/property schema.

### 6.3 Detection errors

If the extension and actual contents disagree, prefer contents when safe. Include detected format in diagnostics. Do not trust filename extensions as the only source of truth.

---

## 7. STL parsing

Support:

- Binary STL.
- ASCII STL.

### 7.1 Binary STL

Each triangle record contains:
- 3 float normal values.
- 3 vertices × 3 float values.
- 2-byte attribute field.

Requirements:
- Validate header, triangle count, and expected file length using checked arithmetic.
- Do not allocate from an untrusted count until it is checked against file length and configured limits.
- Read triangle vertices.
- Ignore supplied facet normals for rendering; recompute normals from vertex positions.
- Ignore attribute bytes in MVP.
- Drop degenerate triangles or count them and report a warning.
- Validate finite coordinates.
- Preserve geometry even if the source winding is inconsistent; use double-sided rendering.

### 7.2 ASCII STL

Parse the grammar structurally enough to extract facet vertices safely. Do not rely on brittle line-number assumptions.

Requirements:
- Validate expected `facet`, `outer loop`, `vertex`, `endloop`, and `endfacet` structure.
- Handle whitespace and case conservatively if the format permits it.
- Require exactly three vertices per facet.
- Report malformed facets with context.
- Ignore source normals and recompute.
- Bound token lengths and total input size.

### 7.3 STL vertex storage

STL repeats vertices per triangle. The MVP may store duplicated vertices rather than deduplicate. This is simpler and often appropriate for thumbnail workloads. Deduplication is not required.

---

## 8. PLY parsing

PLY is schema-driven. The parser must read the header and understand the declared elements and properties rather than assume a fixed property order.

Support:
- ASCII PLY.
- Binary little-endian PLY.
- Binary big-endian PLY.
- Vertex element with `x`, `y`, `z`.
- Optional normals: `nx`, `ny`, `nz`.
- Optional vertex colors.
- Face element with a vertex-index list.
- Polygon faces with 3+ vertices; triangulate them.
- Vertex-only PLY point clouds.

### 8.1 Header parsing

Recognize:
- `ply`.
- `format` and version.
- `element` declarations.
- `property` declarations (scalar and list).
- `comment` and `obj_info` (preserve or ignore safely).
- `end_header`.

Requirements:
- Validate header size and line length limits.
- Validate property types against supported scalar/list types.
- Preserve the declared order of elements and properties.
- Support arbitrary ordering of vertex properties.
- Skip unknown properties and elements safely.
- Do not assume vertex element is first or face element is second.
- Reject unsupported encodings or type combinations with clear errors.

### 8.2 Vertex property mapping

Required coordinate fields:
- `x`, `y`, `z`.

Optional normal aliases:
- `nx`, `ny`, `nz`.

Color aliases in MVP:
- `red`, `green`, `blue`, optionally `alpha`.
- `r`, `g`, `b`, optionally `a`.

Colors may be declared as integer or floating-point properties. Normalize values carefully:
- Integer color values are interpreted according to their declared type/range (commonly 0–255 for uchar).
- Float colors may be in 0–1 or 0–255 in real-world files. Use a documented normalization rule; avoid guessing per vertex. A reasonable initial policy is treat floating colors in `[0,1]` as normalized and values above 1 as 0–255-scaled, with clamping and a diagnostic if unusual. Alternatively, choose strict behavior and document it.
- Alpha defaults to opaque.
- Missing or incomplete RGB means no vertex colors.

Do not allow color parsing to corrupt geometry when a color property is malformed.

### 8.3 Face properties and triangulation

Recognize a face vertex-index list (commonly `vertex_indices`, with compatibility for `vertex_index` if warranted).

For each face:
- Validate list length.
- Validate every index is within the vertex array.
- Faces with fewer than three indices are invalid.
- Triangles are retained.
- Quads and polygons are triangulated.

MVP triangulation may use a fan:
```text
(v0, v1, v2), (v0, v2, v3), ...
```
This is acceptable for convex polygons and many practical files, but can be incorrect for concave polygons. Document the limitation. If the implementation chooses a more robust triangulator, include tests for concave polygons and keep the dependency justified.

### 8.4 Binary PLY

- Honor the declared endian format.
- Implement scalar types supported by the PLY specification, with explicit mapping to Rust types.
- Use checked reads and checked conversion to index/count types.
- Parse list count types safely.
- Reject unreasonable list lengths before allocation.
- Unknown properties/elements must be consumed correctly so the stream remains aligned.
- Avoid silently interpreting unsupported types as another type.

### 8.5 Point-cloud behavior

When a PLY has vertices but no faces:
- Treat it as a point cloud.
- Preserve vertex colors.
- Preserve normals if available, but do not require them.
- Render it with splats and depth testing.
- Provide a sensible default point size based on output resolution and point count, with CLI override.

---

## 9. Geometry preprocessing

### 9.1 Bounds

Calculate axis-aligned bounds:

```text
min = component-wise minimum
max = component-wise maximum
center = (min + max) / 2
extent = max - min
```

Use checked/finite values.

### 9.2 Normalization

Normalize geometry so that different source sizes produce consistent thumbnail composition.

Recommended baseline:
- Translate the model center to the origin.
- Scale uniformly so the maximum extent fits a target unit size.
- Preserve aspect ratio.
- Do not scale axes independently.
- Handle a near-zero maximum extent with a safe fallback scale.

The target scale and margin must be renderer-versioned because changing normalization changes output and cache identity.

### 9.3 Mesh normals

- Recompute triangle face normals from positions.
- Skip degenerate triangles.
- Normalize safely.
- Do not rely on STL normals.
- Double-sided rasterization is required to tolerate inconsistent winding.
- When rendering a back-facing triangle, adjust the shading normal consistently (e.g. flip it toward the camera) so a reversed winding does not create unexpectedly dark surfaces.
- MVP may use flat face normals. Smooth shading is optional and must not delay a correct flat-shaded implementation.

### 9.4 Optional vertex normals

PLY vertex normals may be retained, but MVP may render using recomputed flat face normals. If smooth shading is implemented:
- Validate and normalize source normals.
- Interpolate normals barycentrically.
- Define fallback behavior for invalid or missing normals.
- Ensure consistent visual behavior when source normals are absent.

---

## 10. Camera and projection

### 10.1 Default camera

Use a fixed orthographic isometric-like view that gives a useful three-quarter view of typical objects. The camera must not change based on file name or inferred object type.

A fixed default is critical for comparable thumbnails in a grid.

### 10.2 Camera presets

MVP should include named presets, at minimum:
- `iso` (default).
- `top`.
- `front`.
- `side`.

The exact axes for front and side must be documented. Preset behavior must be stable across versions.

Do not implement semantic orientation detection in MVP.

### 10.3 Orthographic fit

- Transform normalized geometry into camera space.
- Compute projected bounds.
- Fit the geometry to the output canvas while respecting a configurable margin.
- Use a single uniform camera scale.
- Center the projected model.
- Avoid clipping.
- Keep a predictable visual margin.

### 10.4 Transform conventions

Document:
- World coordinate convention.
- Camera direction.
- Up vector.
- Preset rotations.
- Projection convention.
- Screen-space y direction.

These conventions should be covered by tests to prevent accidental thumbnail orientation changes.

---

## 11. CPU renderer (MVP)

The CPU renderer is the reference and default renderer. It is not a fallback that may be removed later.

### 11.1 Framebuffer

- Use a compact RGBA or RGB framebuffer.
- Initialize with requested background.
- Support transparent background if requested.
- Keep alpha behavior consistent through supersampling and PNG output.
- Avoid undefined/uninitialized pixels.

### 11.2 Triangle rasterization

Implement a straightforward triangle rasterizer with:
- Screen-space vertices.
- Bounding-box clipping.
- Edge/barycentric coverage.
- Depth interpolation.
- Depth buffer.
- Backface/double-sided handling.
- Per-triangle or interpolated shading.
- Safe handling of tiny and degenerate triangles.

Do not attempt to implement a general graphics API. The required output is a small raster image.

### 11.3 Depth buffer

- Allocate one depth value per output sample (including supersampling samples).
- Clear to a value representing “no geometry”.
- Test depth consistently.
- Use a documented depth convention (near is smaller or larger).
- Avoid NaN depth values.
- Ensure transparent background pixels remain transparent when no geometry covers them.

### 11.4 Shading

Use a simple normal-based matcap or equivalent compact lighting lookup.

Requirements:
- Embed the matcap image in the executable.
- Do not download it at runtime.
- Neutral material, readable highlights, and gentle shadow/contrast.
- No material editor or texture pipeline.
- Stable appearance for both front and back faces.
- Use the same conceptual shading model for future GPU rendering.

A matcap is a lookup based on the surface normal in view space. A CPU implementation can map a view-space normal to a 2D lookup coordinate and sample the embedded image. If a full matcap is awkward in MVP, implement a small, attractive deterministic normal-based lighting model first, but keep the shading interface compatible with a matcap later. Do not block parser/renderer progress on a sophisticated asset.

### 11.5 Vertex colors

When PLY colors are present:
- Use vertex color as material/albedo.
- Combine with lighting or matcap in a restrained way.
- Preserve hue as much as possible.
- Clamp malformed or out-of-range color values safely.
- Interpolate color across triangles if using vertex colors.
- If the model has no colors, use a consistent neutral material.

Do not treat vertex colors as emission by default; the object should remain readable.

### 11.6 Supersampling

Support a quality setting with 2× supersampling as a practical default or selectable mode:
- Render at scaled dimensions.
- Downsample to requested output dimensions using a suitable filter.
- Ensure depth and alpha are handled correctly.
- Validate the intermediate dimensions against memory limits.

A lower-quality/no-supersampling mode may be provided for speed. Quality options must be stable and included in cache keys.

### 11.7 Output

- PNG.
- Requested dimensions.
- Optional transparent background.
- Solid background support (document color syntax).
- No extra output files unless cache metadata is enabled in the cache location.

---

## 12. Point-cloud renderer

Point clouds must not be silently treated as meshes or discarded.

### 12.1 MVP splats

Render each point as a circular screen-space splat:
- Project point to screen.
- Choose radius from a default or explicit point size.
- Rasterize covered pixels.
- Depth-test against existing splats.
- Preserve point color if present.
- Use neutral color if absent.

Splat size should be expressed in predictable pixel units. If supersampling is active, scale splat radius accordingly.

### 12.2 Point size

The default should produce a visible but not excessively dense cloud. A practical heuristic can consider output size and point count. Keep it deterministic and document it.

Explicit `--point-size` overrides the heuristic.

### 12.3 Large clouds

- Enforce a maximum point count or memory budget.
- If downsampling is needed, it must be deterministic.
- Do not randomly drop points.
- MVP can reject excessive point counts with a clear error; deterministic reduction is recommended when practical.
- A future sampling method should be spatially distributed (e.g. deterministic voxel/grid sampling), not merely the first N points, which can bias results based on file ordering.

### 12.4 Deferred improvements

Do not require:
- Sphere impostors.
- Per-point surface normals.
- Gaussian splatting.
- Point-cloud reconstruction.
- Advanced spatial indexing.

---

## 13. Image output and color

- PNG is the sole MVP output format.
- Support RGB/RGBA output as needed.
- Define color-space assumptions (sRGB is a sensible default).
- Define alpha handling.
- Downsampling must avoid unintended dark/bright fringes for transparent images; use premultiplied alpha or a correct equivalent.
- Validate output dimensions before allocation.
- Encoder errors must be reported.
- Output must be a valid PNG even if the geometry is visually empty only when empty geometry is explicitly allowed; otherwise return a clear error.

---

## 14. Cache

The cache is an important product feature. A hit should avoid parsing and rendering.

### 14.1 Cache key

Use BLAKE3 over the raw input file bytes, streaming where possible. The key must also include all settings that materially affect output.

Conceptually:

```text
cache_key = hash(
  input_content_hash,
  cache_schema_version,
  renderer_version,
  normalized_render_settings,
  backend_identity_if_output_differs
)
```

Settings to include:
- output width and height (or size).
- camera preset.
- background and transparency.
- quality/supersampling.
- point size or auto-point-size algorithm version.
- shading/material settings.
- sampling/reduction settings.
- renderer/backend identity if output differs.
- any future option that changes pixels.

Do not include output path. Do not include irrelevant CLI flags.

### 14.2 Hash-before-parse

Compute the raw input content hash before parsing. A valid cache hit should be discoverable without decoding STL/PLY.

If hashing requires streaming:
- Read in bounded chunks.
- Enforce file-size limit.
- Handle file changes while hashing/rendering. Prefer validating file metadata before and after or hashing through a stable opened file handle and using that same content for parsing.
- Avoid a time-of-check/time-of-use bug where cache identity is based on bytes different from the bytes parsed.

### 14.3 Cache layout

A practical layout:

```text
cache/
  <key-prefix>/
    <cache-key>.png
    <cache-key>.json
```

or a flat layout with prefix directories. Choose a layout that avoids huge directory counts.

### 14.4 Manifest

JSON sidecar should contain:
- Cache schema version.
- Cache key.
- Source content hash.
- Source format, when known.
- Renderer version.
- Backend.
- Render settings.
- Output width/height.
- Output file size.
- Creation timestamp (informational).
- Optional diagnostic/warning counts.

The manifest is not the source of truth for PNG validity. Validate cached files sufficiently (existence, size, and PNG signature; optionally decode) before returning them. A corrupt cache entry should be treated as a miss and rebuilt.

### 14.5 Atomic writes

- Write temporary files in the destination/cache directory.
- Flush and close.
- Rename atomically where supported.
- Do not leave partially written cache PNGs or manifests that appear valid.
- Define behavior for concurrent requests with the same key. Avoid corrupting a valid cache; duplicate rendering may be acceptable initially, but writes must be safe.

### 14.6 Cache bypass

Provide `--no-cache`.

When bypassed:
- Do not read cache.
- Do not populate cache unless a separate explicit option requests it (keep behavior simple and document it).

### 14.7 Cache directory

- Allow a cache directory override.
- Choose a platform-appropriate default.
- If caching cannot be used due to permissions or storage, either fail clearly when cache was explicitly requested or gracefully continue without cache when it is the default; document behavior.
- Do not delete user files outside the cache directory.

Cache pruning is useful but may be deferred until after the core MVP. If implemented, support size-based pruning and never remove entries currently being written.

---

## 15. CLI design

The executable should be named `mesh-thumb`.

### 15.1 Commands

Required:

```text
mesh-thumb --version
mesh-thumb capabilities
mesh-thumb inspect --input FILE
mesh-thumb render --input FILE --output FILE [OPTIONS]
mesh-thumb batch [OPTIONS]
```

Optional/deferred:
```text
mesh-thumb cache prune --max-bytes SIZE
```

### 15.2 Render command

Suggested interface:

```text
mesh-thumb render \
  --input model.stl \
  --output preview.png \
  --size 256 \
  --view iso \
  --background "#20242b" \
  --quality 2 \
  --backend cpu
```

Options:
- `--input`, required.
- `--output`, required.
- `--size`, shorthand for square dimensions.
- `--width`, `--height`, optional independent dimensions; mutually exclusive with `--size` or with clear precedence.
- `--view iso|top|front|side`.
- `--background COLOR` or `transparent`.
- `--quality` / supersampling scale.
- `--point-size`, optional.
- `--backend auto|cpu|gpu` (MVP: `auto` and `cpu` resolve to CPU; GPU must return a clear unsupported message rather than pretend it exists).
- `--cache-dir`.
- `--no-cache`.
- `--json`.
- Additional options only if they serve the product and are documented.

### 15.3 Inspect

`inspect` parses enough of the file to report:
- detected format.
- geometry type (mesh/point cloud).
- vertex count.
- triangle count.
- point count when applicable.
- bounds.
- whether vertex colors exist.
- whether normals exist.
- warnings (degenerate faces, skipped records, etc.).
- parser/format limitations.

Do not render an image during inspect.

### 15.4 Capabilities

`capabilities` returns machine-readable or human-readable supported formats, renderer, views, and features.

In JSON mode, use stable schema/version fields.

### 15.5 Batch

Batch must:
- Accept a list of input files or a directory/glob according to a documented interface.
- Optionally derive output paths.
- Support bounded concurrency.
- Continue after individual failures.
- Produce per-file result status.
- Avoid filename collisions.
- Keep memory use bounded.
- Honor cache behavior.
- Provide a summary (success, failure, skipped/cache-hit counts).

MVP may implement directory scanning with a clear extension filter before later adding glob syntax. Do not require shell expansion to be identical across operating systems.

### 15.6 JSON protocol

Provide structured JSON output for automation. JSON must not be mixed with progress messages on stdout.

Recommended convention:
- stdout: JSON response only when JSON mode is selected.
- stderr: diagnostics/progress.
- Exit status: success/failure as documented.

Example success response:

```json
{
  "schema_version": 1,
  "status": "ok",
  "input": "model.stl",
  "output": "preview.png",
  "format": "stl_binary",
  "geometry": "mesh",
  "vertices": 1200,
  "triangles": 400,
  "cache_hit": false,
  "render_ms": 42,
  "warnings": []
}
```

Example failure response:

```json
{
  "schema_version": 1,
  "status": "error",
  "input": "broken.ply",
  "error": {
    "code": "invalid_ply_header",
    "message": "PLY header is malformed"
  }
}
```

Requirements:
- Stable field names.
- Stable error codes where possible.
- JSON schema version.
- No stack traces by default.
- Do not expose internal paths or sensitive details unnecessarily.
- Batch JSON includes an array of per-file results plus totals.

---

## 16. Error handling and diagnostics

Errors should be actionable, not merely “failed”.

Error categories should include:
- `io_error`.
- `unsupported_format`.
- `invalid_stl`.
- `invalid_ply_header`.
- `invalid_ply_data`.
- `invalid_geometry`.
- `resource_limit_exceeded`.
- `invalid_render_option`.
- `cache_error` (when fatal).
- `png_encode_error`.
- `backend_unavailable`.
- `output_error`.

Diagnostics should identify:
- Input path (appropriately sanitized).
- Parser stage.
- Record/element context when available.
- What failed.
- Whether the file was skipped or the operation aborted.

Warnings may include:
- Degenerate triangles removed.
- Non-finite vertices skipped/rejected.
- Unrecognized optional properties ignored.
- Colors clamped.
- Point cloud reduced.
- Unsupported optional data ignored.

Do not make malformed input fatal to an entire batch unless the batch request itself is invalid.

---

## 17. Security and resource limits

Input files are untrusted. Parser and renderer code must assume malicious and malformed files.

### 17.1 Limits

Provide configurable or documented safe limits for:
- Maximum input file size.
- Maximum header size and line length.
- Maximum vertices.
- Maximum triangles.
- Maximum points.
- Maximum face size/list length.
- Maximum output width and height.
- Maximum total output pixels.
- Maximum supersampling scale.
- Maximum framebuffer memory.
- Maximum temporary/cache output size.
- Batch concurrency.

Choose initial values based on realistic thumbnail workloads and memory calculations. Document them and make them overrideable only when override behavior remains safe.

### 17.2 Validation

- Checked arithmetic everywhere sizes/counts are combined.
- Validate indices.
- Validate property/list types.
- Reject unsupported encodings clearly.
- Validate finite float values.
- Avoid panics on malformed data.
- Avoid recursion for large polygon lists.
- Avoid unbounded allocations from declared counts.
- Validate output paths and handle path errors safely.
- Use safe Rust unless a specific unsafe block is essential; if used, document and isolate it.

### 17.3 Fuzzing

Add fuzz targets for:
- STL detection/parser.
- ASCII STL.
- Binary STL.
- PLY header parser.
- ASCII PLY.
- Binary PLY.
- Geometry validation.
- Triangulation.

Fuzzing objectives:
- No panic.
- No out-of-bounds.
- No integer overflow.
- No unbounded allocation.
- No pathological CPU behavior within reasonable harness limits.

---

## 18. Performance strategy

Do not optimize before measuring. A compact CPU renderer should be sufficient for ordinary thumbnail workloads.

### 18.1 Performance priorities

1. Cache hit avoids parse/render.
2. Parser is efficient and allocation-aware.
3. Renderer avoids per-pixel heap allocation.
4. Rasterization clips triangle bounds before looping.
5. Point-cloud path is simple and bounded.
6. Supersampling is memory-aware.
7. Batch concurrency is bounded.

### 18.2 Initial performance targets (goals, not guarantees)

Use representative fixtures and record benchmarks. Suggested target categories:
- Small models (few thousand triangles): comfortably interactive/near-instant on a typical modern desktop.
- Medium models (around 100k triangles): ideally under a second for a small thumbnail on a typical desktop.
- Larger meshes: practical rendering within limits or a fast, explicit resource-limit error.
- Dense point clouds: practical through deterministic reduction or a clear count limit.
- Cache hit: mostly file/hash and PNG validation I/O, without parsing or rendering.

Do not make performance claims without measurements. Record hardware, build mode, output size, fixture, and benchmark methodology.

### 18.3 Parallelism

Initially, a single render can be serial. Add parallelism only if benchmarks justify it.

If parallelizing:
- Prefer bounded batch concurrency over uncontrolled thread creation.
- Avoid race conditions in cache writes.
- Make deterministic output independent of job scheduling.
- Ensure memory budget is multiplied correctly by concurrent jobs.

---

## 19. Testing plan

Tests are a deliverable, not an afterthought.

### 19.1 Parser unit tests

STL:
- Binary valid.
- ASCII valid.
- Binary header begins with `solid`.
- Exact-size detection.
- Incorrect triangle count.
- Truncated record.
- Extra bytes behavior.
- Non-finite coordinates.
- Degenerate triangles.
- Empty file.
- Malformed ASCII structure.

PLY:
- ASCII.
- Binary little-endian.
- Binary big-endian.
- Properties in different order.
- Additional unknown properties.
- Unknown elements.
- Vertex colors using `red/green/blue`.
- Vertex colors using `r/g/b`.
- Optional alpha.
- Float colors.
- Normals.
- Triangles.
- Quads.
- Polygon faces.
- Vertex-only point cloud.
- Face before vertex element if supported by parser design.
- Invalid indices.
- Truncated list.
- Unsupported scalar type.
- Oversized header.
- Oversized list.
- Empty/invalid geometry.

### 19.2 Geometry tests

- Bounds.
- Normalization.
- Flat model.
- Single point.
- Line-like geometry.
- Degenerate triangles.
- Winding reversal.
- Triangulation.
- Non-finite rejection.
- Camera transform and projection.

### 19.3 Renderer tests

- Cube.
- Sphere.
- Torus.
- Mechanical part.
- Thin plate.
- Open mesh.
- Inverted normals.
- Mixed winding.
- Degenerate triangles.
- Point clouds: sparse, dense, colored, uncolored.
- Transparency.
- Background.
- Dimensions.
- Supersampling.

### 19.4 Golden-image tests

Create fixed fixtures and reference PNGs for:
- Default isometric cube.
- Top/front/side views.
- Colored PLY mesh.
- Sparse point cloud.
- Dense point cloud.
- Thin object.
- Open/inconsistent mesh.

Golden-image comparison should allow a documented tolerance if platform or encoder differences are expected. For the CPU reference renderer, strive for deterministic pixels on the same version/platform. When intentional visual changes occur, update goldens and renderer version in the same review.

Do not make goldens so permissive that significant regressions pass.

### 19.5 CLI integration tests

- `--version`.
- `capabilities`.
- `inspect`.
- Successful render.
- Invalid input.
- Unsupported file.
- Output directory missing.
- Unicode paths.
- Paths with spaces.
- JSON success/failure.
- Cache hit.
- Cache corruption.
- Cache bypass.
- Batch with mixed success/failure.
- Output collision behavior.
- Resource limits.

### 19.6 Cache tests

- Same input/settings produces hit.
- Different input changes key.
- Different render option changes key.
- Renderer version changes key.
- Schema version changes key.
- Corrupt PNG is treated as miss.
- Missing manifest behavior.
- Atomic output.
- Concurrent same-key requests.
- Source changed during operation.
- Cache disabled.

### 19.7 Fuzz and regression tests

Every discovered parser crash or malformed-input issue must become a regression test.

---

## 20. Versioning

Maintain separate concepts:
- Application version.
- Renderer version.
- Cache schema version.
- JSON protocol schema version.

Increment renderer version when pixel output can change due to:
- Normalization.
- Camera transforms.
- Projection.
- Rasterization.
- Shading.
- Color interpretation.
- Point rendering.
- Supersampling/downsampling.
- Defaults affecting output.

Increment cache schema version when cache layout or manifest compatibility changes.

Increment JSON schema version when externally visible response structure changes incompatibly.

The cache key must use renderer/cache versions, not only the executable version.

---

## 21. Future GPU backend (not MVP)

Only add a GPU renderer after CPU correctness and benchmarks exist and demonstrate a real bottleneck.

### 21.1 Preferred approach

If a GPU backend is justified, use `wgpu` as a single cross-platform abstraction rather than separately implementing:
- Metal.
- Vulkan.
- DirectX.
- OpenGL.

This provides a path to native backends without multiplying maintenance.

### 21.2 Backend behavior

- `cpu` always works.
- `auto` chooses GPU only if it is reliable and available, otherwise CPU.
- `gpu` explicitly requested but unavailable must return a clear error or documented fallback.
- GPU initialization must be guarded against hangs/stalls where practical.
- GPU output need not be pixel-identical to CPU output.
- Visual consistency is the requirement.
- Backend identity should be included in the cache key if output differences are material.

### 21.3 GPU acceptance gate

Do not start GPU work until:
- CPU renderer is complete.
- Parser coverage is stable.
- Golden tests exist.
- CPU benchmarks exist.
- There is evidence CPU performance is inadequate for target workloads.
- A clear backend interface is established.

Do not build multiple native GPU implementations.

---

## 22. Future 4D component wrapper

The Rust CLI is the primary product. The 4D component is a consumer of it and should not dictate renderer complexity.

### 22.1 Wrapper purpose

Provide a convenient component for the 4D environment to:
- Render a geometry file to a PNG thumbnail.
- Inspect geometry metadata.
- Query version and capabilities.
- Optionally clear/prune cache.

Suggested public name:

```text
MeshPreview.Thumbnailer
```

### 22.2 Wrapper API

Conceptual methods:

```text
render(inputPath, options) -> result
inspect(inputPath) -> metadata
version() -> versionInfo
capabilities() -> capabilities
clearCache(options) -> result    # optional/deferred
```

Define the exact 4D signatures and object/result types based on the target 4D component API and conventions. Do not invent incompatible framework abstractions.

### 22.3 Process invocation

- Bundle the native executable with the component.
- Do not download the executable at runtime.
- Use `SystemWorker` or the platform-approved process API.
- Pass arguments as an argument array, not a shell-concatenated command string.
- Handle Unicode and paths with spaces.
- Keep stdout reserved for JSON when using JSON mode.
- Capture stderr separately for diagnostics.
- Enforce timeouts.
- Bound simultaneous workers.
- Avoid uncontrolled retries.
- Validate the returned PNG before loading it.
- Load the PNG into the framework's `Picture`/image object using its supported API.
- Return structured errors instead of crashing the host.
- Keep the wrapper thin; parsing and rendering remain in Rust.

### 22.4 Binary distribution

At minimum, plan and document:
- macOS universal binary (arm64 and x86_64).
- Windows x64.
- Linux CLI optional unless required by the component or project users.

The component must select the correct bundled binary. Verify executable permissions and signing/notarization requirements for the actual distribution channel. Do not assume a universal binary is automatically available from the Rust toolchain.

### 22.5 Wrapper tests

- Binary discovery.
- Version compatibility.
- Successful render.
- Successful inspect.
- Invalid input.
- Timeout.
- Process launch failure.
- Unicode/space paths.
- JSON parse failure.
- PNG validation failure.
- Concurrent requests.
- Component packaging includes the binary.
- No network access.

Follow the patterns in the existing repositories `quickdraw` and `4d-component-template` when available, but do not copy code blindly. Check current conventions, licenses, supported framework APIs, and platform packaging.

The wrapper is a later phase; do not implement it before the CLI contract is stable.

---

## 23. Build, CI, and release

### 23.1 CI

CI should:
- Format check (`cargo fmt --check`).
- Lint (`cargo clippy` with project policy).
- Run unit/integration tests.
- Build release executable for supported targets.
- Run parser tests on all platforms where practical.
- Verify CLI smoke tests.
- Verify embedded assets are present.
- Run fuzz smoke tests if infrastructure supports it.
- Check dependency/license policy.

### 23.2 Release artifacts

Produce:
- Versioned executable.
- Checksums.
- Release notes.
- Supported platform list.
- Build provenance/toolchain information.
- Dependency/license notices where required.

### 23.3 Reproducibility

- Lock dependencies.
- Record Rust toolchain.
- Avoid network/runtime dependencies.
- Keep embedded assets versioned.
- Document build commands.

---

## 24. Documentation deliverables

The agent must create and maintain:

### `README.md`
- What it does.
- What it does not do.
- Supported formats.
- Example commands.
- Installation/build instructions.
- Performance caveats.
- Cache behavior.
- Limitations.

### `docs/architecture.md`
- Pipeline.
- Module responsibilities.
- Canonical geometry.
- Renderer boundary.
- Cache design.

### `docs/format-support.md`
- STL behavior.
- PLY schema/property support.
- Polygon triangulation limitations.
- Color interpretation.
- Unsupported features.

### `docs/cli.md`
- All commands.
- Options.
- JSON protocol.
- Exit codes.
- Batch behavior.

### `docs/security.md`
- Resource limits.
- Parser assumptions.
- File handling.
- Fuzzing strategy.

### `docs/decisions.md` or ADRs
Record major choices and trade-offs, including:
- CPU-first renderer.
- Canonical geometry.
- Rasterizer approach.
- PLY parser design.
- Cache identity.
- Point-cloud strategy.
- Future GPU criteria.

---

## 25. Implementation phases and completion gates

Do not attempt the entire project in one unreviewable code dump. Work in phases. At each phase, build, test, document, and summarize what is complete and what remains.

### Phase 0 — Contract and repository setup

Deliver:
- Repository structure.
- Cargo project.
- CLI skeleton.
- Version/capability command.
- Error model.
- Renderer/cache/protocol version constants.
- Initial documentation and decisions.

**Gate:** Project builds, CLI help/version works, CI is established.

### Phase 1 — CPU mesh thumbnail vertical slice

Implement the smallest end-to-end path:
- Binary STL parser.
- Canonical mesh.
- Bounds/normalization.
- Default camera.
- CPU triangle rasterizer.
- Depth buffer.
- Basic normal-based shading.
- PNG output.
- One render command.

Fixtures: cube and sphere.

**Gate:** A valid STL produces a correctly framed PNG; tests cover parsing, projection, rasterization, and errors.

### Phase 2 — Robust STL and renderer quality

Add:
- ASCII STL.
- Detection hardening.
- Double-sided behavior.
- Degenerate handling.
- Embedded matcap or finalized shading.
- Background and output dimensions.
- Supersampling.
- View presets.
- Golden tests.

**Gate:** STL test matrix passes; stable thumbnails and regression tests exist.

### Phase 3 — PLY and point clouds

Add:
- PLY header/schema parser.
- ASCII and both binary endian modes.
- Vertex property mapping.
- Colors/normals.
- Face lists and triangulation.
- Point-cloud rendering.
- Point-size controls.
- Resource limits and diagnostics.

**Gate:** PLY mesh and point-cloud fixtures render correctly; parser tests cover schema variation and malformed input.

### Phase 4 — Production CLI, cache, and batch

Add:
- `inspect`.
- `capabilities`.
- JSON protocol.
- BLAKE3 cache.
- Manifest.
- Atomic writes.
- Cache validation.
- Batch processing.
- Bounded concurrency.
- Deterministic sampling or explicit limits.
- Error codes.
- Performance benchmarks.

**Gate:** CLI is stable enough for another application to consume. Cache and batch tests pass.

### Phase 5 — Hardening and release

Add:
- Resource limits.
- Fuzz targets.
- Security review.
- Unicode/path tests.
- CI target matrix.
- Release packaging.
- Checksums.
- Complete docs.

**Gate:** No known parser panics; limits are tested; release builds and smoke tests pass on target platforms.

### Phase 6 — 4D wrapper

Only after Phase 4/5 CLI contract is stable:
- Bundle binaries.
- Component API.
- SystemWorker process handling.
- JSON parsing.
- PNG loading.
- Timeouts and concurrency.
- Packaging tests.

**Gate:** A 4D project can call render/inspect without external downloads and receive structured results.

### Phase 7 — Optional GPU evaluation

Benchmark and decide. If accepted:
- Define backend trait/interface.
- Add wgpu implementation.
- Implement adapter selection.
- Guard initialization.
- Test visual consistency.
- Keep CPU renderer operational.

**Gate:** GPU backend demonstrates measurable value, has reliable fallback, and passes renderer tests.

---

## 26. Acceptance criteria

The project is considered complete for its core CLI milestone when all of the following are true:

### Functional
- [ ] Binary STL renders.
- [ ] ASCII STL renders.
- [ ] Binary PLY little-endian renders.
- [ ] Binary PLY big-endian renders.
- [ ] ASCII PLY renders.
- [ ] PLY triangle and polygon faces are handled.
- [ ] Vertex-only PLY renders as point cloud.
- [ ] Vertex colors are preserved.
- [ ] Normal data does not cause rendering failures.
- [ ] Default view is orthographic and stable.
- [ ] Views `iso`, `top`, `front`, `side` are implemented and documented.
- [ ] PNG output is valid.
- [ ] Requested dimensions are honored.
- [ ] Background/transparency work.
- [ ] Supersampling works.
- [ ] Inspect returns useful metadata.
- [ ] Batch continues after a file failure.
- [ ] JSON output is machine-readable and stable.

### Reliability
- [ ] Malformed inputs return errors, not panics.
- [ ] Counts and dimensions are bounded.
- [ ] Indexes are validated.
- [ ] Numeric values are checked.
- [ ] Atomic output is used.
- [ ] Cache corruption is recoverable.
- [ ] Concurrent jobs are bounded.
- [ ] Paths with spaces and Unicode work.

### Cache
- [ ] Input hash is calculated before parsing.
- [ ] Cache identity includes settings and versions.
- [ ] Cache hit skips parsing and rendering.
- [ ] Manifest is written.
- [ ] PNG validity is checked.
- [ ] Writes are atomic.
- [ ] Cache bypass is supported.

### Engineering
- [ ] Unit/integration/golden tests exist.
- [ ] Fuzz targets exist for parsers.
- [ ] CI runs formatting, lint, tests, and builds.
- [ ] Documentation is complete.
- [ ] Benchmarks are recorded.
- [ ] Release artifacts and checksums can be produced.

---

## 27. Suggested command examples

```bash
# Build
cargo build --release

# Version
mesh-thumb --version

# Capabilities
mesh-thumb capabilities
mesh-thumb capabilities --json

# Inspect
mesh-thumb inspect --input model.stl
mesh-thumb inspect --input cloud.ply --json

# Render a square thumbnail
mesh-thumb render --input model.stl --output preview.png --size 256

# Render with explicit dimensions
mesh-thumb render --input model.ply --output preview.png \
  --width 320 --height 240 --view iso

# Transparent background
mesh-thumb render --input model.stl --output preview.png \
  --size 256 --background transparent

# Alternate view
mesh-thumb render --input model.stl --output top.png --size 256 --view top

# Point cloud sizing
mesh-thumb render --input scan.ply --output scan.png \
  --size 256 --point-size 2.0

# Disable cache
mesh-thumb render --input model.stl --output preview.png \
  --size 256 --no-cache

# Batch (exact syntax to be finalized and documented)
mesh-thumb batch --input-dir ./models --output-dir ./previews --size 256
```

Examples are illustrative; the implementation must keep command behavior consistent with its help and docs.

---

## 28. Agent working instructions

The coding agent should follow these operating rules.

### 28.1 Start by inspecting the repository

Before changing files:
- Inspect existing repository files and conventions.
- Identify any existing code, tests, licenses, workflows, or build constraints.
- Do not overwrite existing work without understanding it.
- If the repository is empty, initialize the project cleanly.
- If adapting an existing repository, report what is being reused and what is changing.

### 28.2 Plan, then implement

- Create a short implementation plan with milestones.
- Start with the end-to-end CPU STL path.
- Keep each milestone compiling and testable.
- Avoid broad speculative abstractions.
- Do not implement future phases early merely because the architecture anticipates them.

### 28.3 Validate continuously

For every milestone:
- Run formatting.
- Run relevant tests.
- Run the executable on fixture files.
- Inspect generated PNGs visually where possible.
- Run CLI smoke tests.
- Record commands and results.
- Fix failures before moving on.

Do not claim tests passed unless they were actually executed.

### 28.4 Avoid fake completion

Do not:
- Create placeholder parsers that claim format support.
- Return success for unsupported backends.
- Stub rendering with a generic icon.
- Skip validation to make tests green.
- silently ignore invalid geometry.
- claim GPU support without a real backend.
- claim cross-platform support without building/testing target platforms.
- fabricate benchmark results.

### 28.5 Keep scope controlled

When an attractive feature appears:
1. Determine whether it is necessary for the acceptance criteria.
2. If not, add it to a deferred list.
3. Do not let it delay the MVP.
4. Revisit only after the current phase is complete.

### 28.6 Report each milestone

At the end of each phase, report:
- What was implemented.
- Files/modules changed.
- Tests executed.
- Test results.
- Known limitations.
- Next milestone.

The report should be factual and concise. Include exact commands and failure details when tests fail.

---

## 29. Deferred backlog

These are potential improvements, not MVP requirements:

- GPU/wgpu renderer.
- More sophisticated matcap controls.
- Smooth shading.
- Robust concave polygon triangulation.
- Spatial/voxel point-cloud sampling.
- Mesh simplification.
- Cache pruning.
- Stdin/stdout protocol.
- Additional file formats.
- Custom camera angles.
- Automatic orientation detection.
- Sphere or Gaussian point splats.
- Additional background presets.
- More advanced color management.
- Linux component packaging.
- Detailed telemetry/diagnostic timing.
- Benchmark-driven parallel rasterization.

Prioritize backlog items only when supported by user demand, performance measurements, or correctness needs.

---

## 30. Final design decisions (summary)

| Area | Decision |
|---|---|
| Language | Rust |
| Product | Small headless geometry-thumbnail CLI |
| CLI name | `mesh-thumb` |
| MVP formats | STL (ASCII/binary), PLY (ASCII/binary LE/BE) |
| Geometry | Canonical mesh + point-cloud representation |
| Mesh normals | Recompute; do not trust STL normals |
| Winding | Double-sided rendering |
| Camera | Fixed orthographic isometric-like default |
| Views | iso, top, front, side |
| Shading | Embedded matcap or compact deterministic normal-based lighting |
| Vertex color | PLY RGB/RGBA, used as albedo |
| Point clouds | First-class, circular screen-space splats, depth-tested |
| Renderer | CPU reference/default |
| GPU | Deferred; wgpu only if justified |
| Output | PNG |
| Quality | Optional supersampling |
| Cache | BLAKE3 raw input hash + settings/version; PNG + JSON manifest |
| Cache hit | Before parsing |
| Writes | Atomic |
| CLI | Render, inspect, capabilities, batch |
| Automation | Versioned JSON protocol |
| Security | Resource limits, validation, fuzzing, bounded allocations |
| 4D wrapper | Thin bundled-binary component, later phase |
| Runtime downloads | None |
| Scope | Thumbnailing only, not a 3D application |

---

## 31. Definition of done

The agent must not mark the project “done” based solely on compilation.

A release candidate must:
1. Satisfy the functional acceptance criteria.
2. Pass unit and integration tests.
3. Have golden-image coverage for core rendering.
4. Have parser fuzz targets and resource-limit tests.
5. Produce valid PNGs from real representative STL and PLY files.
6. Demonstrate cache hits and cache invalidation.
7. Demonstrate resilient batch behavior.
8. Have documented CLI and JSON contracts.
9. Build release artifacts for declared targets.
10. Report remaining limitations honestly.

**The final product should feel boring in the best sense:** small, fast, predictable, portable, easy to invoke, difficult to crash, and easy to embed. The renderer is a means to that end; the product is dependable thumbnails.
