//! End-to-end SCALE tests for uniform scaling, locked layers, exact undo, invalid
//! factors, and identity-transform contract behavior.

use af_cmd::builtin::scale;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{CircleGeo, Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    scale::register(&mut reg).expect("register SCALE");
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

fn scale_args(ids: &[EntityId], base: [f64; 2], factor: f64) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    json!({ "entities": raw, "base": base, "factor": factor })
}

#[test]
fn scale_doubles_uniformly_from_base() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(2.0, 0.0),
            1.0,
        ))],
    );

    let out = reg
        .execute(&mut session, "SC", &scale_args(&ids, [0.0, 0.0], 2.0))
        .expect("scale succeeds (alias SC)");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");

    let (rec, _) = session.document().entity(ids[0]).unwrap();
    match &rec.geometry {
        EntityGeometry::Circle(g) => {
            assert_eq!(g.center, Point2::new(4.0, 0.0));
            assert_eq!(g.radius, 2.0);
        }
        other => panic!("fue {other:?}"),
    }
}

#[test]
fn scale_rejects_non_positive_factor_before_touching_the_document() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(0.0, 0.0),
            1.0,
        ))],
    );

    let err = reg
        .execute(&mut session, "SCALE", &scale_args(&ids, [0.0, 0.0], 0.0))
        .unwrap_err();
    assert!(matches!(err, CmdError::OutOfRange { .. }), "fue {err:?}");
    let err = reg
        .execute(&mut session, "SCALE", &scale_args(&ids, [0.0, 0.0], -3.0))
        .unwrap_err();
    assert!(matches!(err, CmdError::OutOfRange { .. }), "fue {err:?}");
}

#[test]
fn scale_rejects_when_any_entity_is_on_a_locked_layer() {
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
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(0.0, 0.0),
            1.0,
        ))],
    );
    let held = seed_on(
        &mut session,
        locked_layer,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(5.0, 5.0),
            1.0,
        ))],
    );
    let ids = vec![free[0], held[0]];
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "SCALE", &scale_args(&ids, [0.0, 0.0], 3.0))
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => assert!(msg.contains("locked"), "mensaje: {msg}"),
        other => panic!("esperaba Failed, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn scale_is_reversible_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(1.0, 1.0),
            1.0,
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(&mut session, "SCALE", &scale_args(&ids, [0.0, 0.0], 3.0))
        .expect("scale succeeds");
    assert_ne!(before, serde_json::to_string(session.document()).unwrap());

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn scale_factor_one_makes_no_effective_transaction() {
    // Factor one creates an identity and therefore an empty transaction.
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(1.0, 1.0),
            1.0,
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "SCALE", &scale_args(&ids, [0.0, 0.0], 1.0))
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
