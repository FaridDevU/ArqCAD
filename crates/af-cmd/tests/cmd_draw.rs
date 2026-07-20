//! End-to-end drawing-command tests for variants, aliases, invalid geometry, and
//! the one-transaction contract.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_math::angle::{angle_in_sweep, angle_of};
use af_model::entity::{
    ArcGeo, CircleGeo, Color, EllipseGeo, EntityGeometry, LineTypeRef, Lineweight, PointGeo,
};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

// ---- Helpers ----------------------------------------------------------------

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn close_pt(a: Point2, b: Point2) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

/// Returns the sole created geometry after checking transaction and style invariants.
fn created_geo(session: &Session, out: &af_cmd::CommandOutcome) -> EntityGeometry {
    assert_eq!(out.created.len(), 1, "un comando de dibujo crea 1 entidad");
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

// ---- CIRCLE -----------------------------------------------------------------

#[test]
fn circle_center_radius_default_mode() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "center": [2, 3], "radius": 5 }),
        )
        .expect("CIRCLE center");
    assert_eq!(session.history().undo_depth(), 1);
    assert_eq!(
        created_geo(&session, &out),
        EntityGeometry::Circle(CircleGeo::new(Point2::new(2.0, 3.0), 5.0))
    );
}

#[test]
fn circle_center_diameter() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "center": [2, 3], "diameter": 10 }),
        )
        .expect("CIRCLE center-diameter");
    assert_eq!(
        created_geo(&session, &out),
        EntityGeometry::Circle(CircleGeo::new(Point2::new(2.0, 3.0), 5.0))
    );
}

#[test]
fn circle_alias_c_two_points_diameter() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "c",
            &json!({
                "mode": "2p",
                "p1": [0, 0],
                "p2": [4, 0],
                "radius": 99,
                "diameter": 88,
            }),
        )
        .expect("CIRCLE 2p via alias");
    assert_eq!(
        created_geo(&session, &out),
        EntityGeometry::Circle(CircleGeo::new(Point2::new(2.0, 0.0), 2.0))
    );
}

#[test]
fn circle_three_points_circumcircle() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "mode": "3p", "p1": [1, 0], "p2": [0, 1], "p3": [-1, 0] }),
        )
        .expect("CIRCLE 3p");
    let EntityGeometry::Circle(c) = created_geo(&session, &out) else {
        panic!("se esperaba círculo");
    };
    assert!(close_pt(c.center, Point2::ORIGIN));
    assert!(close(c.radius, 1.0));
}

#[test]
fn circle_three_points_collinear_errors_and_draws_nothing() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "mode": "3p", "p1": [0, 0], "p2": [1, 0], "p3": [2, 0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("colineal")));
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0);
}

#[test]
fn circle_zero_radius_two_points_is_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "mode": "2p", "p1": [3, 3], "p2": [3, 3] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)), "got {err:?}");
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn circle_missing_radius_in_center_mode() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "CIRCLE", &json!({ "center": [0, 0] }))
        .unwrap_err();
    assert_eq!(err, CmdError::MissingParam("radius".to_string()));
}

#[test]
fn circle_center_radius_and_diameter_conflict_is_fail_closed() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "center": [0, 0], "radius": 1, "diameter": 2 }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0);
}

#[test]
fn circle_center_invalid_diameter_is_rejected_before_tx() {
    let (reg, mut session) = setup();
    for diameter in [0.0, -1.0] {
        let err = reg
            .execute(
                &mut session,
                "CIRCLE",
                &json!({ "center": [0, 0], "diameter": diameter }),
            )
            .unwrap_err();
        assert!(
            matches!(&err, CmdError::OutOfRange { param, .. } if param == "diameter"),
            "got {err:?}"
        );
    }
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0);
}

// ---- ARC --------------------------------------------------------------------

#[test]
fn arc_three_points_upper_semicircle() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "ARC",
            &json!({ "p1": [1, 0], "p2": [0, 1], "p3": [-1, 0] }),
        )
        .expect("ARC 3p");
    let EntityGeometry::Arc(a) = created_geo(&session, &out) else {
        panic!("se esperaba arco");
    };
    assert!(close_pt(a.center, Point2::ORIGIN) && close(a.radius, 1.0));
    assert!(angle_in_sweep(
        angle_of(Point2::new(0.0, 1.0) - a.center),
        a.start_angle,
        a.end_angle
    ));
}

