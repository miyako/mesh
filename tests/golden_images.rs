//! Golden-image tests: render fixtures with fixed options and compare the
//! output PNG against a checked-in reference (`tests/golden/ref/`).
//!
//! - If a reference is missing, it is generated in place and the test
//!   PASSES with a loud note — review and commit the new file.
//! - If a reference exists, every channel of every pixel must match within
//!   `TOL` (1/255); otherwise the test fails with mismatch statistics.
//!
//! This pins rendering output (orientation, framing, background, shading,
//! supersampling) against accidental regressions. Regenerate deliberately
//! by deleting a reference file and re-running.

use std::path::{Path, PathBuf};
use std::process::Command;

const TOL: u8 = 1;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mesh-thumb")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn ref_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("ref")
}

struct Case {
    name: &'static str,
    input: &'static str,
    args: &'static [&'static str],
    /// Minimum fraction of non-background pixels expected (sanity check
    /// that the render is not an empty frame).
    min_object_fraction: f64,
}

const CASES: &[Case] = &[
    Case {
        name: "cube_iso_default",
        input: "cube_ascii.stl",
        args: &[],
        min_object_fraction: 0.2,
    },
    Case {
        name: "cube_top",
        input: "cube_ascii.stl",
        args: &["--view", "top", "--size", "128"],
        min_object_fraction: 0.3,
    },
    Case {
        name: "cube_front",
        input: "cube_ascii.stl",
        args: &["--view", "front", "--size", "128"],
        min_object_fraction: 0.3,
    },
    Case {
        name: "cube_side",
        input: "cube_ascii.stl",
        args: &["--view", "side", "--size", "128"],
        min_object_fraction: 0.3,
    },
    Case {
        name: "cube_transparent_bg",
        input: "cube_ascii.stl",
        args: &["--background", "transparent", "--size", "128"],
        min_object_fraction: 0.3,
    },
    Case {
        name: "cube_custom_bg",
        input: "cube_ascii.stl",
        args: &["--background", "#ff8040", "--size", "128"],
        min_object_fraction: 0.3,
    },
    Case {
        name: "cube_quality8",
        input: "cube_ascii.stl",
        args: &["--quality", "8", "--size", "64"],
        min_object_fraction: 0.2,
    },
    Case {
        name: "tri_binary_stl",
        input: "tri_binary.stl",
        args: &["--size", "128"],
        min_object_fraction: 0.05,
    },
    Case {
        name: "ring_ply_colors",
        input: "ring_ascii.ply",
        args: &["--size", "128"],
        min_object_fraction: 0.01,
    },
];

/// Run `render` with the case's options and return the raw PNG bytes.
fn render_png(c: &Case) -> Vec<u8> {
    let out =
        std::env::temp_dir().join(format!("mesh_golden_{}_{}.png", std::process::id(), c.name));
    let mut cmd = Command::new(bin());
    cmd.arg("render");
    for a in c.args {
        cmd.arg(a);
    }
    cmd.arg(fixture(c.input))
        .args(["-o", out.to_str().unwrap()]);
    let output = cmd.output().expect("spawn render");
    assert!(
        output.status.success(),
        "render of {} failed: {}",
        c.name,
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(&out).expect("read rendered PNG");
    let _ = std::fs::remove_file(&out);
    bytes
}

fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(png_bytes).read_info().expect("valid PNG");
    let info = reader.info().clone();
    let mut buf = vec![0u8; info.raw_bytes()];
    reader.next_frame(&mut buf).expect("decode frame");
    (info.width, info.height, buf)
}

fn px(buf: &[u8], sp: usize, w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) as usize) * sp;
    let mut p = [0u8; 4];
    for (k, slot) in p.iter_mut().enumerate() {
        *slot = *buf.get(i + k).unwrap_or(&0);
    }
    p
}

fn object_fraction(buf: &[u8], sp: usize, w: u32, h: u32, opaque_bg: bool) -> f64 {
    let bg = if opaque_bg {
        [32u8, 36, 43]
    } else {
        [0u8, 0, 0]
    };
    let mut hits = 0u64;
    for y in 0..h {
        for x in 0..w {
            let p = px(buf, sp, w, x, y);
            let on_bg = if opaque_bg {
                p[3] > 200
                    && u16::from(p[0].abs_diff(bg[0]))
                        + u16::from(p[1].abs_diff(bg[1]))
                        + u16::from(p[2].abs_diff(bg[2]))
                        <= 6
            } else {
                p[3] <= 200 // Transparent background: object pixels are opaque-ish.
            };
            if !on_bg {
                hits += 1;
            }
        }
    }
    hits as f64 / (w * h) as f64
}

fn has_opaque_bg(c: &Case) -> bool {
    !c.args
        .windows(2)
        .any(|w| w[0] == "--background" && w[1] == "transparent")
}

fn run_case(c: &Case) {
    let png = render_png(c);
    let (w, h, buf) = decode(&png);

    // Sanity: the frame must actually contain the subject.
    let frac = object_fraction(&buf, 4, w, h, has_opaque_bg(c));
    assert!(
        frac >= c.min_object_fraction,
        "{}: object pixel fraction {frac:.3} below sanity floor {}",
        c.name,
        c.min_object_fraction
    );

    let rpath = ref_dir().join(format!("{}.png", c.name));
    if !rpath.exists() {
        std::fs::create_dir_all(ref_dir()).unwrap();
        std::fs::write(&rpath, &png).unwrap();
        eprintln!(
            "GOLDEN: generated new reference {} — review it before committing (pixels {w}x{h}, object fraction {frac:.3}).",
            rpath.display()
        );
        return;
    }

    let (rw, rh, rbuf) = decode(&std::fs::read(&rpath).expect("read reference"));
    assert_eq!((rw, rh), (w, h), "{}: reference dimensions differ", c.name);
    let sp = 4;
    let mut max_delta = 0u8;
    let mut mismatched = 0u64;
    for i in 0..(w * h) as usize * sp {
        let d = buf[i].abs_diff(rbuf[i]);
        if d > max_delta {
            max_delta = d;
        }
        if d > TOL {
            mismatched += 1;
        }
    }
    assert!(
        mismatched == 0,
        "{}: {} channels deviate beyond |{}|, max delta {max_delta} (reference: {})",
        c.name,
        mismatched,
        TOL,
        rpath.display()
    );
}

#[test]
fn golden_renders_match_references() {
    for c in CASES {
        run_case(c);
    }
}
