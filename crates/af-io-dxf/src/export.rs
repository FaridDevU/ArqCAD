//! Exports a [`Document`] as structurally valid DXF R2000 (AC1015) ASCII.
//!
//! A small code/value tag writer keeps byte-level control over DIMSTYLE,
//! BLOCK_RECORD, OBJECTS, and LAYOUT output without another dependency. Every
//! tag uses two CRLF-terminated lines, reals use roundtrip precision, handles are
//! hexadecimal, angles use degrees, and dimensionless bulges pass through.

use std::io::{self, Write};

use af_math::Point2;
use af_model::container::GeoRef;
use af_model::entity::{Color, Lineweight};
use af_model::extents::{ExtentsFilter, doc_extents};
use af_model::{ContainerRef, Document, Layer};

use crate::aci::{exact_aci, nearest_aci, true_color};
use crate::report::{DxfError, ExportOptions, ExportReport};

/// Exports `doc` to `w` as DXF R2000 ASCII.
///
/// Returns per-type counts and warnings in [`ExportReport`]. Unmappable geometry
/// is skipped with a warning instead of aborting the export.
///
/// # Errors
/// Returns [`DxfError::Io`] when writing fails.
pub fn export_dxf(
    doc: &Document,
    w: &mut impl Write,
    _opts: ExportOptions,
) -> Result<ExportReport, DxfError> {
    let mut report = ExportReport::default();
    let mut wr = Writer { w };

    // Structural handles start beyond document IDs to avoid collisions.
    let h = Handles::new(doc.next_object_id());

    write_header(&mut wr, doc, &h)?;
    write_classes(&mut wr)?;
    write_tables(&mut wr, doc, &h, &mut report)?;
    write_blocks(&mut wr, &h)?;
    write_entities(&mut wr, doc, &h, &mut report)?;
    write_objects(&mut wr, &h)?;
    wr.s(0, "EOF")?;

    Ok(report)
}

// ===================== Structural handles =====================

/// Handles for file-structure objects without model `ObjectId` values. They are
/// allocated monotonically after `doc.next_object_id()`.
struct Handles {
    vport_tbl: u64,
    vport_active: u64,
    ltype_tbl: u64,
    lt_byblock: u64,
    lt_bylayer: u64,
    lt_continuous: u64,
    layer_tbl: u64,
    style_tbl: u64,
    style_std: u64,
    appid_tbl: u64,
    appid_acad: u64,
    dimstyle_tbl: u64,
    dimstyle_std: u64,
    br_tbl: u64,
    br_model: u64,
    br_paper: u64,
    blk_m_begin: u64,
    blk_m_end: u64,
    blk_p_begin: u64,
    blk_p_end: u64,
    dict_root: u64,
    dict_layout: u64,
    layout_model: u64,
    layout_paper: u64,
    /// `$HANDSEED`, the first free handle after structural allocation.
    seed: u64,
}

impl Handles {
    fn new(base: u64) -> Self {
        let mut n = base;
        let mut take = || {
            let v = n;
            n += 1;
            v
        };
        let s = Self {
            vport_tbl: take(),
            vport_active: take(),
            ltype_tbl: take(),
            lt_byblock: take(),
            lt_bylayer: take(),
            lt_continuous: take(),
            layer_tbl: take(),
            style_tbl: take(),
            style_std: take(),
            appid_tbl: take(),
            appid_acad: take(),
            dimstyle_tbl: take(),
            dimstyle_std: take(),
            br_tbl: take(),
            br_model: take(),
            br_paper: take(),
            blk_m_begin: take(),
            blk_m_end: take(),
            blk_p_begin: take(),
            blk_p_end: take(),
            dict_root: take(),
            dict_layout: take(),
            layout_model: take(),
            layout_paper: take(),
            seed: 0,
        };
        Self { seed: n, ..s }
    }
}

// ===================== Code/value writer =====================

struct Writer<'w, W: Write> {
    w: &'w mut W,
}

impl<W: Write> Writer<'_, W> {
    /// Writes one DXF code/value tag as two CRLF-terminated lines.
    fn tag(&mut self, code: i16, val: &str) -> io::Result<()> {
        write!(self.w, "{code}\r\n{val}\r\n")
    }
    fn s(&mut self, code: i16, val: &str) -> io::Result<()> {
        self.tag(code, val)
    }
    fn i(&mut self, code: i16, val: i64) -> io::Result<()> {
        self.tag(code, &val.to_string())
    }
    fn r(&mut self, code: i16, val: f64) -> io::Result<()> {
        self.tag(code, &fmt_f64(val))
    }
    fn h(&mut self, code: i16, val: u64) -> io::Result<()> {
        self.tag(code, &format!("{val:X}"))
    }
}

