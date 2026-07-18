//! End-to-end MIRROR tests for source retention/removal, invalid axes, atomic
//! rejection, exact undo, and the transaction contract.

use af_cmd::builtin::mirror;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    mirror::register(&mut reg).expect("register MIRROR");
    reg
}

fn mk_record(layer: LayerId, geometry: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

fn seed_on(session: &mut Session, layer: LayerId, geoms: Vec<EntityGeometry>) -> Vec<EntityId> {
    session
        .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
            geoms
                .into_iter()
                .map(|g| tx.add_entity(ContainerRef::ModelSpace, mk_record(layer, g)))
                .collect()
        })
        .expect("seed commits")
        .value
}

fn seed(session: &mut Session, geoms: Vec<EntityGeometry>) -> Vec<EntityId> {
    let layer = session.document().current_layer();
    seed_on(session, layer, geoms)
}

fn mirror_args(ids: &[EntityId], p1: [f64; 2], p2: [f64; 2], erase_source: Option<bool>) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    let mut v = json!({ "entities": raw, "p1": p1, "p2": p2 });
    if let Some(b) = erase_source {
        v["erase_source"] = json!(b);
    }
    v
}

#[test]
fn mirror_default_keeps_source_and_creates_reflected_copy() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(3.0, 0.0),
        ))],
    );

    let out = reg
        .execute(
            &mut session,
            "MI",
            &mirror_args(&ids, [0.0, 0.0], [0.0, 1.0], None),
        )
        .expect("mirror succeeds (alias MI)");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(out.created.len(), 1);
    let new_id = out.created[0];

    assert!(session.document().entity(ids[0]).is_some());
    let (rec, _) = session.document().entity(new_id).unwrap();
    match &rec.geometry {
        EntityGeometry::Line(g) => {
            assert_eq!(g.p1, Point2::new(-1.0, 0.0));
            assert_eq!(g.p2, Point2::new(-3.0, 0.0));
        }
        other => panic!("fue {other:?}"),
    }
}

#[test]
fn mirror_erase_source_true_removes_the_original_in_the_same_tx() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(3.0, 0.0),
        ))],
    );

    let out = reg
        .execute(
            &mut session,
            "MIRROR",
            &mirror_args(&ids, [0.0, 0.0], [0.0, 1.0], Some(true)),
        )
        .expect("mirror succeeds");
    assert!(out.tx_seq.is_some());
    assert_eq!(out.created.len(), 1);
    assert!(
        session.document().entity(ids[0]).is_none(),
        "erase_source=true borra la fuente"
    );
    assert!(session.document().entity(out.created[0]).is_some());
}

#[test]
fn mirror_rejects_degenerate_axis_without_touching_the_document() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(3.0, 0.0),
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "MIRROR",
            &mirror_args(&ids, [2.0, 2.0], [2.0, 2.0], None),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => assert!(msg.contains("p1 == p2"), "mensaje: {msg}"),
        other => panic!("esperaba Failed, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn mirror_rejects_when_any_entity_is_on_a_locked_layer() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let lt = session.document().line_types().next().unwrap().id();
    let locked_layer = session
        .transact("mk locked layer", |tx| -> Result<LayerId, TxError> {
            tx.add_layer_raw(
                Layer::new(
                    ObjectId::NIL.into(),
                    "Locked",
                    Color::ByLayer,
                    lt,
                    Lineweight::ByLayer,
                )
                .with_locked(true),
            )
        })
        .expect("commits")
        .value;
    let l0 = session.document().current_layer();

    let free = seed_on(
        &mut session,
        l0,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        ))],
    );
    let held = seed_on(
        &mut session,
        locked_layer,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(2.0, 2.0),
            Point2::new(3.0, 3.0),
        ))],
    );
    let ids = vec![free[0], held[0]];
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "MIRROR",
            &mirror_args(&ids, [0.0, 0.0], [0.0, 1.0], None),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => assert!(msg.contains("locked"), "mensaje: {msg}"),
        other => panic!("esperaba Failed, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn mirror_is_reversible_modulo_next_object_id() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(3.0, 0.0),
        ))],
    );

    let out = reg
        .execute(
            &mut session,
            "MIRROR",
            &mirror_args(&ids, [0.0, 0.0], [0.0, 1.0], None),
        )
        .expect("mirror succeeds");
    let new_id = out.created[0];

    session.undo().expect("undo");
    // Undo removes the copy without rewinding the ID allocator.
    assert!(session.document().entity(new_id).is_none());
    assert!(session.document().entity(ids[0]).is_some());
}

#[test]
fn mirror_with_empty_set_is_a_contract_violation() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "MIRROR",
            &mirror_args(&[], [0.0, 0.0], [0.0, 1.0], None),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
