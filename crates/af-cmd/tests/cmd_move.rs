//! End-to-end MOVE tests for mixed entity sets, locked layers, zero deltas, exact
//! undo, stable draw order, polylines, and translated extents.

use af_cmd::builtin::move_cmd;
use af_cmd::{CmdError, CommandRegistry};
use af_math::{BBox, Point2, Vec2};
use af_model::container::ContainerRef;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo, PolyVertex, PolylineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Layer, Session, TxError};
use serde_json::{Value, json};

// ---- Helpers ----------------------------------------------------------------

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    move_cmd::register(&mut reg).expect("register MOVE");
    reg
}

fn mk_record(layer: LayerId, geometry: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

/// Seeds `geoms` in draw order on `layer` and returns their IDs.
fn seed_on(session: &mut Session, layer: LayerId, geoms: Vec<EntityGeometry>) -> Vec<EntityId> {
    session
        .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
            geoms
                .into_iter()
                .map(|g| tx.add_entity(ContainerRef::ModelSpace, mk_record(layer, g)))
                .collect()
        })
        .expect("seed commits")
        .value
}

/// Seeds geometry on the current layer.
fn seed(session: &mut Session, geoms: Vec<EntityGeometry>) -> Vec<EntityId> {
    let layer = session.document().current_layer();
    seed_on(session, layer, geoms)
}

/// Returns mixed line, circle, point, and bulged-polyline geometry.
fn mixed_geoms() -> Vec<EntityGeometry> {
    vec![
        EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 4.0))),
        EntityGeometry::Circle(CircleGeo::new(Point2::new(5.0, 5.0), 2.5)),
        EntityGeometry::Point(PointGeo::new(Point2::new(-3.0, 7.0))),
        EntityGeometry::Polyline(mixed_polyline()),
    ]
}

/// Returns an open polyline with a curved segment.
fn mixed_polyline() -> PolylineGeo {
    PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(-8.0, -8.0), 0.4),
            PolyVertex::new(Point2::new(-8.0, -4.0), 0.0),
        ],
        false,
    )
}

/// Returns a closed polyline with a curved segment.
fn closed_polyline_with_bulge() -> PolylineGeo {
    PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(6.0, 0.0), 0.3),
            PolyVertex::new(Point2::new(6.0, 6.0), 0.0),
            PolyVertex::new(Point2::new(0.0, 6.0), 0.0),
        ],
        true,
    )
}

fn move_args(ids: &[EntityId], from: [f64; 2], to: [f64; 2]) -> Value {
    let raw: Vec<u64> = ids.iter().map(|id| id.raw().0).collect();
    json!({ "entities": raw, "from": from, "to": to })
}

fn geom(session: &Session, id: EntityId) -> EntityGeometry {
    session
        .document()
        .entity(id)
        .expect("entity present")
        .0
        .geometry
        .clone()
}

/// Returns the union of entity bounding boxes for the selected subset.
fn union_bbox(session: &Session, ids: &[EntityId]) -> BBox {
    ids.iter()
        .map(|&id| geom(session, id).bbox())
        .reduce(BBox::union)
        .expect("set no vacío")
}

// ---- Mixed entity set --------------------------------------------------------

