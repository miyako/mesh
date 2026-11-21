# Decisions

This file records significant design decisions and the reasoning behind
them, so future changes can tell an accident from an intentional choice.
Decisions that change *output* or the *cache contract* must bump the
relevant version in `src/versions.rs` and update `docs/protocol.md`.

## D1. Isometric orientation is pinned (top face up, +X right)

The `iso` preset uses the classic isometric basis:

- forward = normalize(1, 1, 1)
- right = normalize(world_up × forward) = (0.7071, 0, -0.7071)
- up = forward × right = (-0.4082, 0.8165, -0.4082)

The top face (+Y) renders at the top of the image and +X points right.
Locked by the `iso_basis_pinned` test and the golden images
(`tests/golden/ref/cube_iso_default.png`).

**Why not "up = right × forward"?** That choice puts the top face at the
*bottom* of the image (upside-down cubes), which reads as a bug.

## D2. Linear-light pipeline; sRGB inputs are linearized at parse time

The framebuffer, shading and matcap operate in linear light and the final
step is `encode_srgb`. Hex backgrounds (`#RRGGBB`) and PLY vertex colors
are sRGB-encoded by convention, so they are converted to linear at parse
time (`srgb_to_linear`) and re-encoded on output.

- `--background #20242b` renders *exactly* as #20242b.
- PLY `uchar red` 255 stays full red; mid-tones are reproduced correctly
  instead of being double-encoded (washed out).

Alternative considered: treat hex as linear and skip the sRGB encode for
the background. That is impossible to do cleanly with premultiplied
compositing (compositing must happen in one space), so linearize-at-parse
is the design. Straight colors after encode (no intermediate
double-encode) are pinned by tests.

## D3. Cache key = BLAKE3(raw bytes) + settings + versions; hit before parse

The cache key is
`mesh-thumb-cache.v{CACHE_SCHEMA}.r{RENDERER}.j{JSON_SCHEMA}.{BLAKE3(settings)}.{BLAKE3(raw input bytes)}`.

