//! `Xline` and `Ray` registration for the Entity Conformance Suite.
//!
//! # Infinite semantics
//!
//! The generic distant-point miss assertion assumes bounded geometry and is false
//! for infinite lines. These tests compose the individual checks with geometry-
//! specific on/off points instead of using [`super::check_entity`].
//!
//! An xline hits arbitrarily distant points on its support line, while a ray does
//! not hit behind its origin.

use af_math::{Point2, Tol, Vec2};
use af_model::entity::{EntityGeometry, EntityOps, GeomIssue, RayGeo, XlineGeo};
use proptest::prelude::*;

use super::{
    arb_valid_affine, assert_bbox_contains_snaps, assert_deterministic, assert_hit_negative,
    assert_hit_positive, assert_serde_roundtrip, assert_transform_coherent, assert_validates,
};

/// Deterministic checks without the bounded-geometry distant-point assertion.
fn check_inf(geo: &EntityGeometry, on: &[Point2], off: &[Point2], tol: &Tol) {
    // The proxy box contains snaps and sampled geometry points.
    assert_bbox_contains_snaps(geo, tol);
    let bb = geo.bbox();
    for p in on {
        assert!(
            bb.expand(tol.linear).contains_point(*p),
            "punto on {p:?} fuera de la bbox {bb:?}"
        );
    }
    // Geometry points hit; off-geometry points miss.
    for p in on {
        assert_hit_positive(geo, *p, tol.point_merge);
    }
    for p in off {
        assert_hit_negative(geo, *p, tol.point_merge);
    }
    // Serialization, validation, and determinism.
    assert_serde_roundtrip(geo);
    assert_validates(geo, tol);
    let probe = on.first().copied().unwrap_or(Point2::ORIGIN);
    assert_deterministic(geo, probe, tol);
}

// ---------------------------------------------------------------------------
// Xline
// ---------------------------------------------------------------------------

fn xline(px: f64, py: f64, dx: f64, dy: f64) -> EntityGeometry {
    EntityGeometry::Xline(XlineGeo::new(Point2::new(px, py), Vec2::new(dx, dy)))
}

#[test]
fn xline_fixtures_deterministas() {
    let tol = Tol::default();
    let near = 2.0 * tol.point_merge;

    // Horizontal line: on-points lie at y=0, while off-points shift in y.
    check_inf(
        &xline(0.0, 0.0, 1.0, 0.0),
        &[
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 0.0),
            Point2::new(-1.0e6, 0.0),
        ],
        &[Point2::new(5.0, near), Point2::new(10.0, 1.0)],
        &tol,
    );

    // Vertical line: on-points lie at x=2, while off-points shift in x.
    check_inf(
        &xline(2.0, 3.0, 0.0, 1.0),
        &[
            Point2::new(2.0, 3.0),
            Point2::new(2.0, 500.0),
            Point2::new(2.0, -500.0),
        ],
        &[Point2::new(2.0 + near, 100.0), Point2::new(3.0, 3.0)],
        &tol,
    );

    // Diagonal y=x with a nonunit direction normalized by hit testing.
    check_inf(
        &xline(0.0, 0.0, 1.0, 1.0),
        &[
            Point2::new(0.0, 0.0),
            Point2::new(5.0, 5.0),
            Point2::new(-300.0, -300.0),
        ],
        &[Point2::new(0.0, 1.0), Point2::new(5.0, 6.0)],
        &tol,
    );
}

/// An xline hits its support line at any distance.
#[test]
fn xline_hit_lejano_sobre_la_recta_es_positivo() {
    let g = xline(0.0, 0.0, 1.0, 0.0);
    assert_eq!(g.hit(Point2::new(1.0e9, 0.0), 1e-6), Some(0.0));
    // `(K, K)` remains on the distant diagonal y=x.
    let d = xline(0.0, 0.0, 1.0, 1.0);
    assert_eq!(d.hit(Point2::new(1.0e6, 1.0e6), 1e-3), Some(0.0));
}

#[test]
fn xline_validate_casos_invalidos() {
    let tol = Tol::default();
    assert_eq!(
        xline(0.0, 0.0, 0.0, 0.0).validate(&tol),
        Err(GeomIssue::ZeroDirection)
    );
    assert_eq!(
        xline(f64::NAN, 0.0, 1.0, 0.0).validate(&tol),
        Err(GeomIssue::NonFinite)
    );
}

