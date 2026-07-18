//! `Line` registration for the Entity Conformance Suite.
//!
//! Fixtures include degenerate and extreme-scale lines and use the generic suite.

use af_math::{Point2, Tol};
use af_model::entity::{EntityGeometry, LineGeo};
use proptest::prelude::*;

use super::{
    EntityFixture, arb_valid_affine, assert_bbox_contains_snaps, assert_deterministic,
    assert_hit_negative, assert_hit_positive, assert_serde_roundtrip, assert_transform_coherent,
    assert_validates, check_entity,
};

fn geo(x1: f64, y1: f64, x2: f64, y2: f64) -> EntityGeometry {
    EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2)))
}

/// Normal, degenerate, large, and small horizontal line fixtures.
fn fixtures(tol: &Tol) -> Vec<EntityFixture> {
    // Near miss just beyond hit tolerance.
    let near = 2.0 * tol.point_merge;
    vec![
        EntityFixture {
            label: "line/normal",
            geo: geo(0.0, 0.0, 10.0, 0.0),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(5.0, 0.0),
            ],
            off_geometry: vec![Point2::new(5.0, near), Point2::new(5.0, 1.0)],
        },
        EntityFixture {
            // Degenerate line whose endpoint and midpoint snaps coincide.
            label: "line/degenerate-L0",
            geo: geo(2.0, 3.0, 2.0, 3.0),
            on_geometry: vec![Point2::new(2.0, 3.0)],
            off_geometry: vec![Point2::new(2.0, 3.0 + near), Point2::new(5.0, 7.0)],
        },
        EntityFixture {
            label: "line/huge-1e5",
            geo: geo(0.0, 0.0, 1.0e5, 0.0),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0e5, 0.0),
                Point2::new(5.0e4, 0.0),
            ],
            off_geometry: vec![Point2::new(5.0e4, near), Point2::new(5.0e4, 1.0)],
        },
        EntityFixture {
            label: "line/micro-1e-5",
            geo: geo(0.0, 0.0, 1.0e-5, 0.0),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0e-5, 0.0),
                Point2::new(5.0e-6, 0.0),
            ],
            off_geometry: vec![Point2::new(5.0e-6, near), Point2::new(5.0e-6, 1.0)],
        },
    ]
}

#[test]
fn fixtures_deterministas() {
    let tol = Tol::default();
    for fix in fixtures(&tol) {
        check_entity(&fix, &tol);
    }
}

prop_compose! {
    fn arb_line()(
        x1 in -1.0e4f64..1.0e4,
        y1 in -1.0e4f64..1.0e4,
        x2 in -1.0e4f64..1.0e4,
        y2 in -1.0e4f64..1.0e4,
    ) -> LineGeo {
        LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2))
    }
}

proptest! {
    // Bounding-box containment.
    #[test]
    fn prop1_bbox_contiene_snaps(l in arb_line()) {
        assert_bbox_contains_snaps(&EntityGeometry::Line(l), &Tol::default());
    }

    // Segment points hit; a unit perpendicular offset misses.
    #[test]
    fn prop2_hit(l in arb_line(), t in 0.0f64..=1.0) {
        let tol = Tol::default();
        // Nondegenerate lines have a defined direction and perpendicular.
        prop_assume!(l.length() > tol.linear);
        let g = EntityGeometry::Line(l);

        let on = l.point_at(t);
        assert_hit_positive(&g, on, tol.point_merge);

        // A unit-direction perpendicular has unit distance from the segment.
        let off = on + l.direction().unwrap().perp();
        assert_hit_negative(&g, off, tol.point_merge);
    }

    // Any valid affine transform is coherent.
    #[test]
    fn prop3_transform_coherente(l in arb_line(), t in arb_valid_affine()) {
        assert_transform_coherent(&EntityGeometry::Line(l), &t, &Tol::default());
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(l in arb_line()) {
        assert_serde_roundtrip(&EntityGeometry::Line(l));
    }

    // Validation.
    #[test]
    fn prop5_validate(l in arb_line()) {
        assert_validates(&EntityGeometry::Line(l), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(l in arb_line()) {
        let tol = Tol::default();
        assert_deterministic(&EntityGeometry::Line(l), l.midpoint(), &tol);
    }
}

/// Nonfinite coordinates fail validation.
#[test]
fn validate_no_finito_es_err() {
    use af_model::entity::EntityOps;
    let bad = geo(f64::INFINITY, 0.0, 1.0, 1.0);
    assert!(bad.validate(&Tol::default()).is_err());
}
