//! DXF import tests for golden roundtrips, bulge angle behavior, true-color
//! precedence, unsupported entities, atomic failure, and arbitrary input.

use std::path::PathBuf;

use af_io_dxf::{ImportOptions, ImportReport, import_dxf};
use af_math::Point2;
use af_model::Session;
use af_model::entity::{Color, EntityGeometry, SegKind};
use af_model::units::{LinearUnit, Units};

// ---------------- helpers ----------------

fn export_golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/golden/dxf/export");
    p.push(name);
    p
}

fn new_session() -> Session {
    Session::new(Units {
        linear: LinearUnit::Mm,
    })
}

/// Imports DXF text into a new session and requires success.
fn import_str(dxf: &str) -> (Session, ImportReport) {
    let mut session = new_session();
    let report = import_dxf(&mut session, dxf.as_bytes(), ImportOptions::default())
        .expect("import must succeed");
    (session, report)
}

/// Imports an export golden into a new session.
fn import_golden(name: &str) -> (Session, ImportReport) {
    let bytes = std::fs::read(export_golden_path(name)).expect("golden readable");
    let mut session = new_session();
    let report = import_dxf(&mut session, &bytes[..], ImportOptions::default())
        .expect("golden import must succeed");
    (session, report)
}

/// Resolves [`Color`] to RGB for comparisons between imported and original data.
fn rgb_of(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Aci(a) => match a.get() {
            1 => (255, 0, 0),
            3 => (0, 255, 0),
            7 => (255, 255, 255),
            n => panic!("unexpected ACI {n} in test"),
        },
        other => panic!("unexpected color {other:?}"),
    }
}

fn layer_name(session: &Session, id: af_model::id::LayerId) -> String {
    session
        .document()
        .layer(id)
        .expect("entity layer resolves")
        .name()
        .to_string()
}

// ---------------- Golden roundtrip ----------------

#[test]
fn roundtrip_fixture_geometry_layers_states_and_draw_order() {
    let (session, report) = import_golden("fixture.dxf");
    let doc = session.document();

    // One entity of each supported type and nothing skipped.
    assert_eq!(report.imported.get("LINE"), Some(&1));
    assert_eq!(report.imported.get("POINT"), Some(&1));
    assert_eq!(report.imported.get("CIRCLE"), Some(&1));
    assert_eq!(report.imported.get("LWPOLYLINE"), Some(&1));
    assert_eq!(report.total_imported(), 4);
    assert_eq!(report.total_skipped(), 0);

    // Geometry and drawing order match the ENTITIES order.
    let ents: Vec<_> = doc.model_space().iter().collect();
    assert_eq!(ents.len(), 4);

    // 0: LINE (0, 0)-(10, 5), layer 0, ByLayer with no group 62.
    match &ents[0].geometry {
        EntityGeometry::Line(l) => {
            assert_eq!(l.p1, Point2::new(0.0, 0.0));
            assert_eq!(l.p2, Point2::new(10.0, 5.0));
        }
        g => panic!("expected LINE first, got {g:?}"),
    }
    assert_eq!(ents[0].color, Color::ByLayer);
    assert_eq!(layer_name(&session, ents[0].layer), "0");

    // 1: POINT (3, 4).
    match &ents[1].geometry {
        EntityGeometry::Point(p) => assert_eq!(p.position, Point2::new(3.0, 4.0)),
        g => panic!("expected POINT, got {g:?}"),
    }

    // 2: CIRCLE centered at (5, 5) with radius 2.5.
    match &ents[2].geometry {
        EntityGeometry::Circle(c) => {
            assert_eq!(c.center, Point2::new(5.0, 5.0));
            assert_eq!(c.radius, 2.5);
        }
        g => panic!("expected CIRCLE, got {g:?}"),
    }

    // 3: Closed LWPOLYLINE with a 0.4142 bulge at its second vertex.
    match &ents[3].geometry {
        EntityGeometry::Polyline(p) => {
            assert!(p.closed);
            assert_eq!(p.vertices.len(), 3);
            assert_eq!(p.vertices[0].pt, Point2::new(0.0, 0.0));
            assert_eq!(p.vertices[0].bulge, 0.0);
            assert_eq!(p.vertices[1].pt, Point2::new(10.0, 0.0));
            assert_eq!(p.vertices[1].bulge, 0.4142);
            assert_eq!(p.vertices[2].pt, Point2::new(10.0, 10.0));
            assert_eq!(p.vertices[2].bulge, 0.0);
        }
        g => panic!("expected LWPOLYLINE, got {g:?}"),
    }
    // Every entity is ByLayer on layer 0.
    for e in &ents {
        assert_eq!(e.color, Color::ByLayer);
        assert_eq!(layer_name(&session, e.layer), "0");
    }

    // Layers, resolved RGB colors, and states.
    let l0 = doc.layer_by_name("0").expect("layer 0");
    assert_eq!(rgb_of(l0.color()), (255, 255, 255)); // ACI 7 is preserved after merging.
    assert!(!l0.is_off() && !l0.is_frozen() && !l0.is_locked());

    let muros = doc.layer_by_name("Muros").expect("layer Muros");
    assert_eq!(rgb_of(muros.color()), (255, 0, 0)); // ACI 1 → Rgb
    assert!(!muros.is_off() && !muros.is_frozen() && !muros.is_locked());
    assert_eq!(muros.lineweight(), af_model::entity::Lineweight::Mm(0.25));

    let cotas = doc.layer_by_name("Cotas").expect("layer Cotas");
    assert_eq!(rgb_of(cotas.color()), (0, 255, 0)); // ACI 3 → Rgb
    assert!(cotas.is_off(), "Cotas: 62 negativo ⇒ apagada");
    assert!(cotas.is_locked(), "Cotas: flag 70 bit4 ⇒ bloqueada");
    assert!(!cotas.is_frozen());
}

