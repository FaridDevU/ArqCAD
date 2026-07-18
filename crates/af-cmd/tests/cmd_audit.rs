//! AUDIT tests for read-only validation and reporting.

use af_cmd::CommandRegistry;
use af_cmd::builtin::register_builtins;
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::ObjectId;
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::Value;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

#[test]
fn audit_clean_document_reports_no_issues_zero_tx() {
    let (reg, mut session) = setup();
    let before = serde_json::to_string(session.document()).unwrap();
    let out = reg
        .execute(&mut session, "AUDIT", &Value::Null)
        .expect("AUDIT executes");
    assert!(out.tx_seq.is_none(), "AUDIT es view-only (0 tx)");
    assert!(
        out.message.unwrap().contains("0 issues"),
        "un documento sano no tiene issues"
    );
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    assert!(!session.can_undo());
}

#[test]
fn audit_does_not_mutate_document_with_content() {
    let (reg, mut session) = setup();
    let l0 = session.document().current_layer();
    session
        .transact("seed line", |tx| -> Result<(), TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    l0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(3.0, 4.0),
                    )),
                ),
            )?;
            Ok(())
        })
        .unwrap();
    let before = serde_json::to_string(session.document()).unwrap();

    let out = reg
        .execute(&mut session, "AUDIT", &Value::Null)
        .expect("AUDIT executes");
    assert!(out.tx_seq.is_none());
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
