//! Integration tests for the `.lin` parser, warnings, transactional loading, and
//! document serialization.
//!
//! Tests use only the public `af_model` surface.

use af_model::{Document, Session, TxContext, TxError, parse_lin};

const FIXTURE: &str = include_str!("fixtures/arcforge_sample.lin");

#[test]
fn parses_the_arccad_fixture() {
    let parsed = parse_lin(FIXTURE);

    // Skip complex GASLINE2 and retain the final DASHED2 definition.
    let names: Vec<&str> = parsed.defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["CENTER2", "DOTDASH2", "DASHED2"]);

    let dashed2 = parsed.defs.iter().find(|d| d.name == "DASHED2").unwrap();
    assert_eq!(dashed2.pattern, vec![0.8, -0.4]);
    assert_eq!(
        dashed2.description,
        "ArcCAD dashed, redefined for the fixture"
    );

    let center2 = parsed.defs.iter().find(|d| d.name == "CENTER2").unwrap();
    assert_eq!(center2.pattern, vec![1.5, -0.3, 0.3, -0.3]);

    let dotdash2 = parsed.defs.iter().find(|d| d.name == "DOTDASH2").unwrap();
    assert_eq!(dotdash2.pattern, vec![0.0, -0.2, 0.6, -0.2]);
}

#[test]
fn fixture_warnings_cover_the_complex_segment_and_the_duplicate_name() {
    let parsed = parse_lin(FIXTURE);
    assert_eq!(parsed.warnings.len(), 2, "warnings: {:?}", parsed.warnings);

    let complex_warning = parsed
        .warnings
        .iter()
        .find(|w| w.contains("GASLINE2"))
        .expect("aviso de segmento complejo para GASLINE2");
    assert!(complex_warning.contains("complejo"));

    let dup_warning = parsed
        .warnings
        .iter()
        .find(|w| w.contains("duplicado"))
        .expect("aviso de nombre duplicado");
    assert!(dup_warning.contains("DASHED2"));
}

#[test]
fn load_linetypes_from_the_parsed_fixture_in_one_transaction() {
    let mut session = Session::new(af_model::units::Units::default());
    assert_eq!(session.document().line_types().count(), 1); // Only "Continuous".

    let parsed = parse_lin(FIXTURE);
    assert_eq!(parsed.defs.len(), 3);

    let report = session
        .transact("load .lin fixture", |tx: &mut TxContext<'_>| {
            tx.load_linetypes(parsed.defs.clone())
        })
        .expect("una tx confirmada")
        .value;

    assert_eq!(report.loaded.len(), 3);
    assert!(report.skipped_existing.is_empty());
    // The default "Continuous" plus three fixture definitions.
    assert_eq!(session.document().line_types().count(), 4);

    // Reloading the fixture skips every existing definition.
    let report2 = session
        .transact("reload .lin fixture", |tx: &mut TxContext<'_>| {
            tx.load_linetypes(parsed.defs)
        })
        .expect("una tx confirmada (0 ops: no cuenta como cambio de documento)")
        .value;
    assert!(report2.loaded.is_empty());
    assert_eq!(report2.skipped_existing.len(), 3);
    assert_eq!(session.document().line_types().count(), 4);
}

#[test]
fn document_with_loaded_linetypes_roundtrips_through_serde() {
    let mut session = Session::new(af_model::units::Units::default());
    let parsed = parse_lin(FIXTURE);
    session
        .transact("load .lin fixture", |tx: &mut TxContext<'_>| {
            tx.load_linetypes(parsed.defs)
        })
        .expect("commits");

    let json = serde_json::to_string(session.document()).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, session.document());

    // The numeric pattern survives bit for bit.
    let dashed2 = back.line_type_by_name("DASHED2").unwrap();
    assert_eq!(dashed2.pattern(), &[0.8, -0.4]);
}

#[test]
fn id_exhaustion_load_linetypes_rolls_back_the_whole_batch() {
    let doc = Document::new(af_model::units::Units::default());
    let mut value = serde_json::to_value(doc).unwrap();
    value["nextObjectId"] = serde_json::json!(u64::MAX - 1);
    let mut session = Session::from_document(serde_json::from_value(value).unwrap());
    let before = serde_json::to_string(session.document()).unwrap();
    let defs = parse_lin(FIXTURE).defs;

    let err = session
        .transact("load .lin exhaustion", |tx: &mut TxContext<'_>| {
            tx.load_linetypes(defs)
        })
        .unwrap_err();

    assert_eq!(
        err,
        TxError::Internal("persistent object id space exhausted")
    );
    assert_eq!(serde_json::to_string(session.document()).unwrap(), before);
    assert_eq!(session.document().next_object_id(), u64::MAX - 1);
    assert!(!session.can_undo());
    assert!(!session.can_redo());
}
