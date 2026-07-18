//! Tolerant DXF import for the supported R12 through R2018 subset.
//!
//! The parser reads the same two-line code/value tags written by `export.rs`.
//! [`str::lines`] accepts CRLF and LF without an additional parser dependency.
//!
//! # Behavior
//! - The entire import is one `"Import DXF"` transaction, so it undoes as one step.
//! - Group-42 bulges remain dimensionless. Unsupported angular entities are
//!   skipped with warnings.
//! - Unsupported or invalid entities increment [`ImportReport::skipped`]. Only
//!   structurally unreadable or oversized files return a typed [`DxfError`].
//! - True color in group 420 takes precedence over ACI group 62.
//! - Layers derive off, frozen, and locked state from standard groups and merge
//!   case-insensitively by name while preserving existing destination properties.
//!
//! # Input bounds
//! [`MAX_INPUT_BYTES`] bounds the O(n) token buffer. File-provided counters never
//! drive preallocation, so hostile values cannot trigger giant allocations.

use std::collections::HashMap;
use std::io::Read;

use af_math::Point2;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
    PolyVertex, PolylineGeo,
};
use af_model::id::{LayerId, ObjectId};
use af_model::{ContainerRef, Layer, Session, TxError};

use crate::aci::{aci_to_rgb, unpack_true_color};
use crate::report::{DxfError, ImportOptions, ImportReport};

/// 16 MiB input limit bounding the O(n) token buffer.
///
/// ponytail: If real drawings exceed this limit, `impl Read` already allows a
/// streaming parser without changing the public API.
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;

/// Imports DXF from `reader` into `session` as one undoable transaction.
///
/// Returns imported and skipped counts plus warnings in [`ImportReport`].
///
/// # Errors
/// - [`DxfError::Io`] when reading fails.
/// - [`DxfError::TooLarge`] above [`MAX_INPUT_BYTES`].
/// - [`DxfError::Malformed`] for invalid ASCII code/value structure.
///
/// Errors apply no transaction and leave the document unchanged.
pub fn import_dxf(
    session: &mut Session,
    reader: impl Read,
    _opts: ImportOptions,
) -> Result<ImportReport, DxfError> {
    let text = read_capped(reader)?;
    // Pure parsing leaves the session untouched on failure.
    let Parsed {
        layers,
        entities,
        mut report,
    } = parse(&text)?;
    // Apply everything in one transaction while recording per-entity failures.
    apply(session, &layers, &entities, &mut report);
    Ok(report)
}

/// Reads `reader` under a hard size limit and decodes it as text.
///
/// Lossy UTF-8 replacement handles arbitrary bytes without panicking. The
/// supported path assumes UTF-8 rather than legacy `$DWGCODEPAGE` encodings.
fn read_capped(mut reader: impl Read) -> Result<String, DxfError> {
    let mut buf = Vec::new();
    reader
        .by_ref()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_INPUT_BYTES {
        return Err(DxfError::TooLarge {
            limit: MAX_INPUT_BYTES,
        });
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

// ===================== Intermediate representation =====================

/// Parsed DXF layer whose ID is assigned during transaction merge.
struct ParsedLayer {
    name: String,
    color: Color,
    off: bool,
    frozen: bool,
    locked: bool,
    plot: bool,
    lineweight: Lineweight,
}

/// Parsed DXF entity ready for an [`EntityRecord`].
struct ParsedEntity {
    /// Source DXF type for reporting.
    dxf_type: &'static str,
    /// Group-8 layer name resolved to `LayerId` while applying.
    layer: String,
    color: Color,
    geometry: EntityGeometry,
}

/// Pure parse result with layers, drawing-order entities, and skipped-content report.
#[derive(Default)]
struct Parsed {
    layers: Vec<ParsedLayer>,
    entities: Vec<ParsedEntity>,
    report: ImportReport,
}

// ===================== Code/value tokenization =====================

/// Borrowed DXF group code and value pair.
type Pair<'a> = (i32, &'a str);

/// Splits text into code/value pairs.
///
/// Each pair uses an integer-code line and value line. Blank lines between tags
/// are ignored, while values may be empty. Non-integer code lines are malformed.
fn tokenize(text: &str) -> Result<Vec<Pair<'_>>, DxfError> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(code_line) = lines.next() {
        let code_str = code_line.trim();
        if code_str.is_empty() {
            continue; // Blank lines between tags are allowed.
        }
        let code = code_str.parse::<i32>().map_err(|_| {
            DxfError::Malformed(format!("group code is not an integer: {code_line:?}"))
        })?;
        // The next line is the value; a trailing lone code receives an empty value.
        let value = lines.next().unwrap_or("");
        out.push((code, value));
    }
    Ok(out)
}