#[test]
fn arc_three_points_swaps_to_keep_ccw_through_midpoint() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "a",
            &json!({ "p1": [1, 0], "p2": [0, -1], "p3": [-1, 0] }),
        )
        .expect("ARC 3p via alias A");
    let EntityGeometry::Arc(a) = created_geo(&session, &out) else {
        panic!("se esperaba arco");
    };
    let mid = angle_of(Point2::new(0.0, -1.0) - a.center);
    assert!(angle_in_sweep(mid, a.start_angle, a.end_angle));
}

#[test]
fn arc_three_points_collinear_is_fail_closed() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "ARC",
            &json!({ "p1": [0, 0], "p2": [1, 0], "p3": [2, 0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("colineal")));
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0);
}

#[test]
fn arc_center_start_end_point() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "ARC",
            &json!({ "mode": "cse", "center": [0, 0], "start": [1, 0], "end": [0, 1] }),
        )
        .expect("ARC cse");
    let EntityGeometry::Arc(a) = created_geo(&session, &out) else {
        panic!("se esperaba arco");
    };
    assert!(close_pt(a.center, Point2::ORIGIN) && close(a.radius, 1.0));
    assert!(close(a.start_angle, 0.0));
    assert!(close(a.end_angle, core::f64::consts::FRAC_PI_2));
}

#[test]
fn arc_center_start_end_angle() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "ARC",
            &json!({ "mode": "cse", "center": [0, 0], "start": [2, 0], "endAngle": core::f64::consts::PI }),
        )
        .expect("ARC cse endAngle");
    let EntityGeometry::Arc(a) = created_geo(&session, &out) else {
        panic!("se esperaba arco");
    };
    assert!(close(a.radius, 2.0));
    assert!(close(a.start_angle, 0.0) && close(a.end_angle, core::f64::consts::PI));
    assert_eq!(
        a,
        ArcGeo::new(Point2::ORIGIN, 2.0, 0.0, core::f64::consts::PI)
    );
}

#[test]
fn arc_cse_end_precedence_and_ccw_sweep_edges() {
    let (reg, mut session) = setup();
    let precedence = reg
        .execute(
            &mut session,
            "ARC",
            &json!({
                "mode": "cse",
                "center": [0, 0],
                "start": [1, 0],
                "end": [0, 1],
                "endAngle": core::f64::consts::PI,
            }),
        )
        .expect("ARC end point precedes endAngle");
    let EntityGeometry::Arc(precedence) = created_geo(&session, &precedence) else {
        panic!("se esperaba arco");
    };
    assert!(close(precedence.end_angle, core::f64::consts::FRAC_PI_2));

    for (start, want) in [
        ([1.0, 0.0], core::f64::consts::TAU),
        ([0.0, 1.0], 3.0 * core::f64::consts::FRAC_PI_2),
    ] {
        let out = reg
            .execute(
                &mut session,
                "ARC",
                &json!({ "mode": "cse", "center": [0, 0], "start": start, "endAngle": 0 }),
            )
            .expect("ARC CCW sweep edge");
        let EntityGeometry::Arc(a) = created_geo(&session, &out) else {
            panic!("se esperaba arco");
        };
        assert!(close(a.sweep(), want));
    }
}

#[test]
fn arc_cse_zero_radius_is_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "ARC",
            &json!({ "mode": "cse", "center": [1, 1], "start": [1, 1], "endAngle": 1 }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)), "got {err:?}");
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0);
}

#[test]
fn arc_cse_missing_end_errors() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "ARC",
            &json!({ "mode": "cse", "center": [0, 0], "start": [1, 0] }),
        )
        .unwrap_err();
    assert_eq!(err, CmdError::MissingParam("end".to_string()));
}

// ---- ELLIPSE ----------------------------------------------------------------

