//! Tests for the SHX parser (`af_render::shx`).
//!
//! # Fixture provenance
//!
//! No copyrighted `.shx` file is vendored or decompiled. Every fixture is built
//! by the minimal [`compile_shapes`] helper in this file using the publicly
//! documented `shapes 1.0` format. Handwritten glyph bytecode covers vectors,
//! bulges, octant arcs, fractional arcs, and subshapes, then compares results
//! with independently calculated geometry.

use af_geom::bulge::bulge_to_arc;
use af_math::Point2;
use af_render::shx::{ShxError, ShxFont};

// ---------------------------------------------------------------------------
// Test SHX compiler for the `shapes 1.0` format.
// ---------------------------------------------------------------------------

/// Assembles a `shapes 1.0` SHX font from metadata and glyph bytecode, adding
/// shape 0 metadata automatically.
fn compile_shapes(
    name: &str,
    above: u8,
    below: u8,
    modes: u8,
    glyphs: &[(u16, Vec<u8>)],
) -> Vec<u8> {
    // Shape 0 contains a NUL-terminated name followed by above, below, and modes.
    let mut shape0 = Vec::new();
    shape0.extend_from_slice(name.as_bytes());
    shape0.push(0);
    shape0.extend_from_slice(&[above, below, modes]);

    let mut entries: Vec<(u16, Vec<u8>)> = vec![(0, shape0)];
    for (code, body) in glyphs {
        entries.push((*code, body.clone()));
    }
    entries.sort_by_key(|(c, _)| *c);

    let start = entries.iter().map(|(c, _)| *c).min().unwrap();
    let end = entries.iter().map(|(c, _)| *c).max().unwrap();
    let count = entries.len() as u16;

    let mut out = Vec::new();
    out.extend_from_slice(b"AutoCAD-86 shapes 1.0\r\n");
    out.push(0x1A);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&end.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    for (code, body) in &entries {
        out.extend_from_slice(&code.to_le_bytes());
        out.extend_from_slice(&(body.len() as u16).to_le_bytes());
    }
    for (_, body) in &entries {
        out.extend_from_slice(body);
    }
    out
}

const ABOVE: u8 = 8;
const BELOW: u8 = 2;

/// Test font with glyphs that exercise each opcode family.
fn test_font_bytes() -> Vec<u8> {
    let glyphs = vec![
        // 32 (space): pen up, move (3, 0), end. No stroke; advance is 3.
        (32u16, vec![0x02, 0x30, 0x00]),
        // 65 (test 'A'): straight vector lines, then pen-up advance.
        (65u16, vec![0x01, 0x30, 0x24, 0x02, 0x30, 0x00]),
        // 66: bulge arc with dx=4, dy=0, h=127.
        (66u16, vec![0x01, 0x0C, 0x04, 0x00, 0x7F, 0x00]),
        // 67: octant arc with radius 4 and a 90-degree counterclockwise sweep.
        (67u16, vec![0x01, 0x0A, 0x04, 0x02, 0x00]),
        // 68: fractional arc with zero offsets, equivalent to the octant arc.
        (68u16, vec![0x01, 0x0B, 0x00, 0x00, 0x00, 0x04, 0x02, 0x00]),
        // 69: subshape opcode referencing glyph 65.
        (69u16, vec![0x07, 0x41, 0x00]),
        // 70: scale-by-two opcode followed by a length-three vector.
        (70u16, vec![0x04, 0x02, 0x01, 0x30, 0x00]),
    ];
    compile_shapes("TEST", ABOVE, BELOW, 0, &glyphs)
}

fn approx(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "expected ~{b}, got {a}");
}

// ---------------------------------------------------------------------------
// Header and metadata.
// ---------------------------------------------------------------------------

#[test]
fn parses_header_and_metrics() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    assert_eq!(font.name(), "TEST");
    assert_eq!(font.descriptor(), "shapes 1.0");
    approx(font.design_height(), 8.0);
    approx(font.ascent(), 1.0);
    approx(font.descent(), 0.25); // below/above = 2/8
    assert_eq!(font.modes(), 0);
    assert_eq!(font.glyph_count(), 7);
    let codes: Vec<u16> = font.codes().collect();
    assert_eq!(codes, vec![32, 65, 66, 67, 68, 69, 70]);
    assert!(font.glyph(999).is_none());
}

// ---------------------------------------------------------------------------
// Line glyphs using vectors and pen-up/pen-down state.
// ---------------------------------------------------------------------------

#[test]
fn decodes_line_glyph() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g = font.glyph(65).unwrap();
    assert_eq!(g.strokes.len(), 1);
    let pts = &g.strokes[0].points;
    // Normalized by above=8; every segment remains straight.
    approx(pts[0].x, 0.0);
    approx(pts[0].y, 0.0);
    approx(pts[0].bulge, 0.0);
    approx(pts[1].x, 0.375);
    approx(pts[1].y, 0.0);
    approx(pts[1].bulge, 0.0);
    approx(pts[2].x, 0.375);
    approx(pts[2].y, 0.25);
    approx(pts[2].bulge, 0.0);
    approx(g.advance, 0.75); // X final = 6 -> 6/8
}

#[test]
fn space_glyph_has_advance_no_strokes() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g = font.glyph(32).unwrap();
    assert!(g.strokes.is_empty());
    approx(g.advance, 0.375); // 3/8
}

