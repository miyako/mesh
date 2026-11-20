# CLI & JSON protocol

Stable, machine-readable contract for `mesh-thumb`. Schema version is
`JSON_SCHEMA_VERSION` (currently 1) and appears in every response. The
human-readable CLI never prints to stdout anything that is not the
command's result; diagnostics go to stderr.

## Command surface

```
mesh-thumb render [--json] [options] <FILE> [-o PNG] [--cache-dir DIR] [--no-cache]
mesh-thumb inspect [--json] <FILE>
mesh-thumb capabilities [--json]
mesh-thumb batch [--json] [--abort-on-error] --out DIR [--cache-dir DIR] [--no-cache] <FILE>...
```

Shared render options: `--size N` (square) or `--width/--height`,
`--view iso|top|front|side`, `--background #RRGGBB|transparent`,
`--quality N` (supersampling 1..8), `--point-size PX`,
`--backend auto|cpu|gpu`.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success. Includes batches with per-file failures (`failed > 0` in the result) |
| 1 | A command failed: fatal error, bad input, backend unavailable, output error, or `--abort-on-error` batch |
| 2 | Usage error (unknown command/flag, invalid combination) — produced by clap |

## Response envelope

Every `--json` command emits exactly one object on stdout:

```json
{
  "schema_version": 1,
  "success": true,
  "result": { ... },
  "error": null
}
```

On failure, **the envelope is still emitted** (with `success: false` and
an `error` object), the human message goes to stderr, and the exit code
is 1:

```json
{
  "schema_version": 1,
  "success": false,
  "result": null,
  "error": {
    "code": "io_error",
    "message": "No such file or directory (os error 2)",
    "path": "missing.stl",
    "stage": null
  }
}
```

### Error codes

| Code | Meaning |
|------|---------|
| `io_error` | File read/write failure |
| `unsupported_format` | Contents match neither STL nor PLY |
| `invalid_stl` | STL parse error |
| `invalid_ply_header` | Malformed or oversized PLY header |
| `invalid_ply_data` | Bad vertex/face data (index, type, non-finite) |
| `invalid_geometry` | Parsed to no usable geometry (empty) |
| `resource_limit_exceeded` | Input size, counts, or framebuffer over limits |
| `invalid_render_option` | Bad option value (size/quality/view/...) |
| `cache_error` | Cache I/O problem (always degrades to a miss) |
| `png_encode_error` | PNG encode/validation failure |
| `backend_unavailable` | `--backend gpu` on the CPU-only MVP |
| `output_error` | Could not write the output PNG |
| `usage_error` | Reserved for usage problems surfaced by commands |
| `batch_error` | Batch-level failure, e.g. output collisions (`a/cube.stl` + `b/cube.stl` both mapping to `out/cube.png`) |

## Result shapes

### `render` result

```json
{
  "input": { "path": "...", "file_size": 2093, "format": "stl_ascii" },
  "output": "/tmp/out.png",
  "width": 256,
  "height": 256,
  "render_ms": 8,
  "cache": "off",
  "geometry": { "kind": "mesh", "vertices": 36, "triangles": 12,
                "colors": false, "normals": false,
                "bounds": { "valid": true, "min": [0,0,0], "max": [1,1,1] } },
  "warnings": { "degenerate_triangles": 0, "non_finite_vertices": 0 },
  "normalize_scale": 1.0
}
```

`cache` is `"off"` (no cache dir), `"miss"` (rendered and stored), or
`"hit"` (PNG reused before parsing).

### `inspect` result

`{ "input": {...}, "geometry": {...}, "warnings": {...} }` with the same
geometry/warnings shapes as above.

### `capabilities` result

Human mode prints an inventory; `--json` emits:

```json
{
  "app": "mesh-thumb",
  "version": "0.1.0",
  "formats": ["stl", "ply"],
  "views": ["iso", "top", "front", "side"],
  "backends": ["auto", "cpu"],
  "gpu_backend_available": false,
  "max_input_bytes": 1073741824,
  "max_output_dimension": 8192,
  "max_output_pixels": 67108864,
  "max_quality": 8
}
```

### `batch` result

```json
{
  "items": [
    { "input": "a.stl", "ok": true, "output": "out/a.png",
      "width": 256, "height": 256, "cache": "hit", "error": null },
    { "input": "bad.ply", "ok": false, "output": null,
      "width": null, "height": null, "cache": null,
      "error": { "code": "invalid_ply_header", "message": "...",
                 "path": "bad.ply", "stage": null } }
  ],
  "total": 2,
  "ok": 1,
  "failed": 1
}
```

Items are reported in input order. With `--abort-on-error`, the first
failure stops the batch, `items` is not emitted, and exit code is 1.

**Output collisions are rejected up front.** Inputs whose stems map to the
same `<out_dir>/<stem>.png` fail the whole batch with `batch_error`
(exit 1) before any file is written — under concurrency the overwrite
winner would be nondeterministic, so `a/cube.stl` + `b/cube.stl` together
is an error, not a last-write-wins race. An empty batch
(`mesh-thumb batch --out dir`) with no files is a clap usage error
(exit 2).

## Versioning contract

- `RENDERER_VERSION` — bump when the pixels change (shading, projection,
  orientation) so cached thumbnails are invalidated.
- `CACHE_SCHEMA_VERSION` — bump when the cache layout/key format changes.
- `JSON_SCHEMA_VERSION` — bump when response shapes change incompatibly.
- These three are the `r`/`v`/`j` fields of the cache key
  (`docs/decisions.md` D3).

Golden images (`tests/golden/ref/`) pin pixel output by renderer
version; regenerate them deliberately per `tests/golden_images.rs`.