//! End-to-end HIDEOBJECTS, ISOLATEOBJECTS, and UNISOLATEOBJECTS visibility tests.

use af_cmd::builtin::isolate;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
use af_model::id::{EntityId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    isolate::register(&mut reg).expect("register isolate commands");
    reg
}

fn seed(session: &mut Session, n: usize) -> Vec<EntityId> {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
            (0..n)
                .map(|i| {
                    tx.add_entity(
                        ContainerRef::ModelSpace,
                        EntityRecord::new(
                            ObjectId::NIL.into(),
                            layer,
                            Color::ByLayer,
                            LineTypeRef::ByLayer,
                            Lineweight::ByLayer,
                            EntityGeometry::Point(PointGeo::new(Point2::new(i as f64, 0.0))),
                        ),
                    )
                })
                .collect()
        })
        .expect("seed commits")
        .value
}

fn ids_json(ids: &[EntityId]) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    json!({ "entities": raw })
}

fn is_visible(session: &Session, id: EntityId) -> bool {
    session.document().entity(id).unwrap().0.visible
}

#[test]
fn hide_hides_only_the_selection_in_one_tx() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 3);

    let out = reg
        .execute(&mut session, "HIDEOBJECTS", &ids_json(&ids[..1]))
        .expect("hide succeeds");
    assert!(out.tx_seq.is_some());
    assert!(!is_visible(&session, ids[0]));
    assert!(is_visible(&session, ids[1]));
    assert!(is_visible(&session, ids[2]));
}

#[test]
fn hide_already_hidden_fails() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 1);
    reg.execute(&mut session, "HIDEOBJECTS", &ids_json(&ids))
        .expect("first hide");
    let err = reg
        .execute(&mut session, "HIDEOBJECTS", &ids_json(&ids))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
}

#[test]
fn hide_is_reversible_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 2);
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(&mut session, "HIDEOBJECTS", &ids_json(&ids[..1]))
        .expect("hide succeeds");
    assert_ne!(before, serde_json::to_string(session.document()).unwrap());

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn isolate_hides_everything_except_selection() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 3);

    let out = reg
        .execute(&mut session, "ISOLATEOBJECTS", &ids_json(&ids[..1]))
        .expect("isolate succeeds");
    assert!(out.tx_seq.is_some());
    assert!(is_visible(&session, ids[0]));
    assert!(!is_visible(&session, ids[1]));
    assert!(!is_visible(&session, ids[2]));
}

#[test]
fn isolate_reveals_a_hidden_member_of_the_kept_set() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 3);
    reg.execute(&mut session, "HIDEOBJECTS", &ids_json(&ids[..1]))
        .expect("hide");
    reg.execute(&mut session, "ISOLATEOBJECTS", &ids_json(&ids[..1]))
        .expect("isolate");
    assert!(is_visible(&session, ids[0]));
    assert!(!is_visible(&session, ids[1]));
    assert!(!is_visible(&session, ids[2]));
}

#[test]
fn isolate_already_isolated_fails() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 2);
    reg.execute(&mut session, "ISOLATEOBJECTS", &ids_json(&ids[..1]))
        .expect("isolate");
    let err = reg
        .execute(&mut session, "ISOLATEOBJECTS", &ids_json(&ids[..1]))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
}

#[test]
fn unisolate_shows_all_hidden_objects() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 3);
    reg.execute(&mut session, "ISOLATEOBJECTS", &ids_json(&ids[..1]))
        .expect("isolate");
    assert!(!is_visible(&session, ids[1]));

    let out = reg
        .execute(&mut session, "UNISOLATEOBJECTS", &Value::Null)
        .expect("unisolate succeeds");
    assert!(out.tx_seq.is_some());
    assert!(is_visible(&session, ids[0]));
    assert!(is_visible(&session, ids[1]));
    assert!(is_visible(&session, ids[2]));
}

#[test]
fn unisolate_with_nothing_hidden_fails() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    seed(&mut session, 2);
    let err = reg
        .execute(&mut session, "UNISOLATEOBJECTS", &Value::Null)
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
}
