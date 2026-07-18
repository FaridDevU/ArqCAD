//! `Wipeout` registration for the Entity Conformance Suite.
//!
//! Fixtures cover triangular and square closed mask polygons.
//!
//! Any affine transform maps vertices and edge midpoints exactly, so wipeouts use
//! the generic transform-coherence assertion.
//!
//! Hit testing covers only polygon edges. Interior centroids are deliberate misses.

use af_math::{Point2, Tol};
use af_model::entity::{EntityGeometry, EntityOps, WipeoutGeo};
use proptest::prelude::*;

use super::{
    EntityFixture, arb_valid_affine, assert_bbox_contains_snaps, assert_deterministic,
    assert_hit_negative, assert_hit_positive, assert_serde_roundtrip, assert_transform_coherent,
    assert_validates, check_entity,
};

fn wipeout(pts: &[(f64, f64)]) -> EntityGeometry {
    let points = pts.iter().map(|&(x, y)| Point2::new(x, y)).collect();
    EntityGeometry::Wipeout(WipeoutGeo::new(points))
}

/// Triangle and square fixtures with edge hits, interior misses, and distant misses.
fn fixtures(_tol: &Tol) -> Vec<EntityFixture> {
    let triangle = wipeout(&[(0.0, 0.0), (4.0, 0.0), (2.0, 3.0)]);
    let square = wipeout(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    vec![
        EntityFixture {
            label: "wipeout/triangle",
            on_geometry: vec![
                Point2::new(0.0, 0.0), // Vertex.
                Point2::new(2.0, 0.0), // Base-edge midpoint.
                Point2::new(3.0, 1.5), // Right-edge midpoint.
                Point2::new(1.0, 1.5), // Left-edge midpoint.
            ],
            off_geometry: vec![Point2::new(2.0, 1.0), Point2::new(-5.0, -5.0)],
            geo: triangle,
        },
        EntityFixture {
            label: "wipeout/square",
            on_geometry: vec![
                Point2::new(5.0, 0.0),   // Bottom-edge midpoint.
                Point2::new(10.0, 5.0),  // Right-edge midpoint.
                Point2::new(0.0, 5.0),   // Closing-edge midpoint.
                Point2::new(10.0, 10.0), // Vertex.
            ],
            off_geometry: vec![Point2::new(5.0, 5.0), Point2::new(100.0, 100.0)],
            geo: square,
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

// Property strategies.

/// Valid wipeout with three to six finite vertices; self-intersection is allowed.
fn arb_wipeout() -> impl Strategy<Value = WipeoutGeo> {
    (3usize..=6)
        .prop_flat_map(|n| {
            (
                prop::collection::vec(-100.0f64..100.0, n),
                prop::collection::vec(-100.0f64..100.0, n),
            )
        })
        .prop_map(|(xs, ys)| {
            let points = xs
                .iter()
                .zip(ys.iter())
                .map(|(&x, &y)| Point2::new(x, y))
                .collect();
            WipeoutGeo::new(points)
        })
}

proptest! {
    // Bounding-box containment.
    #[test]
    fn prop1_bbox_contiene_snaps(g in arb_wipeout()) {
        assert_bbox_contains_snaps(&EntityGeometry::Wipeout(g), &Tol::default());
    }

    // Edge points hit and distant points miss.
    #[test]
    fn prop2_hit(g in arb_wipeout()) {
        let tol = Tol::default();
        let on = g.points[0].midpoint(g.points[1]);
        let geo = EntityGeometry::Wipeout(g);
        assert_hit_positive(&geo, on, tol.point_merge);
        let bb = geo.bbox();
        let far = Point2::new(bb.max.x + 1.0e6, bb.max.y + 1.0e6);
        assert_hit_negative(&geo, far, tol.point_merge);
    }

    // Any valid affine transform preserves vertices and edge midpoints.
    #[test]
    fn prop3_transform_coherente(g in arb_wipeout(), t in arb_valid_affine()) {
        assert_transform_coherent(&EntityGeometry::Wipeout(g), &t, &Tol::default());
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(g in arb_wipeout()) {
        assert_serde_roundtrip(&EntityGeometry::Wipeout(g));
    }

    // Validation.
    #[test]
    fn prop5_validate(g in arb_wipeout()) {
        assert_validates(&EntityGeometry::Wipeout(g), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(g in arb_wipeout()) {
        let tol = Tol::default();
        let probe = g.points[0].midpoint(g.points[1]);
        assert_deterministic(&EntityGeometry::Wipeout(g), probe, &tol);
    }
}

/// Fewer than three points or a nonfinite coordinate fail validation.
#[test]
fn validate_casos_invalidos() {
    let tol = Tol::default();
    assert!(wipeout(&[(0.0, 0.0), (1.0, 1.0)]).validate(&tol).is_err());
    assert!(
        wipeout(&[(0.0, 0.0), (f64::NAN, 0.0), (1.0, 1.0)])
            .validate(&tol)
            .is_err()
    );
}
