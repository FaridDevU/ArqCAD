//! Read-only ID, DIST, AREA, LIST, and MEASUREGEOM query tests.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    CircleGeo, Color, EllipseGeo, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo, PolyVertex, PolylineGeo,
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

fn add_geo(session: &mut Session, geo: EntityGeometry) -> EntityId {
    let l0 = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    l0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    geo,
                ),
            )
        })
        .expect("seed commits")
        .value
}

/// Executes a query and asserts that it creates no transaction.
fn query(
    reg: &CommandRegistry,
    session: &mut Session,
    name: &str,
    args: &serde_json::Value,
) -> String {
    let before = serde_json::to_string(session.document()).unwrap();
    let out = reg.execute(session, name, args).expect("query executes");
    assert!(out.tx_seq.is_none(), "{name} es view-only (0 tx)");
    assert_eq!(
        before,
        serde_json::to_string(session.document()).unwrap(),
        "{name} no debe mutar el documento"
    );
    out.message.expect("una consulta reporta en el mensaje")
}

#[test]
fn id_reports_point_coordinates() {
    let (reg, mut session) = setup();
    let msg = query(&reg, &mut session, "ID", &json!({ "point": [3.0, 4.0] }));
    assert!(
        msg.contains("3.0000") && msg.contains("4.0000"),
        "msg: {msg}"
    );
}

#[test]
fn dist_reports_distance_and_angle() {
    let (reg, mut session) = setup();
    let msg = query(
        &reg,
        &mut session,
        "DIST",
        &json!({ "p1": [0.0, 0.0], "p2": [3.0, 4.0] }),
    );
    assert!(msg.contains("Distance = 5.0000"), "msg: {msg}");
    assert!(msg.contains("Delta X = 3.0000"), "msg: {msg}");
    let msg2 = query(
        &reg,
        &mut session,
        "DI",
        &json!({ "p1": [0.0, 0.0], "p2": [0.0, 2.0] }),
    );
    assert!(msg2.contains("Distance = 2.0000"), "msg: {msg2}");
}

#[test]
fn area_of_circle() {
    let (reg, mut session) = setup();
    let id = add_geo(
        &mut session,
        EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 2.0)),
    );
    let msg = query(
        &reg,
        &mut session,
        "AREA",
        &json!({ "entities": [id.raw().0] }),
    );
    assert!(msg.contains("12.5664"), "área de círculo r=2, msg: {msg}");
    assert!(msg.contains("circle"), "msg: {msg}");
}

#[test]
fn area_of_closed_polyline_square() {
    let (reg, mut session) = setup();
    let sq = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 10.0), 0.0),
            PolyVertex::new(Point2::new(0.0, 10.0), 0.0),
        ],
        true,
    );
    let id = add_geo(&mut session, EntityGeometry::Polyline(sq));
    let msg = query(
        &reg,
        &mut session,
        "AA",
        &json!({ "entities": [id.raw().0] }),
    );
    assert!(msg.contains("Area = 100.0000"), "msg: {msg}");
    assert!(msg.contains("Perimeter = 40.0000"), "msg: {msg}");
}

#[test]
fn area_of_open_polyline_is_skipped() {
    let (reg, mut session) = setup();
    let open = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 10.0), 0.0),
        ],
        false,
    );
    let id = add_geo(&mut session, EntityGeometry::Polyline(open));
    let msg = query(
        &reg,
        &mut session,
        "AREA",
        &json!({ "entities": [id.raw().0] }),
    );
    assert!(
        msg.contains("omitida"),
        "una abierta no tiene área, msg: {msg}"
    );
}

#[test]
fn list_reports_entity_props() {
    let (reg, mut session) = setup();
    let id = add_geo(
        &mut session,
        EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(3.0, 4.0))),
    );
    let msg = query(
        &reg,
        &mut session,
        "LIST",
        &json!({ "entities": [id.raw().0] }),
    );
    assert!(msg.contains("LINE"), "msg: {msg}");
    assert!(msg.contains("Layer: 0"), "msg: {msg}");
    assert!(
        msg.contains("length 5.0000"),
        "longitud de la línea, msg: {msg}"
    );
    let msg2 = query(
        &reg,
        &mut session,
        "LI",
        &json!({ "entities": [id.raw().0] }),
    );
    assert!(msg2.contains("LINE"), "msg: {msg2}");
}

#[test]
fn measuregeom_distance_mode() {
    let (reg, mut session) = setup();
    let msg = query(
        &reg,
        &mut session,
        "MEASUREGEOM",
        &json!({ "mode": "distance", "p1": [0.0, 0.0], "p2": [3.0, 4.0] }),
    );
    assert!(msg.contains("Distance = 5.0000"), "msg: {msg}");
}

#[test]
fn measuregeom_radius_mode() {
    let (reg, mut session) = setup();
    let id = add_geo(
        &mut session,
        EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 2.0)),
    );
    let msg = query(
        &reg,
        &mut session,
        "MEA",
        &json!({ "mode": "radius", "entities": [id.raw().0] }),
    );
    assert!(msg.contains("Radius = 2.0000"), "msg: {msg}");
    assert!(msg.contains("Diameter = 4.0000"), "msg: {msg}");
}

#[test]
fn measuregeom_angle_mode() {
    let (reg, mut session) = setup();
    let msg = query(
        &reg,
        &mut session,
        "MEASUREGEOM",
        &json!({ "mode": "angle", "p1": [0.0, 0.0], "p2": [1.0, 0.0], "p3": [0.0, 1.0] }),
    );
    assert!(msg.contains("90.0000"), "ángulo recto, msg: {msg}");
}

