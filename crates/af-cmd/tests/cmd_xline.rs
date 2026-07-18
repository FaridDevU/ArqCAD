//! End-to-end XLINE and RAY tests for variants, aliases, unit directions, style,
//! transactions, and degenerate-direction rejection.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::{Point2, Vec2};
use af_model::entity::{Color, EntityGeometry, LineTypeRef, Lineweight};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn close_vec(a: Vec2, b: Vec2) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

/// Returns the sole created geometry after checking transaction and style invariants.
fn created_geo(session: &Session, out: &af_cmd::CommandOutcome) -> EntityGeometry {
    assert_eq!(out.created.len(), 1, "crea exactamente 1 entidad");
    assert!(out.tx_seq.is_some(), "hay exactamente 1 tx");
    let id = out.created[0];
    let (rec, _) = session.document().entity(id).expect("entity exists");
    assert_eq!(rec.color, Color::ByLayer);
    assert_eq!(rec.line_type, LineTypeRef::ByLayer);
    assert_eq!(rec.lineweight, Lineweight::ByLayer);
    assert_eq!(rec.layer, session.document().current_layer());
    rec.geometry.clone()
}

fn set_current_layer_state(session: &mut Session, edit: impl FnOnce(Layer) -> Layer) {
    let id = session.document().current_layer();
    let modified = edit(session.document().layer(id).expect("layer").clone());
    session
        .transact("set layer state", |tx| -> Result<(), TxError> {
            tx.modify_layer_raw(id, modified)
        })
        .expect("commits");
}

// ---- XLINE ------------------------------------------------------------------

#[test]
fn xline_two_points_stores_unit_direction() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "p1": [1, 1], "p2": [4, 5] }),
        )
        .expect("XLINE points");
    assert_eq!(session.history().undo_depth(), 1);
    let EntityGeometry::Xline(x) = created_geo(&session, &out) else {
        panic!("se esperaba xline");
    };
    assert_eq!(x.point, Point2::new(1.0, 1.0));
    assert!(close_vec(x.direction, Vec2::new(0.6, 0.8)));
}

#[test]
fn xline_alias_xl_angle_mode() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "xl",
            &json!({ "mode": "ang", "p1": [0, 0], "angle": core::f64::consts::FRAC_PI_2 }),
        )
        .expect("XLINE ang via alias");
    let EntityGeometry::Xline(x) = created_geo(&session, &out) else {
        panic!("se esperaba xline");
    };
    assert!(close_vec(x.direction, Vec2::new(0.0, 1.0)));
}

#[test]
fn xline_hor_and_ver() {
    let (reg, mut session) = setup();
    let hor = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "mode": "hor", "p1": [3, 7] }),
        )
        .expect("XLINE hor");
    let EntityGeometry::Xline(x) = created_geo(&session, &hor) else {
        panic!("xline");
    };
    assert_eq!(x.point, Point2::new(3.0, 7.0));
    assert!(close_vec(x.direction, Vec2::new(1.0, 0.0)));

    let ver = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "mode": "ver", "p1": [3, 7] }),
        )
        .expect("XLINE ver");
    let EntityGeometry::Xline(x) = created_geo(&session, &ver) else {
        panic!("xline");
    };
    assert!(close_vec(x.direction, Vec2::new(0.0, 1.0)));
}

#[test]
fn xline_degenerate_direction_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "p1": [2, 2], "p2": [2, 2] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)), "got {err:?}");
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0);
}

#[test]
fn xline_angle_mode_missing_angle() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "mode": "ang", "p1": [0, 0] }),
        )
        .unwrap_err();
    assert_eq!(err, CmdError::MissingParam("angle".to_string()));
}

#[test]
fn xline_on_locked_layer_errors() {
    let (reg, mut session) = setup();
    set_current_layer_state(&mut session, |l| l.with_locked(true));
    let err = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "p1": [0, 0], "p2": [1, 0] }),
        )
        .unwrap_err();
    assert!(
        matches!(err, CmdError::Failed(ref m) if m.contains("locked")),
        "got {err:?}"
    );
    assert_eq!(session.document().model_space().len(), 0);
}

// ---- RAY --------------------------------------------------------------------

#[test]
fn ray_origin_through_stores_unit_direction() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "RAY",
            &json!({ "origin": [0, 0], "through": [0, 5] }),
        )
        .expect("RAY");
    let EntityGeometry::Ray(r) = created_geo(&session, &out) else {
        panic!("se esperaba ray");
    };
    assert_eq!(r.origin, Point2::new(0.0, 0.0));
    assert!(close_vec(r.direction, Vec2::new(0.0, 1.0)));
}

#[test]
fn ray_degenerate_direction_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "RAY",
            &json!({ "origin": [3, 3], "through": [3, 3] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)), "got {err:?}");
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn ray_missing_through() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "RAY", &json!({ "origin": [0, 0] }))
        .unwrap_err();
    assert_eq!(err, CmdError::MissingParam("through".to_string()));
}

#[test]
fn xline_and_ray_undo_redo_keep_id_stable() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "XLINE",
            &json!({ "p1": [0, 0], "p2": [1, 0] }),
        )
        .expect("XLINE");
    let id = out.created[0];
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO");
    assert!(session.document().entity(id).is_none());
    reg.execute(&mut session, "REDO", &serde_json::Value::Null)
        .expect("REDO");
    assert!(session.document().entity(id).is_some());
}
