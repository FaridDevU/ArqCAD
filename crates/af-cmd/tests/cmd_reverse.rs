//! End-to-end REVERSE tests for polyline traversal, bulge relocation, and type rejection.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PolyVertex, PolylineGeo,
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

fn seed_polyline(session: &mut Session, geo: PolylineGeo) -> EntityId {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Polyline(geo),
                ),
            )
        })
        .expect("seed commits")
        .value
}

#[test]
fn reverse_flips_vertex_order_and_relocates_bulges_in_one_tx() {
    let (reg, mut session) = setup();
    let poly = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.6),
            PolyVertex::new(Point2::new(4.0, 0.0), -0.3),
            PolyVertex::new(Point2::new(4.0, 4.0), 0.0),
        ],
        false,
    );
    let id = seed_polyline(&mut session, poly);

    let out = reg
        .execute(
            &mut session,
            "REVERSE",
            &json!({ "entities": [id.raw().0] }),
        )
        .expect("REVERSE executes");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");

    let EntityGeometry::Polyline(after) = &session.document().entity(id).unwrap().0.geometry else {
        panic!("esperaba polyline");
    };
    assert_eq!(after.vertices[0].pt, Point2::new(4.0, 4.0));
    assert_eq!(after.vertices[1].pt, Point2::new(4.0, 0.0));
    assert_eq!(after.vertices[2].pt, Point2::new(0.0, 0.0));
    assert_eq!(after.vertices[0].bulge, 0.3);
    assert_eq!(after.vertices[1].bulge, -0.6);
}

#[test]
fn reverse_twice_via_registry_is_identity() {
    let (reg, mut session) = setup();
    let poly = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.6),
            PolyVertex::new(Point2::new(4.0, 0.0), -0.3),
            PolyVertex::new(Point2::new(4.0, 4.0), 0.9),
        ],
        true,
    );
    let id = seed_polyline(&mut session, poly);
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "REVERSE",
        &json!({ "entities": [id.raw().0] }),
    )
    .expect("REVERSE executes");
    reg.execute(
        &mut session,
        "REVERSE",
        &json!({ "entities": [id.raw().0] }),
    )
    .expect("REVERSE executes");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn reverse_rejects_non_polyline_entities_without_mutation() {
    let (reg, mut session) = setup();
    let layer = session.document().current_layer();
    let line_id = session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(1.0, 1.0),
                    )),
                ),
            )
        })
        .expect("seed commits")
        .value;
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "REVERSE",
            &json!({ "entities": [line_id.raw().0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
