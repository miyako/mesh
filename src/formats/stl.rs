//! STL parsing: binary and ASCII. Facet normals are ignored; normals are
//! recomputed by the geometry/normal pipeline (spec §9.3).

use crate::diagnostics::error::{Error, ErrorCode, Result};
use crate::diagnostics::limits;
use crate::formats::detect::FormatVariant;
use crate::geometry::types::{Mesh, ParseWarnings, Vec3};

/// Parse STL bytes. `variant` decides the parser; both parsers verify
/// the actual structure themselves.
pub fn parse(data: &[u8], variant: FormatVariant) -> Result<Mesh> {
    match variant {
        FormatVariant::StlBinary => parse_binary(data),
        FormatVariant::StlAscii => parse_ascii(data),
        v => Err(Error::new(
            ErrorCode::InvalidStl,
            format!("internal: wrong STL variant {v:?}"),
        )),
    }
}

fn read_f32_le(buf: &[u8], pos: usize) -> f32 {
    f32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
}

/// Parse a binary STL stream. Validates the header, the declared count
/// against the actual file length (checked arithmetic), and vertex
/// finiteness. Duplicated vertices are retained (documented choice,
/// spec §7.3).
pub fn parse_binary(data: &[u8]) -> Result<Mesh> {
    const HEADER: usize = 84;
    const TRI_STRIDE: usize = 50;

    if data.len() < HEADER {
        return Err(Error::new(
            ErrorCode::InvalidStl,
            "binary STL too short: missing 84-byte header",
        )
        .with_stage("stl_binary"));
    }
    let count_bytes = u32::from_le_bytes([data[80], data[81], data[82], data[83]]);
    let count = u64::from(count_bytes);
    let stride_total = 50u64
        .checked_mul(count)
        .ok_or_else(|| Error::new(ErrorCode::InvalidStl, "triangle count overflow"))?;
    let expected_len = 84u64
        .checked_add(stride_total)
        .ok_or_else(|| Error::new(ErrorCode::InvalidStl, "file length overflow"))?;

    if data.len() as u64 != expected_len {
        return Err(Error::new(
            ErrorCode::InvalidStl,
            format!(
                "binary STL length mismatch: file is {} bytes, header declares {count} triangles ({expected_len} bytes expected)",
                data.len()
            ),
        ));
    }
    let count_usize = limits::checked_usize("triangle count", count)?;
    if count_usize > limits::MAX_TRIANGLES {
        return Err(Error::resource_limit(format!(
            "binary STL declares {count} triangles, exceeding limit {}",
            limits::MAX_TRIANGLES
        )));
    }

    let mut mesh = Mesh::empty();
    mesh.positions.reserve(count_usize * 3);
    mesh.triangles.reserve(count_usize);

    let mut pos = HEADER;
    let mut degenerate = 0usize;
    let mut non_finite = 0usize;
    for _ in 0..count_usize {
        // Attributes occupy the final 2 bytes (ignored, spec §7.1);
        // supplied normal occupies bytes 0..12 (ignored).
        let a = Vec3::new(
            read_f32_le(data, pos + 12),
            read_f32_le(data, pos + 16),
            read_f32_le(data, pos + 20),
        );
        let b = Vec3::new(
            read_f32_le(data, pos + 24),
            read_f32_le(data, pos + 28),
            read_f32_le(data, pos + 32),
        );
        let c = Vec3::new(
            read_f32_le(data, pos + 36),
            read_f32_le(data, pos + 40),
            read_f32_le(data, pos + 44),
        );
        pos += TRI_STRIDE;

        if !(a.is_finite() && b.is_finite() && c.is_finite()) {
            non_finite += 1;
            continue;
        }
        let base = mesh.positions.len() as u32;
        let ab = b - a;
        let ac = c - a;
        if ab.cross(ac).length_squared() <= 0.0 {
            // Degenerate: drop the triangle but keep storage contiguous.
            degenerate += 1;
            continue;
        }
        mesh.positions.push(a);
        mesh.positions.push(b);
        mesh.positions.push(c);
        mesh.triangles.push([base, base + 1, base + 2]);
    }

    finalize_mesh(
        mesh,
        ParseWarnings {
            degenerate_triangles: degenerate,
            non_finite_vertices: non_finite,
        },
        "stl_binary",
    )
}

