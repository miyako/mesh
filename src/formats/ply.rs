//! PLY parsing: ASCII, binary little-endian, and binary big-endian
//! (header-declared format is authoritative). Supports vertex
//! coordinates, optional vertex normals, optional vertex colors
//! (RGB/RGBA), and `face` lists of 3+ vertices (fan-triangulated).
//! Point clouds are files with a `vertex` element and no `face` element
//! (spec §8.2/§10.5/§10.6).

use crate::diagnostics::error::{Error, ErrorCode, Result};
use crate::diagnostics::limits;
use crate::formats::detect::FormatVariant;
use crate::geometry::bounds::Bounds3;
use crate::geometry::types::{Color, Geometry, Mesh, ParseWarnings, PointCloud, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl ScalarType {
    fn parse(s: &str) -> Option<ScalarType> {
        Some(match s {
            "char" | "int8" => ScalarType::I8,
            "uchar" | "uint8" => ScalarType::U8,
            "short" | "int16" => ScalarType::I16,
            "ushort" | "uint16" => ScalarType::U16,
            "int" | "int32" => ScalarType::I32,
            "uint" | "uint32" => ScalarType::U32,
            "float" | "float32" => ScalarType::F32,
            "double" | "float64" => ScalarType::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            ScalarType::I8 | ScalarType::U8 => 1,
            ScalarType::I16 | ScalarType::U16 => 2,
            ScalarType::I32 | ScalarType::U32 | ScalarType::F32 => 4,
            ScalarType::F64 => 8,
        }
    }

    /// Float types signal 0..1 color channels; integers signal 0..255
    /// (spec §10.5).
    fn is_float(self) -> bool {
        matches!(self, ScalarType::F32 | ScalarType::F64)
    }
}

#[derive(Debug, Clone)]
enum Property {
    Scalar {
        ty: ScalarType,
        name: String,
    },
    List {
        count_ty: ScalarType,
        item_ty: ScalarType,
        name: String,
    },
}

impl Property {
    fn name(&self) -> &str {
        match self {
            Property::Scalar { name, .. } | Property::List { name, .. } => name,
        }
    }
    fn scalar_ty(&self) -> ScalarType {
        match self {
            Property::Scalar { ty, .. } => *ty,
            Property::List { .. } => ScalarType::F32,
        }
    }
}

#[derive(Debug, Clone)]
struct Element {
    name: String,
    count: u64,
    properties: Vec<Property>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataFormat {
    Ascii,
    BinaryLe,
    BinaryBe,
}

struct Header {
    format: DataFormat,
    elements: Vec<Element>,
    vertex: usize,
    face: Option<usize>,
}

/// Parse the header text up to and including `end_header`, returning the
/// byte offset where the data begins.
fn parse_header(data: &[u8]) -> Result<(Header, usize)> {
    let header_end = find_header_end(data).ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidPlyHeader,
            format!(
                "no 'end_header' line found within {} bytes",
                limits::MAX_PLY_HEADER_BYTES
            ),
        )
    })?;
    let text = match std::str::from_utf8(&data[..header_end]) {
        Ok(t) => t,
        Err(_) => {
            return Err(Error::new(
                ErrorCode::InvalidPlyHeader,
                "header is not valid ASCII text",
            ))
        }
    };

    let mut format: Option<DataFormat> = None;
    let mut elements: Vec<Element> = Vec::new();
    let mut cur: Option<Element> = None;
    let mut line_no = 0usize;

    macro_rules! hdr_err {
        ($($arg:tt)*) => {
            return Err(Error::new(
                ErrorCode::InvalidPlyHeader,
                format!("{} (header line {line_no})", format!($($arg)*)),
            ))
        };
    }

    let mut saw_ply = false;
    for raw in text.lines() {
        line_no += 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let kw = parts.next().unwrap_or("");
        match kw {
            "ply" => saw_ply = true,
            "comment" | "obj_info" => {}
            "format" => {
                let fmt = parts.next().unwrap_or("");
                let ver = parts.next().unwrap_or("");
                if ver != "1.0" {
                    hdr_err!("unsupported PLY version '{ver}' (only 1.0)");
                }
                format = Some(match fmt {
                    "ascii" => DataFormat::Ascii,
                    "binary_little_endian" => DataFormat::BinaryLe,
                    "binary_big_endian" => DataFormat::BinaryBe,
                    other => hdr_err!("unknown format '{other}'"),
                });
            }
            "element" => {
                if let Some(e) = cur.take() {
                    elements.push(e);
                }
                let name = parts.next().unwrap_or("");
                let count: u64 = match parts.next().and_then(|s| s.parse().ok()) {
                    Some(c) => c,
                    None => hdr_err!("invalid element count"),
                };
                cur = Some(Element {
                    name: name.to_string(),
                    count,
                    properties: Vec::new(),
                });
            }
            "property" => {
                let elem = cur.as_mut().ok_or_else(|| {
                    Error::new(ErrorCode::InvalidPlyHeader, "property before any element")
                })?;
                let first = parts.next().unwrap_or("");
                if first == "list" {
                    let count_ty = parts.next().and_then(ScalarType::parse).ok_or_else(|| {
                        Error::new(ErrorCode::InvalidPlyHeader, "invalid list count type")
                    })?;
                    let item_ty = parts.next().and_then(ScalarType::parse).ok_or_else(|| {
                        Error::new(ErrorCode::InvalidPlyHeader, "invalid list item type")
                    })?;
                    let name = parts.next().unwrap_or("");
                    elem.properties.push(Property::List {
                        count_ty,
                        item_ty,
                        name: name.to_string(),
                    });
                } else {
                    let ty = ScalarType::parse(first).ok_or_else(|| {
                        Error::new(ErrorCode::InvalidPlyHeader, "unknown property type")
                    })?;
                    let name = parts.next().unwrap_or("");
                    elem.properties.push(Property::Scalar {
                        ty,
                        name: name.to_string(),
                    });
                }
            }
            "end_header" => {
                if !saw_ply {
                    hdr_err!("missing 'ply' magic line");
                }
                break;
            }
            other => hdr_err!("unexpected header token '{other}'"),
        }
    }
    if let Some(e) = cur.take() {
        elements.push(e);
    }
    if !saw_ply {
        return Err(Error::new(
            ErrorCode::InvalidPlyHeader,
            "missing 'ply' magic",
        ));
    }
    let format = match format {
        Some(f) => f,
        None => {
            return Err(Error::new(
                ErrorCode::InvalidPlyHeader,
                "missing 'format' declaration",
            ))
        }
    };
    let vertex = elements
        .iter()
        .position(|e| e.name == "vertex")
        .ok_or_else(|| Error::new(ErrorCode::InvalidPlyHeader, "no 'vertex' element"))?;
    let face = elements.iter().position(|e| e.name == "face");

    // Pre-validate declared counts within limits.
    for e in &elements {
        let (limit, noun): (u64, &str) = match e.name.as_str() {
            // Pre-validate declared counts so a huge element fails before
            // any data is read.
            "vertex" => (limits::MAX_VERTICES as u64, "vertices"),
            "face" => (limits::MAX_TRIANGLES as u64, "faces"),
            _ => continue,
        };
        if e.count > limit {
            return Err(Error::resource_limit(format!(
                "{} element declares {} {noun} (limit {limit})",
                e.name, e.count
            )));
        }
    }

    Ok((
        Header {
            format,
            elements,
            vertex,
            face,
        },
        header_end,
    ))
}

