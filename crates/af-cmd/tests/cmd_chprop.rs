//! CHPROP tests for atomic, reversible style changes and locked-layer rejection.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::{EntityId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

/// Seeds two ByLayer lines and returns their IDs.
fn seed_two_lines(session: &mut Session) -> (EntityId, EntityId) {
    let layer = session.document().current_layer();
    let rec = |x: f64| {
        EntityRecord::new(
            ObjectId::NIL.into(),
            layer,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Line(LineGeo::new(Point2::new(x, 0.0), Point2::new(x, 1.0))),
        )
    };
    session
        .transact("seed", |tx| -> Result<(EntityId, EntityId), TxError> {
            Ok((
                tx.add_entity(ContainerRef::ModelSpace, rec(0.0))?,
                tx.add_entity(ContainerRef::ModelSpace, rec(1.0))?,
            ))
        })
        .expect("seed commits")
        .value
}

#[test]
fn chprop_color_changes_both_entities_in_one_tx() {
    let (reg, mut session) = setup();
    let (a, b) = seed_two_lines(&mut session);
    let depth_before = session.history().undo_depth();

    reg.execute(
        &mut session,
        "CHPROP",
        &json!({ "entities": [a.raw().0, b.raw().0], "prop": "color", "value": "7" }),
    )
    .expect("CHPROP executes");

    assert_eq!(session.history().undo_depth(), depth_before + 1);
    for id in [a, b] {
        assert_eq!(
            session.document().entity(id).unwrap().0.color,
            Color::aci(7).unwrap()
        );
    }
}

#[test]
fn chprop_layer_moves_entities_to_target_layer() {
    let (reg, mut session) = setup();
    let (a, b) = seed_two_lines(&mut session);
    let muros = session
        .transact("seed layer", |tx| -> Result<_, TxError> {
            let continuous = tx.doc().line_types().next().unwrap().id();
            tx.add_layer_raw(af_model::Layer::new(
                ObjectId::NIL.into(),
                "Muros",
                Color::aci(1).unwrap(),
                continuous,
                Lineweight::ByLayer,
            ))
        })
        .expect("seed commits")
        .value;

    reg.execute(
        &mut session,
        "CHPROP",
        &json!({ "entities": [a.raw().0, b.raw().0], "prop": "layer", "value": "Muros" }),
    )
    .expect("CHPROP executes");

    for id in [a, b] {
        assert_eq!(session.document().entity(id).unwrap().0.layer, muros);
    }
}

#[test]
fn chprop_lineweight_parses_mm() {
    let (reg, mut session) = setup();
    let (a, _b) = seed_two_lines(&mut session);

    reg.execute(
        &mut session,
        "CHPROP",
        &json!({ "entities": [a.raw().0], "prop": "lineweight", "value": "0.35" }),
    )
    .expect("CHPROP executes");

    assert_eq!(
        session.document().entity(a).unwrap().0.lineweight,
        Lineweight::Mm(0.35)
    );
}

#[test]
fn chprop_rejects_whole_set_when_any_entity_is_locked() {
    let (reg, mut session) = setup();
    let (a, b) = seed_two_lines(&mut session);
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
            "CHPROP",
            &json!({ "entities": [a.raw().0, b.raw().0], "prop": "color", "value": "7" }),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("locked"), "msg: {m}"),
        other => panic!("expected Failed(locked), got {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn chprop_invalid_color_value_is_rejected() {
    let (reg, mut session) = setup();
    let (a, _b) = seed_two_lines(&mut session);
    let err = reg
        .execute(
            &mut session,
            "CHPROP",
            &json!({ "entities": [a.raw().0], "prop": "color", "value": "not-a-color" }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
}

#[test]
fn chprop_is_reversible_byte_identical() {
    let (reg, mut session) = setup();
    let (a, b) = seed_two_lines(&mut session);
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "CHPROP",
        &json!({ "entities": [a.raw().0, b.raw().0], "prop": "color", "value": "byblock" }),
    )
    .expect("CHPROP executes");
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO executes");

    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
