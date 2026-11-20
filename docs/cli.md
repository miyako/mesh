# CLI reference

`mesh-thumb` is a headless command-line renderer: it converts STL and
PLY files to PNG thumbnails entirely on the CPU. This page is the
user-facing manual — every command, option, and convention. The
machine-readable contract (JSON envelope, error codes, versioning) is
specified in [protocol.md](protocol.md); the two documents stay in
sync. For why things are the way they are, see
[decisions.md](decisions.md).

## Global

```
mesh-thumb <COMMAND> [options]
mesh-thumb --help | -h        # full help for the top level
mesh-thumb --version | -V     # semantic version of the build
```

Top-level commands: `render`, `inspect`, `capabilities`, `batch`,
`cache`.

Every subcommand also accepts `--help`.

## Reading geometry

`render` and `inspect` take one `<FILE>` argument. The file can be an
STL (ASCII or binary) or a PLY (ASCII or binary, little- or
big-endian); a vertex-only PLY renders as a point cloud.

**Stdin:** pass `-` as `<FILE>` to read the geometry bytes from stdin
instead of a file. Format detection and the size limits apply to the
piped bytes. `render -` requires `-o/--output` (there is no input path
to derive a default name from); `batch` does not accept `-`.

## `render`

```
mesh-thumb render [OPTIONS] <FILE>
```

Renders one input file to a PNG. The default output path is the input
path with `.png` appended (`model.stl` → `model.png`); `-o/--output`
overrides it.

| Option | Meaning |
|--------|---------|
| `-o, --output <PNG>` | Output PNG path. When reading stdin (`FILE = -`) this is required |
| `--size <N>` | Output side length, square image. Conflicts with `--width`/`--height` |
| `--width <W>` | Output width in pixels |
| `--height <H>` | Output height in pixels |
| `--view iso\|top\|front\|side` | Camera view; default `iso` |
| `--background #RRGGBB\|transparent` | Background; default `#20242b` |
| `--quality <N>` | Supersampling scale 1..8 (1 = off); default 2. Higher = smoother edges, slower |
| `--point-size <PX>` | Manual point-splat radius in output pixels (point clouds only). Omitted = automatic sizing |
| `--backend auto\|cpu` | Render backend; default `auto`. `gpu` fails clearly with `backend_unavailable` |
| `--cache-dir <DIR>` | Cache directory; enables the content-addressed PNG cache |
| `--no-cache` | Disable the cache even if `--cache-dir` is given |
| `--json` | Emit the JSON envelope to stdout (see below and [protocol.md](protocol.md)) |

Rendering is deterministic for a given input + settings: the same
bytes and the same options produce byte-identical PNGs on every
platform and run (see [decisions.md](decisions.md) D1/D12).

Examples:

```sh
mesh-thumb render model.stl
mesh-thumb render model.ply --size 512 --view top -o thumb.png
mesh-thumb render model.stl --size 256 --background transparent -o icon.png
mesh-thumb render model.stl --quality 4 --width 320 --height 240 --view front -o big.png
cat model.stl | mesh-thumb render - -o piped.png
```

## `inspect`

```
mesh-thumb inspect [--json] <FILE>
```

Parses the file without rendering and prints format, counts, bounds,
colors/normals flags, and warnings. With `--json` the metadata comes
back in the JSON envelope. `-` reads from stdin.

```sh
mesh-thumb inspect model.stl
mesh-thumb inspect cloud.ply --json
```

## `capabilities`

```
mesh-thumb capabilities [--json]
```

Reports what this build supports: formats, views, backends, and the
enforced resource limits (max input bytes, max output dimension and
pixels, max quality). Programmatic consumers should call this once,
then use the limits to bound their own requests. The version is
included in the JSON, so the caller can check binary compatibility.

## `batch`

```
mesh-thumb batch [OPTIONS] --out <DIR> <FILE>...
```

Renders many files into one output directory (created if missing).
Shared render options (`--size`, `--width`, `--height`, `--view`,
`--background`, `--quality`, `--point-size`, `--backend`,
`--cache-dir`, `--no-cache`) apply to every item; `--cache-dir` is
shared by all items. Inputs are filesystem paths only — `-` is not
accepted.

Behavior:

- Files are processed with a bounded worker pool (4 workers by
  default, up to 16; batch is concurrent, and a shared cache is
  thread-safe).
- Per-file errors are **reported and skipped** by default; the batch
  continues. `--abort-on-error` stops at the first failure (exit 1).
- The result JSON (with `--json`) lists items in input order with
  per-file `ok`/`error`/`output`/`cache` fields.
- **Output collisions are rejected up front:** two inputs whose output
  stems collide (`a/cube.stl` + `b/cube.stl` → both `out/cube.png`)
  fail the whole batch with `batch_error` (exit 1) before any file is
  written — never a nondeterministic overwrite race.
- An empty batch (no files) is a usage error (exit 2).

```sh
mesh-thumb batch --out thumbs/ --cache-dir .mesh-cache model_*.stl part_*.ply
mesh-thumb batch --out thumbs/ --abort-on-error --width 320 --height 240 a.stl b.ply
```

## `cache`

```
mesh-thumb cache prune [--json] --dir <DIR> (--all | --older-than <DAYS>)
```

Maintenance for a cache directory created with `--cache-dir`.
Deletes every manifest entry older than the cutoff (`--older-than
DAYS`, whole days, exclusive) or every entry (`--all`), plus any blob
files not referenced by the manifest (orphans). Dot-files in
`blobs/` (atomic-write staging) are never touched. The manifest is
rewritten atomically. Exactly one of `--all` / `--older-than` must be
given; neither or both is a usage error (exit 2).

Human output is `pruned N entries, removed M orphan blobs, freed B
bytes (cache: DIR)`; with `--json` the same fields come back in the
envelope (`cache_dir`, `entries_pruned`, `orphan_blobs_removed`,
`bytes_freed`). A second run reports all zeros (idempotent).

Cache lookups and writes degrade gracefully (a corrupt manifest or
blob is a miss), but `cache prune` surfaces cache errors as failures
(exit 1): pruning is an explicit user command.

```sh
mesh-thumb cache prune --dir .mesh-cache --older-than 30
mesh-thumb cache prune --dir .mesh-cache --all
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success (including batches with per-file failures, which report `failed > 0` in the result) |
| 1 | A command failed: fatal error, bad input, backend unavailable, output error, or abort-on-error batch |
| 2 | Usage error (unknown command/flag, invalid combination) — produced by clap |

## Output conventions

- Without `--json`, results print as short human text on **stdout**
  and diagnostics on **stderr**. Stdout is always reserved for the
  command's result — PNG bytes are never written to stdout (`-o -` is
  intentionally unsupported, see [decisions.md](decisions.md) D13).
- With `--json`, **every** command prints exactly one JSON object on
  stdout — including failures, which still exit 1. The envelope's
  exact shape, error codes, and versioning contract live in
  [protocol.md](protocol.md).

## Cache behavior

`--cache-dir` keeps PNGs keyed by BLAKE3 of the input bytes + settings
+ renderer/cache/protocol versions. A hit is served **before parsing**
the file. The cache is safe to share across concurrent batch workers,
is content-addressed (no eviction; use `cache prune` to reclaim
space), and recovers from corruption by treating the bad entry as a
miss.

## Limits

The enforced runtime limits (input size, vertex/face counts, output
dimensions, quality) are reported by `capabilities` and documented in
[security.md](security.md); exceeding one is a `resource_limit_exceeded`
error, never a crash.