- Hashing the **raw input bytes** (not parsed geometry) lets a hit skip
  detection/parse/normalize entirely (spec §19.2: "cache hit before
  parse").
- The **settings string** (`RenderOptions::settings_string`) is the
  canonical serialization of every output-affecting option.
- Validation: on a hit the BLAKE3 in the manifest must match a file whose
  recorded size matches the blob; a corrupt or stale manifest is
  discarded and rebuilt.

## D4. Atomic writes everywhere

Blob and manifest writes go to a unique temp file, `fsync`, then
`rename`. Output PNGs do the same (`write_atomic`). Readers therefore
never observe a torn file; a crash leaves at worst a stray `.tmp`.

## D5. Batch mode is concurrent; the cache is thread-safe

`batch` runs up to `DEFAULT_BATCH_CONCURRENCY` (4; max 16) workers over
a shared cache. `Cache::get`/`put` serialize manifest read-modify-write
through an internal mutex and use unique temp names, so concurrent
workers never collide. Per-file errors are collected non-fatally unless
`--abort-on-error` — see `docs/protocol.md` for exit-code semantics.

## D6. Empty geometry is an explicit error

A file that parses to zero usable triangles/points (e.g. an all-degenerate
STL) is an error (`invalid_geometry`), not a silently blank image. The
spec's warning counters (`degenerate_triangles`, `non_finite_vertices`)
still report what was skipped.

## D7. GPU backend exists on paper and fails clearly

`--backend gpu` resolves to `backend_unavailable` with an actionable
message. This preserves the CLI contract and the "GPU-when-it-exists"
roadmap without pretending there is a GPU path.

## D8. Supersampling is quality-scale squared, downsample is premultiplied

`quality` multiplies the internal resolution in both axes and the buffer
is box-filtered in premultiplied-alpha space (so dark-on-transparent
edges don't bleed color). `downsample_keeps_alpha` locks the behavior.

## D9. Hex colors are sRGB (control D1-D2's formats)

Canonical background form: `#RRGGBB` (also accepted `#RGB`). Only solid
hex and `transparent` are supported; named CSS colors are rejected rather
than half-supported.

## D10. Human vs JSON output

Without `--json`, the CLI prints short human text: results to stdout,
`code: message` errors to stderr. With `--json`, **every** command emits a
structured envelope on stdout — including failures, which print a
`schema_version/success/error` object and still exit 1. Stdout stays
parseable in both cases.

## D11. Batch output collisions fail the whole batch up front

Two inputs whose stems map to the same `<out_dir>/<stem>.png`
(`a/cube.stl` + `b/cube.stl`) are a `batch_error` before any work runs —
never a silent last-write-wins race. Under a worker pool the overwrite
winner would be nondeterministic, so the only honest options are fail or
disambiguate; failing with a clear message (and a JSON envelope) keeps
the tool deterministic. An empty batch is a clap usage error (exit 2),
not a silent no-op.
## D12. Performance work must not change pixels (golden refs enforce)

renderer keeps the golden reference images in `tests/golden/ref/` pinned
pixel-for-pixel; a deleted/never-written reference is regenerated and
reviewed, but byte-equal output is the gate for any shader/raster change.
Consequence: the flat-shading "fast path" (using `normals[0]` directly
when all three vertex normals are equal, skipping the barycentric
interpolate+normalize) is NOT used — barycentric floats rarely sum to
exactly 1.0, so `w0*n+w1*n+w2*n` before normalization is not
bit-identical to `n`, which flips enough matcap texel selections to move
~30k channels (measured). Optimizations instead hoist work that is
bit-identical by construction, e.g. per-vertex normal orientation
precomputed once (O(V)) instead of per triangle vertex (O(3T)).

## D13. Stdin geometry input (`-`); stdout stays reserved

`render` and `inspect` accept `-` as the input file to read geometry
bytes from stdin (batch does not: it needs filesystem stems). There is
no default output name for piped input, so `render -` requires
`-o/--output` (a runtime error, not a clap error, because the flag is
valid — just mandatory here). PNG bytes are never written to stdout:
the `-o -` form is intentionally unsupported, because stdout carries
the command's result (human or JSON envelope) and the spec reserves
stdout for protocol data. Piped bytes are hashed for the cache key
identically to file bytes, so both routes share entries.

## D14. Cache pruning is age-based plus orphan cleanup

`cache prune` removes manifest entries by creation age (`--older-than
DAYS`, exclusive) or everything (`--all`) and deletes blob files not
referenced by the manifest. There is deliberately no LRU/eviction-on-
write and no size budget: the cache is content-addressed and bounded by
the input set in practice, and the spec's backlog asks for an explicit
clearCache operation, not background pressure. Dot-files in `blobs/`
(the atomic-write staging suffix) are never pruned. Unlike lookups and
writes — which degrade to a miss on I/O trouble — prune is an explicit
user command and surfaces cache errors (`cache_error`, exit 1).

## D15. Phase 6 wrapper: contract here, component in the 4D repo

The `MeshPreview.Thumbnailer` wrapper (spec §22) is a 4D component and
lives in the 4D component repository following the existing
`quickdraw` / `4d-component-template` conventions — not in this Rust
repo, which has no 4D toolchain or framework context. What this repo
owns is the *contract* the wrapper consumes (`docs/wrapper.md`) and
the release artifacts (§23.2), so Phase 6's gate ("a 4D project can
call render/inspect without external downloads") is decoupled from
CLI work here. The wrapper starts after the CLI contract is stable;
**additive** CLI changes are allowed after that point because the
wrapper parses the schema-versioned JSON envelope and checks
`schema_version` (protocol.md), and additive options never change
existing result fields.

## D16. Envelope-on-stdout decoding; plain-text --version; PNG validation

`MeshPreview.Thumbnailer` (component `mesh`, class
`Thumbnailer`) treats the stdout JSON envelope as authoritative even
when the process exit code is nonzero: an `io_error`/`unsupported_format`
in the envelope is surfaced as the wrapper `error` (`code`, `message`,
`path`, `stage`) instead of a generic nonzero-exit `cli_error`. Exit
codes and stderr diagnostics remain advisory and are still retained.
`inspect`/`capabilities` decode through the same `_decode`; `version`
does **not** use an envelope — `--version` stays plain text
(`mesh-thumb <semver>`), parsed with a strict regex that rejects other
shapes as `invalid_protocol` while keeping stdout identical to the CLI.
`render` validates the produced PNG header (magic `\x89PNG\r\n\x1a\n`)
before reporting success and exposes the deterministic `pngSize`
(rendering is byte-reproducible per D12/D13), so the real 4D host
fixture asserts exact PNG size and the decoded envelope fields rather
than re-hashing bytes in 4D (avoiding hash-encoding ambiguity while
still pinning determinism). Section 22.1/22.3 options (`size`, `view`,
`background`, `quality`) are type/range-validated in 4D **before** any
process launches, failing with `invalid_argument`. `clearCache` remains
deferred: `render` never populates a cache (`cache:"off"`), so the §22.5
cache cases are covered by the CLI contract, not the wrapper. The host
contract holds exact `stdout` cases only for host-independent wire
outputs (`--version`, `capabilities`); render/inspect cases that embed
staged paths or `render_ms` are asserted structurally
(pointer `equals`/`matches`/`length`/`each`) via the runner, never as
pretended fixed vectors.
