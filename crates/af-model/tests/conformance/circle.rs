//! `Circle` registration for the Entity Conformance Suite.
//!
//! Fixtures cover normal, large, and small circles.
//!
//! Circle transforms use specific assertions because arbitrary rotations do not
//! preserve axis-quadrant snap indexes. Similarities transform the center and
//! scale the radius; anisotropic scale is rejected.

use af_math::{Point2, Tol, Transform2, Vec2};
use af_model::entity::{CircleGeo, EntityGeometry, EntityOps, TransformError};
use proptest::prelude::*;

use super::{
    EntityFixture, approx_eq_scaled, approx_pt_eq, assert_bbox_contains_snaps,
    assert_deterministic, assert_hit_negative, assert_hit_positive, assert_serde_roundtrip,
    assert_validates, check_entity,
};

fn geo(cx: f64, cy: f64, r: f64) -> EntityGeometry {
    EntityGeometry::Circle(CircleGeo::new(Point2::new(cx, cy), r))
}

/// Circle fixtures whose off-geometry points include the center.
fn fixtures(tol: &Tol) -> Vec<EntityFixture> {
    // Radial near miss just beyond hit tolerance.
    let near = 2.0 * tol.point_merge;
    vec![
        EntityFixture {
            label: "circle/normal",
            geo: geo(2.0, -3.0, 5.0),
            on_geometry: vec![
                Point2::new(7.0, -3.0),  // 0°
                Point2::new(2.0, 2.0),   // 90°
                Point2::new(-3.0, -3.0), // 180°
                Point2::new(2.0, -8.0),  // 270°
            ],
            off_geometry: vec![
                Point2::new(2.0, -3.0),        // Center, not on the curve.
                Point2::new(7.0 + near, -3.0), // Radially outside by `near`.
                Point2::new(20.0, -3.0),       // Far outside.
            ],
        },
        EntityFixture {
            label: "circle/huge-1e5",
            geo: geo(0.0, 0.0, 1.0e5),
            on_geometry: vec![
                Point2::new(1.0e5, 0.0),
                Point2::new(0.0, 1.0e5),
                Point2::new(-1.0e5, 0.0),
                Point2::new(0.0, -1.0e5),
            ],
            off_geometry: vec![
                Point2::new(0.0, 0.0),         // Center.
                Point2::new(1.0e5 + 1.0, 0.0), // Outside by 1.0.
            ],
        },
        EntityFixture {
            label: "circle/micro-1e-3",
            geo: geo(0.0, 0.0, 1.0e-3),
            on_geometry: vec![
                Point2::new(1.0e-3, 0.0),
                Point2::new(0.0, 1.0e-3),
                Point2::new(-1.0e-3, 0.0),
                Point2::new(0.0, -1.0e-3),
            ],
            off_geometry: vec![
                Point2::new(0.0, 0.0),           // Center.
                Point2::new(1.0e-3 + near, 0.0), // Outside by `near`.
            ],
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
    /// Circle with radius safely above `point_merge`.
    fn arb_circle()(
        cx in -1.0e4f64..1.0e4,
        cy in -1.0e4f64..1.0e4,
        r in 1.0e-2f64..1.0e4,
    ) -> CircleGeo {
        CircleGeo::new(Point2::new(cx, cy), r)
    }
}

prop_compose! {
    /// Similarity composed of rotation, uniform scale, optional reflection, and translation.
    fn arb_similarity()(
        theta in 0.0f64..std::f64::consts::TAU,
        s in prop_oneof![-10.0f64..-0.1, 0.1f64..10.0],
        tx in -1.0e4f64..1.0e4,
        ty in -1.0e4f64..1.0e4,
    ) -> Transform2 {
        Transform2::rotate(theta)
            .then(Transform2::scale(s, s))
            .then(Transform2::translate(Vec2::new(tx, ty)))
    }
}

prop_compose! {
    /// Nonuniform scale that `Circle` must reject.
    fn arb_non_uniform()(
        sx in prop_oneof![-8.0f64..-0.2, 0.2f64..8.0],
        f in 2.0f64..5.0,
    ) -> Transform2 {
        Transform2::scale(sx, sx * f)
    }
}

proptest! {
    // Bounding-box containment.
    #[test]
    fn prop1_bbox_contiene_snaps(c in arb_circle()) {
        assert_bbox_contains_snaps(&EntityGeometry::Circle(c), &Tol::default());
    }

    // Curve points hit; the center does not.
    #[test]
    fn prop2_hit(c in arb_circle(), ang in 0.0f64..std::f64::consts::TAU) {
        let tol = Tol::default();
        let g = EntityGeometry::Circle(c);
        assert_hit_positive(&g, c.point_at_angle(ang), tol.point_merge);
        assert_hit_negative(&g, c.center, tol.point_merge);
    }

    // Similarities preserve circle geometry.
    #[test]
    fn prop3_transform_similaridad(c in arb_circle(), t in arb_similarity()) {
        let tol = Tol::default();
        let g = EntityGeometry::Circle(c);
        let moved = g.transform(&t).expect("una similaridad mantiene el círculo");

        let osnaps = g.snap_points();
        let msnaps = moved.snap_points();
        prop_assert_eq!(osnaps.len(), msnaps.len());
        // Center transforms coherently.
        prop_assert_eq!(osnaps[0].kind, msnaps[0].kind);
        prop_assert!(approx_pt_eq(t.apply(osnaps[0].point), msnaps[0].point, &tol));
        // Radius follows the uniform scale factor.
        let (scale, _) = t.scale_factors();
        let new_r = msnaps[0].point.dist(msnaps[1].point);
        prop_assert!(approx_eq_scaled(new_r, scale * c.radius, &tol));
        // The transformed box contains its snaps.
        assert_bbox_contains_snaps(&moved, &tol);
    }

    // Nonuniform scale would produce an ellipse and is rejected.
    #[test]
    fn prop3_transform_no_uniforme_es_err(c in arb_circle(), t in arb_non_uniform()) {
        let g = EntityGeometry::Circle(c);
        prop_assert_eq!(
            g.transform(&t),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(c in arb_circle()) {
        assert_serde_roundtrip(&EntityGeometry::Circle(c));
    }

    // Validation.
    #[test]
    fn prop5_validate(c in arb_circle()) {
        assert_validates(&EntityGeometry::Circle(c), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(c in arb_circle()) {
        let tol = Tol::default();
        assert_deterministic(&EntityGeometry::Circle(c), c.point_at_angle(0.0), &tol);
    }
}

/// Nonfinite coordinates fail validation.
#[test]
fn validate_no_finito_es_err() {
    let bad = geo(f64::NAN, 0.0, 1.0);
    assert!(bad.validate(&Tol::default()).is_err());
}

/// A degenerate radius fails validation.
#[test]
fn validate_radio_degenerado_es_err() {
    let zero = geo(0.0, 0.0, 0.0);
    assert!(zero.validate(&Tol::default()).is_err());
    let neg = geo(0.0, 0.0, -1.0);
    assert!(neg.validate(&Tol::default()).is_err());
}