#[test]
fn move_multi_entity_translates_each_geometry_exactly() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, mixed_geoms());

    let out = reg
        .execute(
            &mut session,
            "MOVE",
            &move_args(&ids, [0.0, 0.0], [10.0, 5.0]),
        )
        .expect("move succeeds");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert!(out.created.is_empty(), "MOVE no crea entidades");

    let d = Vec2::new(10.0, 5.0);
    match geom(&session, ids[0]) {
        EntityGeometry::Line(g) => {
            assert_eq!(g.p1, Point2::new(0.0, 0.0) + d);
            assert_eq!(g.p2, Point2::new(10.0, 4.0) + d);
        }
        other => panic!("esperaba línea, fue {other:?}"),
    }
    match geom(&session, ids[1]) {
        EntityGeometry::Circle(g) => {
            assert_eq!(g.center, Point2::new(5.0, 5.0) + d);
            assert_eq!(g.radius, 2.5);
        }
        other => panic!("esperaba círculo, fue {other:?}"),
    }
    match geom(&session, ids[2]) {
        EntityGeometry::Point(g) => assert_eq!(g.position, Point2::new(-3.0, 7.0) + d),
        other => panic!("esperaba punto, fue {other:?}"),
    }
    match geom(&session, ids[3]) {
        EntityGeometry::Polyline(g) => {
            assert_eq!(g.vertices[0].pt, Point2::new(-8.0, -8.0) + d);
            assert_eq!(g.vertices[1].pt, Point2::new(-8.0, -4.0) + d);
            assert_eq!(g.vertices[0].bulge, 0.4);
            assert_eq!(g.vertices[1].bulge, 0.0);
        }
        other => panic!("esperaba polyline, fue {other:?}"),
    }
}

// ---- Locked-layer atomicity --------------------------------------------------

#[test]
fn move_rejects_when_any_entity_is_on_a_locked_layer() {
    let reg = registry();
    let mut session = Session::new(Units::default());
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
    let l0 = session.document().current_layer();

    let free = seed_on(
        &mut session,
        l0,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        ))],
    );
    let held = seed_on(
        &mut session,
        locked_layer,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(2.0, 2.0),
            1.0,
        ))],
    );
    let ids = vec![free[0], held[0]];

    let before = serde_json::to_string(session.document()).unwrap();
    let err = reg
        .execute(
            &mut session,
            "MOVE",
            &move_args(&ids, [0.0, 0.0], [5.0, 5.0]),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(msg) => {
            assert!(msg.contains("locked"), "mensaje: {msg}");
            assert!(
                msg.contains(&held[0].raw().0.to_string()),
                "debe listar el id ofensor: {msg}"
            );
        }
        other => panic!("esperaba Failed con la lista de ofensores, fue {other:?}"),
    }
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

// ---- to == from ------------------------------------------------------------

#[test]
fn move_from_equals_to_makes_no_effective_transaction() {
    // A zero delta creates no operation and therefore violates the mutating contract.
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, mixed_geoms());
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(
            &mut session,
            "MOVE",
            &move_args(&ids, [4.0, 4.0], [4.0, 4.0]),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

// ---- Exact undo --------------------------------------------------------------

#[test]
fn move_is_reversible_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, mixed_geoms());
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "MOVE",
        &move_args(&ids, [0.0, 0.0], [12.0, -7.0]),
    )
    .expect("move succeeds");
    assert_ne!(
        before,
        serde_json::to_string(session.document()).unwrap(),
        "el MOVE debe cambiar el documento"
    );

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

// ---- Stable draw order -------------------------------------------------------

#[test]
fn move_preserves_draw_order() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, mixed_geoms());
    let order_before: Vec<EntityId> = session
        .document()
        .model_space()
        .iter()
        .map(|r| r.id)
        .collect();

    reg.execute(
        &mut session,
        "MOVE",
        &move_args(&ids, [0.0, 0.0], [1.0, 1.0]),
    )
    .expect("move succeeds");

    let order_after: Vec<EntityId> = session
        .document()
        .model_space()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(order_before, order_after);
}

// ---- Translated extents ------------------------------------------------------

#[test]
fn move_translates_extents_by_the_same_delta() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, mixed_geoms());
    let before = union_bbox(&session, &ids);

    let d = Vec2::new(8.5, -3.25);
    reg.execute(
        &mut session,
        "MOVE",
        &move_args(&ids, [0.0, 0.0], [d.x, d.y]),
    )
    .expect("move succeeds");
    let after = union_bbox(&session, &ids);

    let tol = 1e-9;
    assert!((after.min.x - (before.min.x + d.x)).abs() < tol);
    assert!((after.min.y - (before.min.y + d.y)).abs() < tol);
    assert!((after.max.x - (before.max.x + d.x)).abs() < tol);
    assert!((after.max.y - (before.max.y + d.y)).abs() < tol);
}