/// Locate the `end_header` line inside the bounded header window.
///
/// Byte-oriented: the window may contain binary body bytes after the
/// header, which are not valid UTF-8, so a full decode would fail.
fn find_header_end(data: &[u8]) -> Option<usize> {
    let window = data.len().min(limits::MAX_PLY_HEADER_BYTES);
    let hay = &data[..window];
    let mut line_start = 0usize;
    for (i, &b) in hay.iter().enumerate() {
        if b == b'\n' {
            let line = trim_ascii_end(&hay[line_start..i]);
            if line == b"end_header" {
                return Some(i + 1);
            }
            line_start = i + 1;
        }
    }
    // Last line without a trailing newline.
    let last = trim_ascii_end(&hay[line_start..]);
    if line_start < hay.len() && last == b"end_header" {
        return Some(hay.len());
    }
    None
}

/// Trim trailing ASCII whitespace from a byte slice (like `str::trim_end`
/// but for arbitrary bytes, without requiring valid UTF-8).
fn trim_ascii_end(mut b: &[u8]) -> &[u8] {
    while matches!(b.last(), Some(c) if c.is_ascii_whitespace()) {
        b = &b[..b.len() - 1];
    }
    b
}

/// Row reader abstraction: yields one element row as per-property
/// `Vec<f64>` values; list properties expand to [count, item, ...].
trait RowReader {
    fn next_row(&mut self, props: &[Property]) -> Result<Vec<Vec<f64>>>;
}

