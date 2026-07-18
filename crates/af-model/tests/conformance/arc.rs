//! `Arc` registration for the Entity Conformance Suite.
//!
//! Fixtures cover quarter, semicircular, axis-crossing, large, and small arcs.
//!
//! Arc transforms use specific assertions because rotations can change which
//! axis quadrants fall within the sweep. Similarities transform the center and
//! midpoint, scale the radius, and preserve sweep; anisotropic scale is rejected.

use af_math::{Point2, Tol, Transform2, Vec2};
use af_model::entity::{ArcGeo, EntityGeometry, EntityOps, TransformError};
use proptest::prelude::*;

use super::{
    EntityFixture, approx_eq_scaled, approx_pt_eq, assert_bbox_contains_snaps,
    assert_deterministic, assert_hit_negative, assert_hit_positive, assert_serde_roundtrip,
    assert_validates, check_entity,
};

fn geo(cx: f64, cy: f64, r: f64, sa: f64, ea: f64) -> EntityGeometry {
    EntityGeometry::Arc(ArcGeo::new(Point2::new(cx, cy), r, sa, ea))
}

/// Point at fraction `frac` of `arc`'s sweep.
fn on_arc(arc: &ArcGeo, frac: f64) -> Point2 {
    arc.arc_seg().point_at(arc.start_angle + arc.sweep() * frac)
}

/// Arc fixtures whose off-geometry points include the center.
fn fixtures(_tol: &Tol) -> Vec<EntityFixture> {
    use core::f64::consts::{FRAC_PI_2, PI};
    let cases: [(f64, f64, f64, f64, f64); 5] = [
        (2.0, -3.0, 5.0, 0.0, FRAC_PI_2), // Northeast quarter.
        (0.0, 0.0, 4.0, 0.0, PI),         // Upper semicircle.
        (1.0, 1.0, 3.0, 170f64.to_radians(), 190f64.to_radians()), // Crosses 180 degrees.
        (0.0, 0.0, 1.0e5, 0.5, 2.7),      // Large.
        (0.0, 0.0, 1.0e-3, 0.2, 3.0),     // Small.
    ];
    cases
        .iter()
        .map(|&(cx, cy, r, sa, ea)| {
            let arc = ArcGeo::new(Point2::new(cx, cy), r, sa, ea);
            EntityFixture {
                label: "arc",
                geo: EntityGeometry::Arc(arc),
                // Exact points on the curve.
                on_geometry: vec![
                    on_arc(&arc, 0.0),
                    on_arc(&arc, 0.25),
                    on_arc(&arc, 0.5),
                    on_arc(&arc, 0.75),
                    on_arc(&arc, 1.0),
                ],
                // Center and a point clearly outside the supporting circle.
                off_geometry: vec![Point2::new(cx, cy), Point2::new(cx + r * 2.0, cy)],
            }
        })
        .collect()
}

#[test]
fn fixtures_deterministas() {
    let tol = Tol::default();
    for fix in fixtures(&tol) {
        check_entity(&fix, &tol);
    }
}

prop_compose! {
    /// Valid arc with a nondegenerate radius and bounded sweep.
    fn arb_arc()(
        cx in -1.0e4f64..1.0e4,
        cy in -1.0e4f64..1.0e4,
        r in 1.0e-2f64..1.0e4,
        start in 0.0f64..std::f64::consts::TAU,
        sweep in 0.1f64..(std::f64::consts::TAU - 0.1),
    ) -> ArcGeo {
        ArcGeo::new(Point2::new(cx, cy), r, start, start + sweep)
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
    /// Nonuniform scale that `Arc` must reject.
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
    fn prop1_bbox_contiene_snaps(a in arb_arc()) {
        assert_bbox_contains_snaps(&EntityGeometry::Arc(a), &Tol::default());
    }

    // Curve points hit; the center does not.
    #[test]
    fn prop2_hit(a in arb_arc(), frac in 0.0f64..=1.0) {
        let tol = Tol::default();
        let g = EntityGeometry::Arc(a);
        assert_hit_positive(&g, on_arc(&a, frac), tol.point_merge);
        assert_hit_negative(&g, a.center, tol.point_merge);
    }

    // Similarities preserve arc geometry.
    #[test]
    fn prop3_transform_similaridad(a in arb_arc(), t in arb_similarity()) {
        let tol = Tol::default();
        let g = EntityGeometry::Arc(a);
        let moved = g.transform(&t).expect("una similaridad mantiene el arco");
        let EntityGeometry::Arc(m) = moved else { unreachable!("Arc -> Arc") };

        // Center transforms coherently.
        prop_assert!(approx_pt_eq(t.apply(a.center), m.center, &tol));
        // Radius follows the uniform scale factor.
        let (scale, _) = t.scale_factors();
        prop_assert!(approx_eq_scaled(m.radius, scale * a.radius, &tol));
        // Sweep is preserved, including under reflection.
        prop_assert!(approx_eq_scaled(m.sweep(), a.sweep(), &tol));
        // Arc midpoint maps to the transformed midpoint.
        prop_assert!(approx_pt_eq(
            t.apply(a.arc_seg().midpoint()),
            m.arc_seg().midpoint(),
            &tol
        ));
        // The transformed box contains its snaps.
        assert_bbox_contains_snaps(&EntityGeometry::Arc(m), &tol);
    }

    // Nonuniform scale would produce an elliptical arc and is rejected.
    #[test]
    fn prop3_transform_no_uniforme_es_err(a in arb_arc(), t in arb_non_uniform()) {
        let g = EntityGeometry::Arc(a);
        prop_assert_eq!(
            g.transform(&t),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(a in arb_arc()) {
        assert_serde_roundtrip(&EntityGeometry::Arc(a));
    }

    // Validation.
    #[test]
    fn prop5_validate(a in arb_arc()) {
        assert_validates(&EntityGeometry::Arc(a), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(a in arb_arc()) {
        let tol = Tol::default();
        assert_deterministic(&EntityGeometry::Arc(a), on_arc(&a, 0.5), &tol);
    }
}

/// Nonfinite coordinates fail validation.
#[test]
fn validate_no_finito_es_err() {
    let bad = geo(f64::NAN, 0.0, 1.0, 0.0, 1.0);
    assert!(bad.validate(&Tol::default()).is_err());
}

/// A degenerate radius fails validation.
#[test]
fn validate_radio_degenerado_es_err() {
    let zero = geo(0.0, 0.0, 0.0, 0.0, 1.0);
    assert!(zero.validate(&Tol::default()).is_err());
}
