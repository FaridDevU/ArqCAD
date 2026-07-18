//! End-to-end ADDSELECTED tests for inherited type/properties and point-defined geometry.

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

fn seed_line(session: &mut Session) -> EntityId {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer,
                    Color::aci(4).unwrap(),
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

#[test]
fn addselected_creates_a_new_line_with_the_same_color_in_one_tx() {
    let (reg, mut session) = setup();
    let reference = seed_line(&mut session);

    let out = reg
        .execute(
            &mut session,
            "ADDSELECTED",
            &json!({
                "reference": [reference.raw().0],
                "points": [{"pt": [5.0, 5.0]}, {"pt": [7.0, 9.0]}],
            }),
        )
        .expect("ADDSELECTED executes");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(out.created.len(), 1);
    let new_id = out.created[0];
    assert_ne!(new_id, reference, "ids nunca se reciclan");

    let rec = session.document().entity(new_id).unwrap().0;
    assert_eq!(rec.color, Color::aci(4).unwrap());
    match &rec.geometry {
        EntityGeometry::Line(g) => {
            assert_eq!(g.p1, Point2::new(5.0, 5.0));
            assert_eq!(g.p2, Point2::new(7.0, 9.0));
        }
        other => panic!("esperaba línea, fue {other:?}"),
    }
}

#[test]
fn addselected_wrong_point_count_is_a_contract_error_without_mutation() {
    let (reg, mut session) = setup();
    let reference = seed_line(&mut session);
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "ADDSELECTED",
            &json!({
                "reference": [reference.raw().0],
                "points": [{"pt": [5.0, 5.0]}],
            }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn addselected_reference_with_more_than_one_id_is_rejected() {
    let (reg, mut session) = setup();
    let a = seed_line(&mut session);
    let b = seed_line(&mut session);

    let err = reg
        .execute(
            &mut session,
            "ADDSELECTED",
            &json!({
                "reference": [a.raw().0, b.raw().0],
                "points": [{"pt": [5.0, 5.0]}, {"pt": [6.0, 6.0]}],
            }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
}

#[test]
fn addselected_is_reversible_byte_identical_modulo_next_object_id() {
    let (reg, mut session) = setup();
    let reference = seed_line(&mut session);

    reg.execute(
        &mut session,
        "ADDSELECTED",
        &json!({
            "reference": [reference.raw().0],
            "points": [{"pt": [5.0, 5.0]}, {"pt": [7.0, 9.0]}],
        }),
    )
    .expect("ADDSELECTED executes");
    let after_add = serde_json::to_string(session.document()).unwrap();

    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO executes");
    reg.execute(&mut session, "REDO", &serde_json::Value::Null)
        .expect("REDO executes");
    assert_eq!(
        after_add,
        serde_json::to_string(session.document()).unwrap()
    );
}
