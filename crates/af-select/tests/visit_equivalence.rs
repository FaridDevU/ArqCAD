//! Structural equivalence between zero-copy `visit` and materialized `iter_records`.
//!
//! Twenty deterministic queries compare the production index with a materialized
//! reference scan across all ten geometry variants.

use af_math::{BBox, Point2, Vec2};
use af_model::entity::{
    ArcGeo, CircleGeo, Color, EllipseGeo, EntityGeometry, EntityOps, EntityRecord, LineGeo,
    LineTypeRef, Lineweight, PointGeo, PolyVertex, PolylineGeo, RayGeo, SplineGeo, WipeoutGeo,
    XlineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{ContainerRef, Document, Session, TxError};
use af_select::SpatialIndex;

fn rec(layer: LayerId, geom: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geom,
    )
}

/// Builds a model-space document containing all ten geometry variants.
fn doc_con_las_diez_variantes() -> Session {
    let mut session = Session::new(Units::default());
    let layer0 = session.document().layer_by_name("0").unwrap().id();

    session
        .transact::<_, TxError, _>("fixture-10", |tx| {
            let geoms = [
                EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(4.0, 1.0))),
                EntityGeometry::Point(PointGeo::new(Point2::new(-1.0, -2.0))),
                EntityGeometry::Circle(CircleGeo::new(Point2::new(5.0, 5.0), 3.0)),
                EntityGeometry::Arc(ArcGeo::new(Point2::new(-3.0, 2.0), 2.0, 0.0, 1.5)),
                EntityGeometry::Ellipse(EllipseGeo::new(
                    Point2::new(1.0, 1.0),
                    4.0,
                    0.5,
                    0.3,
                    0.0,
                    2.0,
                )),
                EntityGeometry::Polyline(PolylineGeo::new(
                    vec![
                        PolyVertex::new(Point2::new(0.0, 0.0), 0.5),
                        PolyVertex::new(Point2::new(6.0, 0.0), -0.75),
                        PolyVertex::new(Point2::new(9.0, 4.0), 0.0),
                    ],
                    true,
                )),
                EntityGeometry::Xline(XlineGeo::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 1.0))),
                EntityGeometry::Ray(RayGeo::new(Point2::new(2.0, 2.0), Vec2::new(0.0, 1.0))),
                EntityGeometry::Spline(SplineGeo::new(
                    vec![
                        Point2::new(-2.0, -2.0),
                        Point2::new(-1.0, 1.0),
                        Point2::new(2.0, -1.0),
                        Point2::new(4.0, 2.0),
                    ],
                    false,
                )),
                EntityGeometry::Wipeout(WipeoutGeo::new(vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(2.0, 0.0),
                    Point2::new(2.0, 2.0),
                    Point2::new(0.0, 2.0),
                ])),
            ];
            for g in geoms {
                tx.add_entity(ContainerRef::ModelSpace, rec(layer0, g))?;
            }
            Ok(())
        })
        .unwrap();
    session
}

/// Returns 20 deterministic positions and radii.
fn puntos_de_prueba() -> Vec<(Point2, f64)> {
    (0..20)
        .map(|i| {
            let x = f64::from(i) * 1.3 - 6.0;
            let y = f64::from((i * 7) % 13) - 4.0;
            let r = 0.5 + f64::from(i % 3) * 0.6;
            (Point2::new(x, y), r)
        })
        .collect()
}

/// Materialized reference IDs whose recomputed bounds intersect `pt ± r`.
fn brute_force_candidates(doc: &Document, pt: Point2, radius: f64) -> Vec<EntityId> {
    let r = radius.max(0.0);
    let query = BBox::new(
        Point2::new(pt.x - r, pt.y - r),
        Point2::new(pt.x + r, pt.y + r),
    );
    let mut ids: Vec<EntityId> = doc
        .model_space()
        .iter_records()
        .filter(|rec| rec.geometry.bbox().intersects(query))
        .map(|rec| rec.id)
        .collect();
    ids.sort_unstable_by_key(|id| id.raw().0);
    ids
}

#[test]
fn index_picks_visit_equivale_a_ruta_materializada_en_20_posiciones() {
    let session = doc_con_las_diez_variantes();
    let doc = session.document();

    // Production index built through zero-copy `visit`.
    let index = SpatialIndex::build(doc, ContainerRef::ModelSpace);
    assert_eq!(index.len(), 10, "las 10 variantes deben estar indexadas");

    for (pt, r) in puntos_de_prueba() {
        let mut by_index = index.candidates_near(pt, r);
        by_index.sort_unstable_by_key(|id| id.raw().0);

        let by_materialized = brute_force_candidates(doc, pt, r);

        assert_eq!(
            by_index, by_materialized,
            "picks del índice (visit) != ruta materializada en {pt:?} r={r}"
        );
    }
}
