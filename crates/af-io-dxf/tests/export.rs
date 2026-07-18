//! DXF export tests for line-by-line goldens, exact group mappings, ACI
//! approximation, `$INSUNITS`, and a valid empty document.
//!
//! Goldens live under `tests/golden/dxf/export/`. Regenerate deliberate format
//! changes with `UPDATE_GOLDEN=1 cargo test -p af-io-dxf`, then validate them
//! through `tools/gen-goldens.py --check`.

use std::path::PathBuf;

use af_io_dxf::{ExportOptions, export_dxf};
use af_math::Point2;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
    PolyVertex, PolylineGeo,
};
use af_model::id::ObjectId;
use af_model::units::{LinearUnit, Units};
use af_model::{ContainerRef, Layer, Session};

// ---------------- helpers ----------------

fn export_to_string(session: &Session) -> (String, af_io_dxf::ExportReport) {
    let mut buf: Vec<u8> = Vec::new();
    let report = export_dxf(session.document(), &mut buf, ExportOptions::default())
        .expect("export must not fail for an in-memory writer");
    (
        String::from_utf8(buf).expect("DXF ASCII is valid UTF-8"),
        report,
    )
}

/// Fixture document with every supported entity type and two styled layers.
fn fixture_session() -> Session {
    let mut session = Session::new(Units {
        linear: LinearUnit::Mm,
    });
    let continuous = session.document().line_types().next().unwrap().id();
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    let dummy_layer = Layer::new(
        ObjectId(0).into(),
        "tmp",
        Color::aci(7).unwrap(),
        continuous,
        Lineweight::Mm(0.25),
    );
    let dummy_ent = |geom| {
        EntityRecord::new(
            ObjectId(0).into(),
            layer0,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            geom,
        )
    };

    session
        .transact::<(), af_model::TxError, _>("fixture", |tx| {
            tx.add_layer_raw(
                dummy_layer
                    .clone()
                    .with_name("Muros")
                    .with_color(Color::aci(1).unwrap()),
            )?;
            tx.add_layer_raw(
                dummy_layer
                    .clone()
                    .with_name("Cotas")
                    .with_color(Color::aci(3).unwrap())
                    .with_off(true)
                    .with_locked(true),
            )?;
            tx.add_entity(
                ContainerRef::ModelSpace,
                dummy_ent(EntityGeometry::Line(LineGeo::new(
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 5.0),
                ))),
            )?;
            tx.add_entity(
                ContainerRef::ModelSpace,
                dummy_ent(EntityGeometry::Point(PointGeo::new(Point2::new(3.0, 4.0)))),
            )?;
            tx.add_entity(
                ContainerRef::ModelSpace,
                dummy_ent(EntityGeometry::Circle(CircleGeo::new(
                    Point2::new(5.0, 5.0),
                    2.5,
                ))),
            )?;
            tx.add_entity(
                ContainerRef::ModelSpace,
                dummy_ent(EntityGeometry::Polyline(PolylineGeo::new(
                    vec![
                        PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
                        PolyVertex::new(Point2::new(10.0, 0.0), 0.4142),
                        PolyVertex::new(Point2::new(10.0, 10.0), 0.0),
                    ],
                    true,
                ))),
            )?;
            Ok(())
        })
        .expect("fixture transaction commits");
    session
}

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/golden/dxf/export");
    p.push(name);
    p
}

/// Compares `actual` with `name`, rewriting it when `UPDATE_GOLDEN` is set.
/// Comparison normalizes line endings; `crlf_line_endings` verifies live output.
fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual.as_bytes()).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden {}: {e} (run with UPDATE_GOLDEN=1)",
            path.display()
        )
    });
    let exp: Vec<&str> = expected
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    let act: Vec<&str> = actual
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    for (i, (e, a)) in exp.iter().zip(act.iter()).enumerate() {
        assert_eq!(
            e,
            a,
            "golden {name} differs at line {}: expected {e:?}, got {a:?}",
            i + 1
        );
    }
    assert_eq!(
        exp.len(),
        act.len(),
        "golden {name} line count differs: expected {}, got {}",
        exp.len(),
        act.len()
    );
}

/// Returns values after each marker pair until the next group 0 or 100.
fn tags(dxf: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = dxf.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        out.push((lines[i].trim().to_string(), lines[i + 1].to_string()));
        i += 2;
    }
    out
}

/// Returns group codes for the first entity of `dxf_type`.
fn entity_tags(dxf: &str, dxf_type: &str) -> Vec<(String, String)> {
    let all = tags(dxf);
    let start = all
        .iter()
        .position(|(c, v)| c == "0" && v == dxf_type)
        .unwrap_or_else(|| panic!("no {dxf_type} entity in output"));
    let mut out = Vec::new();
    for pair in &all[start + 1..] {
        if pair.0 == "0" {
            break;
        }
        out.push(pair.clone());
    }
    out
}

