//! End-to-end SETBYLAYER tests for atomic style resets and locked-layer rejection.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    AciColor, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
};
use af_model::id::{EntityId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

fn seed_with_overrides(session: &mut Session) -> EntityId {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer,
                    Color::Aci(AciColor::new(5).unwrap()),
                    LineTypeRef::ByLayer,
                    Lineweight::Mm(0.25),
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(1.0, 1.0),
                    )),
                ),
            )
        })
        .expect("seed commits")
        .value
}

#[test]
fn setbylayer_forces_color_and_lineweight_to_bylayer_in_one_tx() {
    let (reg, mut session) = setup();
    let id = seed_with_overrides(&mut session);

    let out = reg
        .execute(
            &mut session,
            "SETBYLAYER",
            &json!({ "entities": [id.raw().0] }),
        )
        .expect("SETBYLAYER executes");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");

    let rec = session.document().entity(id).unwrap().0;
    assert_eq!(rec.color, Color::ByLayer);
    assert_eq!(rec.line_type, LineTypeRef::ByLayer);
    assert_eq!(rec.lineweight, Lineweight::ByLayer);
}

#[test]
fn setbylayer_rejects_whole_set_when_any_entity_is_locked() {
    let (reg, mut session) = setup();
    let id = seed_with_overrides(&mut session);
    let l0 = session.document().current_layer();
    session
        .transact("lock l0", |tx| -> Result<(), TxError> {
            let l = tx.doc().layer(l0).unwrap().clone().with_locked(true);
            tx.modify_layer_raw(l0, l)
        })
        .expect("lock commits");
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "SETBYLAYER",
            &json!({ "entities": [id.raw().0] }),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("locked"), "msg: {m}"),
        other => panic!("expected Failed(locked), got {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn setbylayer_is_reversible_byte_identical() {
    let (reg, mut session) = setup();
    let id = seed_with_overrides(&mut session);
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "SETBYLAYER",
        &json!({ "entities": [id.raw().0] }),
    )
    .expect("SETBYLAYER executes");
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO executes");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