#[test]
fn measuregeom_area_mode() {
    let (reg, mut session) = setup();
    let id = add_geo(
        &mut session,
        EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 2.0)),
    );
    let msg = query(
        &reg,
        &mut session,
        "MEA",
        &json!({ "mode": "area", "entities": [id.raw().0] }),
    );
    assert!(msg.contains("12.5664"), "msg: {msg}");
}

#[test]
fn measuregeom_length_full_and_arc_ellipse() {
    let (reg, mut session) = setup();
    let full = add_geo(
        &mut session,
        EntityGeometry::Ellipse(EllipseGeo::new(
            Point2::ORIGIN,
            40.0,
            0.5,
            0.0,
            0.0,
            std::f64::consts::TAU,
        )),
    );
    let arc = add_geo(
        &mut session,
        EntityGeometry::Ellipse(EllipseGeo::new(
            Point2::ORIGIN,
            40.0,
            0.5,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        )),
    );
    let msg = query(
        &reg,
        &mut session,
        "MEA",
        &json!({ "mode": "length", "entities": [full.raw().0, arc.raw().0] }),
    );
    assert!(msg.contains("Length = 193.7690"), "msg: {msg}");
    assert!(msg.contains("Length = 48.4422"), "msg: {msg}");
    assert!(msg.contains("Total length = 242.2112"), "msg: {msg}");
}

#[test]
fn measuregeom_length_common_types_and_point_omission() {
    let (reg, mut session) = setup();
    let line = add_geo(
        &mut session,
        EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(3.0, 4.0))),
    );
    let circle = add_geo(
        &mut session,
        EntityGeometry::Circle(CircleGeo::new(Point2::ORIGIN, 2.0)),
    );
    let polyline = add_geo(
        &mut session,
        EntityGeometry::Polyline(PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::ORIGIN, 0.0),
                PolyVertex::new(Point2::new(3.0, 4.0), 0.0),
                PolyVertex::new(Point2::new(3.0, 8.0), 0.0),
            ],
            false,
        )),
    );
    let point = add_geo(
        &mut session,
        EntityGeometry::Point(PointGeo::new(Point2::new(1.0, 1.0))),
    );
    let msg = query(
        &reg,
        &mut session,
        "MEASUREGEOM",
        &json!({ "mode": "length", "entities": [
            line.raw().0,
            circle.raw().0,
            polyline.raw().0,
            point.raw().0
        ] }),
    );
    assert!(
        msg.contains(&format!("Entity {} (line): Length = 5.0000", line.raw().0)),
        "msg: {msg}"
    );
    assert!(
        msg.contains(&format!(
            "Entity {} (circle): Length = 12.5664",
            circle.raw().0
        )),
        "msg: {msg}"
    );
    assert!(
        msg.contains(&format!(
            "Entity {} (polyline): Length = 9.0000",
            polyline.raw().0
        )),
        "msg: {msg}"
    );
    assert!(
        msg.contains(&format!(
            "Entity {}: no tiene longitud finita soportada",
            point.raw().0
        )),
        "msg: {msg}"
    );
    assert!(
        msg.contains("Total length = 26.5664 (3 medida(s))"),
        "msg: {msg}"
    );
}

#[test]
fn measuregeom_bounds_line_is_view_only() {
    let (reg, mut session) = setup();
    let id = add_geo(
        &mut session,
        EntityGeometry::Line(LineGeo::new(Point2::new(3.0, -2.0), Point2::new(-1.0, 5.0))),
    );
    let msg = query(
        &reg,
        &mut session,
        "MEASUREGEOM",
        &json!({ "mode": "bounds", "entities": [id.raw().0] }),
    );
    assert!(msg.contains("Min = -1.0000,-2.0000"), "msg: {msg}");
    assert!(msg.contains("Max = 3.0000,5.0000"), "msg: {msg}");
    assert!(msg.contains("Width = 4.0000"), "msg: {msg}");
    assert!(msg.contains("Height = 7.0000"), "msg: {msg}");
}

#[test]
fn measuregeom_bounds_rotated_ellipse_is_view_only() {
    let (reg, mut session) = setup();
    let id = add_geo(
        &mut session,
        EntityGeometry::Ellipse(EllipseGeo::new(
            Point2::ORIGIN,
            4.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
            0.0,
            std::f64::consts::TAU,
        )),
    );
    let msg = query(
        &reg,
        &mut session,
        "MEA",
        &json!({ "mode": "bounds", "entities": [id.raw().0] }),
    );
    assert!(msg.contains("Min = -3.1623,-3.1623"), "msg: {msg}");
    assert!(msg.contains("Max = 3.1623,3.1623"), "msg: {msg}");
    assert!(msg.contains("Width = 6.3246"), "msg: {msg}");
    assert!(msg.contains("Height = 6.3246"), "msg: {msg}");
}

#[test]
fn measuregeom_bounds_empty_selection_errors_without_mutation() {
    let (reg, mut session) = setup();
    let before = serde_json::to_string(session.document()).unwrap();
    let error = reg
        .execute(
            &mut session,
            "MEASUREGEOM",
            &json!({ "mode": "bounds", "entities": [] }),
        )
        .unwrap_err();
    assert_eq!(
        error,
        CmdError::Failed("MEASUREGEOM bounds: se requiere exactamente una entidad".into())
    );
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn measuregeom_missing_args_for_mode_errors() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "MEASUREGEOM", &json!({ "mode": "distance" }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
}

#[test]
fn id_missing_point_errors() {
    let (reg, mut session) = setup();
    assert_eq!(
        reg.execute(&mut session, "ID", &json!({})).unwrap_err(),
        CmdError::MissingParam("point".to_string())
    );
}