#[test]
fn scale_code_multiplies_vector_length() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g = font.glyph(70).unwrap();
    let pts = &g.strokes[0].points;
    approx(pts[0].x, 0.0);
    approx(pts[1].x, 0.75); // 3 * 2 = 6 -> 6/8
    approx(g.advance, 0.75);
}

#[test]
fn subshape_reproduces_referenced_glyph() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g69 = font.glyph(69).unwrap();
    let g65 = font.glyph(65).unwrap();
    assert_eq!(g69.strokes, g65.strokes);
}

// ---------------------------------------------------------------------------
// Arc glyphs using bulge, octant, and fractional forms.
// ---------------------------------------------------------------------------

#[test]
fn decodes_bulge_arc() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g = font.glyph(66).unwrap();
    let pts = &g.strokes[0].points;
    // h=127 produces a semicircle with bulge 1.0.
    approx(pts[0].x, 0.0);
    approx(pts[0].y, 0.0);
    approx(pts[0].bulge, 0.0);
    approx(pts[1].x, 0.5);
    approx(pts[1].y, 0.0);
    approx(pts[1].bulge, 1.0);
}

/// Verifies the octant arc by reconstructing its circle through
/// `af_geom::bulge` as an interoperability cross-check.
#[test]
fn decodes_octant_arc_quarter_circle() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g = font.glyph(67).unwrap();
    let pts = &g.strokes[0].points;
    assert_eq!(pts.len(), 2); // 90 degrees fit within one MAX_ARC_SEG segment.

    approx(pts[0].x, 0.0);
    approx(pts[0].y, 0.0);
    // Raw endpoint (-4, 4) normalizes to (-0.5, 0.5).
    approx(pts[1].x, -0.5);
    approx(pts[1].y, 0.5);
    // A 90-degree bulge is tan(22.5 degrees).
    approx(pts[1].bulge, (std::f32::consts::FRAC_PI_8).tan());

    let arc = bulge_to_arc(
        Point2::new(f64::from(pts[0].x), f64::from(pts[0].y)),
        Point2::new(f64::from(pts[1].x), f64::from(pts[1].y)),
        f64::from(pts[1].bulge),
    )
    .unwrap();
    assert!((arc.radius - 0.5).abs() < 1e-4, "radio {}", arc.radius);
    assert!((arc.center.x - (-0.5)).abs() < 1e-4, "cx {}", arc.center.x);
    assert!((arc.center.y - 0.0).abs() < 1e-4, "cy {}", arc.center.y);
}

/// A fractional arc with zero offsets must equal its octant-arc counterpart.
#[test]
fn fractional_arc_zero_offset_equals_octant() {
    let font = ShxFont::parse(&test_font_bytes()).unwrap();
    let g_frac = font.glyph(68).unwrap();
    let g_oct = font.glyph(67).unwrap();
    assert_eq!(g_frac.strokes, g_oct.strokes);
}

// ---------------------------------------------------------------------------
// Rejection and robustness without panics.
// ---------------------------------------------------------------------------

#[test]
fn rejects_bigfont() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"AutoCAD-86 bigfont 1.0\r\n");
    buf.push(0x1A);
    buf.extend_from_slice(&[0u8; 8]);
    match ShxFont::parse(&buf) {
        Err(ShxError::Unsupported(what)) => assert!(what.contains("bigfont")),
        other => panic!("se esperaba Unsupported(bigfont), fue {other:?}"),
    }
}

#[test]
fn rejects_unifont() {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"AutoCAD-86 unifont 1.0\r\n");
    buf.push(0x1A);
    buf.extend_from_slice(&[0u8; 8]);
    assert!(matches!(
        ShxFont::parse(&buf),
        Err(ShxError::Unsupported(_))
    ));
}

#[test]
fn missing_sentinel_is_bad_header() {
    assert_eq!(
        ShxFont::parse(b"no eof marker here").err(),
        Some(ShxError::BadHeader)
    );
    assert_eq!(ShxFont::parse(&[]).err(), Some(ShxError::BadHeader));
}

#[test]
fn truncated_prefixes_never_panic() {
    let full = test_font_bytes();
    // Every prefix of a valid file returns Ok or Err without panicking.
    for n in 0..full.len() {
        let _ = ShxFont::parse(&full[..n]);
    }
    // The complete file parses successfully.
    assert!(ShxFont::parse(&full).is_ok());
}

#[test]
fn truncated_index_is_error() {
    let full = test_font_bytes();
    // Truncate shortly after 0x1A to leave an incomplete index.
    let eof = full.iter().position(|&b| b == 0x1A).unwrap();
    let res = ShxFont::parse(&full[..eof + 3]);
    assert!(res.is_err());
}

proptest::proptest! {
    /// Lightweight fuzzing: every byte sequence returns without panicking.
    #[test]
    fn parse_arbitrary_bytes_never_panics(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..600)) {
        let _ = ShxFont::parse(&bytes);
    }

    /// Decoder fuzzing: arbitrary glyph bytecode inside a valid font returns
    /// without panicking, including arcs, subshapes, and poly loops.
    #[test]
    fn random_glyph_bytecode_never_panics(body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..300)) {
        let buf = compile_shapes("F", 10, 3, 0, &[(65, body)]);
        let font = ShxFont::parse(&buf).unwrap();
        let _ = font.glyph(65);
    }
}
