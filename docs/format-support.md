# Format support

`mesh-thumb` supports exactly two input formats (spec: "No formats beyond
STL/PLY") — STL and PLY. Detection is content-based
(`src/formats/detect.rs`), not extension-based, and is pinned by tests.

| Format | Variants | Detected | Notes |
|--------|----------|----------|-------|
| STL    | ASCII    | `stl_ascii` | token state machine; case-insensitive keywords, tolerant of extra whitespace and `outer loop`/`end loop`/`end facet` gaps |
| STL    | Binary   | `stl_binary` | 80-byte header + uint32 count; 50-byte records; endianness agnostic via length signature |
| PLY    | ASCII    | `ply_ascii` | `format ascii 1.0`; header `end_header`; vertex/face elements |
| PLY    | Binary   | `ply_binary_le` / `ply_binary_be` | `format binary_little_endian 1.0` / `binary_big_endian 1.0`; endian-aware `BinaryReader` |

## What is parsed

### STL

- One facet = one triangle (3 vertices, optional facet normal).
- Case-insensitive; `Solid`/`endsolid` not required (scan state allows a
  bare facet stream as long as facet structure matches). A file that
  closes its last facet with `endfacet` but omits the trailing
  `endsolid` is accepted; a file that ends *inside* the last facet
  (e.g., after `endloop` without `endfacet`) is an `invalid_stl` error —
  the uncommitted triangle is never silently dropped.
- Line endings are not significant: `\r\n` (Windows) and \r-only files
  parse identically to `\n`. A UTF-8 byte-order mark before `solid` is
  tolerated, as is content after `endsolid`.
- Numbers may use scientific notation (`1e3`, `1.5E-2`).
- Facet normals are read when present; the renderer *recomputes* normals
  anyway (spec: "recompute normals"), so `normals: false` in the summary
  reflects that vertex normals were not present, not that they are unused.
- Non-finite coordinates are skipped and counted in
  `warnings.non_finite_vertices`.
- Degenerate triangles (zero area) are skipped and counted in
  `warnings.degenerate_triangles`.

### PLY

- Header: up to `MAX_PLY_HEADER_BYTES` (64 KiB), ASCII lines up to
  `MAX_PLY_LINE_LEN` (4096). Properties with explicit `list` types for
  faces; other lists are ignored.
- Vertex layout: `x`, `y`, `z` floats (any scalar type), optional `nx/ny/nz`
  normals, optional `red/green/blue` or `red/green/blue/alpha` (uchar,
  ushort, or float — spec §10.5: integers 0–255 normalized, floats 0–1).
  Integer channels are interpreted as **byte scale** regardless of the
  declared storage type: `uchar` and `ushort` holding 0–255 behave
  identically, and integer values above 255 clamp (they are never
  re-mapped to 0..1 from the wider range). All color values are sRGB and
  are linearized for the renderer (see `docs/decisions.md` D2).
- Faces: `3 vertex_indices` triangles; 4+ vertex polygons are fan-
  triangulated (`quad_face_triangulated` test). Non-finite or
  out-of-range indices are an error (`invalid_ply_data`).
- A `vertex` element is **required**; a file with, e.g., only a face
  element fails with a clear message.
- A face element that yields **no usable triangles** is an error
  (`invalid_geometry`), matching the empty-geometry rule (D6).

## Rendering rules

- **Meshes** render double-sided with per-face shading; vertex normals
  are sanitized and normalized, and faces without them get computed
  flat normals.
- **Point clouds** (a PLY with vertices and no faces) render as circular
  splats; the radius is auto-sized per bounds unless `--point-size` is
  given.
- **Colors** are multipliable geometry shading: points and colored
  triangles are tinted by matcap shading × albedo.
- Both kinds are subject to the same limits (`docs/security.md`) and
  cache rules (`docs/decisions.md` D3).

## Known limitations (honest)

- No materials, textures, or multiple object/group handling: only raw
  vertex/face/color/normal data is interpreted.
- STL ASCII tolerates structure only up to `MAX_STL_LINE_LEN`
  (256 bytes); longer tokens are rejected. This is intentionally
  conservative for the thumbnail workload.
- PLY `obj_info`, comments, and unknown elements are ignored, not
  preserved through to output (they have no output representation).
- Very large files are rejected at the configured limits rather than
  streamed: input ≤ 1 GiB, vertices ≤ 20 M, triangles ≤ 10 M
  (`src/diagnostics/limits.rs`).
- Point-cloud splats are flat circles, not perspective; no depth
  sorting between splats (z-buffer on splats).
- No animation, no multi-frame, no PBR lighting; shading is a fixed
  procedural matcap (diffuse + spec + rim).