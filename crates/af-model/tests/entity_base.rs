//! Integration tests for `af_model::entity` base types.
//!
//! These cover enum dispatch, construction-time color validation, exact record
//! serialization, and the bounding-box snap containment invariant.

use af_math::{Point2, Tol, Transform2, Vec2};
use af_model::entity::{
    Color, ColorError, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo, SnapKind,
};
use af_model::id::{EntityId, LayerId, ObjectId};

// ---------------------------------------------------------------------------
// ACI color validation by construction.
// ---------------------------------------------------------------------------

#[test]
fn color_aci_valida_rango_1_a_255() {
    assert!(Color::aci(1).is_ok());
    assert!(Color::aci(255).is_ok());
    assert_eq!(Color::aci(0), Err(ColorError::AciOutOfRange(0)));
}

#[test]
fn color_aci_get_devuelve_indice() {
    let c = Color::aci(7).unwrap();
    match c {
        Color::Aci(a) => assert_eq!(a.get(), 7),
        _ => panic!("esperaba Aci"),
    }
}

#[test]
fn aci_0_es_imposible_al_deserializar() {
    // Deserialization uses the same validation, so `Aci(0)` cannot be loaded.
    let ok: Result<Color, _> = serde_json::from_str(r#"{"aci":5}"#);
    assert!(ok.is_ok());
    let bad: Result<Color, _> = serde_json::from_str(r#"{"aci":0}"#);
    assert!(bad.is_err());
}

// ---------------------------------------------------------------------------
// Dispatch from `EntityGeometry` to concrete geometry.
// ---------------------------------------------------------------------------

#[test]
fn dispatch_bbox_hit_snaps_por_variante() {
    let line = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)));
    let point = EntityGeometry::Point(PointGeo::new(Point2::new(3.0, 4.0)));

    // Bounding-box dispatch is exact.
    assert_eq!(line.bbox().max, Point2::new(10.0, 0.0));
    assert_eq!(point.bbox().min, Point2::new(3.0, 4.0));

    // Hit testing uses geometry rather than its box.
    assert_eq!(line.hit(Point2::new(5.0, 0.0), 0.1), Some(0.0));

    // Line has two endpoint snaps and one midpoint; point has one node.
    assert_eq!(line.snap_points().len(), 3);
    let ps = point.snap_points();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].kind, SnapKind::Node);
}

#[test]
fn transform_via_enum_es_exacta_para_line_y_point() {
    let line = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(2.0, 0.0)));
    let t = Transform2::translate(Vec2::new(1.0, 1.0));
    let moved = line.transform(&t).unwrap();
    assert_eq!(
        moved,
        EntityGeometry::Line(LineGeo::new(Point2::new(1.0, 1.0), Point2::new(3.0, 1.0),))
    );
}

#[test]
fn validate_via_enum() {
    let tol = Tol::default();
    let good = EntityGeometry::Point(PointGeo::new(Point2::new(0.0, 0.0)));
    assert!(good.validate(&tol).is_ok());
    let bad = EntityGeometry::Line(LineGeo::new(
        Point2::new(f64::NAN, 0.0),
        Point2::new(1.0, 1.0),
    ));
    assert!(bad.validate(&tol).is_err());
}

// ---------------------------------------------------------------------------
// Every enum-level bounding box contains all snap points.
// ---------------------------------------------------------------------------

#[test]
fn bbox_contiene_snaps_para_ambas_variantes() {
    let geos = [
        EntityGeometry::Line(LineGeo::new(Point2::new(-3.0, 2.0), Point2::new(7.0, -5.0))),
        EntityGeometry::Point(PointGeo::new(Point2::new(1.5, 9.0))),
    ];
    for g in &geos {
        let bb = g.bbox();
        for s in g.snap_points() {
            assert!(
                bb.expand(1e-9).contains_point(s.point),
                "snap fuera de bbox"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Exact line serialization and record round trip.
// ---------------------------------------------------------------------------

#[test]
fn line_json_string_exacto() {
    let geo = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)));
    // `serde_json` renders integral `f64` values with a `.0` suffix.
    assert_eq!(
        serde_json::to_string(&geo).unwrap(),
        r#"{"type":"line","p1":[0.0,0.0],"p2":[1.0,1.0]}"#
    );
}

#[test]
fn entity_record_roundtrip() {
    let rec = EntityRecord::new(
        EntityId::from(ObjectId(42)),
        LayerId::from(ObjectId(1)),
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::Mm(0.25),
        EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 50.0),
        )),
    );
    assert!(rec.visible, "visible por defecto true");

    let json = serde_json::to_string(&rec).unwrap();
    let back: EntityRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rec);
}

#[test]
fn entity_record_geometry_va_anidada_con_tag() {
    // Geometry serializes as a nested object with a `type` discriminator.
    let rec = EntityRecord::new(
        EntityId::from(ObjectId(1)),
        LayerId::from(ObjectId(1)),
        Color::aci(3).unwrap(),
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(Point2::new(3.0, 4.0))),
    );
    let json = serde_json::to_string(&rec).unwrap();
    assert!(
        json.contains(r#""geometry":{"type":"point","position":[3.0,4.0]}"#),
        "geometry anidada con tag; json = {json}"
    );
}
