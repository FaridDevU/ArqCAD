//! CLAYER tests for one-transaction changes, exact undo, and unavailable-layer rejection.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

#[test]
fn clayer_sets_current_layer_in_one_tx() {
    let (reg, mut session) = setup();
    let muros = session
        .transact("seed layer", |tx| {
            let continuous = tx.doc().line_types().next().unwrap().id();
            tx.add_layer_raw(af_model::Layer::new(
                af_model::id::ObjectId::NIL.into(),
                "Muros",
                af_model::entity::Color::aci(1).unwrap(),
                continuous,
                af_model::entity::Lineweight::ByLayer,
            ))
        })
        .expect("seed commits")
        .value;
    let depth_before = session.history().undo_depth();

    reg.execute(&mut session, "CLAYER", &json!({ "layer": muros.raw().0 }))
        .expect("CLAYER executes");

    assert_eq!(session.document().current_layer(), muros);
    assert_eq!(session.history().undo_depth(), depth_before + 1);
}

#[test]
fn clayer_undo_restores_previous_current_layer() {
    let (reg, mut session) = setup();
    let l0 = session.document().current_layer();
    let muros = session
        .transact("seed layer", |tx| -> Result<_, TxError> {
            let continuous = tx.doc().line_types().next().unwrap().id();
            tx.add_layer_raw(af_model::Layer::new(
                af_model::id::ObjectId::NIL.into(),
                "Muros",
                af_model::entity::Color::aci(1).unwrap(),
                continuous,
                af_model::entity::Lineweight::ByLayer,
            ))
        })
        .expect("seed commits")
        .value;
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(&mut session, "CLAYER", &json!({ "layer": muros.raw().0 }))
        .expect("CLAYER executes");
    assert_eq!(session.document().current_layer(), muros);

    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO executes");
    assert_eq!(session.document().current_layer(), l0);
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn clayer_rejects_off_layer() {
    let (reg, mut session) = setup();
    let muros = session
        .transact("seed layer", |tx| -> Result<_, TxError> {
            let continuous = tx.doc().line_types().next().unwrap().id();
            let id = tx.add_layer_raw(af_model::Layer::new(
                af_model::id::ObjectId::NIL.into(),
                "Muros",
                af_model::entity::Color::aci(1).unwrap(),
                continuous,
                af_model::entity::Lineweight::ByLayer,
            ))?;
            let off = tx.doc().layer(id).unwrap().clone().with_off(true);
            tx.modify_layer_raw(id, off)?;
            Ok(id)
        })
        .expect("seed commits")
        .value;

    let err = reg
        .execute(&mut session, "CLAYER", &json!({ "layer": muros.raw().0 }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_ne!(session.document().current_layer(), muros);
}

#[test]
fn clayer_missing_layer_param_errors() {
    let (reg, mut session) = setup();
    assert_eq!(
        reg.execute(&mut session, "CLAYER", &json!({})).unwrap_err(),
        CmdError::MissingParam("layer".to_string())
    );
}