#[test]
fn roundtrip_empty_is_a_no_op() {
    let (mut session, report) = import_golden("empty.dxf");
    // No entities; existing layer 0 merges into an empty transaction.
    assert_eq!(report.total_imported(), 0);
    assert_eq!(report.total_skipped(), 0);
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.document().layers().count(), 1);
    assert!(
        !session.can_undo(),
        "un import vacío no registra transacción (nada que deshacer)"
    );
    // The session remains idempotent with nothing to undo.
    let _ = &mut session;
}

// ---------------- Bulge angle behavior ----------------

#[test]
fn bulge_yields_a_radian_arc() {
    // A 90-degree arc has bulge tan(22.5 degrees); the model resolves it to pi/2.
    let bulge = (std::f64::consts::FRAC_PI_8).tan();
    let dxf = format!(
        concat!(
            "0\r\nSECTION\r\n2\r\nENTITIES\r\n",
            "0\r\nLWPOLYLINE\r\n8\r\n0\r\n90\r\n2\r\n70\r\n0\r\n",
            "10\r\n0.0\r\n20\r\n0.0\r\n42\r\n{bulge}\r\n",
            "10\r\n1.0\r\n20\r\n1.0\r\n",
            "0\r\nENDSEC\r\n0\r\nEOF\r\n"
        ),
        bulge = bulge
    );
    let (session, report) = import_str(&dxf);
    assert_eq!(report.imported.get("LWPOLYLINE"), Some(&1));

    let rec = session
        .document()
        .model_space()
        .iter()
        .next()
        .expect("one entity");
    let EntityGeometry::Polyline(p) = &rec.geometry else {
        panic!("expected polyline");
    };
    assert!(
        (p.vertices[0].bulge - bulge).abs() < 1e-12,
        "bulge verbatim"
    );
    let SegKind::Arc(arc) = p.segments().next().expect("one segment") else {
        panic!("bulge must resolve to an arc segment");
    };
    // Verify the radian sweep and unit radius.
    assert!(
        (arc.sweep().abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "sweep {} should be π/2 rad",
        arc.sweep()
    );
    assert!((arc.radius - 1.0).abs() < 1e-9);
}

// ---------------- ACI conversion and group-420 precedence ----------------

#[test]
fn layer_color_aci_to_rgb_and_truecolor_precedence() {
    // Precede has red ACI 1 but blue true color, so group 420 wins.
    // AciOnly has only ACI 3 and therefore resolves to green.
    let dxf = concat!(
        "0\r\nSECTION\r\n2\r\nTABLES\r\n",
        "0\r\nTABLE\r\n2\r\nLAYER\r\n70\r\n2\r\n",
        "0\r\nLAYER\r\n2\r\nPrecede\r\n70\r\n0\r\n62\r\n1\r\n420\r\n255\r\n6\r\nContinuous\r\n370\r\n25\r\n",
        "0\r\nLAYER\r\n2\r\nAciOnly\r\n70\r\n0\r\n62\r\n3\r\n6\r\nContinuous\r\n370\r\n25\r\n",
        "0\r\nENDTAB\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n"
    );
    let (session, _) = import_str(dxf);
    let doc = session.document();
    assert_eq!(
        doc.layer_by_name("Precede").unwrap().color(),
        Color::Rgb(0, 0, 255),
        "420 (true color) tiene precedencia sobre 62"
    );
    assert_eq!(
        doc.layer_by_name("AciOnly").unwrap().color(),
        Color::Rgb(0, 255, 0),
        "62=3 ⇒ verde por la tabla ACI"
    );
}

// ---------------- Unsupported entities ----------------

#[test]
fn unsupported_entity_is_skipped_rest_imported() {
    // Skip and warn for TEXT while still importing the valid LINE beside it.
    let dxf = concat!(
        "0\r\nSECTION\r\n2\r\nENTITIES\r\n",
        "0\r\nTEXT\r\n8\r\n0\r\n10\r\n1.0\r\n20\r\n2.0\r\n40\r\n2.5\r\n1\r\nhola\r\n",
        "0\r\nLINE\r\n8\r\n0\r\n10\r\n0.0\r\n20\r\n0.0\r\n11\r\n5.0\r\n21\r\n5.0\r\n",
        "0\r\nSPLINE\r\n8\r\n0\r\n",
        "0\r\nENDSEC\r\n0\r\nEOF\r\n"
    );
    let (session, report) = import_str(dxf);
    assert_eq!(report.imported.get("LINE"), Some(&1));
    assert_eq!(report.skipped.get("TEXT"), Some(&1));
    assert_eq!(report.skipped.get("SPLINE"), Some(&1));
    assert_eq!(report.total_imported(), 1);
    assert_eq!(session.document().model_space().len(), 1);
    assert!(
        report.warnings.iter().any(|w| w.contains("TEXT")),
        "el skip nunca es silencioso (aviso con el tipo)"
    );
}

// ---------------- Atomic failure ----------------

#[test]
fn malformed_input_is_rejected_and_leaves_document_untouched() {
    use af_io_dxf::DxfError;
    use af_model::ContainerRef;
    use af_model::TxError;
    use af_model::entity::{EntityRecord, LineTypeRef, Lineweight, PointGeo};
    use af_model::id::ObjectId;

    let mut session = new_session();
    // Seed one existing entity that the failed import must not touch.
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    session
        .transact::<(), TxError, _>("seed", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Point(PointGeo::new(Point2::new(9.0, 9.0))),
                ),
            )?;
            Ok(())
        })
        .unwrap();
    let entities_before = session.document().model_space().len();
    let seed_label = session.undo_label().map(str::to_string);

    // A non-integer code after valid content rejects the file before application.
    let bad = "0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nLINE\r\nNOT_A_CODE\r\nx\r\n";
    let result = import_dxf(&mut session, bad.as_bytes(), ImportOptions::default());
    assert!(
        matches!(result, Err(DxfError::Malformed(_))),
        "código de grupo no entero ⇒ Malformed, got {result:?}"
    );

    // Observable document and undo state remain identical.
    assert_eq!(session.document().model_space().len(), entities_before);
    assert_eq!(session.undo_label().map(str::to_string), seed_label);
    assert_ne!(session.undo_label(), Some("Import DXF"));
}

