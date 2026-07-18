//! `Polyline` registration for the Entity Conformance Suite.
//!
//! Fixtures cover open, closed, straight, and bulged polylines.
//!
//! Bulged polylines remain representable only under similarities. Tests use a
//! polyline-specific snap assertion for supported transforms and verify that
//! anisotropic scale is rejected when an arc segment exists.

use af_math::{Point2, Tol, Transform2, Vec2};
use af_model::entity::{EntityGeometry, EntityOps, PolyVertex, PolylineGeo, TransformError};
use proptest::prelude::*;

use super::{
    EntityFixture, approx_pt_eq, assert_bbox_contains_snaps, assert_deterministic,
    assert_hit_negative, assert_hit_positive, assert_serde_roundtrip, assert_validates,
    check_entity,
};

fn poly(verts: &[(f64, f64, f64)], closed: bool) -> EntityGeometry {
    let vs = verts
        .iter()
        .map(|&(x, y, b)| PolyVertex::new(Point2::new(x, y), b))
        .collect();
    EntityGeometry::Polyline(PolylineGeo::new(vs, closed))
}

/// Open and closed straight fixtures plus semicircle and D-shaped bulged fixtures.
fn fixtures(tol: &Tol) -> Vec<EntityFixture> {
    let near = 2.0 * tol.point_merge;
    vec![
        EntityFixture {
            label: "polyline/open-lines",
            geo: poly(
                &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)],
                false,
            ),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(5.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 5.0),
                Point2::new(10.0, 10.0),
            ],
            off_geometry: vec![
                Point2::new(5.0, near),
                Point2::new(5.0, 5.0),
                Point2::new(0.0, 10.0),
            ],
        },
        EntityFixture {
            label: "polyline/closed-square",
            geo: poly(
                &[
                    (0.0, 0.0, 0.0),
                    (10.0, 0.0, 0.0),
                    (10.0, 10.0, 0.0),
                    (0.0, 10.0, 0.0),
                ],
                true,
            ),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(5.0, 0.0),
                Point2::new(10.0, 10.0),
                Point2::new(0.0, 5.0),
            ],
            off_geometry: vec![Point2::new(5.0, 5.0), Point2::new(5.0, near)],
        },
        EntityFixture {
            // A bulge-1 semicircle on chord (0,0) to (2,0).
            label: "polyline/semicircle",
            geo: poly(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], false),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(1.0, -1.0),
            ],
            off_geometry: vec![
                Point2::new(1.0, 0.0),  // Center inside the box, off the curve.
                Point2::new(1.0, 1.0),  // Opposite side of the arc.
                Point2::new(1.0, -0.5), // Radially inside.
            ],
        },
        EntityFixture {
            // D shape: lower arc closed by its straight chord.
            label: "polyline/d-shape",
            geo: poly(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], true),
            on_geometry: vec![
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 0.0),
                Point2::new(1.0, -1.0), // Arc midpoint.
                Point2::new(1.0, 0.0),  // Closing-chord midpoint.
            ],
            off_geometry: vec![Point2::new(1.0, 0.5), Point2::new(1.0, -0.4)],
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

/// Verifies that a semicircular segment box is not merely its chord box.
#[test]
fn bbox_semicirculo_no_es_la_cuerda() {
    let g = poly(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], false);
    let bb = g.bbox();
    // The horizontal chord has zero height; the arc reaches y=-1.
    assert!(bb.height() > 0.9);
    assert!((bb.min.y + 1.0).abs() < 1e-9);
}

// Property strategies.

/// Valid polyline with separated vertices and arbitrary straight or arc bulges.
fn arb_polyline() -> impl Strategy<Value = PolylineGeo> {
    (2usize..=5)
        .prop_flat_map(|n| {
            (
                prop::collection::vec(-100.0f64..100.0, n),
                prop::collection::vec(-100.0f64..100.0, n),
                prop::collection::vec(prop_oneof![Just(0.0f64), -1.5f64..1.5], n),
                any::<bool>(),
            )
        })
        .prop_map(|(xs, ys, bulges, closed)| build(&xs, &ys, &bulges, closed, 300.0))
}

/// Moderate-bulge polyline for similarity coherence tests.
fn arb_polyline_arc() -> impl Strategy<Value = PolylineGeo> {
    (2usize..=4)
        .prop_flat_map(|n| {
            (
                prop::collection::vec(-40.0f64..40.0, n),
                prop::collection::vec(-40.0f64..40.0, n),
                prop::collection::vec(prop_oneof![Just(0.0f64), 0.2f64..1.2, -1.2f64..-0.2], n),
                any::<bool>(),
            )
        })
        .prop_map(|(xs, ys, bulges, closed)| build(&xs, &ys, &bulges, closed, 200.0))
}

