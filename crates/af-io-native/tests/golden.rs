//! Golden `formatVersion: 1` `.arcf` preserved from the first release. Historical
//! goldens remain permanently so future migrations can keep loading and
//! validating them.
//!
//! The ignored `emit_golden_v1` test creates the file once. Normal tests only
//! read it because regeneration would change its UUID and break stability.

use std::path::PathBuf;

use af_io_native::{Recovery, load, save};
use af_math::Point2;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::ObjectId;
use af_model::units::Units;
use af_model::{ContainerRef, Session, Severity, TxError};

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/arcf/v1/sample.arcf")
}

/// Builds the golden document with one extra layer and nontrivial coordinates.
fn golden_doc() -> af_model::Document {
    let mut session = Session::new(Units::default());
    let continuous = session.document().line_types().next().unwrap().id();
    let muros = session
        .transact("layer", |tx| -> Result<_, TxError> {
            tx.add_layer_raw(af_model::Layer::new(
                ObjectId::NIL.into(),
                "Muros",
                Color::aci(1).unwrap(),
                continuous,
                Lineweight::Mm(0.5),
            ))
        })
        .unwrap()
        .value;
    session
        .transact("line", |tx| -> Result<(), TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    muros,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(1234.5678, -9.876_543_21),
                    )),
                ),
            )?;
            Ok(())
        })
        .unwrap();
    session.document().clone()
}

/// Regenerates the golden manually; ignored during normal test runs.
#[test]
#[ignore]
fn emit_golden_v1() {
    let path = golden_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save(&golden_doc(), &path).unwrap();
    eprintln!("wrote golden: {}", path.display());
}

#[test]
fn golden_v1_loads_and_roundtrips() {
    let path = golden_path();
    assert!(
        path.exists(),
        "golden ausente ({}); genéralo con `cargo test --test golden -- --ignored emit_golden_v1`",
        path.display()
    );

    let (doc, report) = load(&path).expect("el golden v1 debe cargar");
    assert_eq!(report.recovery, Recovery::Normal);
    assert!(
        !report.issues.iter().any(|i| i.severity == Severity::Error),
        "el golden no debe tener errores irrecuperables: {:?}",
        report.issues
    );

    // Expected content is present.
    assert!(doc.layer_by_name("Muros").is_some());
    assert_eq!(doc.model_space().len(), 1);

    // Saving and loading the golden again preserves the document.
    let tmp = std::env::temp_dir().join(format!("arcf-golden-rt-{}.arcf", std::process::id()));
    save(&doc, &tmp).unwrap();
    let (again, _) = load(&tmp).unwrap();
    assert_eq!(again, doc);
    let _ = std::fs::remove_file(&tmp);
}
