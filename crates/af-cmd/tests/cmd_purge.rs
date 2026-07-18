//! PURGE tests for unused layers, protected layers, undo, aliases, and empty results.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
use af_model::id::{LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

fn add_layer(session: &mut Session, name: &str) -> LayerId {
    session
        .transact("seed layer", |tx| -> Result<LayerId, TxError> {
            let continuous = tx.doc().line_types().next().unwrap().id();
            tx.add_layer_raw(Layer::new(
                ObjectId::NIL.into(),
                name,
                Color::aci(1).unwrap(),
                continuous,
                Lineweight::ByLayer,
            ))
        })
        .expect("layer commits")
        .value
}

fn add_point_on(session: &mut Session, layer: LayerId) {
    session
        .transact("seed point", |tx| -> Result<(), TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Point(PointGeo::new(Point2::new(0.0, 0.0))),
                ),
            )?;
            Ok(())
        })
        .expect("point commits");
}

#[test]
fn purge_removes_unused_layer_in_one_tx() {
    let (reg, mut session) = setup();
    let muros = add_layer(&mut session, "Muros"); // Unused.
    let depth = session.history().undo_depth();

    let out = reg
        .execute(&mut session, "PURGE", &Value::Null)
        .expect("PURGE executes");
    assert!(out.tx_seq.is_some(), "purgar es 1 tx");
    assert_eq!(session.history().undo_depth(), depth + 1);
    assert!(session.document().layer(muros).is_none(), "Muros purgada");
    let msg = out.message.unwrap();
    assert!(msg.contains("Muros"), "msg: {msg}");
    assert!(msg.contains("Purged 1 layer"), "msg: {msg}");
}

#[test]
fn purge_keeps_used_layer_removes_only_free_one() {
    let (reg, mut session) = setup();
    let muros = add_layer(&mut session, "Muros");
    add_point_on(&mut session, muros); // The layer is now in use.
    let vacia = add_layer(&mut session, "Vacia"); // Unused.

    let out = reg
        .execute(&mut session, "PURGE", &json!(null))
        .expect("PURGE executes");
    assert!(session.document().layer(muros).is_some(), "usada: se queda");
    assert!(session.document().layer(vacia).is_none(), "libre: purgada");
    let msg = out.message.unwrap();
    assert!(
        msg.contains("Vacia") && !msg.contains("Muros"),
        "msg: {msg}"
    );
}

#[test]
fn purge_excludes_current_layer() {
    let (reg, mut session) = setup();
    let cur = add_layer(&mut session, "Cur");
    session
        .transact("set current", |tx| -> Result<(), TxError> {
            tx.set_current_layer(cur)
        })
        .unwrap();
    let free = add_layer(&mut session, "Free"); // Unused and not current.

    reg.execute(&mut session, "PURGE", &Value::Null)
        .expect("PURGE executes");
    assert!(session.document().layer(cur).is_some(), "actual: se queda");
    assert!(session.document().layer(free).is_none(), "libre: purgada");
}

#[test]
fn purge_nothing_to_purge_is_error_zero_tx() {
    let (reg, mut session) = setup();
    let before = serde_json::to_string(session.document()).unwrap();
    let err = reg
        .execute(&mut session, "PURGE", &Value::Null)
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    assert!(!session.can_undo(), "un PURGE vacío no crea transacción");
}

#[test]
fn purge_undo_restores_layer() {
    let (reg, mut session) = setup();
    let muros = add_layer(&mut session, "Muros");
    reg.execute(&mut session, "PU", &Value::Null)
        .expect("PU alias purges");
    assert!(session.document().layer(muros).is_none());
    reg.execute(&mut session, "UNDO", &Value::Null)
        .expect("UNDO");
    assert!(
        session.document().layer_by_name("Muros").is_some(),
        "UNDO repone la capa purgada"
    );
}
