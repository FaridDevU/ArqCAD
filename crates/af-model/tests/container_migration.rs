//! Integration tests for `EntityContainer` over typed data-oriented pools.
//!
//! Public `Session` and `TxContext` operations verify stable draw order,
//! byte-identical serialization, and exact position restoration after undo.

use af_math::{Point2, Vec2};
use af_model::entity::{
    ArcGeo, CircleGeo, Color, EllipseGeo, EntityGeometry, EntityRecord, LineGeo, LineTypeRef,
    Lineweight, PointGeo, PolyVertex, PolylineGeo, RayGeo, SplineGeo, WipeoutGeo, XlineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{ContainerRef, Document, Session};

/// Builds a record with a placeholder ID on the given layer.
fn rec(layer: LayerId, geometry: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

/// Adds a model-space entity and returns its assigned ID.
fn add(session: &mut Session, geometry: EntityGeometry) -> EntityId {
    let layer = session.document().current_layer();
    session
        .transact("add", |tx| {
            tx.add_entity(ContainerRef::ModelSpace, rec(layer, geometry))
        })
        .expect("commit")
        .value
}

fn point(x: f64, y: f64) -> EntityGeometry {
    EntityGeometry::Point(PointGeo::new(Point2::new(x, y)))
}

/// Model-space IDs in draw order.
fn order(session: &Session) -> Vec<EntityId> {
    session
        .document()
        .model_space()
        .iter_records()
        .map(|r| r.id)
        .collect()
}

/// One valid geometry for each `EntityGeometry` variant.
fn one_of_each() -> Vec<EntityGeometry> {
    vec![
        EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 5.0))),
        point(2.0, 3.0),
        EntityGeometry::Circle(CircleGeo::new(Point2::new(1.0, 1.0), 4.25)),
        EntityGeometry::Arc(ArcGeo::new(Point2::new(0.0, 0.0), 3.5, 0.25, 2.1)),
        EntityGeometry::Ellipse(EllipseGeo::new(
            Point2::new(-1.0, 2.0),
            6.0,
            0.5,
            0.3,
            0.0,
            std::f64::consts::TAU,
        )),
        EntityGeometry::Polyline(PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.4),
                PolyVertex::new(Point2::new(2.0, 1.0), -0.25),
                PolyVertex::new(Point2::new(4.5, -0.5), 0.0),
            ],
            true,
        )),
        EntityGeometry::Xline(XlineGeo::new(Point2::new(1.0, 1.0), Vec2::new(1.0, 0.5))),
        EntityGeometry::Ray(RayGeo::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))),
        EntityGeometry::Spline(SplineGeo::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 2.0),
                Point2::new(3.0, 1.5),
                Point2::new(5.0, 3.0),
            ],
            false,
        )),
        EntityGeometry::Wipeout(WipeoutGeo::new(vec![
            Point2::new(0.0, 0.0),
            Point2::new(3.0, 0.0),
            Point2::new(3.0, 2.0),
            Point2::new(0.0, 2.0),
        ])),
    ]
}

// Remove and undo restore the middle entity at its original draw position.
#[test]
fn remove_y_undo_restauran_la_posicion_de_dibujo() {
    let mut session = Session::new(Units::default());
    let a = add(&mut session, point(0.0, 0.0));
    let b = add(&mut session, point(1.0, 0.0));
    let c = add(&mut session, point(2.0, 0.0));
    assert_eq!(order(&session), vec![a, b, c]);

    // Remove the middle entity, leaving [a, c].
    session
        .transact("erase b", |tx| tx.remove_entity(b))
        .expect("commit");
    assert_eq!(order(&session), vec![a, c]);

    // Undo restores `b` at index 1 rather than appending it.
    session.undo().expect("undo");
    assert_eq!(order(&session), vec![a, b, c]);
    assert_eq!(session.document().model_space().index_of(b), Some(1));
}

// Draw order remains coherent across mixed mutations and history operations.
#[test]
fn orden_de_dibujo_estable_con_insert_remove_undo_redo_mezclados() {
    let mut session = Session::new(Units::default());
    let a = add(&mut session, point(0.0, 0.0));
    let b = add(&mut session, point(1.0, 0.0));
    let c = add(&mut session, point(2.0, 0.0));

    // Same-variant modification preserves order.
    session
        .transact("move b", |tx| {
            tx.modify_entity(b, |r| {
                r.geometry = point(1.0, 9.0);
            })
        })
        .expect("commit");
    assert_eq!(order(&session), vec![a, b, c]);

    // Append `d`, then remove the first entity `a`.
    let d = add(&mut session, point(3.0, 0.0));
    assert_eq!(order(&session), vec![a, b, c, d]);
    session
        .transact("erase a", |tx| tx.remove_entity(a))
        .expect("commit");
    assert_eq!(order(&session), vec![b, c, d]);

    // Undo restores `a` at index 0.
    session.undo().expect("undo erase");
    assert_eq!(order(&session), vec![a, b, c, d]);
    assert_eq!(session.document().model_space().index_of(a), Some(0));

    // Redo returns to [b, c, d].
    session.redo().expect("redo erase");
    assert_eq!(order(&session), vec![b, c, d]);

    // Undo removal and insertion, leaving modified [a, b, c].
    session.undo().expect("undo erase 2");
    session.undo().expect("undo add d");
    assert_eq!(order(&session), vec![a, b, c]);
    // The modification to `b` remains applied.
    if let EntityGeometry::Point(p) = session.document().entity(b).unwrap().0.geometry {
        assert_eq!(p.position, Point2::new(1.0, 9.0));
    } else {
        panic!("esperaba punto");
    }
}

// Serialization round trip is byte-identical for all geometry variants.
#[test]
fn roundtrip_serde_byte_identico_con_las_diez_variantes() {
    let mut session = Session::new(Units::default());
    let mut ids = Vec::new();
    for g in one_of_each() {
        ids.push(add(&mut session, g));
    }
    // The document contains all ten in insertion order.
    assert_eq!(order(&session), ids);

    let doc = session.document();
    let json1 = serde_json::to_string(doc).expect("serialize");
    let doc2: Document = serde_json::from_str(&json1).expect("deserialize");
    let json2 = serde_json::to_string(&doc2).expect("re-serialize");

    // The round trip is bit-exact and semantically equal.
    assert_eq!(json1, json2, "roundtrip serde no fue byte-idéntico");
    assert_eq!(doc, &doc2, "documento deserializado != original");

    // Draw order and IDs survive intact.
    let order2: Vec<EntityId> = doc2.model_space().iter_records().map(|r| r.id).collect();
    assert_eq!(order2, ids);
}

// Model space serializes as a draw-ordered record array, not internal pools.
#[test]
fn model_space_serializa_como_array_de_records() {
    let mut session = Session::new(Units::default());
    add(&mut session, point(1.0, 2.0));
    let json = serde_json::to_string(session.document()).expect("serialize");
    // Verify that `modelSpace` is an array.
    let needle = "\"modelSpace\":[";
    assert!(
        json.contains(needle),
        "modelSpace debe serializar como array de records; json = {json}"
    );
}

// Cloning a document preserves order, IDs, and content.
#[test]
fn clone_de_documento_preserva_orden_y_contenido() {
    let mut session = Session::new(Units::default());
    let mut ids = Vec::new();
    for g in one_of_each() {
        ids.push(add(&mut session, g));
    }
    let doc = session.document();
    let cloned: Document = doc.clone();
    assert_eq!(doc, &cloned);
    let cloned_order: Vec<EntityId> = cloned.model_space().iter_records().map(|r| r.id).collect();
    assert_eq!(cloned_order, ids);
}