struct AsciiReader<'a> {
    rest: &'a str,
    row: usize,
}

impl<'a> AsciiReader<'a> {
    fn next_token(&mut self) -> Result<&'a str> {
        let t = self.rest.trim_start();
        if t.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidPlyData,
                format!("unexpected end of data (row {})", self.row),
            ));
        }
        let end = t.find(char::is_whitespace).unwrap_or(t.len());
        let tok = &t[..end];
        self.rest = &t[end..];
        Ok(tok)
    }
}

impl<'a> RowReader for AsciiReader<'a> {
    fn next_row(&mut self, props: &[Property]) -> Result<Vec<Vec<f64>>> {
        self.row += 1;
        let mut out = Vec::with_capacity(props.len());
        for prop in props {
            match prop {
                Property::Scalar { .. } => {
                    let tok = self.next_token()?;
                    let v: f64 = tok.parse().map_err(|_| {
                        Error::new(
                            ErrorCode::InvalidPlyData,
                            format!("invalid number '{tok}' in row {}", self.row),
                        )
                    })?;
                    out.push(vec![v]);
                }
                Property::List {
                    count_ty, item_ty, ..
                } => {
                    let _ = (count_ty, item_ty);
                    let count_tok = self.next_token()?;
                    let count: i64 = count_tok.parse().map_err(|_| {
                        Error::new(
                            ErrorCode::InvalidPlyData,
                            format!("invalid list count '{count_tok}' in row {}", self.row),
                        )
                    })?;
                    if count < 0 {
                        return Err(Error::new(
                            ErrorCode::InvalidPlyData,
                            format!("negative list count in row {}", self.row),
                        ));
                    }
                    if count as u64 > limits::MAX_FACE_LIST_LEN as u64 {
                        return Err(Error::resource_limit(format!(
                            "list count {count} exceeds limit {}",
                            limits::MAX_FACE_LIST_LEN
                        )));
                    }
                    let mut row = Vec::with_capacity(count as usize + 1);
                    row.push(count as f64);
                    for _ in 0..count {
                        let tok = self.next_token()?;
                        let v: f64 = tok.parse().map_err(|_| {
                            Error::new(
                                ErrorCode::InvalidPlyData,
                                format!("invalid list item '{tok}' in row {}", self.row),
                            )
                        })?;
                        row.push(v);
                    }
                    out.push(row);
                }
            }
        }
        Ok(out)
    }
}

struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
    big_endian: bool,
    row: usize,
}

impl<'a> BinaryReader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.data.len() {
            return Err(Error::new(
                ErrorCode::InvalidPlyData,
                format!(
                    "truncated binary data (need {n} bytes at offset {}, row {})",
                    self.pos, self.row
                ),
            ));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn read_scalar(&mut self, ty: ScalarType) -> Result<f64> {
        let bytes = self.take(ty.size())?;
        let mut arr = [0u8; 8];
        arr[..bytes.len()].copy_from_slice(bytes);
        let v = match ty {
            ScalarType::I8 => f64::from(arr[0] as i8),
            ScalarType::U8 => f64::from(arr[0]),
            ScalarType::I16 => {
                let raw = if self.big_endian {
                    u16::from_be_bytes([arr[0], arr[1]])
                } else {
                    u16::from_le_bytes([arr[0], arr[1]])
                };
                f64::from(raw as i16)
            }
            ScalarType::U16 => {
                let raw = if self.big_endian {
                    u16::from_be_bytes([arr[0], arr[1]])
                } else {
                    u16::from_le_bytes([arr[0], arr[1]])
                };
                f64::from(raw)
            }
            ScalarType::I32 => {
                let raw = u32::from_le_bytes([arr[0], arr[1], arr[2], arr[3]]);
                let raw = if self.big_endian {
                    raw.swap_bytes()
                } else {
                    raw
                };
                f64::from(raw as i32)
            }
            ScalarType::U32 => {
                let raw = u32::from_le_bytes([arr[0], arr[1], arr[2], arr[3]]);
                let raw = if self.big_endian {
                    raw.swap_bytes()
                } else {
                    raw
                };
                f64::from(raw)
            }
            ScalarType::F32 => {
                let raw = u32::from_le_bytes([arr[0], arr[1], arr[2], arr[3]]);
                let raw = if self.big_endian {
                    raw.swap_bytes()
                } else {
                    raw
                };
                f64::from(f32::from_bits(raw))
            }
            ScalarType::F64 => {
                let raw = u64::from_le_bytes(arr);
                let raw = if self.big_endian {
                    raw.swap_bytes()
                } else {
                    raw
                };
                f64::from_bits(raw)
            }
        };
        Ok(v)
    }
}

