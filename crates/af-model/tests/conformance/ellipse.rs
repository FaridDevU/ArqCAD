//! `Ellipse` registration for the Entity Conformance Suite.
//!
//! Fixtures cover full, partial, rotated, large, and small ellipses.
//!
//! Ellipse transforms use specific assertions because rotations can change which
//! axis vertices fall within the sweep. Similarities transform the center and
//! midpoint, scale the major axis, and preserve ratio and sweep. Anisotropic
//! scale is rejected.

use af_math::{Point2, Tol, Transform2, Vec2};
use af_model::entity::{EllipseGeo, EntityGeometry, EntityOps, TransformError};
use proptest::prelude::*;

use super::{
    EntityFixture, approx_eq_scaled, approx_pt_eq, assert_bbox_contains_snaps,
    assert_deterministic, assert_hit_negative, assert_hit_positive, assert_serde_roundtrip,
    assert_validates, check_entity,
};

fn geo(cx: f64, cy: f64, a: f64, ratio: f64, rot: f64, sp: f64, ep: f64) -> EntityGeometry {
    EntityGeometry::Ellipse(EllipseGeo::new(Point2::new(cx, cy), a, ratio, rot, sp, ep))
}

/// Point at fraction `frac` of ellipse `e`'s sweep.
fn on_ellipse(e: &EllipseGeo, frac: f64) -> Point2 {
    e.ellipse().point_at(e.start_param + e.sweep() * frac)
}

/// Ellipse fixtures whose off-geometry points include the center.
fn fixtures(_tol: &Tol) -> Vec<EntityFixture> {
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
    // (cx, cy, a, ratio, rot, start, end)
    let cases: [(f64, f64, f64, f64, f64, f64, f64); 5] = [
        (0.0, 0.0, 3.0, 0.5, 0.0, 0.0, TAU),        // Full and aligned.
        (2.0, -3.0, 5.0, 0.4, FRAC_PI_4, 0.3, 4.0), // Rotated arc.
        (1.0, 1.0, 4.0, 0.6, FRAC_PI_2, 0.0, PI),   // Rotated half sweep.
        (0.0, 0.0, 1.0e5, 0.7, 0.2, 0.5, 2.7),      // Large.
        (0.0, 0.0, 1.0e-2, 0.5, 0.0, 0.2, 3.0),     // Small.
    ];
    cases
        .iter()
        .map(|&(cx, cy, a, ratio, rot, sp, ep)| {
            let e = EllipseGeo::new(Point2::new(cx, cy), a, ratio, rot, sp, ep);
            EntityFixture {
                label: "ellipse",
                geo: EntityGeometry::Ellipse(e),
                on_geometry: vec![
                    on_ellipse(&e, 0.0),
                    on_ellipse(&e, 0.25),
                    on_ellipse(&e, 0.5),
                    on_ellipse(&e, 0.75),
                    on_ellipse(&e, 1.0),
                ],
                // Center and one radially distant point.
                off_geometry: vec![Point2::new(cx, cy), Point2::new(cx + a * 3.0, cy)],
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
    /// Valid ellipse with nondegenerate axes and a bounded sweep.
    fn arb_ellipse()(
        cx in -1.0e4f64..1.0e4,
        cy in -1.0e4f64..1.0e4,
        a in 1.0e-1f64..1.0e4,
        ratio in 0.1f64..1.0,
        rot in 0.0f64..std::f64::consts::TAU,
        start in 0.0f64..std::f64::consts::TAU,
        sweep in 0.1f64..(std::f64::consts::TAU - 0.1),
    ) -> EllipseGeo {
        EllipseGeo::new(Point2::new(cx, cy), a, ratio, rot, start, start + sweep)
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
    /// Nonuniform scale with `|sy|` at least twice `|sx|`.
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
    fn prop1_bbox_contiene_snaps(e in arb_ellipse()) {
        assert_bbox_contains_snaps(&EntityGeometry::Ellipse(e), &Tol::default());
    }

    // Curve points hit; the center does not.
    #[test]
    fn prop2_hit(e in arb_ellipse(), frac in 0.0f64..=1.0) {
        let tol = Tol::default();
        let g = EntityGeometry::Ellipse(e);
        assert_hit_positive(&g, on_ellipse(&e, frac), tol.point_merge);
        assert_hit_negative(&g, e.center, tol.point_merge);
    }

    // Similarities preserve ellipse geometry.
    #[test]
    fn prop3_transform_similaridad(e in arb_ellipse(), t in arb_similarity()) {
        let tol = Tol::default();
        let g = EntityGeometry::Ellipse(e);
        let moved = g.transform(&t).expect("una similaridad mantiene la elipse");
        let EntityGeometry::Ellipse(m) = moved else { unreachable!("Ellipse -> Ellipse") };

        prop_assert!(approx_pt_eq(t.apply(e.center), m.center, &tol));
        let (scale, _) = t.scale_factors();
        prop_assert!(approx_eq_scaled(m.semi_major, scale * e.semi_major, &tol));
        prop_assert!(approx_eq_scaled(m.ratio, e.ratio, &tol));
        prop_assert!(approx_eq_scaled(m.sweep(), e.sweep(), &tol));
        prop_assert!(approx_pt_eq(
            t.apply(e.ellipse().midpoint()),
            m.ellipse().midpoint(),
            &tol
        ));
        assert_bbox_contains_snaps(&EntityGeometry::Ellipse(m), &tol);
    }

    // Nonuniform scale is rejected.
    #[test]
    fn prop3_transform_no_uniforme_es_err(e in arb_ellipse(), t in arb_non_uniform()) {
        let g = EntityGeometry::Ellipse(e);
        prop_assert_eq!(
            g.transform(&t),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(e in arb_ellipse()) {
        assert_serde_roundtrip(&EntityGeometry::Ellipse(e));
    }

    // Validation.
    #[test]
    fn prop5_validate(e in arb_ellipse()) {
        assert_validates(&EntityGeometry::Ellipse(e), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(e in arb_ellipse()) {
        let tol = Tol::default();
        assert_deterministic(&EntityGeometry::Ellipse(e), on_ellipse(&e, 0.5), &tol);
    }
}

/// Nonfinite coordinates fail validation.
#[test]
fn validate_no_finito_es_err() {
    let bad = geo(f64::NAN, 0.0, 3.0, 0.5, 0.0, 0.0, 1.0);
    assert!(bad.validate(&Tol::default()).is_err());
}

/// A degenerate semiaxis fails validation.
#[test]
fn validate_semieje_degenerado_es_err() {
    let flat = geo(0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 1.0);
    assert!(flat.validate(&Tol::default()).is_err());
}