/// Arc polyline guaranteed to contain a nonzero bulge.
fn arb_polyline_with_bulge() -> impl Strategy<Value = PolylineGeo> {
    (2usize..=4)
        .prop_flat_map(|n| {
            (
                prop::collection::vec(-40.0f64..40.0, n),
                prop::collection::vec(-40.0f64..40.0, n),
                0.3f64..1.0,
                any::<bool>(),
            )
        })
        .prop_map(|(xs, ys, b0, closed)| {
            let bulges: Vec<f64> = (0..xs.len())
                .map(|i| if i == 0 { b0 } else { 0.0 })
                .collect();
            build(&xs, &ys, &bulges, closed, 200.0)
        })
}

fn build(xs: &[f64], ys: &[f64], bulges: &[f64], closed: bool, spacing: f64) -> PolylineGeo {
    let verts = xs
        .iter()
        .zip(ys.iter())
        .zip(bulges.iter())
        .enumerate()
        .map(|(i, ((x, y), b))| PolyVertex::new(Point2::new(x + i as f64 * spacing, *y), *b))
        .collect();
    PolylineGeo::new(verts, closed)
}

prop_compose! {
    /// Similarity composed of rotation, uniform scale, optional reflection, and translation.
    fn arb_similarity()(
        theta in 0.0f64..std::f64::consts::TAU,
        s in prop_oneof![-8.0f64..-0.2, 0.2f64..8.0],
        tx in -1.0e3f64..1.0e3,
        ty in -1.0e3f64..1.0e3,
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

/// Verifies indexed snap coherence and containment under a similarity.
fn assert_similarity_coherent(poly: &PolylineGeo, t: &Transform2, tol: &Tol) {
    let g = EntityGeometry::Polyline(poly.clone());
    let moved = g
        .transform(t)
        .expect("una similaridad mantiene la polilínea");
    let osnaps = g.snap_points();
    let msnaps = moved.snap_points();
    assert_eq!(
        osnaps.len(),
        msnaps.len(),
        "similaridad preserva el nº de snaps"
    );
    for (o, m) in osnaps.iter().zip(msnaps.iter()) {
        assert_eq!(o.kind, m.kind, "similaridad preserva la clase de snap");
        assert!(
            approx_pt_eq(t.apply(o.point), m.point, tol),
            "snap incoherente: T({:?}) = {:?} != {:?}",
            o.point,
            t.apply(o.point),
            m.point
        );
    }
    assert_bbox_contains_snaps(&moved, tol);
}

proptest! {
    // Bounding-box containment.
    #[test]
    fn prop1_bbox_contiene_snaps(p in arb_polyline()) {
        assert_bbox_contains_snaps(&EntityGeometry::Polyline(p), &Tol::default());
    }

    // Segment midpoints hit and distant points miss.
    #[test]
    fn prop2_hit(p in arb_polyline()) {
        let tol = Tol::default();
        let g = EntityGeometry::Polyline(p.clone());
        if let Some(seg) = p.segments().next() {
            assert_hit_positive(&g, seg.midpoint(), tol.point_merge);
        }
        let bb = g.bbox();
        let far = Point2::new(bb.max.x + 1.0e6, bb.max.y + 1.0e6);
        assert_hit_negative(&g, far, tol.point_merge);
    }

    // Similarities preserve straight and arc segments.
    #[test]
    fn prop3_transform_similaridad(p in arb_polyline_arc(), t in arb_similarity()) {
        assert_similarity_coherent(&p, &t, &Tol::default());
    }

    // Nonuniform scale with a bulge is rejected.
    #[test]
    fn prop3_transform_no_uniforme_con_bulge_es_err(p in arb_polyline_with_bulge(), t in arb_non_uniform()) {
        prop_assert_eq!(
            EntityGeometry::Polyline(p).transform(&t),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    // Serialization round trip.
    #[test]
    fn prop4_serde_roundtrip(p in arb_polyline()) {
        assert_serde_roundtrip(&EntityGeometry::Polyline(p));
    }

    // Validation.
    #[test]
    fn prop5_validate(p in arb_polyline()) {
        assert_validates(&EntityGeometry::Polyline(p), &Tol::default());
    }

    // Determinism.
    #[test]
    fn prop6_determinismo(p in arb_polyline()) {
        let tol = Tol::default();
        let probe = p.segments().next().map_or(Point2::ORIGIN, |s| s.midpoint());
        assert_deterministic(&EntityGeometry::Polyline(p), probe, &tol);
    }
}

/// Too few, nonfinite, or coincident vertices fail validation.
#[test]
fn validate_casos_invalidos() {
    let tol = Tol::default();
    assert!(poly(&[(0.0, 0.0, 0.0)], false).validate(&tol).is_err());
    assert!(
        poly(&[(0.0, 0.0, 0.0), (f64::NAN, 0.0, 0.0)], false)
            .validate(&tol)
            .is_err()
    );
    assert!(
        poly(&[(0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (1.0, 1.0, 0.0)], false)
            .validate(&tol)
            .is_err()
    );
}
