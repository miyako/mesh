# Security

`mesh-thumb` parses untrusted files (STL/PLY) into memory and renders
them. This document states the threat model, the concrete limits, and
what happens when a limit is exceeded. Limits live in
`src/diagnostics/limits.rs`; the same values are reported by
`mesh-thumb capabilities`.

## Threat model

Inputs are assumed **untrusted and potentially malicious**. Rust memory
safety plus the fact that no `unsafe` appears anywhere in crate code
removes whole classes of memory bugs, but the remaining risks are
resource exhaustion and algorithmic abuse:

- **Bombs**: tiny file → huge declared counts (PLY header lies about
  element counts; STL binary length is authoritative so it is
  self-limiting).
- **CPU spins**: pathological geometries (many degenerate triangles),
  huge supersampling, huge framebuffers.
- **Path traversal via cache/output dirs**: a crafted input *filename*
  must never escape the requested output directory.

No network access, no shelling out, no inputs other than the file, the
CLI args, and the cache dir. The renderer is essentially pure.

## Concrete limits

| Limit | Value | Enforced where |
|-------|-------|----------------|
| Input file size | 1 GiB (1<<30) | `read_file_limited` before parsing |
| PLY header size | 64 KiB | PLY parser (header window) |
| PLY header line length | 4096 | PLY parser |
| STL ASCII line length | 256 | STL scanner |
| Vertices | 20 000 000 | `validate_counts` |
| Triangles | 10 000 000 | `validate_counts` |
| Point cloud points | 20 000 000 | `validate_counts` |
| Face vertex-list length | 1 000 000 | face list parsing |
| Output dimension | 8192 px / side | `validate_output_dimensions` |
| Output pixels | 64 MP | `validate_output_dimensions` |
| Supersampling | quality ≤ 8 | `validate_output_dimensions` |
| Framebuffer (supersampled RGBA8) | 512 MiB | `validate_output_dimensions` |
| Batch concurrency | 4 default / 16 max | `batch` runner |

All size comparisons use checked arithmetic; a count that would overflow
is rejected with `resource_limit_exceeded` (see
`limits::checked_usize`).

## Behavior on abuse

- Exceeding any limit fails that command with
  `resource_limit_exceeded` and exit 1 — a *deterministic* error, no
  partial render, no retry loop.
- A **corrupt manifest or cache blob** is treated as a miss and rebuilt
  (`cache_error` never aborts a render; the cache gracefully degrades).
- A **malformed input** (bad magic, invalid PLY header, non-finite
  vertex data, out-of-range indices, empty geometry) is a typed error
  (`unsupported_format`, `invalid_ply_header`, `invalid_ply_data`,
  `invalid_geometry`) — never a panic. The lib test suite and the
  integration/batch tests exercise these paths.

## Path escapes

- Output paths are joined against the requested output directory only
  (`out_dir.join(stem.png)` in batch; CLI `-o` resolved directly in
  render). Input stems with `..`/`/` are sanitized to the stem alone, so
  a file named `../../etc/cron` yields `cron.png`, never a traversal.
- Cache keys are content-derived hex strings, never input names.

## Panic discipline

Parsing and rendering code avoid indexing and unwrap-on-untrusted-data.
A **deterministic mutation walk** (see `tests/fuzz_smoke.rs`) feeds
hundreds of bit-flipped, truncated and ASCII-mangled files through the
full pipeline and asserts invariants: no panic, a typed `Error` on every
failure (never poisoned state), and a valid PNG whenever the pipeline
succeeds. These run in CI; a real libFuzzer target can be layered on the
same corpus where `cargo-fuzz` is available.