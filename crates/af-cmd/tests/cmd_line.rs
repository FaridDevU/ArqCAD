//! End-to-end LINE tests for current-layer creation, unavailable layers, undo/redo,
//! malformed JSON arguments, and model-space serialization.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, LineGeo, LineTypeRef, Lineweight};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

// ---- Helpers ----------------------------------------------------------------

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

/// Changes current-layer state for unavailable-layer tests.
fn set_current_layer_state(session: &mut Session, edit: impl FnOnce(Layer) -> Layer) {
    let id = session.document().current_layer();
    let modified = edit(
        session
            .document()
            .layer(id)
            .expect("current layer exists")
            .clone(),
    );
    session
        .transact("set layer state", |tx| -> Result<(), TxError> {
            tx.modify_layer_raw(id, modified)
        })
        .expect("layer modification commits");
}

// ---- Happy path -------------------------------------------------------------

#[test]
fn line_happy_creates_entity_on_current_layer_with_bylayer_props() {
    let (reg, mut session) = setup();
    let layer_id = session.document().current_layer();

    let out = reg
        .execute(&mut session, "LINE", &json!({ "p1": [1, 2], "p2": [4, 6] }))
        .expect("LINE executes");

    // Returns the created ID and the sole transaction sequence, with no warning.
    assert_eq!(out.created.len(), 1);
    assert!(out.tx_seq.is_some());
    assert_eq!(out.message, None);

    // Exactly one transaction is stored in history (invariant 1).
    assert_eq!(session.history().undo_depth(), 1);

    let id = out.created[0];
    let (rec, container) = session.document().entity(id).expect("entity exists");
    assert_eq!(container, ContainerRef::ModelSpace);
    assert_eq!(rec.layer, layer_id);
    assert_eq!(rec.color, Color::ByLayer);
    assert_eq!(rec.line_type, LineTypeRef::ByLayer);
    assert_eq!(rec.lineweight, Lineweight::ByLayer);
    assert!(rec.visible);
    assert_eq!(
        rec.geometry,
        EntityGeometry::Line(LineGeo::new(Point2::new(1.0, 2.0), Point2::new(4.0, 6.0)))
    );
}

/// The `L` alias also draws, case-insensitively.
#[test]
fn line_alias_l_draws() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(&mut session, "l", &json!({ "p1": [0, 0], "p2": [1, 0] }))
        .expect("alias L executes");
    assert_eq!(out.created.len(), 1);
    assert_eq!(session.document().model_space().len(), 1);
}

// ---- Zero length with warning ------------------------------------------------

#[test]
fn zero_length_line_is_allowed_with_warning() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(&mut session, "LINE", &json!({ "p1": [3, 3], "p2": [3, 3] }))
        .expect("zero-length LINE executes");

    assert_eq!(out.created.len(), 1);
    assert!(out.tx_seq.is_some());
    let msg = out.message.expect("warning message present");
    assert!(msg.to_lowercase().contains("zero"), "msg: {msg}");
    assert!(session.document().entity(out.created[0]).is_some());
}

// ---- Unavailable current layer ----------------------------------------------

#[test]
fn line_on_locked_layer_errors_and_draws_nothing() {
    let (reg, mut session) = setup();
    set_current_layer_state(&mut session, |l| l.with_locked(true));

    let err = reg
        .execute(&mut session, "LINE", &json!({ "p1": [0, 0], "p2": [1, 1] }))
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("locked"), "msg: {m}"),
        other => panic!("expected Failed(locked), got {other:?}"),
    }
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn line_on_frozen_layer_errors() {
    let (reg, mut session) = setup();
    set_current_layer_state(&mut session, |l| l.with_frozen(true));

    let err = reg
        .execute(&mut session, "LINE", &json!({ "p1": [0, 0], "p2": [1, 1] }))
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("frozen"), "msg: {m}"),
        other => panic!("expected Failed(frozen), got {other:?}"),
    }
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn line_on_off_layer_errors() {
    let (reg, mut session) = setup();
    set_current_layer_state(&mut session, |l| l.with_off(true));

    let err = reg
        .execute(&mut session, "LINE", &json!({ "p1": [0, 0], "p2": [1, 1] }))
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("off"), "msg: {m}"),
        other => panic!("expected Failed(off), got {other:?}"),
    }
    assert_eq!(session.document().model_space().len(), 0);
}

// ---- Stable-ID undo / redo ---------------------------------------------------

#[test]
fn undo_removes_line_and_redo_restores_same_id() {
    let (reg, mut session) = setup();

    let out = reg
        .execute(&mut session, "LINE", &json!({ "p1": [1, 2], "p2": [4, 6] }))
        .expect("LINE executes");
    let id = out.created[0];
    assert!(session.document().entity(id).is_some());

    reg.execute(&mut session, "UNDO", &Value::Null)
        .expect("UNDO executes");
    assert!(session.document().entity(id).is_none());
    assert_eq!(session.document().model_space().len(), 0);

    reg.execute(&mut session, "REDO", &Value::Null)
        .expect("REDO executes");
    let (rec, _) = session.document().entity(id).expect("entity restored");
    assert_eq!(rec.id, id);
    assert_eq!(session.document().model_space().len(), 1);
}

// ---- Malformed JSON arguments ------------------------------------------------

#[test]
fn malformed_args_are_rejected_by_registry() {
    let (reg, mut session) = setup();

    assert_eq!(
        reg.execute(&mut session, "LINE", &json!({ "p1": [1, 2] }))
            .unwrap_err(),
        CmdError::MissingParam("p2".to_string())
    );

    assert!(matches!(
        reg.execute(&mut session, "LINE", &json!({ "p1": "x,y", "p2": [0, 0] }))
            .unwrap_err(),
        CmdError::TypeMismatch { .. }
    ));

    assert!(matches!(
        reg.execute(
            &mut session,
            "LINE",
            &json!({ "p1": [0, 0, 0], "p2": [1, 1] })
        )
        .unwrap_err(),
        CmdError::TypeMismatch { .. }
    ));

    assert_eq!(
        reg.execute(
            &mut session,
            "LINE",
            &json!({ "p1": [0, 0], "p2": [1, 1], "bogus": 3 })
        )
        .unwrap_err(),
        CmdError::UnknownParam("bogus".to_string())
    );

    assert_eq!(session.document().model_space().len(), 0);
}

// ---- Model-space serialization ----------------------------------------------

#[test]
fn model_space_serialization_golden_after_create() {
    let (reg, mut session) = setup();
    let layer_id = session.document().current_layer();

    let out = reg
        .execute(&mut session, "LINE", &json!({ "p1": [1, 2], "p2": [4, 6] }))
        .expect("LINE executes");
    let id = out.created[0];

    // IDs are opaque; the golden fixes record shape and property serialization.
    let json = serde_json::to_string(session.document().model_space()).unwrap();
    let expected = format!(
        r#"[{{"id":{id},"layer":{layer},"color":"byLayer","lineType":"byLayer","lineweight":"byLayer","visible":true,"geometry":{{"type":"line","p1":[1.0,2.0],"p2":[4.0,6.0]}}}}]"#,
        id = id.raw().0,
        layer = layer_id.raw().0,
    );
    assert_eq!(json, expected);
}