prop_compose! {
    fn arb_xline()(
        px in -1.0e3f64..1.0e3,
        py in -1.0e3f64..1.0e3,
        // Any angle yields a nondegenerate unit direction.
        theta in 0.0f64..std::f64::consts::TAU,
    ) -> EntityGeometry {
        xline(px, py, theta.cos(), theta.sin())
    }
}

proptest! {
    #[test]
    fn xline_prop1_bbox_contiene_snaps(g in arb_xline()) {
        assert_bbox_contains_snaps(&g, &Tol::default());
    }

    #[test]
    fn xline_prop3_transform_coherente(g in arb_xline(), t in arb_valid_affine()) {
        assert_transform_coherent(&g, &t, &Tol::default());
    }

    #[test]
    fn xline_prop4_serde_roundtrip(g in arb_xline()) {
        assert_serde_roundtrip(&g);
    }

    #[test]
    fn xline_prop5_validate(g in arb_xline()) {
        assert_validates(&g, &Tol::default());
    }
}

// ---------------------------------------------------------------------------
// Ray
// ---------------------------------------------------------------------------

fn ray(ox: f64, oy: f64, dx: f64, dy: f64) -> EntityGeometry {
    EntityGeometry::Ray(RayGeo::new(Point2::new(ox, oy), Vec2::new(dx, dy)))
}

#[test]
fn ray_fixtures_deterministas() {
    let tol = Tol::default();
    let near = 2.0 * tol.point_merge;

    // Positive-X ray: on-points are ahead; off-points are perpendicular or behind.
    check_inf(
        &ray(0.0, 0.0, 1.0, 0.0),
        &[
            Point2::new(0.0, 0.0),
            Point2::new(5.0, 0.0),
            Point2::new(1.0e6, 0.0),
        ],
        &[
            Point2::new(5.0, near),
            Point2::new(-5.0, 0.0),
            Point2::new(-near, 0.0),
        ],
        &tol,
    );

    // Ray toward (-1,-1): on-points are forward and off-points are behind.
    check_inf(
        &ray(10.0, 10.0, -1.0, -1.0),
        &[
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 0.0),
            Point2::new(-90.0, -90.0),
        ],
        &[Point2::new(20.0, 20.0), Point2::new(11.0, 10.0)],
        &tol,
    );
}

/// A ray hits forward at any distance but never behind its origin.
#[test]
fn ray_hit_solo_hacia_adelante() {
    let g = ray(0.0, 0.0, 1.0, 0.0);
    assert_eq!(g.hit(Point2::new(1.0e9, 0.0), 1e-6), Some(0.0));
    // Behind the origin, distance is measured to the endpoint rather than the line.
    assert_eq!(g.hit(Point2::new(-1.0e6, 0.0), 1.0), None);
}

#[test]
fn ray_validate_casos_invalidos() {
    let tol = Tol::default();
    assert_eq!(
        ray(1.0, 1.0, 0.0, 0.0).validate(&tol),
        Err(GeomIssue::ZeroDirection)
    );
    assert_eq!(
        ray(0.0, f64::INFINITY, 1.0, 0.0).validate(&tol),
        Err(GeomIssue::NonFinite)
    );
}

prop_compose! {
    fn arb_ray()(
        ox in -1.0e3f64..1.0e3,
        oy in -1.0e3f64..1.0e3,
        theta in 0.0f64..std::f64::consts::TAU,
    ) -> EntityGeometry {
        ray(ox, oy, theta.cos(), theta.sin())
    }
}

proptest! {
    #[test]
    fn ray_prop1_bbox_contiene_snaps(g in arb_ray()) {
        assert_bbox_contains_snaps(&g, &Tol::default());
    }

    #[test]
    fn ray_prop3_transform_coherente(g in arb_ray(), t in arb_valid_affine()) {
        assert_transform_coherent(&g, &t, &Tol::default());
    }

    #[test]
    fn ray_prop4_serde_roundtrip(g in arb_ray()) {
        assert_serde_roundtrip(&g);
    }

    #[test]
    fn ray_prop5_validate(g in arb_ray()) {
        assert_validates(&g, &Tol::default());
    }
}
