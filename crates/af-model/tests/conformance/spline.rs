//! `Spline` registration for the Entity Conformance Suite.
//!
//! Fixtures cover open and closed splines and use the generic suite.
//!
//! Any affine transform maps fit points and their snaps exactly, so splines use
//! the generic transform-coherence assertion.

use af_math::{Point2, Tol};
use af_model::entity::{EntityGeometry, EntityOps, SplineGeo};
use proptest::prelude::*;

use super::{
    EntityFixture, arb_valid_affine, assert_bbox_contains_snaps, assert_deterministic,
    assert_hit_negative, assert_hit_positive, assert_serde_roundtrip, assert_transform_coherent,
    assert_validates, check_entity,
};

fn spline(fit: &[(f64, f64)], closed: bool) -> EntityGeometry {
    let pts = fit.iter().map(|&(x, y)| Point2::new(x, y)).collect();
    EntityGeometry::Spline(SplineGeo::new(pts, closed))
}

/// Evaluates one on-curve point for a spline fixture.
fn on(geo: &EntityGeometry, frac: f64) -> Point2 {
    let EntityGeometry::Spline(g) = geo else {
        unreachable!("fixture de spline")
    };
    let sp = g.fit_spline().expect("spline válida");
    let (t0, t1) = sp.param_range();
    sp.eval(t0 + (t1 - t0) * frac)
}

/// Open S-shaped and closed periodic quadrilateral fixtures.
fn fixtures(_tol: &Tol) -> Vec<EntityFixture> {
    let open = spline(
        &[(0.0, 0.0), (1.0, 2.0), (3.0, -1.0), (4.0, 1.0), (6.0, 0.0)],
        false,
    );
    let closed = spline(&[(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (0.0, 3.0)], true);
    vec![
        EntityFixture {
            label: "spline/open-S",
            on_geometry: vec![
                on(&open, 0.0),
                on(&open, 0.25),
                on(&open, 0.5),
                on(&open, 0.75),
                on(&open, 1.0),
            ],
            off_geometry: vec![Point2::new(3.0, 5.0), Point2::new(-2.0, -2.0)],
            geo: open,
        },
        EntityFixture {
            label: "spline/closed-quad",
            on_geometry: vec![on(&closed, 0.1), on(&closed, 0.4), on(&closed, 0.85)],
            off_geometry: vec![Point2::new(1.5, 1.5), Point2::new(10.0, 10.0)],
            geo: closed,
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

/// Valid open spline with separated fit points.
fn arb_spline() -> impl Strategy<Value = SplineGeo> {
    (2usize..=5)
        .prop_flat_map(|n| prop::collection::vec(-100.0f64..100.0, n))
        .prop_map(|ys| {
            let pts = ys
                .iter()
                .enumerate()
                .map(|(i, &y)| Point2::new(i as f64 * 100.0, y))
                .collect();
            SplineGeo::new(pts, false)
        })
}

proptest! {
    // Bounding-box containment.
    #[test]
    fn prop1_bbox_contiene_snaps(g in arb_spline()) {
        assert_bbox_contains_snaps(&EntityGeometry::Spline(g), &Tol::default());
    }

    // Curve points hit and distant points miss.
    #[test]
    fn prop2_hit(g in arb_spline()) {
        let tol = Tol::default();
        let geo = EntityGeometry::Spline(g.clone());
        let sp = g.fit_spline().unwrap();
        let (t0, t1) = sp.param_range();
        assert_hit_positive(&geo, sp.eval(0.5 * (t0 + t1)), tol.point_merge);
        let bb = geo.bbox();
        let far = Point2::new(bb.max.x + 1.0e6, bb.max.y + 1.0e6);
        assert_hit_negative(&geo, far, tol.point_merge);
    }

    // Any valid affine transform preserves fit-point snaps.
    #[test]
    fn prop3_transform_coherente(g in arb_spline(), t in arb_valid_affine()) {
        assert_transform_coherent(&EntityGeometry::Spline(g), &t, &Tol::default());
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(g in arb_spline()) {
        assert_serde_roundtrip(&EntityGeometry::Spline(g));
    }

    // Validation.
    #[test]
    fn prop5_validate(g in arb_spline()) {
        assert_validates(&EntityGeometry::Spline(g), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(g in arb_spline()) {
        let tol = Tol::default();
        let sp = g.fit_spline().unwrap();
        let (t0, t1) = sp.param_range();
        let probe = sp.eval(0.5 * (t0 + t1));
        assert_deterministic(&EntityGeometry::Spline(g), probe, &tol);
    }
}

/// Too few, nonfinite, or coincident fit points fail validation.
#[test]
fn validate_casos_invalidos() {
    let tol = Tol::default();
    assert!(spline(&[(0.0, 0.0)], false).validate(&tol).is_err());
    assert!(
        spline(&[(0.0, 0.0), (f64::NAN, 0.0)], false)
            .validate(&tol)
            .is_err()
    );
    assert!(
        spline(&[(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)], false)
            .validate(&tol)
            .is_err()
    );
}
