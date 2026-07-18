//! Parser for compiled SHX stroke fonts used to render drawing text.
//!
//! # Scope
//!
//! - Supports single-byte `shapes` 1.0 and 1.1 stroke fonts. `bigfont` and
//!   `unifont` are detected and rejected with [`ShxError::Unsupported`].
//! - Decodes special opcodes 0 through 14 and length-direction vectors.
//! - Produces [`GlyphOutline`] polylines with bulges in height-normalized font
//!   space, plus horizontal advance.
//!
//! # Provenance
//!
//! This module implements a publicly documented file format. It does not
//! decompile or vendor copyrighted `.shx` files; tests generate their own
//! fixtures with a minimal compiler.
//!
//! # Robustness
//!
//! Truncated, corrupt, and arbitrary input returns [`ShxError`] or a valid
//! partial font without panicking. The crate uses no unsafe code.
//!
//! # Format references
//!
//! The public shape-description and special-code references define opcodes 0
//! through 14. Open-source parsers corroborate the sentinel-terminated header,
//! little-endian index, and concatenated definition layout.

use std::collections::BTreeMap;

mod decode;

/// Glyph-polyline vertex in font space normalized by the `above` height.
///
/// `bulge` describes the arc from the previous vertex to this one as
/// `tan(delta_angle / 4)`, with positive values counterclockwise. Zero is a
/// straight segment, one is a semicircle, and the first vertex is always zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolyPoint {
    /// X coordinate normalized by font height.
    pub x: f32,
    /// Y coordinate normalized by font height, positive upward.
    pub y: f32,
    /// Arc bulge from the previous vertex to this one; zero means straight.
    pub bulge: f32,
}

/// Continuous stroke traced while the virtual pen is down.
///
/// Single-point strokes are not emitted.
#[derive(Debug, Clone, PartialEq)]
pub struct PolySeg {
    /// Vertices in stroke order; the first has `bulge == 0`.
    pub points: Vec<PolyPoint>,
}

/// Decoded glyph strokes and horizontal advance.
///
/// Coordinates are normalized by font height. A blank glyph can have no strokes
/// and a positive advance.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphOutline {
    /// Glyph stroke polylines in drawing order.
    pub strokes: Vec<PolySeg>,
    /// Horizontal cursor advance normalized by font height.
    pub advance: f32,
}

/// SHX parsing or decoding error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShxError {
    /// The file ends before its declared structure.
    Truncated,
    /// The sentinel header is missing, unterminated, or malformed.
    BadHeader,
    /// Unsupported font type or version, including bigfont and unifont.
    Unsupported(String),
    /// Inconsistent shape index with impossible counts or out-of-range data.
    BadIndex,
}

impl core::fmt::Display for ShxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ShxError::Truncated => f.write_str("SHX file is truncated"),
            ShxError::BadHeader => f.write_str("SHX header is missing or malformed"),
            ShxError::Unsupported(what) => write!(f, "unsupported SHX font: {what}"),
            ShxError::BadIndex => f.write_str("SHX shape index is inconsistent"),
        }
    }
}

impl std::error::Error for ShxError {}

/// Parsed SHX metadata and decoded [`GlyphOutline`] values.
///
/// Glyphs and subshapes are decoded eagerly so [`ShxFont::glyph`] is a lookup.
#[derive(Debug, Clone)]
pub struct ShxFont {
    name: String,
    /// Raw design height above the baseline and normalization divisor.
    above: f32,
    /// Raw depth below the baseline.
    below: f32,
    /// Header modes byte: 0 is horizontal only, 2 is dual orientation.
    modes: u8,
    /// Type and version descriptor from the header.
    descriptor: String,
    glyphs: BTreeMap<u16, GlyphOutline>,
}

impl ShxFont {
    /// Parses a complete SHX font from bytes.
    ///
    /// # Errors
    ///
    /// - [`ShxError::BadHeader`] for a missing or malformed sentinel.
    /// - [`ShxError::Unsupported`] for bigfont, unifont, or non-1.x versions.
    /// - [`ShxError::Truncated`] or [`ShxError::BadIndex`] for out-of-range data.
    ///
    /// Arbitrary input never intentionally panics.
    pub fn parse(bytes: &[u8]) -> Result<Self, ShxError> {
        let header = parse_header(bytes)?;
        let index = parse_index(bytes, header.body_start)?;

        // Map codes to borrowed bytecode so subshapes can resolve in a second pass.
        let raw: BTreeMap<u16, &[u8]> = index.iter().map(|e| (e.code, e.body)).collect();

        // Shape 0 carries font metadata when present.
        let meta = match raw.get(&0) {
            Some(body) => parse_font_shape0(body),
            None => FontMeta::default(),
        };
        let above = if meta.above > 0.0 { meta.above } else { 1.0 };

        // Decode every glyph except metadata shape 0.
        let mut glyphs = BTreeMap::new();
        for entry in &index {
            if entry.code == 0 {
                continue;
            }
            let outline = decode::decode_glyph(entry.code, &raw, above as f64);
            glyphs.insert(entry.code, outline);
        }

        Ok(ShxFont {
            name: meta.name,
            above,
            below: meta.below,
            modes: meta.modes,
            descriptor: header.descriptor,
            glyphs,
        })
    }

