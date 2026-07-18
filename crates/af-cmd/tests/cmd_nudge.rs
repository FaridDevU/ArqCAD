//! End-to-end NUDGE tests for fixed-delta movement through MOVE's atomic path.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

fn seed_line_on(session: &mut Session, layer: LayerId) -> EntityId {
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

fn seed_line(session: &mut Session) -> EntityId {
    let layer = session.document().current_layer();
    seed_line_on(session, layer)
}

#[test]
fn nudge_translates_by_delta_in_one_tx() {
    let (reg, mut session) = setup();
    let id = seed_line(&mut session);

    let out = reg
        .execute(
            &mut session,
            "NUDGE",
            &json!({ "entities": [id.raw().0], "delta": [-2.0, 3.5] }),
        )
        .expect("NUDGE executes");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");

    match &session.document().entity(id).unwrap().0.geometry {
        EntityGeometry::Line(g) => {
            assert_eq!(g.p1, Point2::new(-2.0, 3.5));
            assert_eq!(g.p2, Point2::new(-1.0, 4.5));
        }
        other => panic!("esperaba línea, fue {other:?}"),
    }
}

#[test]
fn nudge_rejects_when_entity_is_on_a_locked_layer() {
    let (reg, mut session) = setup();
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
    let id = seed_line_on(&mut session, locked_layer);
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "NUDGE",
            &json!({ "entities": [id.raw().0], "delta": [1.0, 1.0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn nudge_zero_delta_is_a_contract_violation_without_mutation() {
    // A zero delta creates no effective transaction, like MOVE with equal endpoints.
    let (reg, mut session) = setup();
    let id = seed_line(&mut session);
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "NUDGE",
            &json!({ "entities": [id.raw().0], "delta": [0.0, 0.0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn nudge_is_reversible_byte_identical() {
    let (reg, mut session) = setup();
    let id = seed_line(&mut session);
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "NUDGE",
        &json!({ "entities": [id.raw().0], "delta": [5.0, -5.0] }),
    )
    .expect("NUDGE executes");
    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