/// Formats a roundtrip-exact `f64` while ensuring a decimal point.
fn fmt_f64(v: f64) -> String {
    let s = format!("{v}");
    if s.contains(['.', 'e', 'E']) || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

// ===================== Colors (62 / 420) =====================

/// Entity color tags for group 62 and optional group 420.
///
/// ByLayer omits group 62, ByBlock uses zero, ACI passes through, and RGB uses an
/// exact or nearest ACI plus true color when needed.
fn entity_color_62_420(color: Color) -> (Option<i64>, Option<i64>) {
    match color {
        Color::ByLayer => (None, None),
        Color::ByBlock => (Some(0), None),
        Color::Aci(a) => (Some(i64::from(a.get())), None),
        Color::Rgb(r, g, b) => match exact_aci(r, g, b) {
            Some(idx) => (Some(i64::from(idx)), None),
            None => (
                Some(i64::from(nearest_aci(r, g, b))),
                Some(true_color(r, g, b)),
            ),
        },
    }
}

/// Positive base group-62 value for a layer plus optional group 420. The caller
/// negates group 62 for off layers.
fn layer_color_62_420(color: Color, report: &mut ExportReport) -> (i64, Option<i64>) {
    match color {
        Color::Aci(a) => (i64::from(a.get()), None),
        Color::Rgb(r, g, b) => match exact_aci(r, g, b) {
            Some(idx) => (i64::from(idx), None),
            None => (i64::from(nearest_aci(r, g, b)), Some(true_color(r, g, b))),
        },
        Color::ByLayer | Color::ByBlock => {
            report.warn("layer color is ByLayer/ByBlock (not concrete); exported as ACI 7");
            (7, None)
        }
    }
}

/// Layer lineweight group 370 in hundredths of a millimeter or a negative enum.
fn layer_lineweight(lw: Lineweight) -> i64 {
    match lw {
        // A layer cannot recursively use ByLayer, so emit the default.
        Lineweight::ByLayer => -3,
        Lineweight::ByBlock => -2,
        Lineweight::Mm(mm) => (f64::from(mm) * 100.0).round() as i64,
    }
}

/// Layer group-6 line type. Unsupported patterns fall back to Continuous with a
/// warning so output never references a missing LTYPE.
fn layer_linetype_name(doc: &Document, layer: &Layer, report: &mut ExportReport) -> &'static str {
    match doc.line_type(layer.line_type()) {
        Some(lt) if lt.name().eq_ignore_ascii_case("Continuous") => "Continuous",
        Some(lt) => {
            report.warn(format!(
                "layer {:?} linetype {:?} not exported (MVP writes only Continuous)",
                layer.name(),
                lt.name()
            ));
            "Continuous"
        }
        None => "Continuous",
    }
}

// ===================== Sections =====================

fn write_header<W: Write>(wr: &mut Writer<W>, doc: &Document, h: &Handles) -> io::Result<()> {
    let (min, max) = match doc_extents(doc, ContainerRef::ModelSpace, ExtentsFilter::Visible) {
        Some(bb) => (bb.min, bb.max),
        None => (Point2::ORIGIN, Point2::ORIGIN),
    };
    wr.s(0, "SECTION")?;
    wr.s(2, "HEADER")?;
    wr.s(9, "$ACADVER")?;
    wr.s(1, "AC1015")?;
    wr.s(9, "$HANDSEED")?;
    wr.h(5, h.seed)?;
    wr.s(9, "$INSUNITS")?;
    wr.i(70, i64::from(doc.units().linear.dxf_insunits_code()))?;
    wr.s(9, "$EXTMIN")?;
    wr.r(10, min.x)?;
    wr.r(20, min.y)?;
    wr.r(30, 0.0)?;
    wr.s(9, "$EXTMAX")?;
    wr.r(10, max.x)?;
    wr.r(20, max.y)?;
    wr.r(30, 0.0)?;
    wr.s(0, "ENDSEC")
}

fn write_classes<W: Write>(wr: &mut Writer<W>) -> io::Result<()> {
    wr.s(0, "SECTION")?;
    wr.s(2, "CLASSES")?;
    wr.s(0, "ENDSEC")
}

