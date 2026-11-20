# Acceptance tracking

Living checklist mirroring §26 of the [project spec](spec/MeshPreview_Project_Specification.md).
Status symbols: `[x]` = implemented and covered by tests/docs, `[~]` =
partially covered (see note), `[ ]` = not done. This file is updated
with each milestone (spec §28.6).

## Functional

- [x] Binary STL renders — `tri_binary.stl` fixture
- [x] ASCII STL renders — `cube_ascii.stl` fixture + golden refs
- [x] Binary PLY little-endian renders — LE unit + CLI tests, `binary_ply_uchar` colors test
- [x] Binary PLY big-endian renders — BE unit tests (src/formats/ply.rs)
- [x] ASCII PLY renders — `ring_ascii.ply` fixture + golden
- [x] PLY triangle and polygon faces handled — face lists + fan triangulation
- [x] Vertex-only PLY renders as point cloud — splat renderer + fixture tests
- [x] Vertex colors are preserved — uchar/float color mapping tested
- [x] Normal data does not cause rendering failures
- [x] Default view is orthographic and stable — D1 pins orientation
- [x] Views `iso`, `top`, `front`, `side` implemented and documented — [cli.md](cli.md), [protocol.md](protocol.md)
- [x] PNG output is valid — decoded + dimension-checked in tests
- [x] Requested dimensions are honored — `--size`/`--width`/`--height`
- [x] Background/transparency work — `#RRGGBB` + `transparent` (alpha zeroed)
- [x] Supersampling works — `--quality 1..8`, premultiplied downsample
- [x] Inspect returns useful metadata — format, counts, bounds, colors/normals, warnings
- [x] Batch continues after a file failure — per-file errors reported, `failed > 0`
- [x] JSON output is machine-readable and stable — schema-versioned envelope

## Reliability

- [x] Malformed inputs return errors, not panics — fuzz smoke + malformed-input tests
- [x] Counts and dimensions are bounded — `capabilities` limits, `resource_limit_exceeded`
- [x] Indexes are validated — PLY index range checks
- [x] Numeric values are checked — non-finite vertex rejection
- [x] Atomic output is used — temp-file + rename everywhere (D4)
- [x] Cache corruption is recoverable — corrupt manifest/blob → miss
- [x] Concurrent jobs are bounded — batch worker pool (4..16), shared cache is thread-safe
- [x] Paths with spaces and Unicode work — CLI tests

## Cache

- [x] Input hash is calculated before parsing — BLAKE3 in `load` path; hit-before-parse
- [x] Cache identity includes settings and versions — key = `r.v.j` + settings + raw bytes (D3)
- [x] Cache hit skips parsing and rendering — `cache: hit` + byte-identical output test
- [x] Manifest is written — `manifest.json`, schema-versioned
- [x] PNG validity is checked — cached bytes are full, size-verified PNG blobs
- [x] Writes are atomic — blob + manifest temp/rename (D4)
- [x] Cache bypass is supported — `--no-cache`
- [x] Cache maintenance — `cache prune` subcommand (orphans + age), tested

## Engineering

- [x] Unit/integration/golden tests exist — lib + CLI + golden pixel refs
- [~] Fuzz targets exist for parsers — `tests/fuzz_smoke.rs` harness; cargo-fuzz integration pending (no cargo-fuzz in the toolchain; smoke fuzz runs every test invocation instead)
- [x] CI runs formatting, lint, tests, and builds — `.github/workflows/ci.yml`
- [x] Documentation is complete — README + docs/ (architecture, cli, format-support, protocol, security, decisions, wrapper, acceptance)
- [x] Benchmarks are recorded — `benches/render_bench.rs` + D12 baselines
- [x] Release artifacts and checksums can be produced — release.yml, `checksums.txt`, incl. macOS universal binary

## Phase gates (spec §25)

| Phase | Status |
|-------|--------|
| 0 Contract & setup | ✅ shipped (v0.1.0) |
| 1 CPU mesh vertical slice | ✅ |
| 2 Robust STL + renderer quality | ✅ |
| 3 PLY + point clouds | ✅ |
| 4 Production CLI/cache/batch | ✅ shipping since v0.1.0 (stdin + cache prune added v0.2.0) |
| 5 Hardening + release | ✅ v0.1.0/v0.2.0, checksums, universal macOS |
| 6 4D wrapper | 🚧 contract pinned in [wrapper.md](wrapper.md); component implementation in the 4D repo — gated on stable CLI contract (shipped v0.2.0) |
| 7 Optional GPU | ⬜ deferred (backlog) |

Last updated: v0.2.0 cycle.