//! End-to-end OVERKILL tests for tolerance-equal geometry and property duplicates.

use af_cmd::CommandRegistry;
use af_cmd::builtin::register_builtins;
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

fn seed_line(session: &mut Session, p1: Point2, p2: Point2) -> EntityId {
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
                    EntityGeometry::Line(LineGeo::new(p1, p2)),
                ),
            )
        })
        .expect("seed commits")
        .value
}

#[test]
fn overkill_removes_exact_duplicates_and_reports_the_count() {
    let (reg, mut session) = setup();
    let a = seed_line(&mut session, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    let b = seed_line(&mut session, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    let c = seed_line(&mut session, Point2::new(5.0, 5.0), Point2::new(6.0, 6.0));

    let out = reg
        .execute(
            &mut session,
            "OVERKILL",
            &json!({ "entities": [a.raw().0, b.raw().0, c.raw().0] }),
        )
        .expect("OVERKILL executes");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(
        out.message.as_deref(),
        Some("OVERKILL: 1 duplicate entity removed")
    );

    assert!(session.document().entity(a).is_some());
    assert!(session.document().entity(b).is_none());
    assert!(session.document().entity(c).is_some());
}

#[test]
fn overkill_is_reversible_byte_identical() {
    let (reg, mut session) = setup();
    let a = seed_line(&mut session, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    let b = seed_line(&mut session, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "OVERKILL",
        &json!({ "entities": [a.raw().0, b.raw().0] }),
    )
    .expect("OVERKILL executes");
    assert_ne!(before, serde_json::to_string(session.document()).unwrap());

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