fn write_tables<W: Write>(
    wr: &mut Writer<W>,
    doc: &Document,
    h: &Handles,
    report: &mut ExportReport,
) -> io::Result<()> {
    wr.s(0, "SECTION")?;
    wr.s(2, "TABLES")?;

    // ---- VPORT (*Active) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "VPORT")?;
    wr.h(5, h.vport_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, 1)?;
    wr.s(0, "VPORT")?;
    wr.h(5, h.vport_active)?;
    wr.h(330, h.vport_tbl)?;
    wr.s(100, "AcDbSymbolTableRecord")?;
    wr.s(100, "AcDbViewportTableRecord")?;
    wr.s(2, "*Active")?;
    wr.i(70, 0)?;
    wr.r(10, 0.0)?;
    wr.r(20, 0.0)?;
    wr.r(11, 1.0)?;
    wr.r(21, 1.0)?;
    wr.r(12, 0.0)?;
    wr.r(22, 0.0)?;
    wr.r(13, 0.0)?;
    wr.r(23, 0.0)?;
    wr.r(14, 0.5)?;
    wr.r(24, 0.5)?;
    wr.r(15, 0.5)?;
    wr.r(25, 0.5)?;
    wr.r(16, 0.0)?;
    wr.r(26, 0.0)?;
    wr.r(36, 1.0)?;
    wr.r(17, 0.0)?;
    wr.r(27, 0.0)?;
    wr.r(37, 0.0)?;
    wr.r(40, 1000.0)?;
    wr.r(41, 1.0)?;
    wr.r(42, 50.0)?;
    wr.r(43, 0.0)?;
    wr.r(44, 0.0)?;
    wr.r(50, 0.0)?;
    wr.r(51, 0.0)?;
    wr.i(71, 0)?;
    wr.i(72, 100)?;
    wr.i(73, 1)?;
    wr.i(74, 3)?;
    wr.i(75, 0)?;
    wr.i(76, 0)?;
    wr.i(77, 0)?;
    wr.i(78, 0)?;
    wr.s(0, "ENDTAB")?;

    // ---- LTYPE (ByBlock, ByLayer, Continuous) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "LTYPE")?;
    wr.h(5, h.ltype_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, 3)?;
    for (handle, name, desc) in [
        (h.lt_byblock, "ByBlock", ""),
        (h.lt_bylayer, "ByLayer", ""),
        (h.lt_continuous, "Continuous", "Solid line"),
    ] {
        wr.s(0, "LTYPE")?;
        wr.h(5, handle)?;
        wr.h(330, h.ltype_tbl)?;
        wr.s(100, "AcDbSymbolTableRecord")?;
        wr.s(100, "AcDbLinetypeTableRecord")?;
        wr.s(2, name)?;
        wr.i(70, 0)?;
        wr.s(3, desc)?;
        wr.i(72, 65)?;
        wr.i(73, 0)?;
        wr.r(40, 0.0)?;
    }
    wr.s(0, "ENDTAB")?;

    // ---- LAYER (all document layers with color and state) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "LAYER")?;
    wr.h(5, h.layer_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, doc.layers().count() as i64)?;
    for layer in doc.layers() {
        let (base, truecolor) = layer_color_62_420(layer.color(), report);
        let linetype = layer_linetype_name(doc, layer, report);
        let mut flags = 0i64;
        if layer.is_frozen() {
            flags |= 1; // Bit 1: frozen.
        }
        if layer.is_locked() {
            flags |= 4; // Bit 4: locked.
        }
        wr.s(0, "LAYER")?;
        wr.h(5, layer.id().raw().0)?;
        wr.h(330, h.layer_tbl)?;
        wr.s(100, "AcDbSymbolTableRecord")?;
        wr.s(100, "AcDbLayerTableRecord")?;
        wr.s(2, layer.name())?;
        wr.i(70, flags)?;
        // DXF represents an off layer with a negative color value.
        wr.i(62, if layer.is_off() { -base } else { base })?;
        wr.s(6, linetype)?;
        wr.i(290, i64::from(layer.is_plottable()))?;
        wr.i(370, layer_lineweight(layer.lineweight()))?;
        if let Some(tc) = truecolor {
            wr.i(420, tc)?;
        }
    }
    wr.s(0, "ENDTAB")?;

    // ---- STYLE (Standard) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "STYLE")?;
    wr.h(5, h.style_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, 1)?;
    wr.s(0, "STYLE")?;
    wr.h(5, h.style_std)?;
    wr.h(330, h.style_tbl)?;
    wr.s(100, "AcDbSymbolTableRecord")?;
    wr.s(100, "AcDbTextStyleTableRecord")?;
    wr.s(2, "Standard")?;
    wr.i(70, 0)?;
    wr.r(40, 0.0)?;
    wr.r(41, 1.0)?;
    wr.r(50, 0.0)?;
    wr.i(71, 0)?;
    wr.r(42, 2.5)?;
    wr.s(3, "txt")?;
    wr.s(4, "")?;
    wr.s(0, "ENDTAB")?;

    // ---- APPID (ACAD) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "APPID")?;
    wr.h(5, h.appid_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, 1)?;
    wr.s(0, "APPID")?;
    wr.h(5, h.appid_acad)?;
    wr.h(330, h.appid_tbl)?;
    wr.s(100, "AcDbSymbolTableRecord")?;
    wr.s(100, "AcDbRegAppTableRecord")?;
    wr.s(2, "ACAD")?;
    wr.i(70, 0)?;
    wr.s(0, "ENDTAB")?;

    // ---- DIMSTYLE (Standard) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "DIMSTYLE")?;
    wr.h(5, h.dimstyle_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, 1)?;
    wr.s(100, "AcDbDimStyleTable")?;
    wr.i(71, 0)?;
    wr.s(0, "DIMSTYLE")?;
    wr.tag(105, &format!("{:X}", h.dimstyle_std))?;
    wr.h(330, h.dimstyle_tbl)?;
    wr.s(100, "AcDbSymbolTableRecord")?;
    wr.s(100, "AcDbDimStyleTableRecord")?;
    wr.s(2, "Standard")?;
    wr.i(70, 0)?;
    wr.s(0, "ENDTAB")?;

    // ---- BLOCK_RECORD (*Model_Space, *Paper_Space) ----
    wr.s(0, "TABLE")?;
    wr.s(2, "BLOCK_RECORD")?;
    wr.h(5, h.br_tbl)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbSymbolTable")?;
    wr.i(70, 2)?;
    for (rec, name, layout) in [
        (h.br_model, "*Model_Space", h.layout_model),
        (h.br_paper, "*Paper_Space", h.layout_paper),
    ] {
        wr.s(0, "BLOCK_RECORD")?;
        wr.h(5, rec)?;
        wr.h(330, h.br_tbl)?;
        wr.s(100, "AcDbSymbolTableRecord")?;
        wr.s(100, "AcDbBlockTableRecord")?;
        wr.s(2, name)?;
        wr.h(340, layout)?;
    }
    wr.s(0, "ENDTAB")?;

    wr.s(0, "ENDSEC")
}