#[test]
fn ellipse_center_mode_full() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "ELLIPSE",
            &json!({ "center": [0, 0], "axisEnd": [3, 0], "ratio": 0.5 }),
        )
        .expect("ELLIPSE center");
    assert_eq!(session.history().undo_depth(), 1);
    assert_eq!(
        created_geo(&session, &out),
        EntityGeometry::Ellipse(EllipseGeo::new(
            Point2::ORIGIN,
            3.0,
            0.5,
            0.0,
            0.0,
            core::f64::consts::TAU,
        ))
    );
}

#[test]
fn ellipse_center_mode_rotated_axis() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "ELLIPSE",
            &json!({ "center": [1, 1], "axisEnd": [1, 5], "ratio": 0.25 }),
        )
        .expect("ELLIPSE center rotated");
    let EntityGeometry::Ellipse(e) = created_geo(&session, &out) else {
        panic!("se esperaba elipse");
    };
    assert!(close(e.semi_major, 4.0));
    assert!(close(e.ratio, 0.25));
    assert!(close(e.rotation, core::f64::consts::FRAC_PI_2));
    assert!(close_pt(e.center, Point2::new(1.0, 1.0)));
}

#[test]
fn ellipse_alias_el_arc_mode() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "EL",
            &json!({
                "mode": "arc",
                "center": [0, 0],
                "axisEnd": [5, 0],
                "ratio": 0.6,
                "startParam": 0.5,
                "endParam": 2.0,
            }),
        )
        .expect("ELLIPSE arc via alias EL");
    let EntityGeometry::Ellipse(e) = created_geo(&session, &out) else {
        panic!("se esperaba elipse");
    };
    assert!(close(e.semi_major, 5.0));
    assert!(close(e.ratio, 0.6));
    assert!(close(e.start_param, 0.5) && close(e.end_param, 2.0));
}

#[test]
fn ellipse_degenerate_axis_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "ELLIPSE",
            &json!({ "center": [2, 2], "axisEnd": [2, 2], "ratio": 0.5 }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)));
    assert_eq!(session.history().undo_depth(), 0);
}

#[test]
fn ellipse_arc_missing_param_errors() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "ELLIPSE",
            &json!({ "mode": "arc", "center": [0, 0], "axisEnd": [4, 0], "ratio": 0.5, "startParam": 0.0 }),
        )
        .unwrap_err();
    assert_eq!(err, CmdError::MissingParam("endParam".to_string()));
}

#[test]
fn ellipse_ratio_one_is_valid_and_above_one_is_out_of_range() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "ELLIPSE",
            &json!({ "center": [0, 0], "axisEnd": [3, 0], "ratio": 1 }),
        )
        .expect("ratio 1 is a circle-shaped ellipse");
    let EntityGeometry::Ellipse(e) = created_geo(&session, &out) else {
        panic!("se esperaba elipse");
    };
    assert!(close(e.ratio, 1.0));

    let entities = session.document().model_space().len();
    let history = session.history().undo_depth();
    let err = reg
        .execute(
            &mut session,
            "ELLIPSE",
            &json!({ "center": [0, 0], "axisEnd": [3, 0], "ratio": 1.000_001 }),
        )
        .unwrap_err();
    assert!(
        matches!(&err, CmdError::OutOfRange { param, .. } if param == "ratio"),
        "got {err:?}"
    );
    assert_eq!(session.document().model_space().len(), entities);
    assert_eq!(session.history().undo_depth(), history);
}

#[test]
fn ellipse_arc_wraps_ccw_and_equal_params_are_full() {
    let (reg, mut session) = setup();
    for (start, end, want) in [
        (5.0, 1.0, core::f64::consts::TAU - 4.0),
        (0.75, 0.75, core::f64::consts::TAU),
    ] {
        let out = reg
            .execute(
                &mut session,
                "ELLIPSE",
                &json!({
                    "mode": "arc",
                    "center": [0, 0],
                    "axisEnd": [3, 0],
                    "ratio": 0.5,
                    "startParam": start,
                    "endParam": end,
                }),
            )
            .expect("ELLIPSE CCW sweep edge");
        let EntityGeometry::Ellipse(e) = created_geo(&session, &out) else {
            panic!("se esperaba elipse");
        };
        assert!(close(e.sweep(), want));
    }
}