/// Parse ASCII STL: a conservative structural parser over the
/// `facet`/`outer loop`/`vertex`/`endloop`/`endfacet` grammar. Tolerant
/// of whitespace and keyword case; strict about exactly three vertices
/// per facet. Normalization replaces two-token keywords so the state
/// machine sees single tokens.
pub fn parse_ascii(data: &[u8]) -> Result<Mesh> {
    let text = std::str::from_utf8(data).map_err(|_| {
        Error::new(
            ErrorCode::InvalidStl,
            "ASCII STL is not valid ASCII/UTF-8 text",
        )
        .with_stage("stl_ascii")
    })?;

    // Lowercasing never alters float literals (digits, '+', '-', '.', 'e'),
    // so keyword matching and coordinate parsing can share the stream.
    let mut normalized = text.to_ascii_lowercase();
    // Merge two-token keywords appearing on the same line.
    normalized = normalized.replace("outer loop", "outer_loop");
    normalized = normalized.replace("end loop", "end_loop");
    normalized = normalized.replace("end facet", "end_facet");

    let mut degenerate = 0usize;
    let mut saw_any_structure = false;
    parse_ascii_tokens(&normalized, &mut degenerate, &mut saw_any_structure)
}

fn parse_ascii_tokens(
    normalized: &str,
    degenerate: &mut usize,
    saw_any_structure: &mut bool,
) -> Result<Mesh> {
    let mut mesh = Mesh::empty();
    let mut tokens = normalized.split_whitespace().enumerate();
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Scan,
        FacetNormal,
        FacetNormalVals(usize),
        OuterLoop,
        Vertex,
        EndLoop,
        EndFacet,
    }
    let mut st = State::Scan;
    let mut facet_verts: Vec<Vec3> = Vec::with_capacity(3);
    let mut tok_no = 0usize;

    macro_rules! err_at {
        ($msg:expr) => {
            return Err(Error::new(
                ErrorCode::InvalidStl,
                format!("{} (near token #{tok_no})", $msg),
            )
            .with_stage("stl_ascii"))
        };
    }

    while let Some((_i, tok)) = tokens.next() {
        tok_no += 1;
        match st {
            State::Scan => {
                if tok == "facet" {
                    *saw_any_structure = true;
                    facet_verts.clear();
                    st = State::FacetNormal;
                }
            }
            State::FacetNormal => {
                if tok == "normal" {
                    st = State::FacetNormalVals(3);
                } else if tok == "outer_loop" {
                    st = State::OuterLoop;
                } else {
                    err_at!("expected 'normal' or 'outer loop' after 'facet'");
                }
            }
            State::FacetNormalVals(left) => {
                if tok.parse::<f32>().is_ok() {
                    if left == 1 {
                        st = State::FacetNormal;
                    } else {
                        st = State::FacetNormalVals(left - 1);
                    }
                } else {
                    err_at!("expected facet normal value");
                }
            }
            State::OuterLoop => {
                if tok == "vertex" {
                    st = State::Vertex;
                } else {
                    err_at!("expected 'vertex' after 'outer loop'");
                }
            }
            State::Vertex => {
                // `tok` is the first coordinate (the "vertex" keyword was
                // consumed to reach this state); read only the remaining
                // two from the stream.
                let mut v = [0.0f32; 3];
                v[0] = tok
                    .parse::<f32>()
                    .map_err(|_| stl_ascii_error(&format!("invalid vertex coordinate '{tok}'")))?;
                if !v[0].is_finite() {
                    return Err(stl_ascii_error(&format!(
                        "non-finite vertex coordinate '{tok}'"
                    )));
                }
                for slot in v.iter_mut().skip(1) {
                    match tokens.next() {
                        Some((_, t)) => {
                            let val: f32 = t.parse().map_err(|_| {
                                stl_ascii_error(&format!("invalid vertex coordinate '{t}'"))
                            })?;
                            if !val.is_finite() {
                                return Err(stl_ascii_error(&format!(
                                    "non-finite vertex coordinate '{t}'"
                                )));
                            }
                            *slot = val;
                        }
                        None => err_at!("truncated vertex coordinates"),
                    }
                }
                facet_verts.push(Vec3::new(v[0], v[1], v[2]));
                if facet_verts.len() == 3 {
                    st = State::EndLoop;
                } else {
                    st = State::OuterLoop; // wait for next 'vertex'
                }
            }
            State::EndLoop => {
                if tok == "endloop" || tok == "end_loop" {
                    st = State::EndFacet;
                } else {
                    err_at!("expected 'endloop' after three vertices");
                }
            }
            State::EndFacet => {
                if tok == "endfacet" || tok == "end_facet" {
                    // commit facet
                    let a = facet_verts[0];
                    let b = facet_verts[1];
                    let c = facet_verts[2];
                    let base = mesh.positions.len() as u32;
                    let ab = b - a;
                    let ac = c - a;
                    if ab.cross(ac).length_squared() <= 0.0 {
                        *degenerate += 1;
                    } else {
                        mesh.positions.push(a);
                        mesh.positions.push(b);
                        mesh.positions.push(c);
                        mesh.triangles.push([base, base + 1, base + 2]);
                    }
                    facet_verts.clear();
                    st = State::Scan;
                } else if tok == "endsolid" {
                    return finalize_mesh(
                        mesh,
                        ParseWarnings {
                            degenerate_triangles: *degenerate,
                            non_finite_vertices: 0,
                        },
                        "stl_ascii",
                    );
                } else {
                    err_at!("expected 'endfacet' or 'endsolid'");
                }
            }
        }
    }

    if !*saw_any_structure {
        return Err(stl_ascii_error("no 'facet' structure found"));
    }
    if matches!(st, State::OuterLoop | State::FacetNormal) {
        return Err(stl_ascii_error("unterminated facet"));
    }
    finalize_mesh(
        mesh,
        ParseWarnings {
            degenerate_triangles: *degenerate,
            non_finite_vertices: 0,
        },
        "stl_ascii",
    )
}