fn write_blocks<W: Write>(wr: &mut Writer<W>, h: &Handles) -> io::Result<()> {
    wr.s(0, "SECTION")?;
    wr.s(2, "BLOCKS")?;
    for (begin, end, rec, name) in [
        (h.blk_m_begin, h.blk_m_end, h.br_model, "*Model_Space"),
        (h.blk_p_begin, h.blk_p_end, h.br_paper, "*Paper_Space"),
    ] {
        wr.s(0, "BLOCK")?;
        wr.h(5, begin)?;
        wr.h(330, rec)?;
        wr.s(100, "AcDbEntity")?;
        wr.s(8, "0")?;
        wr.s(100, "AcDbBlockBegin")?;
        wr.s(2, name)?;
        wr.i(70, 0)?;
        wr.r(10, 0.0)?;
        wr.r(20, 0.0)?;
        wr.r(30, 0.0)?;
        wr.s(3, name)?;
        wr.s(1, "")?;
        wr.s(0, "ENDBLK")?;
        wr.h(5, end)?;
        wr.h(330, rec)?;
        wr.s(100, "AcDbEntity")?;
        wr.s(8, "0")?;
        wr.s(100, "AcDbBlockEnd")?;
    }
    wr.s(0, "ENDSEC")
}

fn write_entities<W: Write>(
    wr: &mut Writer<W>,
    doc: &Document,
    h: &Handles,
    report: &mut ExportReport,
) -> io::Result<()> {
    wr.s(0, "SECTION")?;
    wr.s(2, "ENTITIES")?;
    // Preserve container drawing order in ENTITIES through zero-copy `try_visit`.
    doc.model_space()
        .try_visit(|id, common, geo| -> io::Result<()> {
            let handle = id.raw().0;
            let layer_name = match doc.layer(common.layer()) {
                Some(l) => l.name(),
                None => {
                    report.warn(format!(
                        "entity {handle} references unknown layer; exported on layer 0"
                    ));
                    "0"
                }
            };
            let (c62, c420) = entity_color_62_420(common.color());

            // Keep this match exhaustive so every new model geometry must either
            // gain a DXF mapping or be reported as skipped with a warning.
            match geo {
                GeoRef::Line(g) => {
                    entity_head(
                        wr, "LINE", "AcDbLine", handle, h.br_model, layer_name, c62, c420,
                    )?;
                    wr.r(10, g.p1.x)?;
                    wr.r(20, g.p1.y)?;
                    wr.r(30, 0.0)?;
                    wr.r(11, g.p2.x)?;
                    wr.r(21, g.p2.y)?;
                    wr.r(31, 0.0)?;
                    report.bump_exported("LINE");
                }
                GeoRef::Point(g) => {
                    entity_head(
                        wr,
                        "POINT",
                        "AcDbPoint",
                        handle,
                        h.br_model,
                        layer_name,
                        c62,
                        c420,
                    )?;
                    wr.r(10, g.position.x)?;
                    wr.r(20, g.position.y)?;
                    wr.r(30, 0.0)?;
                    report.bump_exported("POINT");
                }
                GeoRef::Circle(g) => {
                    entity_head(
                        wr,
                        "CIRCLE",
                        "AcDbCircle",
                        handle,
                        h.br_model,
                        layer_name,
                        c62,
                        c420,
                    )?;
                    wr.r(10, g.center.x)?;
                    wr.r(20, g.center.y)?;
                    wr.r(30, 0.0)?;
                    wr.r(40, g.radius)?;
                    report.bump_exported("CIRCLE");
                }
                GeoRef::Polyline(g) => {
                    entity_head(
                        wr,
                        "LWPOLYLINE",
                        "AcDbPolyline",
                        handle,
                        h.br_model,
                        layer_name,
                        c62,
                        c420,
                    )?;
                    wr.i(90, g.vertices.len() as i64)?;
                    wr.i(70, i64::from(g.closed))?; // Bit 1 means closed.
                    // Group 43 carries nonzero constant width for wide polylines.
                    if g.width != 0.0 {
                        wr.r(43, g.width)?;
                    }
                    for v in &g.vertices {
                        wr.r(10, v.pt.x)?;
                        wr.r(20, v.pt.y)?;
                        // Emit nonzero dimensionless bulges without conversion.
                        if v.bulge != 0.0 {
                            wr.r(42, v.bulge)?;
                        }
                    }
                    report.bump_exported("LWPOLYLINE");
                }
                GeoRef::Arc(_) => {
                    // ARC export is not implemented yet; report it explicitly.
                    report.bump_skipped("ARC");
                    report.warn(format!(
                        "entity {handle} is an ARC; DXF ARC row deferred, skipped"
                    ));
                }
                GeoRef::Ellipse(_) => {
                    // ELLIPSE export is not implemented yet; report it explicitly.
                    report.bump_skipped("ELLIPSE");
                    report.warn(format!(
                        "entity {handle} is an ELLIPSE; DXF ELLIPSE row deferred, skipped"
                    ));
                }
                GeoRef::Xline(_) => {
                    // XLINE export is not implemented yet; report it explicitly.
                    report.bump_skipped("XLINE");
                    report.warn(format!(
                        "entity {handle} is an XLINE; DXF XLINE row deferred, skipped"
                    ));
                }
                GeoRef::Ray(_) => {
                    // RAY export is not implemented yet; report it explicitly.
                    report.bump_skipped("RAY");
                    report.warn(format!(
                        "entity {handle} is a RAY; DXF RAY row deferred, skipped"
                    ));
                }
                GeoRef::Spline(_) => {
                    // SPLINE export is not implemented yet; report it explicitly.
                    report.bump_skipped("SPLINE");
                    report.warn(format!(
                        "entity {handle} is a SPLINE; DXF SPLINE row deferred, skipped"
                    ));
                }
                GeoRef::Wipeout(_) => {
                    // WIPEOUT export is not implemented yet; report it explicitly.
                    report.bump_skipped("WIPEOUT");
                    report.warn(format!(
                        "entity {handle} is a WIPEOUT; DXF WIPEOUT row deferred, skipped"
                    ));
                }
            }
            Ok(())
        })?;
    wr.s(0, "ENDSEC")
}