// ---- PLINE ------------------------------------------------------------------

#[test]
fn pline_open_straight() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "PL",
            &json!({ "vertices": [{ "pt": [0, 0] }, { "pt": [10, 0] }, { "pt": [10, 5] }] }),
        )
        .expect("PLINE open");
    let EntityGeometry::Polyline(p) = created_geo(&session, &out) else {
        panic!("se esperaba polilínea");
    };
    assert_eq!(p.vertices.len(), 3);
    assert!(!p.closed);
    assert!(p.vertices.iter().all(|v| v.bulge == 0.0));
}

#[test]
fn pline_closed_with_bulge() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "PLINE",
            &json!({
                "vertices": [{ "pt": [0, 0], "bulge": 1.0 }, { "pt": [2, 0] }],
                "closed": true
            }),
        )
        .expect("PLINE closed+bulge");
    let EntityGeometry::Polyline(p) = created_geo(&session, &out) else {
        panic!("se esperaba polilínea");
    };
    assert!(p.closed);
    assert_eq!(p.vertices[0].bulge, 1.0);
    assert_eq!(p.vertices[1].bulge, 0.0);
}

#[test]
fn pline_needs_two_vertices() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "PLINE",
            &json!({ "vertices": [{ "pt": [0, 0] }] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("2 vértices")));
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn pline_empty_path_rejected_by_schema() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "PLINE", &json!({ "vertices": [] }))
        .unwrap_err();
    assert!(matches!(err, CmdError::OutOfRange { .. }), "got {err:?}");
}

// ---- SPLINE -----------------------------------------------------------------

#[test]
fn spline_open_fit_points() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "SPL",
            &json!({ "points": [{ "pt": [0, 0] }, { "pt": [1, 2] }, { "pt": [3, 0] }] }),
        )
        .expect("SPLINE open via alias SPL");
    assert_eq!(session.history().undo_depth(), 1);
    let EntityGeometry::Spline(s) = created_geo(&session, &out) else {
        panic!("se esperaba spline");
    };
    assert_eq!(s.fit_points.len(), 3);
    assert!(!s.closed);
    assert_eq!(s.fit_points[0], Point2::new(0.0, 0.0));
    assert_eq!(s.fit_points[2], Point2::new(3.0, 0.0));
    let sp = s.fit_spline().expect("spline construible");
    assert!(close_pt(sp.eval(sp.param_range().0), Point2::new(0.0, 0.0)));
}

#[test]
fn spline_closed_periodic() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "SPLINE",
            &json!({
                "points": [{ "pt": [0, 0] }, { "pt": [2, 0] }, { "pt": [1, 2] }],
                "closed": true
            }),
        )
        .expect("SPLINE closed");
    let EntityGeometry::Spline(s) = created_geo(&session, &out) else {
        panic!("se esperaba spline");
    };
    assert!(s.closed);
    assert_eq!(s.fit_points.len(), 3);
}

#[test]
fn spline_ignores_bulge_of_the_path() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "SPLINE",
            &json!({ "points": [{ "pt": [0, 0], "bulge": 1.0 }, { "pt": [4, 0] }] }),
        )
        .expect("SPLINE ignora bulge");
    let EntityGeometry::Spline(s) = created_geo(&session, &out) else {
        panic!("se esperaba spline");
    };
    assert_eq!(
        s.fit_points,
        vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)]
    );
}

#[test]
fn spline_needs_two_points() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "SPLINE",
            &json!({ "points": [{ "pt": [0, 0] }] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("2 puntos")));
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn spline_closed_needs_three_points() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "SPLINE",
            &json!({ "points": [{ "pt": [0, 0] }, { "pt": [1, 1] }], "closed": true }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("3 puntos")));
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn spline_coincident_points_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "SPLINE",
            &json!({ "points": [{ "pt": [0, 0] }, { "pt": [0, 0] }, { "pt": [1, 1] }] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)), "got {err:?}");
    assert_eq!(session.document().model_space().len(), 0);
}

// ---- WIPEOUT ----------------------------------------------------------------