// ---------------- tests ----------------

#[test]
fn fixture_matches_golden() {
    let (dxf, report) = export_to_string(&fixture_session());
    assert_golden("fixture.dxf", &dxf);
    assert_eq!(report.exported.get("LINE"), Some(&1));
    assert_eq!(report.exported.get("POINT"), Some(&1));
    assert_eq!(report.exported.get("CIRCLE"), Some(&1));
    assert_eq!(report.exported.get("LWPOLYLINE"), Some(&1));
    assert_eq!(report.total_exported(), 4);
    assert!(report.skipped.is_empty());
}

#[test]
fn empty_doc_is_valid_golden() {
    let session = Session::new(Units {
        linear: LinearUnit::Mm,
    });
    let (dxf, report) = export_to_string(&session);
    assert_golden("empty.dxf", &dxf);
    assert_eq!(report.total_exported(), 0);
    // Required minimal structure is present.
    for needle in [
        "AC1015",
        "$HANDSEED",
        "$INSUNITS",
        "$EXTMIN",
        "$EXTMAX",
        "\r\nCLASSES\r\n",
        "\r\nTABLES\r\n",
        "\r\nBLOCK_RECORD\r\n",
        "*Model_Space",
        "*Paper_Space",
        "\r\nENTITIES\r\n",
        "\r\nOBJECTS\r\n",
        "ACAD_LAYOUT",
        "\r\nEOF\r\n",
    ] {
        assert!(dxf.contains(needle), "empty DXF missing {needle:?}");
    }
    // An empty document has zero extents.
    assert!(dxf.contains("$EXTMIN\r\n10\r\n0.0\r\n20\r\n0.0\r\n"));
    assert!(dxf.contains("$EXTMAX\r\n10\r\n0.0\r\n20\r\n0.0\r\n"));
}

#[test]
fn crlf_line_endings_always() {
    let (dxf, _) = export_to_string(&fixture_session());
    assert!(dxf.contains("\r\n"), "must use CRLF");
    // Every newline must be preceded by carriage return.
    let bytes = dxf.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            assert!(i > 0 && bytes[i - 1] == b'\r', "lone LF at byte {i}");
        }
    }
}

#[test]
fn line_group_codes() {
    let (dxf, _) = export_to_string(&fixture_session());
    let t = entity_tags(&dxf, "LINE");
    // Verify hexadecimal handle, owner, subclass, and geometry.
    assert!(t.iter().any(|(c, _)| c == "5"), "LINE needs a handle (5)");
    assert!(t.contains(&("100".into(), "AcDbLine".into())));
    assert!(t.contains(&("8".into(), "0".into())), "LINE on layer 0");
    assert!(t.contains(&("10".into(), "0.0".into())));
    assert!(t.contains(&("20".into(), "0.0".into())));
    assert!(t.contains(&("11".into(), "10.0".into())));
    assert!(t.contains(&("21".into(), "5.0".into())));
    // ByLayer omits group 62.
    assert!(!t.iter().any(|(c, _)| c == "62"), "ByLayer omits 62");
}

#[test]
fn point_and_circle_group_codes() {
    let (dxf, _) = export_to_string(&fixture_session());
    let p = entity_tags(&dxf, "POINT");
    assert!(p.contains(&("100".into(), "AcDbPoint".into())));
    assert!(p.contains(&("10".into(), "3.0".into())));
    assert!(p.contains(&("20".into(), "4.0".into())));

    let c = entity_tags(&dxf, "CIRCLE");
    assert!(c.contains(&("100".into(), "AcDbCircle".into())));
    assert!(c.contains(&("10".into(), "5.0".into())));
    assert!(c.contains(&("20".into(), "5.0".into())));
    assert!(c.contains(&("40".into(), "2.5".into())), "circle radius 40");
}

#[test]
fn lwpolyline_group_codes_and_bulge_verbatim() {
    let (dxf, _) = export_to_string(&fixture_session());
    let t = entity_tags(&dxf, "LWPOLYLINE");
    assert!(t.contains(&("100".into(), "AcDbPolyline".into())));
    assert!(t.contains(&("90".into(), "3".into())), "3 vertices (90)");
    assert!(t.contains(&("70".into(), "1".into())), "closed flag (70=1)");
    // Group 42 preserves the bulge exactly.
    assert!(
        t.contains(&("42".into(), "0.4142".into())),
        "bulge 42 verbatim, got {t:?}"
    );
    // Only the curved segment emits group 42.
    assert_eq!(t.iter().filter(|(c, _)| c == "42").count(), 1);
    // All three vertices remain ordered.
    let v10: Vec<&String> = t
        .iter()
        .filter(|(c, _)| c == "10")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(v10, vec!["0.0", "10.0", "10.0"]);
}