/// Writes the common entity header: type, handle, owner, layer, color, and subclass.
#[allow(clippy::too_many_arguments)]
fn entity_head<W: Write>(
    wr: &mut Writer<W>,
    dxf_type: &str,
    subclass: &str,
    handle: u64,
    owner: u64,
    layer: &str,
    c62: Option<i64>,
    c420: Option<i64>,
) -> io::Result<()> {
    wr.s(0, dxf_type)?;
    wr.h(5, handle)?;
    wr.h(330, owner)?;
    wr.s(100, "AcDbEntity")?;
    wr.s(8, layer)?;
    if let Some(c) = c62 {
        wr.i(62, c)?;
    }
    if let Some(c) = c420 {
        wr.i(420, c)?;
    }
    wr.s(100, subclass)
}

fn write_objects<W: Write>(wr: &mut Writer<W>, h: &Handles) -> io::Result<()> {
    wr.s(0, "SECTION")?;
    wr.s(2, "OBJECTS")?;
    // Root named-object dictionary owned by 0.
    wr.s(0, "DICTIONARY")?;
    wr.h(5, h.dict_root)?;
    wr.i(330, 0)?;
    wr.s(100, "AcDbDictionary")?;
    wr.i(281, 1)?;
    wr.s(3, "ACAD_LAYOUT")?;
    wr.h(350, h.dict_layout)?;
    // ACAD_LAYOUT dictionary containing Model and Layout1.
    wr.s(0, "DICTIONARY")?;
    wr.h(5, h.dict_layout)?;
    wr.h(330, h.dict_root)?;
    wr.s(100, "AcDbDictionary")?;
    wr.i(281, 1)?;
    wr.s(3, "Model")?;
    wr.h(350, h.layout_model)?;
    wr.s(3, "Layout1")?;
    wr.h(350, h.layout_paper)?;
    // Model maps to Model_Space and Layout1 maps to Paper_Space.
    write_layout(wr, h.layout_model, "Model", h.br_model, 0, h.dict_layout)?;
    write_layout(wr, h.layout_paper, "Layout1", h.br_paper, 1, h.dict_layout)?;
    wr.s(0, "ENDSEC")
}

