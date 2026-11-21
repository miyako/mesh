# Phase 6: `MeshPreview.Thumbnailer` wrapper contract

Spec reference: [project spec §22](spec/MeshPreview_Project_Specification.md)
("Future 4D component wrapper") and §25 Phase 6. The Rust CLI is the
primary product; the 4D component is a thin consumer and must not
dictate renderer complexity.

**Scope of this document.** This repository ships the CLI, the binary
artifacts, and the stable JSON contract the wrapper consumes. The
wrapper component itself lives in the 4D component repository and
follows the convention of the existing `quickdraw` and
`4d-component-template` projects — the "how to write a 4D component"
parts are intentionally not reinvented here. This document is the
contract: what the wrapper must call, what it must parse, and how it
must behave. Phase 6 is only implemented once a 4D project can call
`render`/`inspect` with no external downloads and receive structured
results (§25 gate).

## 22.1 API mapping

Suggested public name: `MeshPreview.Thumbnailer`. Conceptual methods
and their CLI backing:

| Method | CLI invocation | Consumes |
|--------|----------------|----------|
| `render(inputPath, options) -> result` | `mesh-thumb render --json <options> <inputPath> -o <tmp.png>` (stdin allowed: `-` + piped bytes + `-o`) | stdout JSON envelope (`success`, `result.width/height/render_ms/cache/geometry`, `error`) + the PNG bytes at the `-o` path |
| `inspect(inputPath) -> metadata` | `mesh-thumb inspect --json <inputPath>` (stdin allowed) | stdout JSON envelope `result` (input, geometry, warnings) |
| `version() -> versionInfo` | `mesh-thumb --version` (human) or `mesh-thumb capabilities --json` (structured) | `mesh-thumb capabilities --json` → `result.version` |
| `capabilities() -> capabilities` | `mesh-thumb capabilities --json` | `result` fields: `formats`, `views`, `backends`, limits |
| `clearCache(options) -> result` *(optional/deferred)* | `mesh-thumb cache prune --json --dir <DIR> (--all \| --older-than N)` | `result` fields: `entries_pruned`, `orphan_blobs_removed`, `bytes_freed` |

Option translation for `render` (`--json` result fields the wrapper
should surface): `--size`, `--width/--height`, `--view`,
`--background`, `--quality`, `--point-size`. The wrapper does not
reimplement any of this logic; it passes flags through.

## 22.2 Process invocation rules

- **Bundle, never download:** ship the native executable inside the
  component package. No runtime downloads, no network access (also a
  wrapper test).
- **Argument arrays:** construct the child process with an argument
  array (a path list), never a shell-concatenated command string.
- **stdout is protocol:** in `--json` mode stdout carries exactly one
  JSON envelope. Capture stdout as data; capture **stderr
  separately** for diagnostics.
- **PNG handling:** pass `-o` to an **application-owned temp file**
  (never guess in the child), validate the PNG (magic bytes
  `\x89PNG` + decode header) before loading it into the framework's
  `Picture`/image object with the framework's supported API. Never
  ask the CLI to write PNG bytes to stdout (`-o -` is unsupported by
  design, D13).
- **Timeouts:** enforce a per-call timeout and kill the worker on
  expiry. `render` on huge inputs is bounded by the CLI's own limits
  (`capabilities`), but the wrapper must still not block the host.
- **Concurrency:** bound simultaneous worker processes; queue
  requests. Do not launch unbounded processes.
- **No uncontrolled retries:** a failed call returns a structured
  error; only the caller may decide to retry.
- **Errors:** map child failures (non-zero exit, JSON parse failure,
  PNG validation failure, timeout, launch failure) to structured
  errors; never crash the host.
- **Unicode/paths:** pass paths with spaces and non-ASCII as literal
  argument entries; on Windows use UTF-16-aware process APIs where
  required by the platform.

## 22.3 Binary distribution and selection

Release artifacts (produced by `.github/workflows/release.yml`, every
tagged release):

| Platform | Artifact | Notes |
|----------|----------|-------|
| macOS | `mesh-thumb-macos-universal` | arm64 + x86_64 fat binary (lipo), macOS 14+ (since v0.2.0; satisfies §22.4) |
| Windows | `mesh-thumb-windows-x86_64.exe` | x64 |
| Linux | `mesh-thumb-linux-x86_64` | glibc Ubuntu build; optional for the component unless required |

Each release also ships `checksums.txt` (SHA-256 of every attached
binary). Requirements:

- The component **selects the correct bundled binary** per host
  (`CPU`/`platform`), preferring the universal macOS binary on macOS.
