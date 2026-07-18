//! MATCHPROP tests for atomic style copying, geometry preservation, locked-layer
//! rejection, and exact undo.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

fn mk_record(layer: LayerId, color: Color, geometry: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        color,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

/// Seeds a styled source and two ByLayer targets.
fn seed(session: &mut Session) -> (LayerId, EntityId, EntityId, EntityId) {
    let l0 = session.document().current_layer();
    let continuous = session.document().line_types().next().unwrap().id();
    session
        .transact("seed", |tx| -> Result<_, TxError> {
            let muros = tx.add_layer_raw(Layer::new(
                ObjectId::NIL.into(),
                "Muros",
                Color::aci(1).unwrap(),
                continuous,
                Lineweight::ByLayer,
            ))?;
            let src = tx.add_entity(
                ContainerRef::ModelSpace,
                mk_record(
                    muros,
                    Color::aci(1).unwrap(),
                    EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 2.0)),
                ),
            )?;
            let dst1 = tx.add_entity(
                ContainerRef::ModelSpace,
                mk_record(
                    l0,
                    Color::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(5.0, 5.0),
                    )),
                ),
            )?;
            let dst2 = tx.add_entity(
                ContainerRef::ModelSpace,
                mk_record(
                    l0,
                    Color::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(1.0, 1.0),
                        Point2::new(2.0, 2.0),
                    )),
                ),
            )?;
            Ok((muros, src, dst1, dst2))
        })
        .expect("seed commits")
        .value
}

#[test]
fn matchprop_copies_style_props_not_geometry_in_one_tx() {
    let (reg, mut session) = setup();
    let (muros, src, dst1, dst2) = seed(&mut session);
    let bbox_dst1_before = session.document().entity(dst1).unwrap().0.geometry.bbox();
    let depth_before = session.history().undo_depth();

    reg.execute(
        &mut session,
        "MA",
        &json!({ "source": [src.raw().0], "targets": [dst1.raw().0, dst2.raw().0] }),
    )
    .expect("MATCHPROP executes via alias MA");

    assert_eq!(session.history().undo_depth(), depth_before + 1);
    for dst in [dst1, dst2] {
        let (rec, _) = session.document().entity(dst).unwrap();
        assert_eq!(rec.layer, muros);
        assert_eq!(rec.color, Color::aci(1).unwrap());
    }
    let bbox_dst1_after = session.document().entity(dst1).unwrap().0.geometry.bbox();
    assert_eq!(bbox_dst1_before, bbox_dst1_after);
    assert_eq!(
        session.document().entity(dst1).unwrap().0.geometry,
        EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(5.0, 5.0)))
    );
}

#[test]
fn matchprop_source_must_be_exactly_one_entity() {
    let (reg, mut session) = setup();
    let (_muros, src, dst1, dst2) = seed(&mut session);

    let err = reg
        .execute(
            &mut session,
            "MATCHPROP",
            &json!({ "source": [src.raw().0, dst1.raw().0], "targets": [dst2.raw().0] }),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("exactly 1"), "msg: {m}"),
        other => panic!("expected Failed(exactly 1), got {other:?}"),
    }
}

#[test]
fn matchprop_rejects_target_on_locked_layer_atomically() {
    let (reg, mut session) = setup();
    let (_muros, src, dst1, dst2) = seed(&mut session);
    let l0 = session.document().current_layer();
    session
        .transact("lock l0", |tx| -> Result<(), TxError> {
            let l = tx.doc().layer(l0).unwrap().clone().with_locked(true);
            tx.modify_layer_raw(l0, l)
        })
        .expect("lock commits");
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "MATCHPROP",
            &json!({ "source": [src.raw().0], "targets": [dst1.raw().0, dst2.raw().0] }),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("locked"), "msg: {m}"),
        other => panic!("expected Failed(locked), got {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn matchprop_is_reversible_byte_identical() {
    let (reg, mut session) = setup();
    let (_muros, src, dst1, dst2) = seed(&mut session);
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "MATCHPROP",
        &json!({ "source": [src.raw().0], "targets": [dst1.raw().0, dst2.raw().0] }),
    )
    .expect("MATCHPROP executes");
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO executes");

    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