// ===================== Structural parsing =====================

/// Parses supported TABLES/LAYER and ENTITIES sections from complete DXF text.
fn parse(text: &str) -> Result<Parsed, DxfError> {
    let pairs = tokenize(text)?;
    let mut parsed = Parsed::default();
    let mut i = 0;
    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && val == "EOF" {
            break;
        }
        if code == 0 && val == "SECTION" {
            match pairs.get(i + 1) {
                Some(&(2, "ENTITIES")) => {
                    i = parse_entities(&pairs, i + 2, &mut parsed);
                    continue;
                }
                Some(&(2, "TABLES")) => {
                    i = parse_tables(&pairs, i + 2, &mut parsed);
                    continue;
                }
                // Ignore unsupported sections until the next SECTION tag.
                _ => {
                    i += 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    Ok(parsed)
}

/// Walks ENTITIES from `start` and returns its terminator index. Each `(0, TYPE)`
/// opens an entity whose body continues until the next group 0.
fn parse_entities(pairs: &[Pair<'_>], start: usize, parsed: &mut Parsed) -> usize {
    let mut i = start;
    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && (val == "ENDSEC" || val == "EOF") {
            return i;
        }
        if code == 0 {
            let etype = val;
            let mut j = i + 1;
            while j < pairs.len() && pairs[j].0 != 0 {
                j += 1;
            }
            parse_one_entity(etype, &pairs[i + 1..j], parsed);
            i = j;
        } else {
            i += 1;
        }
    }
    i
}

/// Walks TABLES from `start`, interpreting only LAYER, and returns the terminator.
fn parse_tables(pairs: &[Pair<'_>], start: usize, parsed: &mut Parsed) -> usize {
    let mut i = start;
    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && (val == "ENDSEC" || val == "EOF") {
            return i;
        }
        if code == 0 && val == "TABLE" {
            let is_layer = matches!(pairs.get(i + 1), Some(&(2, "LAYER")));
            i = parse_one_table(pairs, i + 1, parsed, is_layer);
        } else {
            i += 1;
        }
    }
    i
}

/// Walks a table from `(2, name)` through ENDTAB. In a layer table, each
/// `(0, LAYER)` produces one layer.
fn parse_one_table(pairs: &[Pair<'_>], start: usize, parsed: &mut Parsed, is_layer: bool) -> usize {
    let mut i = start;
    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && (val == "ENDTAB" || val == "ENDSEC" || val == "EOF") {
            // Consume ENDTAB; leave ENDSEC and EOF to the section parser.
            return if val == "ENDTAB" { i + 1 } else { i };
        }
        if is_layer && code == 0 && val == "LAYER" {
            let mut j = i + 1;
            while j < pairs.len() && pairs[j].0 != 0 {
                j += 1;
            }
            parse_layer(&pairs[i + 1..j], parsed);
            i = j;
        } else {
            i += 1;
        }
    }
    i
}

// ===================== Layer and entity bodies =====================

/// Parses a LAYER record body into `parsed.layers`.
fn parse_layer(body: &[Pair<'_>], parsed: &mut Parsed) {
    let name = match find_str(body, 2) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => {
            parsed.report.warn("LAYER record without a name skipped");
            return;
        }
    };
    let flags = find_i(body, 70).unwrap_or(0);
    let frozen = (flags & 1) != 0;
    let locked = (flags & 4) != 0;
    let c62 = find_i(body, 62);
    let off = c62.is_some_and(|v| v < 0); // DXF convention: a negative color means an off layer.
    let tc = find_i(body, 420);
    let color = resolve_color(c62, tc, true);
    let plot = find_i(body, 290) != Some(0); // Missing or nonzero means printable.
    let lineweight = resolve_lineweight(body);
    parsed.layers.push(ParsedLayer {
        name,
        color,
        off,
        frozen,
        locked,
        plot,
        lineweight,
    });
}

/// Parses an entity body or records it as skipped when unsupported or invalid.
fn parse_one_entity(etype: &str, body: &[Pair<'_>], parsed: &mut Parsed) {
    let geometry: Option<(&'static str, EntityGeometry)> = match etype {
        "LINE" => Some((
            "LINE",
            EntityGeometry::Line(LineGeo::new(
                Point2::new(find_f(body, 10), find_f(body, 20)),
                Point2::new(find_f(body, 11), find_f(body, 21)),
            )),
        )),
        "POINT" => Some((
            "POINT",
            EntityGeometry::Point(PointGeo::new(Point2::new(
                find_f(body, 10),
                find_f(body, 20),
            ))),
        )),
        "CIRCLE" => Some((
            "CIRCLE",
            EntityGeometry::Circle(CircleGeo::new(
                Point2::new(find_f(body, 10), find_f(body, 20)),
                find_f(body, 40),
            )),
        )),
        "LWPOLYLINE" => parse_lwpolyline(body).map(|g| ("LWPOLYLINE", EntityGeometry::Polyline(g))),
        _ => None,
    };
    match geometry {
        Some((dxf_type, geom)) => parsed.entities.push(ParsedEntity {
            dxf_type,
            layer: find_str(body, 8).unwrap_or("0").to_string(),
            color: resolve_color(find_i(body, 62), find_i(body, 420), false),
            geometry: geom,
        }),
        None => {
            parsed.report.bump_skipped(etype);
            let handle = find_str(body, 5).unwrap_or("?");
            parsed.report.warn(format!(
                "unsupported entity {etype:?} (handle {handle}) skipped"
            ));
        }
    }
}

/// Builds [`PolylineGeo`] from flag 70, coordinate groups 10/20, and dimensionless
/// group-42 bulges. Returns `None` with no vertices.
///
/// Group 90 never drives preallocation; vertices append as they arrive.
fn parse_lwpolyline(body: &[Pair<'_>]) -> Option<PolylineGeo> {
    let mut verts: Vec<PolyVertex> = Vec::new();
    let mut closed = false;
    let mut width = 0.0f64; // Group 43 stores constant width for donuts and wide polylines.
    for &(code, val) in body {
        match code {
            70 => closed = (val.trim().parse::<i64>().unwrap_or(0) & 1) != 0,
            43 => width = parse_f(val),
            10 => verts.push(PolyVertex::new(Point2::new(parse_f(val), 0.0), 0.0)),
            20 => {
                if let Some(v) = verts.last_mut() {
                    v.pt.y = parse_f(val);
                }
            }
            42 => {
                if let Some(v) = verts.last_mut() {
                    v.bulge = parse_f(val);
                }
            }
            _ => {}
        }
    }
    if verts.is_empty() {
        return None;
    }
    Some(PolylineGeo::new(verts, closed).with_width(width))
}

// ===================== Color and lineweight resolution =====================

/// Resolves color from signed ACI group 62 and true-color group 420. True color
/// wins; ACI 1 through 255 maps through [`crate::aci`], 256 means ByLayer, and 0
/// means ByBlock for entities.
///
/// Layers require a concrete color, so recursive modes fall back to ACI 7 white.
fn resolve_color(c62: Option<i64>, tc: Option<i64>, is_layer: bool) -> Color {
    if let Some(v) = tc
        && (0..=0x00FF_FFFF).contains(&v)
    {
        let (r, g, b) = unpack_true_color(v);
        return Color::Rgb(r, g, b);
    }
    let color = match c62 {
        Some(v) => match v.unsigned_abs() {
            0 if !is_layer => Color::ByBlock,
            256 if !is_layer => Color::ByLayer,
            a @ 1..=255 => {
                let (r, g, b) = aci_to_rgb(a as u8);
                Color::Rgb(r, g, b)
            }
            _ => Color::ByLayer, // Out-of-range values are corrected below for layers.
        },
        None => Color::ByLayer,
    };
    if is_layer && matches!(color, Color::ByLayer | Color::ByBlock) {
        let (r, g, b) = aci_to_rgb(7);
        return Color::Rgb(r, g, b);
    }
    color
}

/// Resolves group 370 hundredths of a millimeter or negative lineweight enums.
fn resolve_lineweight(body: &[Pair<'_>]) -> Lineweight {
    match find_i(body, 370) {
        Some(-2) => Lineweight::ByBlock,
        Some(v) if v >= 0 => Lineweight::Mm((v as f32) / 100.0),
        // Export default -3, DXF ByLayer -1, and unknown values fall back to ByLayer.
        _ => Lineweight::ByLayer,
    }
}

// ===================== Body group readers =====================

/// Returns the first borrowed value for `code` in `body`.
fn find_str<'a>(body: &[Pair<'a>], code: i32) -> Option<&'a str> {
    body.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
}

/// Parses the first `code` value as an integer.
fn find_i(body: &[Pair<'_>], code: i32) -> Option<i64> {
    find_str(body, code).and_then(|v| v.trim().parse::<i64>().ok())
}

/// Parses the first `code` value as a real, defaulting to 0.0.
///
/// This tolerance lets entity validation route missing or non-finite geometry to
/// `skipped` without aborting the whole import.
fn find_f(body: &[Pair<'_>], code: i32) -> f64 {
    find_str(body, code).map_or(0.0, parse_f)
}

/// Parses a DXF real, returning NaN for later validation when parsing fails.
fn parse_f(v: &str) -> f64 {
    v.trim().parse::<f64>().unwrap_or(f64::NAN)
}

// ===================== Single-transaction application =====================

/// Applies layers and entities in one `"Import DXF"` transaction.
///
/// Per-record transaction errors become skipped warnings. Existing layers win
/// during case-insensitive name merges.
fn apply(
    session: &mut Session,
    layers: &[ParsedLayer],
    entities: &[ParsedEntity],
    report: &mut ImportReport,
) {
    let outcome = session.transact::<(), TxError, _>("Import DXF", |tx| {
        // Every document provides Continuous as the default for new layers.
        let continuous = tx.doc().line_types().next().map(|lt| lt.id());
        let current = tx.doc().current_layer();
        // Seed a lowercase-name-to-ID index from existing layers.
        let mut name_map: HashMap<String, LayerId> = tx
            .doc()
            .layers()
            .map(|l| (l.name().to_lowercase(), l.id()))
            .collect();

        for pl in layers {
            let key = pl.name.to_lowercase();
            if name_map.contains_key(&key) {
                // Preserve the existing destination layer during a name merge.
                report.warn(format!(
                    "layer {:?} merged with existing layer (existing kept)",
                    pl.name
                ));
                continue;
            }
            let Some(lt) = continuous else {
                report.warn(format!(
                    "layer {:?} skipped: document has no line type to assign",
                    pl.name
                ));
                continue;
            };
            let layer = Layer::new(
                ObjectId::NIL.into(),
                pl.name.clone(),
                pl.color,
                lt,
                pl.lineweight,
            )
            .with_off(pl.off)
            .with_frozen(pl.frozen)
            .with_locked(pl.locked)
            .with_plot(pl.plot);
            match tx.add_layer_raw(layer) {
                Ok(id) => {
                    name_map.insert(key, id);
                }
                Err(e) => report.warn(format!("layer {:?} skipped: {e}", pl.name)),
            }
        }

        for pe in entities {
            let layer_id = match name_map.get(&pe.layer.to_lowercase()) {
                Some(&id) => id,
                None => {
                    report.warn(format!(
                        "entity on unknown layer {:?} imported on current layer",
                        pe.layer
                    ));
                    current
                }
            };
            let record = EntityRecord::new(
                ObjectId::NIL.into(),
                layer_id,
                pe.color,
                LineTypeRef::ByLayer,
                Lineweight::ByLayer,
                pe.geometry.clone(),
            );
            match tx.add_entity(ContainerRef::ModelSpace, record) {
                Ok(_) => report.bump_imported(pe.dxf_type),
                Err(e) => {
                    report.bump_skipped(pe.dxf_type);
                    report.warn(format!("{} skipped: {e}", pe.dxf_type));
                }
            }
        }
        Ok(())
    });
    // The closure captures record errors; any outer transaction failure applies nothing.
    debug_assert!(outcome.is_ok(), "import closure must not propagate errors");
    let _ = outcome;
}