impl<'a> RowReader for BinaryReader<'a> {
    fn next_row(&mut self, props: &[Property]) -> Result<Vec<Vec<f64>>> {
        self.row += 1;
        let mut out = Vec::with_capacity(props.len());
        for prop in props {
            match prop {
                Property::Scalar { ty, .. } => {
                    out.push(vec![self.read_scalar(*ty)?]);
                }
                Property::List {
                    count_ty, item_ty, ..
                } => {
                    let count = self.read_scalar(*count_ty)? as i64;
                    if count < 0 {
                        return Err(Error::new(
                            ErrorCode::InvalidPlyData,
                            format!("negative list count in row {}", self.row),
                        ));
                    }
                    if count as u64 > limits::MAX_FACE_LIST_LEN as u64 {
                        return Err(Error::resource_limit(format!(
                            "list count {count} exceeds limit {}",
                            limits::MAX_FACE_LIST_LEN
                        )));
                    }
                    let mut row = Vec::with_capacity(count as usize + 1);
                    row.push(count as f64);
                    for _ in 0..count {
                        row.push(self.read_scalar(*item_ty)?);
                    }
                    out.push(row);
                }
            }
        }
        Ok(out)
    }
}

/// Layout: which vertex properties are present.
struct VertexLayout {
    x: usize,
    y: usize,
    z: usize,
    nx: Option<(usize, ScalarType)>,
    ny: Option<(usize, ScalarType)>,
    nz: Option<(usize, ScalarType)>,
    r: Option<(usize, ScalarType)>,
    g: Option<(usize, ScalarType)>,
    b: Option<(usize, ScalarType)>,
    a: Option<(usize, ScalarType)>,
}

impl VertexLayout {
    fn build(props: &[Property]) -> Result<VertexLayout> {
        let find = |name: &str| -> Option<usize> {
            props
                .iter()
                .position(|p| p.name() == name && matches!(p, Property::Scalar { .. }))
        };
        let x = find("x")
            .ok_or_else(|| Error::new(ErrorCode::InvalidPlyHeader, "vertex element missing 'x'"))?;
        let y = find("y")
            .ok_or_else(|| Error::new(ErrorCode::InvalidPlyHeader, "vertex element missing 'y'"))?;
        let z = find("z")
            .ok_or_else(|| Error::new(ErrorCode::InvalidPlyHeader, "vertex element missing 'z'"))?;
        // x/y/z must be scalar (List would be a malformed file).
        for &i in &[x, y, z] {
            if matches!(props[i], Property::List { .. }) {
                return Err(Error::new(
                    ErrorCode::InvalidPlyHeader,
                    "vertex coordinate property must be scalar",
                ));
            }
        }
        let pick = |names: &[&str]| -> Option<(usize, ScalarType)> {
            for n in names {
                if let Some(i) = find(n) {
                    return Some((i, props[i].scalar_ty()));
                }
            }
            None
        };
        Ok(VertexLayout {
            x,
            y,
            z,
            nx: pick(&["nx"]),
            ny: pick(&["ny"]),
            nz: pick(&["nz"]),
            r: pick(&["red", "r"]),
            g: pick(&["green", "g"]),
            b: pick(&["blue", "b"]),
            a: pick(&["alpha", "a"]),
        })
    }

    fn has_normals(&self) -> bool {
        self.nx.is_some() && self.ny.is_some() && self.nz.is_some()
    }