fn stl_ascii_error(msg: &str) -> Error {
    Error::new(ErrorCode::InvalidStl, msg.to_string()).with_stage("stl_ascii")
}

fn finalize_mesh(mesh: Mesh, warnings: ParseWarnings, stage: &'static str) -> Result<Mesh> {
    // Per spec §5.4, empty geometry is an explicit error.
    if mesh.positions.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidStl,
            "STL contains no usable triangles (all facets degenerate, missing, or non-finite)",
        )
        .with_stage(stage));
    }
    let mut mesh = mesh.finalize().map_err(|e| e.with_stage(stage))?;
    mesh.warnings = Some(warnings);
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_binary(header: &[u8; 80], count: u32, tris: &[[f32; 9]]) -> Vec<u8> {
        let mut out = header.to_vec();
        out.extend_from_slice(&count.to_le_bytes());
        for t in tris {
            for _ in 0..3 {
                out.extend_from_slice(&0.0f32.to_le_bytes()); // normal (ignored)
            }
            for v in t {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&[0u8, 0u8]); // attribute (ignored)
        }
        out
    }

    #[test]
    fn binary_valid_triangle() {
        let tris = [[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]];
        let data = pack_binary(&[0u8; 80], 1, &tris);
        let m = parse_binary(&data).unwrap();
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.triangles.len(), 1);
        assert_eq!(m.warnings.as_ref().unwrap().degenerate_triangles, 0);
    }

    #[test]
    fn binary_length_mismatch_rejected() {
        let mut data = pack_binary(&[0u8; 80], 2, &[[0.0; 9]]);
        data.pop();
        assert!(parse_binary(&data).is_err());
    }

    #[test]
    fn binary_truncated_rejected() {
        let data = pack_binary(&[0u8; 80], 2, &[[0.0; 9]]);
        let truncated = &data[..data.len() - 10];
        assert!(parse_binary(truncated).is_err());
    }

    #[test]
    fn binary_all_degenerate_errors() {
        let tris = [[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]];
        let data = pack_binary(&[0u8; 80], 1, &tris);
        assert!(parse_binary(&data).is_err());
    }

    #[test]
    fn binary_mixed_degenerate_dropped_with_warning() {
        let tris = [
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ];
        let data = pack_binary(&[0u8; 80], 2, &tris);
        let m = parse_binary(&data).unwrap();
        assert_eq!(m.triangles.len(), 1);
        assert_eq!(m.warnings.as_ref().unwrap().degenerate_triangles, 1);
    }

    #[test]
    fn binary_non_finite_skipped() {
        let tris = [[f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]];
        let data = pack_binary(&[0u8; 80], 1, &tris);
        assert!(parse_binary(&data).is_err());
    }

    #[test]
    fn ascii_valid() {
        let text = "solid cube\n\
                    facet normal 0 0 1\nouter loop\n\
                    vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n\
                    endloop\nendfacet\n\
                    endsolid cube\n";
        let m = parse_ascii(text.as_bytes()).unwrap();
        assert_eq!(m.triangles.len(), 1);
    }

    #[test]
    fn ascii_uppercase_and_extra_ws() {
        let text = "SOLID x\n  FACET NORMAL 0 0 1\n  OUTER LOOP\n\
                    VERTEX 0 0 0    VERTEX 1 0 0    VERTEX 0 1 0\n\
                    ENDLOOP\nENDFACET\nENDSOLID x\n";
        let m = parse_ascii(text.as_bytes()).unwrap();
        assert_eq!(m.triangles.len(), 1);
    }

    #[test]
    fn ascii_end_facet_two_tokens() {
        let text = "solid x\nfacet normal 0 0 1\nouter loop\n\
                    vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\n\
                    end loop\nend facet\nendsolid x\n";
        let m = parse_ascii(text.as_bytes()).unwrap();
        assert_eq!(m.triangles.len(), 1);
    }

    #[test]
    fn ascii_multiple_facets() {
        let text = "solid x\nfacet normal 0 0 1\nouter loop\n\
                    vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\n\
                    facet normal 0 0 1\nouter loop\n\
                    vertex 0 0 0\nvertex 0 1 0\nvertex 0 1 1\nendloop\nendfacet\nendsolid x\n";
        let m = parse_ascii(text.as_bytes()).unwrap();
        assert_eq!(m.triangles.len(), 2);
    }

    #[test]
    fn ascii_all_degenerate_errors() {
        let text = "solid x\nfacet normal 0 0 0\nouter loop\n\
                    vertex 0 0 0\nvertex 0 0 0\nvertex 0 0 0\n\
                    endloop\nendfacet\nendsolid x\n";
        assert!(parse_ascii(text.as_bytes()).is_err());
    }

    #[test]
    fn ascii_short_facet_rejected() {
        let text = "solid x\nfacet normal 0 0 1\nouter loop\n\
                    vertex 0 0 0\nvertex 1 0 0\nendloop\nendfacet\nendsolid x\n";
        assert!(parse_ascii(text.as_bytes()).is_err());
    }

    #[test]
    fn ascii_no_facet_rejected() {
        let text = "solid x\nendsolid x\n";
        assert!(parse_ascii(text.as_bytes()).is_err());
    }

    #[test]
    fn ascii_non_finite_rejected() {
        let text = "solid x\nfacet normal 0 0 1\nouter loop\n\
                    vertex nan 0 0\nvertex 1 0 0\nvertex 0 1 0\n\
                    endloop\nendfacet\nendsolid x\n";
        assert!(parse_ascii(text.as_bytes()).is_err());
    }

    #[test]
    fn detection_roundtrip_binary() {
        let tris = [[0.0f32; 9]];
        let mut header = [0u8; 80];
        header[..5].copy_from_slice(b"solid");
        let data = pack_binary(&header, 1, &tris);
        // All-degenerate facet -> no usable geometry is an explicit error
        // (spec §5.4: empty geometry must fail loudly).
        assert!(parse(&data, FormatVariant::StlBinary).is_err());
    }

    #[test]
    fn binary_valid_roundtrip() {
        // A real triangle: parse succeeds with 1 triangle and 3 positions.
        let tri = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let data = pack_binary(&[0u8; 80], 1, &[tri]);
        let m = parse(&data, FormatVariant::StlBinary).unwrap();
        assert_eq!(m.triangles.len(), 1);
        assert_eq!(m.positions.len(), 3);
    }
}
