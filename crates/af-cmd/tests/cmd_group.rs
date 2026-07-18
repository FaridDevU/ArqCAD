//! End-to-end GROUP, UNGROUP, and GROUPEDIT tests.

use af_cmd::builtin::group;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
use af_model::id::{EntityId, ObjectId};
use af_model::units::Units;
use af_model::{Group, Session, TxError};
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    group::register(&mut reg).expect("register group commands");
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
    json!(raw)
}

#[test]
fn group_creates_group_in_one_tx_via_alias_g() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 3);

    let out = reg
        .execute(
            &mut session,
            "G",
            &json!({ "name": "Walls", "entities": ids_json(&ids[..2]) }),
        )
        .expect("group succeeds via alias G");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    let g = session
        .document()
        .group_by_name("walls")
        .expect("grupo creado (case-insensitive)");
    assert_eq!(g.members(), &ids[..2]);
    assert!(g.is_selectable());
}

#[test]
fn group_dedupes_repeated_members() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 2);
    let dup = json!([ids[0].raw().0, ids[1].raw().0, ids[0].raw().0]);

    reg.execute(
        &mut session,
        "GROUP",
        &json!({ "name": "G", "entities": dup }),
    )
    .expect("group succeeds");
    assert_eq!(
        session.document().group_by_name("G").unwrap().members(),
        &ids
    );
}

#[test]
fn group_rejects_duplicate_name() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 1);
    let args = json!({ "name": "G", "entities": ids_json(&ids) });
    reg.execute(&mut session, "GROUP", &args).expect("first ok");
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg.execute(&mut session, "GROUP", &args).unwrap_err();
    assert_eq!(
        err,
        CmdError::Tx(TxError::DuplicateGroupName("G".to_string()))
    );
    assert_eq!(session.document().groups().count(), 1);
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn group_empty_name_fails() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 1);
    let err = reg
        .execute(
            &mut session,
            "GROUP",
            &json!({ "name": "   ", "entities": ids_json(&ids) }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(session.document().groups().count(), 0);
}

#[test]
fn group_undo_removes_group_redo_restores_it() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 2);
    reg.execute(
        &mut session,
        "GROUP",
        &json!({ "name": "G", "entities": ids_json(&ids) }),
    )
    .expect("group succeeds");
    let gid = session.document().group_by_name("G").unwrap().id();

    // Undo removes the group without rewinding the ID allocator.
    session.undo().expect("undo");
    assert!(session.document().group(gid).is_none());
    session.redo().expect("redo");
    assert_eq!(session.document().group(gid).map(Group::name), Some("G"));
}

#[test]
fn ungroup_dissolves_group_without_deleting_entities() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 2);
    reg.execute(
        &mut session,
        "GROUP",
        &json!({ "name": "G", "entities": ids_json(&ids) }),
    )
    .expect("group succeeds");

    let out = reg
        .execute(&mut session, "UNGROUP", &json!({ "name": "G" }))
        .expect("ungroup succeeds");
    assert!(out.tx_seq.is_some());
    assert!(session.document().group_by_name("G").is_none());
    assert!(session.document().entity(ids[0]).is_some());
    assert!(session.document().entity(ids[1]).is_some());
}

#[test]
fn ungroup_unknown_name_fails() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let err = reg
        .execute(&mut session, "UNGROUP", &json!({ "name": "ghost" }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
}

#[test]
fn groupedit_adds_and_removes_members_in_one_tx() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 3);
    reg.execute(
        &mut session,
        "GROUP",
        &json!({ "name": "G", "entities": ids_json(&ids[..1]) }),
    )
    .expect("group succeeds");

    let out = reg
        .execute(
            &mut session,
            "GROUPEDIT",
            &json!({
                "name": "G",
                "add": ids_json(&ids[1..]),
                "remove": ids_json(&ids[..1]),
            }),
        )
        .expect("groupedit succeeds");
    assert!(out.tx_seq.is_some());
    assert_eq!(
        session.document().group_by_name("G").unwrap().members(),
        &ids[1..]
    );
}

#[test]
fn groupedit_no_change_fails_without_violating_contract() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, 1);
    reg.execute(
        &mut session,
        "GROUP",
        &json!({ "name": "G", "entities": ids_json(&ids) }),
    )
    .expect("group succeeds");

    // A membership no-op is a business failure rather than an empty transaction.
    let err = reg
        .execute(
            &mut session,
            "GROUPEDIT",
            &json!({ "name": "G", "add": ids_json(&ids) }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
}