    fn has_colors(&self) -> bool {
        self.r.is_some() && self.g.is_some() && self.b.is_some()
    }
}

/// Color channel conversion: floats are 0..1, integers are 0..255
/// (spec §10.5). PLY color values are sRGB-encoded; the render pipeline
/// is linear, so they are linearized here (full red/white are unaffected,
/// mid-tones are reproduced correctly after the final sRGB re-encode).
fn color_channel(ty: ScalarType, value: f64) -> f32 {
    if ty.is_float() {
        crate::render::image::srgb_to_linear(value.clamp(0.0, 1.0) as f32)
    } else {
        crate::render::image::srgb_to_linear((value.clamp(0.0, 255.0) / 255.0) as f32)
    }
}

/// Safe u32 index conversion from a decimal value.
fn to_index(v: f64, row: usize) -> Result<u32> {
    if !v.is_finite() || v < 0.0 || v > f64::from(u32::MAX) {
        return Err(Error::new(
            ErrorCode::InvalidPlyData,
            format!("invalid vertex index {v} in row {row}"),
        ));
    }
    if v.fract() != 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidPlyData,
            format!("non-integer vertex index {v} in row {row}"),
        ));
    }
    Ok(v as u32)
}

/// Common data-body parser used by all three formats.
fn parse_body<R: RowReader>(header: &Header, reader: &mut R) -> Result<(Geometry, ParseWarnings)> {
    let v_elem = &header.elements[header.vertex];
    let layout = VertexLayout::build(&v_elem.properties)?;
    let v_count = limits::checked_usize("vertex count", v_elem.count)?;
    let mut positions = Vec::with_capacity(v_count);
    let mut normals: Vec<Vec3> = Vec::new();
    let mut colors: Vec<Color> = Vec::new();

    for _ in 0..v_count {
        let r = reader.next_row(&v_elem.properties)?;
        let p = Vec3::new(
            r[layout.x][0] as f32,
            r[layout.y][0] as f32,
            r[layout.z][0] as f32,
        );
        if !p.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidPlyData,
                format!("non-finite vertex coordinate at vertex {}", positions.len()),
            ));
        }
        positions.push(p);
        if layout.has_normals() {
            let n = Vec3::new(
                r[layout.nx.unwrap().0][0] as f32,
                r[layout.ny.unwrap().0][0] as f32,
                r[layout.nz.unwrap().0][0] as f32,
            );
            normals.push(n);
        }
        if layout.has_colors() {
            let (ri, rty) = layout.r.unwrap();
            let (gi, gty) = layout.g.unwrap();
            let (bi, bty) = layout.b.unwrap();
            let alpha = match layout.a {
                Some((i, t)) => color_channel(t, r[i][0]).max(0.0),
                None => 1.0,
            };
            colors.push(Color::new(
                color_channel(rty, r[ri][0]),
                color_channel(gty, r[gi][0]),
                color_channel(bty, r[bi][0]),
                alpha,
            ));
        }
    }

    let mut degenerate = 0usize;
    let mut triangles: Vec<[u32; 3]> = Vec::new();

    if let Some(f_idx) = header.face {
        let f_elem = &header.elements[f_idx];
        let list_idx = f_elem
            .properties
            .iter()
            .position(|p| matches!(p, Property::List { .. }))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::InvalidPlyHeader,
                    "face element has no list property",
                )
            })?;
        let count = limits::checked_usize("face count", f_elem.count)?;
        let vcount = u32::try_from(positions.len())
            .map_err(|_| Error::resource_limit("too many vertices for u32 indices"))?;
        for i in 0..count {
            let r = reader.next_row(&f_elem.properties)?;
            let items = &r[list_idx][1..];
            let face: Vec<u32> = items
                .iter()
                .enumerate()
                .map(|(k, &v)| {
                    to_index(v, i).map_err(|e| e.contextualize("ply", format!("face {i} item {k}")))
                })
                .collect::<Result<_>>()?;
            let tris = match crate::geometry::triangulate::triangulate_face(&face, Some(vcount)) {
                Ok(t) => t,
                Err(_) => {
                    degenerate += 1;
                    continue; // malformed face: skip, count as degenerate
                }
            };
            if triangles.len() + tris.len() > limits::MAX_TRIANGLES {
                return Err(Error::resource_limit(format!(
                    "triangulated triangle count exceeds limit {}",
                    limits::MAX_TRIANGLES
                )));
            }
            for t in tris {
                let a = positions[t[0] as usize];
                let b = positions[t[1] as usize];
                let c = positions[t[2] as usize];
                if (b - a).cross(c - a).length_squared() <= 0.0 {
                    degenerate += 1;
                    continue;
                }
                triangles.push(t);
            }
        }
    }

    let has_faces = header.face.is_some();
    let geometry = if has_faces {
        // A face element that yielded no usable triangles is an error
        // (same policy as STL, spec §5.4), not a point cloud.
        if triangles.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidPlyData,
                "PLY face element produced no usable triangles",
            ));
        }
        Geometry::Mesh(Mesh {
            positions,
            triangles,
            normals: if layout.has_normals() {
                Some(normals)
            } else {
                None
            },
            colors: if layout.has_colors() {
                Some(colors)
            } else {
                None
            },
            bounds: Bounds3::empty(),
            warnings: None,
        })
    } else {
        Geometry::PointCloud(PointCloud {
            positions,
            colors: if layout.has_colors() {
                Some(colors)
            } else {
                None
            },
            normals: if layout.has_normals() {
                Some(normals)
            } else {
                None
            },
            bounds: Bounds3::empty(),
        })
    };
    let geometry = match geometry {
        Geometry::Mesh(m) => Geometry::Mesh(m.finalize()?),
        Geometry::PointCloud(p) => Geometry::PointCloud(p.finalize()?),
    };
    Ok((
        geometry,
        ParseWarnings {
            degenerate_triangles: degenerate,
            non_finite_vertices: 0,
        },
    ))
}