#[test]
fn oversized_input_is_rejected() {
    use af_io_dxf::DxfError;
    // One byte above 16 MiB exceeds the limit without an oversized allocation.
    let big = vec![b' '; 16 * 1024 * 1024 + 1];
    let mut session = new_session();
    let result = import_dxf(&mut session, &big[..], ImportOptions::default());
    assert!(
        matches!(result, Err(DxfError::TooLarge { .. })),
        "got {result:?}"
    );
    assert!(!session.can_undo());
}

// ---------------- Single-transaction undo ----------------

#[test]
fn import_is_a_single_undoable_transaction() {
    let (mut session, report) = import_golden("fixture.dxf");
    assert_eq!(report.total_imported(), 4);
    assert!(session.can_undo());
    assert_eq!(session.undo_label(), Some("Import DXF"));

    // One undo reverts all four entities and two new layers.
    session.undo().expect("undo the import");
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.document().layers().count(), 1);
    assert!(session.document().layer_by_name("0").is_some());
    assert!(!session.can_undo());
}

// ---------------- Property tests without panics ----------------

use proptest::prelude::*;

proptest! {
    /// Arbitrary bytes return success or error without panic or oversized allocation.
    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..8192)) {
        let mut session = new_session();
        let _ = import_dxf(&mut session, &bytes[..], ImportOptions::default());
    }

    /// Mutating and truncating a valid golden exercises deeper paths without panic.
    #[test]
    fn never_panics_on_mutated_valid_dxf(
        flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..96),
        cut in any::<prop::sample::Index>(),
    ) {
        let mut bytes = std::fs::read(export_golden_path("fixture.dxf")).unwrap();
        for (idx, b) in &flips {
            if !bytes.is_empty() {
                let i = idx.index(bytes.len());
                bytes[i] = *b;
            }
        }
        if !bytes.is_empty() {
            let c = cut.index(bytes.len());
            bytes.truncate(c);
        }
        let mut session = new_session();
        let _ = import_dxf(&mut session, &bytes[..], ImportOptions::default());
    }
}