#[test]
fn wipeout_closed_polygon() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "WIPEOUT",
            &json!({ "points": [{ "pt": [0, 0] }, { "pt": [10, 0] }, { "pt": [10, 10] }] }),
        )
        .expect("WIPEOUT triangle");
    let EntityGeometry::Wipeout(w) = created_geo(&session, &out) else {
        panic!("se esperaba wipeout");
    };
    assert_eq!(w.points.len(), 3);
    assert_eq!(w.points[0], Point2::new(0.0, 0.0));
    assert_eq!(w.points[2], Point2::new(10.0, 10.0));
}

#[test]
fn wipeout_ignores_bulge() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "WIPEOUT",
            &json!({
                "points": [
                    { "pt": [0, 0], "bulge": 1.0 },
                    { "pt": [4, 0] },
                    { "pt": [2, 3] }
                ]
            }),
        )
        .expect("WIPEOUT with bulge");
    let EntityGeometry::Wipeout(w) = created_geo(&session, &out) else {
        panic!("se esperaba wipeout");
    };
    assert_eq!(w.points.len(), 3);
}

#[test]
fn wipeout_needs_three_points() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "WIPEOUT",
            &json!({ "points": [{ "pt": [0, 0] }, { "pt": [10, 0] }] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("3 puntos")));
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0, "0 tx en fallo");
}

#[test]
fn wipeout_missing_points_is_missing_param() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "WIPEOUT", &json!({}))
        .unwrap_err();
    assert!(
        matches!(&err, CmdError::MissingParam(p) if p == "points"),
        "got {err:?}"
    );
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn wipeout_frames_mode_deferred() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "WIPEOUT", &json!({ "frames": true }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("Frames")));
    assert_eq!(session.document().model_space().len(), 0);
    assert_eq!(session.history().undo_depth(), 0, "0 tx en diferido");
}

#[test]
fn wipeout_from_polyline_deferred() {
    let (reg, mut session) = setup();
    let pl = reg
        .execute(
            &mut session,
            "PLINE",
            &json!({
                "vertices": [{ "pt": [0, 0] }, { "pt": [4, 0] }, { "pt": [2, 3] }],
                "closed": true
            }),
        )
        .expect("PLINE seed");
    let pid = pl.created[0].raw().0;
    let before = session.document().model_space().len();
    let err = reg
        .execute(&mut session, "WIPEOUT", &json!({ "polyline": [pid] }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(m) if m.contains("polilínea")));
    assert_eq!(session.document().model_space().len(), before);
}

// ---- RECTANG ----------------------------------------------------------------

#[test]
fn rectang_closed_four_vertices() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(&mut session, "REC", &json!({ "p1": [0, 0], "p2": [4, 3] }))
        .expect("RECTANG");
    let EntityGeometry::Polyline(p) = created_geo(&session, &out) else {
        panic!("se esperaba polilínea");
    };
    assert_eq!(p.vertices.len(), 4);
    assert!(p.closed);
    assert!(close(p.length(), 14.0));
}

#[test]
fn rectang_degenerate_rejected_by_tx() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "RECTANG",
            &json!({ "p1": [0, 0], "p2": [0, 5] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Tx(_)), "got {err:?}");
    assert_eq!(session.document().model_space().len(), 0);
}

// ---- POLYGON ----------------------------------------------------------------

#[test]
fn polygon_inscribed_hexagon() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "POL",
            &json!({ "sides": 6, "center": [0, 0], "radius": 2 }),
        )
        .expect("POLYGON inscribed");
    let EntityGeometry::Polyline(p) = created_geo(&session, &out) else {
        panic!("se esperaba polilínea");
    };
    assert_eq!(p.vertices.len(), 6);
    assert!(p.closed);
    for v in &p.vertices {
        assert!(close(v.pt.dist(Point2::ORIGIN), 2.0));
    }
}

#[test]
fn polygon_circumscribed_square_apothem() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "POLYGON",
            &json!({ "sides": 4, "center": [0, 0], "radius": 1, "mode": "circumscribed" }),
        )
        .expect("POLYGON circumscribed");
    let EntityGeometry::Polyline(p) = created_geo(&session, &out) else {
        panic!("se esperaba polilínea");
    };
    assert_eq!(p.vertices.len(), 4);
    let mid = p.vertices[0].pt.midpoint(p.vertices[1].pt);
    assert!(close(mid.dist(Point2::ORIGIN), 1.0));
}