- On macOS, verify **executable permissions** (the artifact keeps the
  `+x` bit through the release pipeline — re-assert if the bundling
  step strips it) and validate **signing/notarization** for the real
  distribution channel (the CI `lipo` output is unsigned).
- Version compatibility is decided at runtime: read
  `mesh-thumb capabilities --json` (or `--version`) and refuse to
  operate when the binary's major version does not match the
  component's expected contract.

## 22.4 CLI contract stability

The wrapper depends on the CLI contract in [protocol.md](protocol.md):
the JSON envelope (`schema_version`, `success`, `result`, `error`),
error codes, exit-code contract, and stdin rules. That contract is
versioned:

- `JSON_SCHEMA_VERSION` — bump on incompatible envelope changes;
  wrappers must check `schema_version` before trusting `result`.
- `RENDERER_VERSION` — bump when pixels change (cache invalidation).
- `CACHE_SCHEMA_VERSION` — bump on cache layout/key changes.

Additive CLI changes (new commands/options) do not break the
contract; `capabilities` is the wrapper's way to discover them. A
wrapper that parses the envelope strictly per this document and checks
`schema_version` is forward-compatible within a schema version.

## 22.5 Wrapper test checklist

Each item, with the executable check it maps to (run against a local
binary under the component's harness):

| §22.5 test | Check |
|------------|-------|
| Binary discovery | Component finds the correct artifact per platform (universal on macOS); missing binary → structured error |
| Version compatibility | `capabilities --json` version/schema match; mismatch rejected before any render |
| Successful render | `render --json -o tmp.png` → exit 0, `success: true`, PNG decodes |
| Successful inspect | `inspect --json` → exit 0, valid metadata |
| Invalid input | garbage file → exit 1, `success: false`, error code + message, no PNG produced |
| Timeout | long-running call is killed at the wrapper timeout; host stays responsive |
| Process launch failure | missing binary / exhausted resources → structured error, no crash |
| Unicode/space paths | file named `héllo wörld.stl` in a spaced directory renders |
| JSON parse failure | non-JSON stdout (should never happen; test defensive path) → structured error |
| PNG validation failure | truncated/corrupt PNG at the temp path → structured error, never an image load crash |
| Concurrent requests | N simultaneous renders with bounded workers, all succeed |
| Packaging includes binary | component bundle contains the correct artifact + checksum verified |
| No network access | render works with network disabled |

## Implementation status

- [x] Universal macOS binary (v0.2.0+), Windows x64, Linux optional — §23.2 artifacts
- [x] Stable JSON protocol with schema/versioning — [protocol.md](protocol.md)
- [x] Stdin input for `render`/`inspect`, `cache prune` — CLI surface the wrapper maps to
- [x] Component API implemented: `component/mesh` in this
      monorepo, `cs.MeshPreview.Thumbnailer` with `render` / `inspect` /
      `version` / `capabilities` (per `quickdraw` / `4d-component-template`
      conventions, D15).
- [x] `SystemWorker` handling: generic `_Process` adapter (synchronous,
      stdin closed, UTF-8, 30 s timeout), `_CLI` resolution of the bundled
      platform binary, envelope decoding per D16, PNG magic validation in
      `render`. The wrapper is synchronous (single in-flight call); the
      §22.5 timeout case is covered by `_Process`'s finite timeout, and
      concurrency bounding is deferred until a real asynchronous caller
      exists.
- [x] Real 4D compilation: `scripts/compile_component.py` passes with the
      pinned free tool4d 20.7/101905 on this host — `x86_64_generic` +
      `arm64_macOS_lib`, input/output digests bound in
      `build/compile-status.json`; the compiler regenerated
      `Resources/en.lproj/syntaxEN.json` with the `Thumbnailer` class (no
      Greeting metadata). `version()` and `_pngValid()` were hardened by the
      actual compiler (no `Trim`; `Match regex` position outputs instead of a
      groups Collection; PNG signature checked byte-wise via `DOCUMENT TO BLOB`,
      no `FileHandle`).
- [x] Packaging + binary provisioning: the `external_cli` adapter pins the
      released `v0.3.0` ZIP (`asset-lock.json`, real released sidecar digests)
      and assembles `Resources/bin/<platform>/mesh-thumb[.exe]`; source-host
      smoke and compiled-package host runs pass on real tool4d 20.7/101905
      (macOS ARM64). Windows x64 binary is provisioned and digest-bound; native
      Windows 4D loading remains a separate hosted check (`not_run` locally).

Phase 6 started in the component repository once the CLI contract here
was stable; this document pins the contract side and this monorepo hosts
the component implementation. The exact 4D signatures are defined in
[the component docs](../component/mesh/component/mesh/Documentation/Classes/Thumbnailer.md).