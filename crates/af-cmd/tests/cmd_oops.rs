//! End-to-end OOPS tests for history-based ERASE restoration with new IDs.

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

fn seed_line(session: &mut Session, x: f64) -> EntityId {
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
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(x, 0.0),
                        Point2::new(x + 1.0, 1.0),
                    )),
                ),
            )
        })
        .expect("seed commits")
        .value
}

#[test]
fn oops_restores_the_last_erase_with_new_ids_in_one_tx() {
    let (reg, mut session) = setup();
    let a = seed_line(&mut session, 0.0);
    let b = seed_line(&mut session, 10.0);

    reg.execute(
        &mut session,
        "ERASE",
        &json!({ "entities": [a.raw().0, b.raw().0] }),
    )
    .expect("ERASE executes");
    assert!(session.document().entity(a).is_none());
    assert!(session.document().entity(b).is_none());

    let out = reg
        .execute(&mut session, "OOPS", &serde_json::Value::Null)
        .expect("OOPS executes");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(out.created.len(), 2);
    for (new_id, old_id) in out.created.iter().zip([a, b].iter()) {
        assert_ne!(new_id, old_id, "ids nunca se reciclan");
        assert!(session.document().entity(*new_id).is_some());
    }
}

#[test]
fn oops_finds_the_erase_even_after_a_later_unrelated_command() {
    let (reg, mut session) = setup();
    let a = seed_line(&mut session, 0.0);

    reg.execute(&mut session, "ERASE", &json!({ "entities": [a.raw().0] }))
        .expect("ERASE executes");
    let extra = seed_line(&mut session, 20.0);

    let out = reg
        .execute(&mut session, "OOPS", &serde_json::Value::Null)
        .expect("OOPS finds the Erase despite the later command");
    assert_eq!(out.created.len(), 1);
    assert!(session.document().entity(extra).is_some());
}

#[test]
fn oops_without_a_prior_erase_fails_without_a_transaction() {
    let (reg, mut session) = setup();
    seed_line(&mut session, 0.0);
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "OOPS", &serde_json::Value::Null)
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