    /// Returns the outline for a glyph code, or `None` when absent.
    #[must_use]
    pub fn glyph(&self, code: u16) -> Option<&GlyphOutline> {
        self.glyphs.get(&code)
    }

    /// Font name declared by shape 0, or an empty string when absent.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Type and version descriptor from the header, such as `"shapes 1.0"`.
    #[must_use]
    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }

    /// Ascender height in normalized units, always 1.0.
    #[must_use]
    pub fn ascent(&self) -> f32 {
        1.0
    }

    /// Descender depth in normalized units (`below / above`).
    #[must_use]
    pub fn descent(&self) -> f32 {
        self.below / self.above
    }

    /// Raw `above` design height used as the normalization divisor.
    #[must_use]
    pub fn design_height(&self) -> f32 {
        self.above
    }

    /// Header modes byte.
    #[must_use]
    pub fn modes(&self) -> u8 {
        self.modes
    }

    /// Number of decoded glyphs, excluding metadata shape 0.
    #[must_use]
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Iterates present glyph codes in ascending order.
    pub fn codes(&self) -> impl Iterator<Item = u16> + '_ {
        self.glyphs.keys().copied()
    }
}

/// Parsed header descriptor and binary-body offset after the `0x1A` terminator.
struct Header {
    descriptor: String,
    body_start: usize,
}

/// Parses and validates the sentinel, accepting only `shapes 1.x`.
fn parse_header(bytes: &[u8]) -> Result<Header, ShxError> {
    // The sentinel ends at the first 0x1A after its CRLF sequence.
    let eof = bytes
        .iter()
        .position(|&b| b == 0x1A)
        .ok_or(ShxError::BadHeader)?;
    let sentinel = core::str::from_utf8(&bytes[..eof]).map_err(|_| ShxError::BadHeader)?;

    // "AutoCAD-86 shapes 1.0\r\n" -> ["AutoCAD-86", "shapes", "1.0"].
    let mut parts = sentinel.split_whitespace();
    let _vendor = parts.next().ok_or(ShxError::BadHeader)?;
    let kind = parts.next().ok_or(ShxError::BadHeader)?;
    let version = parts.next().unwrap_or("");
    let descriptor = format!("{kind} {version}");

    match kind {
        "shapes" if version.starts_with("1.") => Ok(Header {
            descriptor,
            body_start: eof + 1,
        }),
        _ => Err(ShxError::Unsupported(descriptor)),
    }
}

/// Shape index entry containing its code and raw bytecode.
struct IndexEntry<'a> {
    code: u16,
    body: &'a [u8],
}

/// Parses the shapes index and partitions the definition region.
///
/// After `0x1A`, the layout is `start:u16, end:u16, count:u16`, followed by
/// `(code:u16, len:u16)` entries and concatenated definitions in the same order.
fn parse_index(bytes: &[u8], start: usize) -> Result<Vec<IndexEntry<'_>>, ShxError> {
    let mut cur = Cursor::new(bytes, start);
    let _first = cur.u16()?;
    let _last = cur.u16()?;
    let count = cur.u16()? as usize;

    // Reject impossible counts before allocating memory for a corrupt index.
    let remaining = bytes.len().saturating_sub(cur.pos);
    if count.saturating_mul(4) > remaining {
        return Err(ShxError::BadIndex);
    }

    let mut dir = Vec::with_capacity(count);
    for _ in 0..count {
        let code = cur.u16()?;
        let len = cur.u16()? as usize;
        dir.push((code, len));
    }

    // Definitions are sequential immediately after the entry table.
    let mut pos = cur.pos;
    let mut entries = Vec::with_capacity(count);
    for (code, len) in dir {
        let end = pos.checked_add(len).ok_or(ShxError::BadIndex)?;
        let body = bytes.get(pos..end).ok_or(ShxError::Truncated)?;
        entries.push(IndexEntry { code, body });
        pos = end;
    }
    Ok(entries)
}

/// Font metadata extracted from shape 0.
#[derive(Default)]
struct FontMeta {
    name: String,
    above: f32,
    below: f32,
    modes: u8,
}

/// Parses shape 0: NUL-terminated name followed by above, below, and modes.
fn parse_font_shape0(body: &[u8]) -> FontMeta {
    let nul = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    let name = core::str::from_utf8(&body[..nul]).unwrap_or("").to_owned();
    let rest = body.get(nul + 1..).unwrap_or(&[]);
    FontMeta {
        name,
        above: rest.first().copied().unwrap_or(0) as f32,
        below: rest.get(1).copied().unwrap_or(0) as f32,
        modes: rest.get(2).copied().unwrap_or(0),
    }
}

/// Bounds-checked sequential cursor over the complete buffer. Out-of-range reads
/// return [`ShxError::Truncated`].
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Cursor { data, pos }
    }

    fn u16(&mut self) -> Result<u16, ShxError> {
        let end = self.pos.checked_add(2).ok_or(ShxError::Truncated)?;
        let slice = self.data.get(self.pos..end).ok_or(ShxError::Truncated)?;
        self.pos = end;
        Ok(u16::from_le_bytes([slice[0], slice[1]]))
    }
}