fn write_layout<W: Write>(
    wr: &mut Writer<W>,
    handle: u64,
    name: &str,
    block_record: u64,
    tab_order: i64,
    owner: u64,
) -> io::Result<()> {
    wr.s(0, "LAYOUT")?;
    wr.h(5, handle)?;
    wr.h(330, owner)?;
    // Minimal AcDbPlotSettings.
    wr.s(100, "AcDbPlotSettings")?;
    wr.s(1, "")?;
    wr.s(4, "")?;
    wr.s(6, "")?;
    for code in [40i16, 41, 42, 43, 44, 45, 46, 47, 48, 49, 140, 141] {
        wr.r(code, 0.0)?;
    }
    wr.r(142, 1.0)?;
    wr.r(143, 1.0)?;
    wr.i(70, 0)?;
    wr.i(72, 0)?;
    wr.i(73, 0)?;
    wr.i(74, 0)?;
    wr.s(7, "")?;
    wr.i(75, 0)?;
    wr.r(147, 1.0)?;
    wr.r(148, 0.0)?;
    wr.r(149, 0.0)?;
    // AcDbLayout.
    wr.s(100, "AcDbLayout")?;
    wr.s(1, name)?;
    wr.i(70, 1)?;
    wr.i(71, tab_order)?;
    wr.r(10, 0.0)?;
    wr.r(20, 0.0)?;
    wr.r(11, 420.0)?;
    wr.r(21, 297.0)?;
    wr.r(12, 0.0)?;
    wr.r(22, 0.0)?;
    wr.r(32, 0.0)?;
    wr.r(14, 0.0)?;
    wr.r(24, 0.0)?;
    wr.r(34, 0.0)?;
    wr.r(15, 0.0)?;
    wr.r(25, 0.0)?;
    wr.r(35, 0.0)?;
    wr.r(146, 0.0)?;
    wr.r(13, 0.0)?;
    wr.r(23, 0.0)?;
    wr.r(33, 0.0)?;
    wr.r(16, 1.0)?;
    wr.r(26, 0.0)?;
    wr.r(36, 0.0)?;
    wr.r(17, 0.0)?;
    wr.r(27, 1.0)?;
    wr.r(37, 0.0)?;
    wr.i(76, 0)?;
    wr.h(330, block_record)?;
    Ok(())
}
