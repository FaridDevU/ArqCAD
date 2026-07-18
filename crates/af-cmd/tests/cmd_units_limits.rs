//! UNITS and LIMITS tests for one-transaction, undoable document-property changes.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::Limits;
use af_model::Session;
use af_model::units::Units;
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

#[test]
fn units_sets_linear_precision_in_one_tx() {
    let (reg, mut session) = setup();
    assert_eq!(session.document().linear_precision(), 4);
    let depth = session.history().undo_depth();

    let out = reg
        .execute(&mut session, "UNITS", &json!({ "precision": 2 }))
        .expect("UNITS executes");
    assert!(out.tx_seq.is_some(), "UNITS con cambio real es 1 tx");
    assert_eq!(session.document().linear_precision(), 2);
    assert_eq!(session.history().undo_depth(), depth + 1);
    let msg = out.message.unwrap();
    assert!(msg.contains("Precision: 2 decimals"), "msg: {msg}");
    assert!(msg.contains("Linear units: mm"), "msg: {msg}");
}

#[test]
fn units_undo_restores_precision_byte_identical() {
    let (reg, mut session) = setup();
    let before = serde_json::to_string(session.document()).unwrap();
    reg.execute(&mut session, "UNITS", &json!({ "precision": 6 }))
        .expect("UNITS");
    assert_eq!(session.document().linear_precision(), 6);
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO");
    assert_eq!(session.document().linear_precision(), 4);
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn units_rejects_out_of_range_precision() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "UNITS", &json!({ "precision": 9 }))
        .unwrap_err();
    assert!(matches!(err, CmdError::OutOfRange { .. }));
    assert_eq!(session.document().linear_precision(), 4, "sin cambio");
    assert!(!session.can_undo());
}

#[test]
fn units_missing_precision_errors() {
    let (reg, mut session) = setup();
    assert_eq!(
        reg.execute(&mut session, "UNITS", &json!({})).unwrap_err(),
        CmdError::MissingParam("precision".to_string())
    );
}

#[test]
fn units_dash_alias_works() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "-UNITS", &json!({ "precision": 3 }))
        .expect("-UNITS alias");
    assert_eq!(session.document().linear_precision(), 3);
}

#[test]
fn limits_sets_min_max_in_one_tx() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "LIMITS",
            &json!({ "min": [-5.0, -5.0], "max": [100.0, 80.0] }),
        )
        .expect("LIMITS executes");
    assert!(out.tx_seq.is_some(), "LIMITS con cambio real es 1 tx");
    assert_eq!(
        session.document().limits(),
        Limits {
            min: Point2::new(-5.0, -5.0),
            max: Point2::new(100.0, 80.0),
        }
    );
    let msg = out.message.unwrap();
    assert!(msg.contains("100.0000"), "msg: {msg}");
    assert!(msg.contains("-5.0000"), "msg: {msg}");
}

#[test]
fn limits_undo_restores_default() {
    let (reg, mut session) = setup();
    let default = session.document().limits();
    let before = serde_json::to_string(session.document()).unwrap();
    reg.execute(
        &mut session,
        "LIMITS",
        &json!({ "min": [1.0, 1.0], "max": [50.0, 50.0] }),
    )
    .expect("LIMITS");
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO");
    assert_eq!(session.document().limits(), default);
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn limits_missing_max_errors() {
    let (reg, mut session) = setup();
    assert_eq!(
        reg.execute(&mut session, "LIMITS", &json!({ "min": [0.0, 0.0] }))
            .unwrap_err(),
        CmdError::MissingParam("max".to_string())
    );
}
