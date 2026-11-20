# CLI & JSON protocol

Stable, machine-readable contract for `mesh-thumb`. Schema version is
`JSON_SCHEMA_VERSION` (currently 1) and appears in every response. The
human-readable CLI never prints to stdout anything that is not the
command's result; diagnostics go to stderr.

For the user-facing manual (every command and option) see
[cli.md](cli.md); this file is the contract programmers rely on.

## Command surface

```
mesh-thumb render [--json] [options] <FILE> [-o PNG] [--cache-dir DIR] [--no-cache]
mesh-thumb inspect [--json] <FILE>
mesh-thumb capabilities [--json]
mesh-thumb batch [--json] [--abort-on-error] --out DIR [--cache-dir DIR] [--no-cache] <FILE>...
mesh-thumb cache prune [--json] --dir DIR (--all | --older-than DAYS)
```

Shared render options: `--size N` (square) or `--width/--height`,
`--view iso|top|front|side`, `--background #RRGGBB|transparent`,
`--quality N` (supersampling 1..8), `--point-size PX`,
`--backend auto|cpu|gpu`.

### Reading geometry from stdin

For `render` and `inspect`, `<FILE>` may be `-` to read the geometry
bytes from stdin (format detection and limits apply to the piped
bytes; the diagnostics `path` is `-`). Rules:

- `batch` never accepts `-` (it needs a filesystem path per input);
  passing `-` to batch is a normal file-not-found error.
- `render -` requires `-o/--output` (there is no input path to derive
  a default output name from); omitting it is an error (exit 1,
  `invalid_render_option`).
- Programmatic consumers: stdin bytes are hashed for the cache key
  exactly like file bytes, so a piped render and a file render of the
  same bytes share cache entries.
- stdout is reserved for results (JSON when `--json`, the plain result
  otherwise); PNG bytes are never written to stdout (the `-o -` form
  is intentionally unsupported; see docs/decisions D13).

### `cache prune`

Maintenance for a cache directory created by `--cache-dir`. Deletes
every manifest entry older than the cutoff (`--older-than DAYS`, whole
days, exclusive) or every entry (`--all`), plus any blob files not
referenced by the manifest (orphans). Dot-files in `blobs/` (atomic
write staging) are never touched. The manifest is atomically rewritten
when anything was removed. Exactly one of `--all` / `--older-than` must
be given; giving neither or both is a clap usage error (exit 2).

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
| `cache_error` | Cache I/O problem (lookups/writes degrade to a miss; `cache prune` reports it as a real error) |
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

### `cache prune` result

Human mode prints `pruned N entries, removed M orphan blobs, freed B
bytes (cache: DIR)`. `--json` emits:

```json
{
  "cache_dir": "/tmp/cache",
  "entries_pruned": 2,
  "orphan_blobs_removed": 1,
  "bytes_freed": 48219
}
```

A second run reports all zeros (idempotent).

## Versioning contract

- `RENDERER_VERSION` — bump when the pixels change (shading, projection,
  orientation) so cached thumbnails are invalidated.
- `CACHE_SCHEMA_VERSION` — bump when the cache layout/key format changes.
- `JSON_SCHEMA_VERSION` — bump when response shapes change incompatibly.
- These three are the `r`/`v`/`j` fields of the cache key
  (`docs/decisions.md` D3).

Golden images (`tests/golden/ref/`) pin pixel output by renderer
version; regenerate them deliberately per `tests/golden_images.rs`.