#[test]
fn polygon_sides_out_of_range() {
    let (reg, mut session) = setup();
    for bad in [2u64, 1025] {
        let err = reg
            .execute(
                &mut session,
                "POLYGON",
                &json!({ "sides": bad, "center": [0, 0], "radius": 1 }),
            )
            .unwrap_err();
        assert!(matches!(err, CmdError::OutOfRange { .. }), "got {err:?}");
    }
    assert_eq!(session.document().model_space().len(), 0);
}

// ---- POINT ------------------------------------------------------------------

#[test]
fn point_creates_node() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(&mut session, "PO", &json!({ "position": [3, 4] }))
        .expect("POINT via alias PO");
    assert_eq!(
        created_geo(&session, &out),
        EntityGeometry::Point(PointGeo::new(Point2::new(3.0, 4.0)))
    );
}

// ---- Cross-command rules -----------------------------------------------------

#[test]
fn draw_on_locked_layer_errors_for_every_command() {
    let (reg, mut session) = setup();
    set_current_layer_state(&mut session, |l| l.with_locked(true));

    let cases: &[(&str, Value)] = &[
        ("CIRCLE", json!({ "center": [0, 0], "radius": 1 })),
        ("ARC", json!({ "p1": [1, 0], "p2": [0, 1], "p3": [-1, 0] })),
        (
            "PLINE",
            json!({ "vertices": [{ "pt": [0, 0] }, { "pt": [1, 1] }] }),
        ),
        ("RECTANG", json!({ "p1": [0, 0], "p2": [2, 2] })),
        (
            "POLYGON",
            json!({ "sides": 3, "center": [0, 0], "radius": 1 }),
        ),
        ("POINT", json!({ "position": [0, 0] })),
    ];
    for (cmd, args) in cases {
        let err = reg.execute(&mut session, cmd, args).unwrap_err();
        assert!(
            matches!(err, CmdError::Failed(ref m) if m.contains("locked")),
            "{cmd}: got {err:?}"
        );
    }
    assert_eq!(session.document().model_space().len(), 0);
}

#[test]
fn color_command_changes_color_of_new_entities_for_every_draw_command() {
    let (reg, mut session) = setup();
    assert_eq!(session.document().current_color(), Color::ByLayer);

    reg.execute(&mut session, "COLOR", &json!({ "color": "3" }))
        .expect("COLOR aci 3");
    let expected = Color::aci(3).expect("aci válido");
    assert_eq!(session.document().current_color(), expected);

    let cases: &[(&str, Value)] = &[
        ("CIRCLE", json!({ "center": [0, 0], "radius": 1 })),
        ("ARC", json!({ "p1": [1, 0], "p2": [0, 1], "p3": [-1, 0] })),
        (
            "PLINE",
            json!({ "vertices": [{ "pt": [0, 0] }, { "pt": [1, 1] }] }),
        ),
        ("RECTANG", json!({ "p1": [5, 5], "p2": [7, 7] })),
        (
            "POLYGON",
            json!({ "sides": 3, "center": [10, 10], "radius": 1 }),
        ),
        ("POINT", json!({ "position": [20, 20] })),
    ];
    for (cmd, args) in cases {
        let out = reg.execute(&mut session, cmd, args).expect(cmd);
        let id = out.created[0];
        let (rec, _) = session.document().entity(id).expect("entity exists");
        assert_eq!(rec.color, expected, "{cmd}: no heredó el CECOLOR vigente");
    }
}

#[test]
fn undo_redo_keeps_id_stable() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({ "center": [0, 0], "radius": 1 }),
        )
        .expect("CIRCLE");
    let id = out.created[0];
    reg.execute(&mut session, "UNDO", &Value::Null)
        .expect("UNDO");
    assert!(session.document().entity(id).is_none());
    reg.execute(&mut session, "REDO", &Value::Null)
        .expect("REDO");
    assert!(session.document().entity(id).is_some());
}
