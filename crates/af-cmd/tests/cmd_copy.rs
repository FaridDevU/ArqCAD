//! End-to-end COPY tests for new IDs, inherited layers, atomic rejection, undo,
//! aliases, and the transaction contract.

use af_cmd::builtin::copy;
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
    copy::register(&mut reg).expect("register COPY");
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

fn copy_args(ids: &[EntityId], from: [f64; 2], to: [f64; 2]) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    json!({ "entities": raw, "from": from, "to": to })
}

#[test]
fn copy_creates_new_monotonic_ids_inheriting_layer() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
        ))],
    );
    let source_layer = session.document().entity(ids[0]).unwrap().0.layer;

    let out = reg
        .execute(
            &mut session,
            "CO",
            &copy_args(&ids, [0.0, 0.0], [10.0, 0.0]),
        )
        .expect("copy succeeds (alias CO)");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(out.created.len(), 1);
    let new_id = out.created[0];
    assert!(new_id.raw().0 > ids[0].raw().0, "id nuevo monotónico");

    match &session.document().entity(ids[0]).unwrap().0.geometry {
        EntityGeometry::Line(g) => {
            assert_eq!(g.p1, Point2::new(0.0, 0.0));
            assert_eq!(g.p2, Point2::new(2.0, 0.0));
        }
        other => panic!("fue {other:?}"),
    }
    let (copy_rec, _) = session.document().entity(new_id).unwrap();
    assert_eq!(copy_rec.layer, source_layer);
    match &copy_rec.geometry {
        EntityGeometry::Line(g) => {
            assert_eq!(g.p1, Point2::new(10.0, 0.0));
            assert_eq!(g.p2, Point2::new(12.0, 0.0));
        }
        other => panic!("fue {other:?}"),
    }

    let reg2 = registry();
    let mut session2 = Session::new(Units::default());
    let ids2 = seed(
        &mut session2,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        ))],
    );
    reg2.execute(
        &mut session2,
        "CP",
        &copy_args(&ids2, [0.0, 0.0], [1.0, 1.0]),
    )
    .expect("copy succeeds (alias CP)");
}

#[test]
fn copy_rejects_when_any_entity_is_on_a_locked_layer() {
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
            "COPY",
            &copy_args(&ids, [0.0, 0.0], [5.0, 5.0]),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => assert!(msg.contains("locked"), "mensaje: {msg}"),
        other => panic!("esperaba Failed, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn copy_is_reversible_modulo_next_object_id() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        ))],
    );

    let out = reg
        .execute(
            &mut session,
            "COPY",
            &copy_args(&ids, [0.0, 0.0], [4.0, 4.0]),
        )
        .expect("copy succeeds");
    let new_id = out.created[0];
    assert!(session.document().entity(new_id).is_some());

    session.undo().expect("undo");
    // Undo removes the copy without rewinding the ID allocator.
    assert!(session.document().entity(new_id).is_none());
    assert!(session.document().entity(ids[0]).is_some());
}

#[test]
fn copy_with_empty_set_is_a_contract_violation() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "COPY",
            &copy_args(&[], [0.0, 0.0], [1.0, 1.0]),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
