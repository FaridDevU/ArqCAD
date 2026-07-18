//! End-to-end ERASE tests for success, locked-layer atomicity, exact undo, and the
//! transaction contract.

use af_cmd::builtin::erase;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    erase::register(&mut reg).expect("register ERASE");
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

fn erase_args(ids: &[EntityId]) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    json!({ "entities": raw })
}

#[test]
fn erase_removes_all_entities_in_exactly_one_tx() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))),
            EntityGeometry::Circle(CircleGeo::new(Point2::new(2.0, 2.0), 1.0)),
        ],
    );

    let out = reg
        .execute(&mut session, "E", &erase_args(&ids))
        .expect("erase succeeds (alias E)");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert!(session.document().entity(ids[0]).is_none());
    assert!(session.document().entity(ids[1]).is_none());
}

#[test]
fn erase_rejects_when_any_entity_is_on_a_locked_layer() {
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
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(2.0, 2.0),
            1.0,
        ))],
    );
    let ids = vec![free[0], held[0]];
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "ERASE", &erase_args(&ids))
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => {
            assert!(msg.contains("locked"), "mensaje: {msg}");
            assert!(msg.contains(&held[0].raw().0.to_string()));
        }
        other => panic!("esperaba Failed, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn erase_is_reversible_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(5.0, 5.0),
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(&mut session, "ERASE", &erase_args(&ids))
        .expect("erase succeeds");
    assert_ne!(before, serde_json::to_string(session.document()).unwrap());

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn erase_with_empty_set_produces_no_transaction_and_is_a_contract_violation() {
    // An empty set creates no operation and therefore violates the mutating contract.
    let reg = registry();
    let mut session = Session::new(Units::default());
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "ERASE", &erase_args(&[]))
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
