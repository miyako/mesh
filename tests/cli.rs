//! Bin-level integration tests: exercise the released `mesh-thumb`
//! binary end-to-end (exit codes, stdout/stderr, PNG validity, cache,
//! batch behavior, JSON protocol).
//!
//! Exit code contract (docs/protocol.md):
//!   0 = ok; 1 = a command failed (fatal error or abort-on-error batch);
//!   2 = usage error (clap argument parsing).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mesh-thumb")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to spawn mesh-thumb binary")
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    use std::io::Write;
    let mut child = Command::new(bin())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn mesh-thumb binary");
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("wait for mesh-thumb")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(bytes)
        .read_info()
        .expect("valid PNG header");
    let info = reader.info().clone();
    (info.width, info.height)
}

fn png_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("output file exists")
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mesh_thumb_test_{}_{name}", std::process::id()))
}

// ---------- help / usage ----------

#[test]
fn help_lists_commands_and_exits_zero() {
    let o = run(&["--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).contains("render"));
    assert!(stdout(&o).contains("inspect"));
    assert!(stdout(&o).contains("batch"));
    assert!(stdout(&o).contains("capabilities"));
}

#[test]
fn usage_error_exits_2() {
    // Unknown command/flag -> clap usage error -> exit 2.
    let o = run(&["definitely_not_a_command"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn conflicting_size_and_width_exits_2() {
    let o = run(&[
        "render",
        "--size",
        "64",
        "--width",
        "64",
        &fixture("cube_ascii.stl").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(2));
}

// ---------- render ----------

#[test]
fn render_default_output_is_valid_png() {
    let input = fixture("cube_ascii.stl");
    // No -o: default output is the input path with .png appended.
    let expected = input.with_extension("png");
    let _ = std::fs::remove_file(&expected);
    let o = run(&["render", &input.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let bytes = png_bytes(&expected);
    assert_eq!(png_dimensions(&bytes), (256, 256));
    let _ = std::fs::remove_file(&expected);
}

#[test]
fn render_writes_square_default_size() {
    let out = temp("render_square.png");
    let o = run(&[
        "render",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(png_dimensions(&png_bytes(&out)), (256, 256));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn render_respects_size_flag() {
    let out = temp("render_size.png");
    let o = run(&[
        "render",
        "--size",
        "128",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(png_dimensions(&png_bytes(&out)), (128, 128));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn render_transparent_background_has_alpha_zeros() {
    let out = temp("render_trans.png");
    let o = run(&[
        "render",
        "--background",
        "transparent",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    let bytes = png_bytes(&out);
    let mut reader = png::Decoder::new(&bytes[..]).read_info().unwrap();
    let sp = reader.info().color_type.samples();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    // Corner background should be fully transparent (alpha 0).
    let corner_alpha = buf[sp - 1];
    assert_eq!(corner_alpha, 0, "corner should be transparent");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn render_is_deterministic_without_cache() {
    let a = temp("det_a.png");
    let b = temp("det_b.png");
    let oa = run(&[
        "render",
        &fixture("ring_ascii.ply").to_string_lossy(),
        "-o",
        &a.to_string_lossy(),
    ]);
    let ob = run(&[
        "render",
        &fixture("ring_ascii.ply").to_string_lossy(),
        "-o",
        &b.to_string_lossy(),
    ]);
    assert_eq!(oa.status.code(), Some(0));
    assert_eq!(ob.status.code(), Some(0));
    assert_eq!(
        png_bytes(&a),
        png_bytes(&b),
        "renders must be byte-identical"
    );
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

// ---------- stdin input (`-`) ----------

#[test]
fn render_stdin_matches_file_render_byte_for_byte() {
    let fixture_bytes = std::fs::read(fixture("cube_ascii.stl")).unwrap();
    let file_out = temp("stdin_file.png");
    let stdin_out = temp("stdin_pipe.png");
    let o = run(&[
        "render",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &file_out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let o = run_with_stdin(
        &["render", "-", "-o", &stdin_out.to_string_lossy()],
        &fixture_bytes,
    );
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert_eq!(png_bytes(&file_out), png_bytes(&stdin_out));
    let _ = std::fs::remove_file(&file_out);
    let _ = std::fs::remove_file(&stdin_out);
}

#[test]
fn render_stdin_requires_output_path() {
    let fixture_bytes = std::fs::read(fixture("cube_ascii.stl")).unwrap();
    let o = run_with_stdin(&["render", "-"], &fixture_bytes);
    assert_eq!(o.status.code(), Some(1), "stderr: {}", stderr(&o));
    assert!(stderr(&o).contains("output path"), "stderr: {}", stderr(&o));
}

#[test]
fn render_stdin_accepts_binary_stl() {
    let fixture_bytes = std::fs::read(fixture("tri_binary.stl")).unwrap();
    let out = temp("stdin_binary.png");
    let o = run_with_stdin(
        &["render", "-", "-o", &out.to_string_lossy()],
        &fixture_bytes,
    );
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert_eq!(png_dimensions(&png_bytes(&out)), (256, 256));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn inspect_stdin_reports_format() {
    let fixture_bytes = std::fs::read(fixture("cube_ascii.stl")).unwrap();
    let o = run_with_stdin(&["inspect", "-"], &fixture_bytes);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(stdout(&o).contains("stl_ascii"), "stdout: {}", stdout(&o));
}

// ---------- render --json ----------

#[test]
fn render_json_success_shape() {
    let out = temp("render_json.png");
    let o = run(&[
        "render",
        "--json",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["success"], true);
    assert_eq!(v["result"]["width"], 256);
    assert_eq!(v["result"]["height"], 256);
    assert_eq!(v["result"]["cache"], "off");
    assert_eq!(v["result"]["geometry"]["kind"], "mesh");
    assert_eq!(v["result"]["geometry"]["triangles"], 12);
    assert!(!v["result"]["render_ms"].is_null());
    let _ = std::fs::remove_file(&out);
}

#[test]
fn render_json_error_shape_and_exit_1() {
    let o = run(&[
        "render",
        "--json",
        &temp("missing.stl").to_string_lossy(),
        "-o",
        &temp("x.png").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).expect("valid JSON even on error");
    assert_eq!(v["success"], false);
    assert_eq!(v["error"]["code"], "io_error");
    assert!(!v["error"]["message"].as_str().unwrap().is_empty());
}

// ---------- inspect ----------

#[test]
fn inspect_human_output() {
    let o = run(&["inspect", &fixture("cube_ascii.stl").to_string_lossy()]);
    assert_eq!(o.status.code(), Some(0));
    let s = stdout(&o);
    assert!(s.contains("mesh ("), "got: {s}");
    assert!(s.contains("bounds:"), "got: {s}");
}

#[test]
fn inspect_json_counts() {
    let o = run(&[
        "inspect",
        "--json",
        &fixture("cube_ascii.stl").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["result"]["geometry"]["vertices"], 36);
    assert_eq!(v["result"]["geometry"]["triangles"], 12);
    assert_eq!(v["result"]["warnings"]["degenerate_triangles"], 0);
}

// ---------- capabilities ----------

#[test]
fn capabilities_reports_inventory() {
    let o = run(&["capabilities"]);
    assert_eq!(o.status.code(), Some(0));
    let s = stdout(&o);
    assert!(s.contains("stl"));
    assert!(s.contains("ply"));
    assert!(s.contains("cpu"));
    assert!(s.contains("gpu backend: unavailable"));
}

// ---------- error paths ----------

#[test]
fn missing_file_exits_1_with_code() {
    let o = run(&[
        "render",
        &temp("nonexistent.stl").to_string_lossy(),
        "-o",
        &temp("x.png").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("io_error"), "got: {}", stderr(&o));
}

#[test]
fn malformed_input_exits_1_with_code() {
    let bad = temp("bad.bin");
    std::fs::write(&bad, b"this is definitely not a mesh").unwrap();
    let o = run(&[
        "render",
        &bad.to_string_lossy(),
        "-o",
        &temp("x.png").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("unsupported_format"),
        "got: {}",
        stderr(&o)
    );
    let _ = std::fs::remove_file(&bad);
}

#[test]
fn gpu_backend_fails_clearly() {
    let o = run(&[
        "render",
        "--backend",
        "gpu",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &temp("x.png").to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("backend_unavailable"),
        "got: {}",
        stderr(&o)
    );
}

// ---------- cache ----------

#[test]
fn cache_misses_then_hits() {
    let cache = temp("cache");
    let out1 = temp("cache_1.png");
    let out2 = temp("cache_2.png");
    let o1 = run(&[
        "render",
        "--cache-dir",
        &cache.to_string_lossy(),
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out1.to_string_lossy(),
    ]);
    let o2 = run(&[
        "render",
        "--cache-dir",
        &cache.to_string_lossy(),
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out2.to_string_lossy(),
    ]);
    assert_eq!(o1.status.code(), Some(0));
    assert_eq!(o2.status.code(), Some(0));
    assert!(stdout(&o1).contains("cache: miss"), "got: {}", stdout(&o1));
    assert!(stdout(&o2).contains("cache: hit"), "got: {}", stdout(&o2));
    assert_eq!(
        png_bytes(&out1),
        png_bytes(&out2),
        "cached and fresh output must match"
    );
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&out1);
    let _ = std::fs::remove_file(&out2);
}

#[test]
fn prune_all_empties_cache() {
    let cache = temp("prune_cache");
    let out = temp("prune_out.png");
    let o = run(&[
        "render",
        "--cache-dir",
        &cache.to_string_lossy(),
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    let o = run(&["cache", "prune", "--dir", &cache.to_string_lossy(), "--all"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(
        stdout(&o).contains("pruned 1 entries"),
        "got: {}",
        stdout(&o)
    );
    assert_eq!(
        std::fs::read_dir(cache.join("blobs")).unwrap().count(),
        0,
        "blobs dir should be empty after --all"
    );
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn prune_removes_orphan_blobs() {
    let cache = temp("prune_orphan_cache");
    let out = temp("prune_orphan_out.png");
    run(&[
        "render",
        "--cache-dir",
        &cache.to_string_lossy(),
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    std::fs::create_dir_all(cache.join("blobs")).unwrap();
    std::fs::write(cache.join("blobs").join("deadbeef.png"), b"stray").unwrap();
    // Dot-files (atomic-write staging) must never be pruned.
    std::fs::write(cache.join("blobs").join(".staging.tmp"), b"x").unwrap();
    let o = run(&["cache", "prune", "--dir", &cache.to_string_lossy(), "--all"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(
        stdout(&o).contains("removed 1 orphan blobs"),
        "got: {}",
        stdout(&o)
    );
    assert!(!cache.join("blobs").join("deadbeef.png").exists());
    assert!(cache.join("blobs").join(".staging.tmp").exists());
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn prune_older_than_removes_only_old_entries() {
    let cache = temp("prune_old_cache");
    let out = temp("prune_old_out.png");
    run(&[
        "render",
        "--cache-dir",
        &cache.to_string_lossy(),
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    // Backdate the single entry so it looks a year old.
    let manifest_path = cache.join("manifest.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    for entry in v["entries"].as_object_mut().unwrap().values_mut() {
        entry["created_ms"] = serde_json::json!(1000u64);
    }
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let o = run(&[
        "cache",
        "prune",
        "--dir",
        &cache.to_string_lossy(),
        "--older-than",
        "1",
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    assert!(
        stdout(&o).contains("pruned 1 entries"),
        "got: {}",
        stdout(&o)
    );
    let o = run(&["cache", "prune", "--dir", &cache.to_string_lossy(), "--all"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        stdout(&o).contains("pruned 0 entries"),
        "got: {}",
        stdout(&o)
    );
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn prune_missing_dir_is_error() {
    let missing = temp("prune_missing").join("nope");
    let o = run(&[
        "cache",
        "prune",
        "--dir",
        &missing.to_string_lossy(),
        "--all",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("does not exist"), "got: {}", stderr(&o));
}

#[test]
fn prune_requires_exactly_one_policy_flag() {
    let cache = temp("prune_usage_cache");
    // Neither --all nor --older-than: usage error.
    let o = run(&["cache", "prune", "--dir", &cache.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(2), "stderr: {}", stderr(&o));
    // Both together: usage error.
    let o = run(&[
        "cache",
        "prune",
        "--dir",
        &cache.to_string_lossy(),
        "--all",
        "--older-than",
        "1",
    ]);
    assert_eq!(o.status.code(), Some(2), "stderr: {}", stderr(&o));
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
fn prune_json_reports_stats() {
    let cache = temp("prune_json_cache");
    let out = temp("prune_json_out.png");
    run(&[
        "render",
        "--cache-dir",
        &cache.to_string_lossy(),
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    let o = run(&[
        "cache",
        "prune",
        "--json",
        "--dir",
        &cache.to_string_lossy(),
        "--all",
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["result"]["entries_pruned"], 1);
    assert_eq!(v["result"]["orphan_blobs_removed"], 0);
    assert!(v["result"]["bytes_freed"].as_u64().unwrap() > 0);
    assert_eq!(
        v["result"]["cache_dir"],
        cache.to_string_lossy().into_owned()
    );
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&out);
}

// ---------- batch ----------

#[test]
fn batch_renders_ok_files_and_skips_bad() {
    let good = fixture("cube_ascii.stl");
    let bad = temp("batch_bad.stl");
    std::fs::write(&bad, b"complete garbage").unwrap();
    let out = temp("batch_out");
    let o = run(&[
        "batch",
        "--json",
        "--out",
        &out.to_string_lossy(),
        &good.to_string_lossy(),
        &bad.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0), "per-file errors should not abort");
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["result"]["total"], 2);
    assert_eq!(v["result"]["ok"], 1);
    assert_eq!(v["result"]["failed"], 1);
    assert!(out.join("cube_ascii.png").exists());
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_file(&bad);
}

#[test]
fn batch_abort_on_error_exits_1() {
    let good = fixture("cube_ascii.stl");
    let bad = temp("batch_abort_bad.stl");
    std::fs::write(&bad, b"garbage").unwrap();
    let out = temp("batch_abort_out");
    let o = run(&[
        "batch",
        "--abort-on-error",
        "--out",
        &out.to_string_lossy(),
        &bad.to_string_lossy(),
        &good.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_file(&bad);
}

#[test]
fn batch_zero_files_is_usage_error() {
    // clap's `required = true` on <FILE>... makes an empty batch a usage
    // error (exit 2) rather than a silent no-op.
    let out = temp("batch_empty_out");
    let o = run(&["batch", "--out", &out.to_string_lossy()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        stderr(&o).contains("following required arguments"),
        "got: {}",
        stderr(&o)
    );
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn batch_rejects_output_collisions_before_working() {
    // Two inputs with the same stem would silently overwrite one output
    // (nondeterministic winner under concurrency) — the batch must fail
    // up front with a batch_error and leave the output dir untouched.
    let work = temp("batch_col_work");
    std::fs::create_dir_all(work.join("a")).unwrap();
    std::fs::create_dir_all(work.join("b")).unwrap();
    let a = work.join("a/cube.stl");
    let b = work.join("b/cube.stl");
    std::fs::copy(fixture("cube_ascii.stl"), &a).unwrap();
    std::fs::copy(fixture("cube_ascii.stl"), &b).unwrap();
    let out = temp("batch_col_out");
    let o = run(&[
        "batch",
        "--json",
        "--out",
        &out.to_string_lossy(),
        &a.to_string_lossy(),
        &b.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["success"], false);
    assert_eq!(v["error"]["code"], "batch_error");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("collision"),
        "got: {}",
        v["error"]["message"]
    );
    // Nothing may have been rendered into the output directory.
    assert_eq!(std::fs::read_dir(&out).map(|d| d.count()).unwrap_or(0), 0);
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn batch_concurrent_with_shared_cache() {
    // Distinct-named inputs (realistic batch) sharing one cache: stresses
    // the threaded runner against the atomic cache (unique temp files,
    // serialized manifest). Outputs must all exist and the byte content
    // must match the single-shot render.
    let out = temp("batch_conc_out");
    let cache = temp("batch_conc_cache");
    let work = temp("batch_conc_work");
    std::fs::create_dir_all(&work).unwrap();
    let cube = std::fs::read(fixture("cube_ascii.stl")).unwrap();
    let ring = std::fs::read(fixture("ring_ascii.ply")).unwrap();
    let mut files: Vec<String> = Vec::new();
    for k in 0..12 {
        let cp = work.join(format!("cube_{k}.stl"));
        std::fs::write(&cp, &cube).unwrap();
        let rp = work.join(format!("ring_{k}.ply"));
        std::fs::write(&rp, &ring).unwrap();
        files.push(cp.to_string_lossy().into_owned());
        files.push(rp.to_string_lossy().into_owned());
    }
    let mut args: Vec<String> = vec![
        "batch".into(),
        "--json".into(),
        "--cache-dir".into(),
        cache.to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
    ];
    args.extend(files.iter().cloned());
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let o = run(&arg_refs);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["result"]["total"], 24);
    assert_eq!(v["result"]["ok"], 24);
    assert_eq!(v["result"]["failed"], 0);
    // All outputs present and byte-compared against reference singles.
    let single_cube = temp("single_cube.png");
    let single_ring = temp("single_ring.png");
    run(&[
        "render",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &single_cube.to_string_lossy(),
    ]);
    run(&[
        "render",
        &fixture("ring_ascii.ply").to_string_lossy(),
        "-o",
        &single_ring.to_string_lossy(),
    ]);
    let ref_cube = png_bytes(&single_cube);
    let ref_ring = png_bytes(&single_ring);
    for k in 0..12 {
        assert_eq!(&png_bytes(&out.join(format!("cube_{k}.png"))), &ref_cube);
        assert_eq!(&png_bytes(&out.join(format!("ring_{k}.png"))), &ref_ring);
    }
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::remove_file(&single_cube);
    let _ = std::fs::remove_file(&single_ring);
}

// ---------- point cloud / formats ----------

#[test]
fn renders_binary_stl_and_ply_point_cloud() {
    let out = temp("binary.png");
    let o = run(&[
        "render",
        &fixture("tri_binary.stl").to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(png_dimensions(&png_bytes(&out)), (256, 256));

    let out2 = temp("ring.png");
    let o2 = run(&[
        "render",
        &fixture("ring_ascii.ply").to_string_lossy(),
        "-o",
        &out2.to_string_lossy(),
    ]);
    assert_eq!(o2.status.code(), Some(0));
    // Point cloud render must have non-background pixels (not a no-op).
    let bytes = png_bytes(&out2);
    let mut reader = png::Decoder::new(&bytes[..]).read_info().unwrap();
    let sp = reader.info().color_type.samples();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    let bg = [32u8, 36, 43];
    let hits = buf
        .chunks_exact(sp)
        .filter(|px| px[0] != bg[0] && px[1] != bg[1] && px[2] != bg[2] && px[3] > 200)
        .count();
    assert!(
        hits > 50,
        "expected a visible point cloud, got {hits} object pixels"
    );
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&out2);
}

#[test]
fn cli_crlf_ascii_stl_renders_identically() {
    // Windows line endings must produce the exact same pixels as LF.
    let lf = std::fs::read(fixture("cube_ascii.stl")).unwrap();
    let crlf = temp("cube_crlf.stl");
    std::fs::write(&crlf, {
        let mut v: Vec<u8> = lf
            .iter()
            .flat_map(|&b| {
                if b == b'\n' {
                    vec![b'\r', b'\n']
                } else {
                    vec![b]
                }
            })
            .collect();
        v.shrink_to_fit();
        v
    })
    .unwrap();
    let out_lf = temp("out_lf.png");
    let out_crlf = temp("out_crlf.png");
    let o1 = run(&[
        "render",
        &fixture("cube_ascii.stl").to_string_lossy(),
        "-o",
        &out_lf.to_string_lossy(),
    ]);
    let o2 = run(&[
        "render",
        &crlf.to_string_lossy(),
        "-o",
        &out_crlf.to_string_lossy(),
    ]);
    assert_eq!(o1.status.code(), Some(0));
    assert_eq!(o2.status.code(), Some(0));
    assert_eq!(
        png_bytes(&out_lf),
        png_bytes(&out_crlf),
        "CRLF ASCII STL must render byte-identically to LF"
    );
    let _ = std::fs::remove_file(&crlf);
    let _ = std::fs::remove_file(&out_lf);
    let _ = std::fs::remove_file(&out_crlf);
}

#[test]
fn cli_renders_binary_ply_with_uchar_colors() {
    // End-to-end: binary little-endian PLY with per-vertex uchar colors is
    // detected, parsed, and rendered (colors make the triangle visible).
    let ply = temp("bicol.ply");
    let mut data = Vec::new();
    data.extend_from_slice(
        b"ply\nformat binary_little_endian 1.0\nelement vertex 3\n\
          property float x\nproperty float y\nproperty float z\n\
          property uchar red\nproperty uchar green\nproperty uchar blue\n\
          element face 1\nproperty list uchar int vertex_indices\nend_header\n",
    );
    let verts = [
        ([0.0f32, 0.0, 0.0], [255u8, 0, 0]),
        ([1.0, 0.0, 0.0], [0, 255, 0]),
        ([0.0, 1.0, 0.0], [0, 0, 255]),
    ];
    for (p, c) in verts {
        for v in p {
            data.extend_from_slice(&v.to_le_bytes());
        }
        data.extend_from_slice(&c);
    }
    data.push(3u8);
    for i in 0i32..3 {
        data.extend_from_slice(&i.to_le_bytes());
    }
    std::fs::write(&ply, &data).unwrap();
    let out = temp("bicol.png");
    let o = run(&[
        "render",
        &ply.to_string_lossy(),
        "-o",
        &out.to_string_lossy(),
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr(&o));
    let bytes = png_bytes(&out);
    assert_eq!(png_dimensions(&bytes), (256, 256));
    let mut reader = png::Decoder::new(&bytes[..]).read_info().unwrap();
    let sp = reader.info().color_type.samples();
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).unwrap();
    let bg = [32u8, 36, 43];
    let hits = buf
        .chunks_exact(sp)
        .filter(|px| px[0] != bg[0] && px[1] != bg[1] && px[2] != bg[2] && px[3] > 200)
        .count();
    assert!(hits > 50, "expected a visible triangle, got {hits} pixels");
    let _ = std::fs::remove_file(&ply);
    let _ = std::fs::remove_file(&out);
}
