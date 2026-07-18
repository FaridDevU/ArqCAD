//! End-to-end ROTATE tests for counterclockwise radians, locked layers, exact undo,
//! and identity-transform contract behavior.

use af_cmd::builtin::rotate;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};
use std::f64::consts::FRAC_PI_2;

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    rotate::register(&mut reg).expect("register ROTATE");
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

fn rotate_args(ids: &[EntityId], base: [f64; 2], angle: f64) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    json!({ "entities": raw, "base": base, "angle": angle })
}

#[test]
fn rotate_quarter_turn_about_base_ccw() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 0.0),
        ))],
    );

    let out = reg
        .execute(
            &mut session,
            "RO",
            &rotate_args(&ids, [0.0, 0.0], FRAC_PI_2),
        )
        .expect("rotate succeeds (alias RO)");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert!(out.created.is_empty(), "ROTATE no crea entidades");

    let (rec, _) = session.document().entity(ids[0]).unwrap();
    match &rec.geometry {
        EntityGeometry::Line(g) => {
            let tol = 1e-9;
            assert!((g.p1.x - 0.0).abs() < tol && (g.p1.y - 1.0).abs() < tol);
            assert!((g.p2.x - 0.0).abs() < tol && (g.p2.y - 2.0).abs() < tol);
        }
        other => panic!("fue {other:?}"),
    }
}

#[test]
fn rotate_rejects_when_any_entity_is_on_a_locked_layer() {
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
            "ROTATE",
            &rotate_args(&ids, [0.0, 0.0], FRAC_PI_2),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => assert!(msg.contains("locked"), "mensaje: {msg}"),
        other => panic!("esperaba Failed, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn rotate_is_reversible_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 0.0),
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(&mut session, "ROTATE", &rotate_args(&ids, [0.0, 0.0], 0.7))
        .expect("rotate succeeds");
    assert_ne!(before, serde_json::to_string(session.document()).unwrap());

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn rotate_zero_angle_makes_no_effective_transaction() {
    // A zero angle creates an exact identity and therefore an empty transaction.
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 0.0),
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "ROTATE", &rotate_args(&ids, [0.0, 0.0], 0.0))
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