#[test]
fn layer_states_off_and_locked() {
    let (dxf, _) = export_to_string(&fixture_session());
    let all = tags(&dxf);
    // Cotas uses negative color when off and flag-70 bit 4 when locked.
    let idx = all
        .iter()
        .position(|(c, v)| c == "2" && v == "Cotas")
        .expect("layer Cotas present");
    let window = &all[idx..idx + 6];
    assert!(
        window.contains(&("70".into(), "4".into())),
        "Cotas locked ⇒ 70=4, got {window:?}"
    );
    assert!(
        window.contains(&("62".into(), "-3".into())),
        "Cotas off ⇒ color negativo -3, got {window:?}"
    );
    // Muros uses ACI 1 and remains on and unlocked.
    let m = all
        .iter()
        .position(|(c, v)| c == "2" && v == "Muros")
        .unwrap();
    let mw = &all[m..m + 6];
    assert!(mw.contains(&("70".into(), "0".into())));
    assert!(mw.contains(&("62".into(), "1".into())));
}

#[test]
fn rgb_entity_uses_nearest_aci_and_truecolor() {
    // A non-palette RGB uses nearest ACI in 62 and exact true color in 420.
    let mut session = Session::new(Units::default());
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    session
        .transact::<(), af_model::TxError, _>("rgb", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId(0).into(),
                    layer0,
                    Color::Rgb(100, 100, 100),
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Point(PointGeo::new(Point2::new(0.0, 0.0))),
                ),
            )?;
            Ok(())
        })
        .unwrap();
    let (dxf, _) = export_to_string(&session);
    let t = entity_tags(&dxf, "POINT");
    // Gray (100, 100, 100) maps nearest to ACI 251 and exact 0x646464.
    assert!(
        t.contains(&("62".into(), "251".into())),
        "nearest ACI, got {t:?}"
    );
    assert!(
        t.contains(&("420".into(), "6579300".into())),
        "true color 420, got {t:?}"
    );
}

#[test]
fn exact_rgb_uses_aci_without_truecolor() {
    let mut session = Session::new(Units::default());
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    session
        .transact::<(), af_model::TxError, _>("red", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId(0).into(),
                    layer0,
                    Color::Rgb(255, 0, 0),
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Point(PointGeo::new(Point2::new(0.0, 0.0))),
                ),
            )?;
            Ok(())
        })
        .unwrap();
    let (dxf, _) = export_to_string(&session);
    let t = entity_tags(&dxf, "POINT");
    assert!(t.contains(&("62".into(), "1".into())), "pure red ⇒ ACI 1");
    assert!(!t.iter().any(|(c, _)| c == "420"), "exact match omits 420");
}

#[test]
fn insunits_per_unit() {
    for (unit, code) in [
        (LinearUnit::Unitless, "0"),
        (LinearUnit::In, "1"),
        (LinearUnit::Ft, "2"),
        (LinearUnit::Mm, "4"),
        (LinearUnit::Cm, "5"),
        (LinearUnit::M, "6"),
    ] {
        let session = Session::new(Units { linear: unit });
        let (dxf, _) = export_to_string(&session);
        let needle = format!("$INSUNITS\r\n70\r\n{code}\r\n");
        assert!(dxf.contains(&needle), "unit {unit:?} ⇒ {needle:?}");
    }
}

/// A wide polyline emits constant-width group 43 and preserves it through a
/// roundtrip; a zero-width polyline omits the group.
#[test]
fn lwpolyline_constant_width_group_43_roundtrip() {
    use af_io_dxf::{ImportOptions, import_dxf};

    let mut session = Session::new(Units {
        linear: LinearUnit::Mm,
    });
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    session
        .transact::<(), af_model::TxError, _>("wide", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId(0).into(),
                    layer0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Polyline(
                        PolylineGeo::new(
                            vec![
                                PolyVertex::new(Point2::new(-4.0, 0.0), 1.0),
                                PolyVertex::new(Point2::new(4.0, 0.0), 1.0),
                            ],
                            true,
                        )
                        .with_width(2.0),
                    ),
                ),
            )?;
            Ok(())
        })
        .expect("commits");

    let (dxf, _) = export_to_string(&session);
    assert!(
        dxf.contains("43\r\n2"),
        "el grosor constante se emite en el grupo 43"
    );

    // Re-import and recover the width.
    let mut back = Session::new(Units {
        linear: LinearUnit::Mm,
    });
    import_dxf(&mut back, dxf.as_bytes(), ImportOptions::default()).expect("import");
    let rec = back.document().model_space().iter().next().unwrap();
    match &rec.geometry {
        EntityGeometry::Polyline(p) => assert_eq!(p.width, 2.0, "grosor recuperado"),
        other => panic!("se esperaba Polyline, fue {other:?}"),
    }
}