/// Parse a PLY file. `_variant` from detection is advisory; the header's
/// declared format is authoritative for parsing.
pub fn parse(data: &[u8], _variant: FormatVariant) -> Result<(Geometry, ParseWarnings)> {
    let (header, start) = parse_header(data)?;
    let body = &data[start..];
    match header.format {
        DataFormat::Ascii => {
            let mut r = AsciiReader { rest: "", row: 0 };
            let text = std::str::from_utf8(body).map_err(|_| {
                Error::new(ErrorCode::InvalidPlyData, "data is not valid ASCII text")
            })?;
            r.rest = text;
            parse_body(&header, &mut r)
        }
        DataFormat::BinaryLe => {
            let mut r = BinaryReader {
                data: body,
                pos: 0,
                big_endian: false,
                row: 0,
            };
            parse_body(&header, &mut r)
        }
        DataFormat::BinaryBe => {
            let mut r = BinaryReader {
                data: body,
                pos: 0,
                big_endian: true,
                row: 0,
            };
            parse_body(&header, &mut r)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench_geo(hdr: &str, data: &[u8], variant: FormatVariant) -> (Geometry, ParseWarnings) {
        let mut bytes = hdr.as_bytes().to_vec();
        bytes.extend_from_slice(data);
        parse(&bytes, variant).unwrap()
    }

    #[test]
    fn ascii_triangle_mesh() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let data = b"0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
        let (g, w) = bench_geo(hdr, data, FormatVariant::PlyAscii);
        assert_eq!(w.degenerate_triangles, 0);
        match g {
            Geometry::Mesh(m) => {
                assert_eq!(m.positions.len(), 3);
                assert_eq!(m.triangles.len(), 1);
                assert_eq!(m.triangles[0], [0, 1, 2]);
                assert!(m.colors.is_none());
            }
            _ => panic!("expected mesh"),
        }
    }

    #[test]
    fn ascii_point_cloud() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nend_header\n";
        let data = b"1 2 3\n4 5 6\n7 8 9\n";
        let (g, _) = bench_geo(hdr, data, FormatVariant::PlyAscii);
        assert!(g.is_point_cloud());
        assert_eq!(g.vertex_count(), 3);
    }

    #[test]
    fn ascii_colors_normalized() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let data = b"0 0 0 255 0 0\n1 0 0 0 255 0\n0 1 0 0 0 255\n3 0 1 2\n";
        let (g, _) = bench_geo(hdr, data, FormatVariant::PlyAscii);
        let m = match g {
            Geometry::Mesh(m) => m,
            _ => panic!(),
        };
        let c = m.colors.unwrap();
        assert_eq!(c[0], Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(c[1], Color::new(0.0, 1.0, 0.0, 1.0));
    }

    #[test]
    fn binary_le_uchar_colors() {
        let hdr = "ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let mut data = Vec::new();
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
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryLe);
        let m = match g {
            Geometry::Mesh(m) => m,
            _ => panic!(),
        };
        let c = m.colors.unwrap();
        // Same expectations as the ASCII color test: full channels are
        // unaffected by sRGB linearization.
        assert_eq!(c[0], Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(c[1], Color::new(0.0, 1.0, 0.0, 1.0));
        assert_eq!(c[2], Color::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn binary_be_uchar_colors() {
        let hdr = "ply\nformat binary_big_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let mut data = Vec::new();
        let verts = [
            ([0.0f32, 0.0, 0.0], [255u8, 0, 0]),
            ([1.0, 0.0, 0.0], [0, 255, 0]),
            ([0.0, 1.0, 0.0], [0, 0, 255]),
        ];
        for (p, c) in verts {
            for v in p {
                data.extend_from_slice(&v.to_be_bytes());
            }
            data.extend_from_slice(&c);
        }
        data.push(3u8);
        for i in 0i32..3 {
            data.extend_from_slice(&i.to_be_bytes());
        }
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryBe);
        let m = match g {
            Geometry::Mesh(m) => m,
            _ => panic!(),
        };
        let c = m.colors.unwrap();
        assert_eq!(c[0], Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(c[2], Color::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn binary_ushort_colors_are_byte_scale() {
        // Integer color channels are interpreted as byte scale 0..255 per
        // spec §10.5, so ushort storage with byte-scale values maps
        // identically, and values beyond 255 clamp (never wrap).
        let hdr = "ply\nformat binary_little_endian 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nproperty ushort red\nproperty ushort green\nproperty ushort blue\nend_header\n";
        let mut data = Vec::new();
        for (p, c) in [
            ([0.0f32, 0.0, 0.0], [128u16, 255, 65280]), // 65280 > 255 -> clamps red to 255
            ([1.0, 0.0, 0.0], [65280, 65535, 0]),       // both clamp to 255
        ] {
            for v in p {
                data.extend_from_slice(&v.to_le_bytes());
            }
            for v in c {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryLe);
        let pc = match g {
            Geometry::PointCloud(p) => p,
            _ => panic!(),
        };
        let c = pc.colors.unwrap();
        // 16-bit storage, byte-scale interpretation (spec §10.5): 128 stays
        // mid-tone (sRGB-linearized), and 65280/65535 clamp to full (1.0) —
        // never wrap-around, never 0..65535 remapping (0.996/1.000).
        assert_eq!(
            c[0],
            Color::new(
                crate::render::image::srgb_to_linear(128.0 / 255.0),
                1.0,
                1.0,
                1.0
            )
        );
        assert_eq!(c[1], Color::new(1.0, 1.0, 0.0, 1.0));
    }

    #[test]
    fn binary_float_colors_normalized() {
        // Float channels are 0..1 and sRGB-linearized like integer ones.
        let hdr = "ply\nformat binary_little_endian 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty float red\nproperty float green\nproperty float blue\nend_header\n";
        let mut data = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.5, 0.0] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryLe);
        let pc = match g {
            Geometry::PointCloud(p) => p,
            _ => panic!(),
        };
        let c = pc.colors.unwrap()[0];
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, crate::render::image::srgb_to_linear(0.5));
        assert_eq!(c.b, 0.0);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn ascii_alpha_channel_read() {
        // An explicit alpha property flows into Color.a through the same
        // channel conversion (alpha 128/255 is not 1.0).
        let hdr = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nproperty uchar alpha\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let data = b"0 0 0 255 0 0 128\n1 0 0 0 0 0 255\n0 1 0 0 0 0 255\n3 0 1 2\n";
        let (g, _) = bench_geo(hdr, data, FormatVariant::PlyAscii);
        let m = match g {
            Geometry::Mesh(m) => m,
            _ => panic!(),
        };
        let c = m.colors.unwrap()[0];
        assert_eq!(c.r, 1.0);
        assert!(c.a < 1.0, "alpha must be read, got {}", c.a);
        assert_eq!(c.a, crate::render::image::srgb_to_linear(128.0 / 255.0));
    }

    #[test]
    fn binary_le_point_cloud_with_colors() {
        let hdr = "ply\nformat binary_little_endian 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n";
        let mut data = Vec::new();
        for (p, c) in [
            ([0.0f32, 0.0, 0.0], [255u8, 0, 0]),
            ([1.0, 1.0, 1.0], [0, 128, 255]),
        ] {
            for v in p {
                data.extend_from_slice(&v.to_le_bytes());
            }
            data.extend_from_slice(&c);
        }
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryLe);
        let pc = match g {
            Geometry::PointCloud(p) => p,
            _ => panic!("expected point cloud"),
        };
        assert_eq!(pc.positions.len(), 2);
        let c = pc.colors.unwrap();
        assert_eq!(c[0], Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(
            c[1],
            Color::new(
                0.0,
                crate::render::image::srgb_to_linear(128.0 / 255.0),
                1.0,
                1.0
            )
        );
    }

    #[test]
    fn binary_le_roundtrip() {
        let hdr = "ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let mut data = Vec::new();
        for (x, y, z) in [(0f32, 0f32, 0f32), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)] {
            data.extend_from_slice(&x.to_le_bytes());
            data.extend_from_slice(&y.to_le_bytes());
            data.extend_from_slice(&z.to_le_bytes());
        }
        data.push(3u8); // count
        for i in 0i32..3 {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryLe);
        match g {
            Geometry::Mesh(m) => {
                assert_eq!(m.positions.len(), 3);
                assert_eq!(m.triangles, vec![[0, 1, 2]]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn binary_be_roundtrip() {
        let hdr = "ply\nformat binary_big_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let mut data = Vec::new();
        for (x, y, z) in [(0f32, 0f32, 0f32), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)] {
            data.extend_from_slice(&x.to_be_bytes());
            data.extend_from_slice(&y.to_be_bytes());
            data.extend_from_slice(&z.to_be_bytes());
        }
        data.push(3u8);
        for i in 0i32..3 {
            data.extend_from_slice(&i.to_be_bytes());
        }
        let (g, _) = bench_geo(hdr, &data, FormatVariant::PlyBinaryBe);
        assert_eq!(g.vertex_count(), 3);
        assert_eq!(g.triangle_count(), 1);
    }

    #[test]
    fn quad_face_triangulated() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let data = b"0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";
        let (g, _) = bench_geo(hdr, data, FormatVariant::PlyAscii);
        assert_eq!(g.triangle_count(), 2);
    }

    #[test]
    fn missing_vertex_element_rejected() {
        let hdr = "ply\nformat ascii 1.0\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let mut b = hdr.as_bytes().to_vec();
        b.extend_from_slice(b"3 0 1 2\n");
        assert!(parse(&b, FormatVariant::PlyAscii).is_err());
    }

    #[test]
    fn truncated_binary_rejected() {
        let hdr = "ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nend_header\n";
        let mut b = hdr.as_bytes().to_vec();
        b.extend_from_slice(&[0u8; 4]); // only 1 of 3 vertices
        assert!(parse(&b, FormatVariant::PlyBinaryLe).is_err());
    }

    #[test]
    fn non_finite_rejected() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nend_header\n";
        let data = b"1 2 nan\n3 4 5\n";
        let mut b = hdr.as_bytes().to_vec();
        b.extend_from_slice(data);
        assert!(parse(&b, FormatVariant::PlyAscii).is_err());
    }

    #[test]
    fn out_of_range_face_index_rejected() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let data = b"0 0 0\n1 0 0\n0 1 0\n3 0 1 9\n";
        let mut b = hdr.as_bytes().to_vec();
        b.extend_from_slice(data);
        assert!(parse(&b, FormatVariant::PlyAscii).is_err());
    }

    #[test]
    fn face_list_too_short_rejected() {
        let hdr = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n";
        let data = b"0 0 0\n1 0 0\n0 1 0\n2 0 1\n";
        let mut b = hdr.as_bytes().to_vec();
        b.extend_from_slice(data);
        assert!(parse(&b, FormatVariant::PlyAscii).is_err());
    }
}
