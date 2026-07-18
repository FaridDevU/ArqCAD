//! Shared af-select integration-test helpers.
//!
//! Each test target compiles this module independently, so some helpers are unused.
#![allow(dead_code)]

use af_math::Point2;
use af_model::entity::{
    ArcGeo, CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo, PolyVertex, PolylineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{ContainerRef, Session, TxError};

/// Builds a line record on the given layer.
#[must_use]
pub fn line_rec(layer: LayerId, a: Point2, b: Point2) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Line(LineGeo::new(a, b)),
    )
}

/// Builds a circle record on the given layer.
#[must_use]
pub fn circle_rec(layer: LayerId, center: Point2, radius: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Circle(CircleGeo::new(center, radius)),
    )
}

/// Builds a point record on the given layer.
#[must_use]
pub fn point_rec(layer: LayerId, p: Point2) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(p)),
    )
}

/// Builds an arc record with a counterclockwise radian sweep.
#[must_use]
pub fn arc_rec(layer: LayerId, center: Point2, radius: f64, start: f64, end: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Arc(ArcGeo::new(center, radius, start, end)),
    )
}

/// Builds a straight polyline from points with the requested closure.
#[must_use]
pub fn polyline_rec(layer: LayerId, pts: &[Point2], closed: bool) -> EntityRecord {
    let verts = pts.iter().map(|&p| PolyVertex::new(p, 0.0)).collect();
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Polyline(PolylineGeo::new(verts, closed)),
    )
}

/// Adds an entity to model space in one transaction and returns its ID.
pub fn add(session: &mut Session, rec: EntityRecord) -> EntityId {
    session
        .transact("add", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, rec)
        })
        .expect("add tx")
        .value
}

/// Creates a session with the default millimeter document.
#[must_use]
pub fn session() -> Session {
    Session::new(Units::default())
}