// ---- Open bulged polyline ----------------------------------------------------

#[test]
fn move_open_polyline_with_bulge_translates_vertices_and_preserves_bulge() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let poly = mixed_polyline();
    let bulges_before: Vec<f64> = poly.vertices.iter().map(|v| v.bulge).collect();
    let bbox_before = poly.bbox();
    let ids = seed(&mut session, vec![EntityGeometry::Polyline(poly.clone())]);

    let d = Vec2::new(-6.0, 9.5);
    reg.execute(
        &mut session,
        "MOVE",
        &move_args(&ids, [0.0, 0.0], [d.x, d.y]),
    )
    .expect("move succeeds");

    let EntityGeometry::Polyline(after) = geom(&session, ids[0]) else {
        panic!("esperaba polyline");
    };
    assert_eq!(after.closed, poly.closed);
    for (before_v, after_v) in poly.vertices.iter().zip(after.vertices.iter()) {
        assert_eq!(
            after_v.pt,
            before_v.pt + d,
            "el vértice se traslada por (to-from)"
        );
    }
    let bulges_after: Vec<f64> = after.vertices.iter().map(|v| v.bulge).collect();
    assert_eq!(
        bulges_after, bulges_before,
        "el bulge es invariante bajo traslación"
    );

    let bbox_after = after.bbox();
    let tol = 1e-9;
    assert!((bbox_after.min.x - (bbox_before.min.x + d.x)).abs() < tol);
    assert!((bbox_after.min.y - (bbox_before.min.y + d.y)).abs() < tol);
    assert!((bbox_after.max.x - (bbox_before.max.x + d.x)).abs() < tol);
    assert!((bbox_after.max.y - (bbox_before.max.y + d.y)).abs() < tol);
}

// ---- Closed polyline ---------------------------------------------------------

#[test]
fn move_closed_polyline_translates_vertices_and_preserves_closed_flag() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let poly = closed_polyline_with_bulge();
    let bbox_before = poly.bbox();
    let ids = seed(&mut session, vec![EntityGeometry::Polyline(poly.clone())]);

    let d = Vec2::new(3.0, -2.0);
    reg.execute(
        &mut session,
        "MOVE",
        &move_args(&ids, [0.0, 0.0], [d.x, d.y]),
    )
    .expect("move succeeds");

    let EntityGeometry::Polyline(after) = geom(&session, ids[0]) else {
        panic!("esperaba polyline");
    };
    assert!(after.closed, "el flag closed se conserva");
    for (before_v, after_v) in poly.vertices.iter().zip(after.vertices.iter()) {
        assert_eq!(after_v.pt, before_v.pt + d);
        assert_eq!(
            after_v.bulge, before_v.bulge,
            "bulge invariante bajo traslación"
        );
    }

    let bbox_after = after.bbox();
    let tol = 1e-9;
    assert!((bbox_after.min.x - (bbox_before.min.x + d.x)).abs() < tol);
    assert!((bbox_after.min.y - (bbox_before.min.y + d.y)).abs() < tol);
    assert!((bbox_after.max.x - (bbox_before.max.x + d.x)).abs() < tol);
    assert!((bbox_after.max.y - (bbox_before.max.y + d.y)).abs() < tol);
}

// ---- Polyline undo -----------------------------------------------------------

#[test]
fn move_polyline_is_reversible_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Polyline(mixed_polyline())],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    reg.execute(
        &mut session,
        "MOVE",
        &move_args(&ids, [0.0, 0.0], [-5.0, 11.0]),
    )
    .expect("move succeeds");
    assert_ne!(
        before,
        serde_json::to_string(session.document()).unwrap(),
        "el MOVE debe cambiar el documento"
    );

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
