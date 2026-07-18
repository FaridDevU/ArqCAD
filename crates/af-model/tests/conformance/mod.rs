//! Parameterized Entity Conformance Suite.
//!
//! This module provides generic assertions. Each entity module supplies fixtures
//! and property strategies that call them.
//!
//! # Verified properties
//!
//! 1. Bounding boxes contain snaps and sampled fixture geometry.
//! 2. Hit testing succeeds on geometry and fails beyond tolerance.
//! 3. Supported affine transforms preserve snap geometry and containment.
//! 4. Serialization round-trips and is deterministic.
//! 5. Finite valid geometry passes validation.
//! 6. Pure geometry operations are deterministic.
//!
//! The engine depends only on [`EntityGeometry`], [`EntityOps`], and
//! [`EntityFixture`]. Fixtures provide explicit on-geometry points because
//! semantic snaps such as centers need not lie on a curve.

mod arc;
mod circle;
mod ellipse;
mod infinite;
mod line;
mod polyline;
mod snaps;
mod spline;
mod wipeout;

use af_math::{Point2, Tol, Transform2, Vec2};
use af_model::entity::{EntityGeometry, EntityOps};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Generic fixtures and tolerance-aware comparisons.
// ---------------------------------------------------------------------------

/// Concrete entity fixture for deterministic properties.
///
/// Entity-specific points distinguish real geometry from nearby misses.
pub(crate) struct EntityFixture {
    /// Human-readable failure label.
    pub(crate) label: &'static str,
    /// Concrete enum geometry under test.
    pub(crate) geo: EntityGeometry,
    /// Points exactly on the geometry.
    pub(crate) on_geometry: Vec<Point2>,
    /// Points clearly beyond hit tolerance.
    pub(crate) off_geometry: Vec<Point2>,
}

/// Relative and absolute equality scaled from `tol.linear`.
pub(crate) fn approx_eq_scaled(a: f64, b: f64, tol: &Tol) -> bool {
    (a - b).abs() <= tol.linear * (1.0 + a.abs().max(b.abs()))
}

/// Applies [`approx_eq_scaled`] component-wise to points.
pub(crate) fn approx_pt_eq(a: Point2, b: Point2, tol: &Tol) -> bool {
    approx_eq_scaled(a.x, b.x, tol) && approx_eq_scaled(a.y, b.y, tol)
}

// ---------------------------------------------------------------------------
// Generic property assertions over `EntityGeometry` and `EntityOps`.
// ---------------------------------------------------------------------------

/// Verifies that `bbox()` contains every snap point.
///
/// Containment uses `tol.linear` for midpoint rounding.
pub(crate) fn assert_bbox_contains_snaps(geo: &EntityGeometry, tol: &Tol) {
    let bb = geo.bbox();
    for s in geo.snap_points() {
        assert!(
            bb.expand(tol.linear).contains_point(s.point),
            "snap {:?} fuera de la bbox {bb:?}",
            s.point
        );
    }
}

/// Verifies a hit for a point on geometry.
pub(crate) fn assert_hit_positive(geo: &EntityGeometry, on: Point2, hit_tol: f64) {
    match geo.hit(on, hit_tol) {
        Some(d) => assert!(
            (0.0..=hit_tol).contains(&d),
            "hit sobre la geometría en {on:?} devolvió distancia fuera de rango: {d}"
        ),
        None => panic!("hit negativo sobre un punto que está en la geometría: {on:?}"),
    }
}

/// Verifies a miss for a point beyond tolerance.
pub(crate) fn assert_hit_negative(geo: &EntityGeometry, off: Point2, hit_tol: f64) {
    assert!(
        geo.hit(off, hit_tol).is_none(),
        "hit positivo sobre un punto fuera de tolerancia: {off:?}"
    );
}

/// Verifies coherent snap transformation under affine transform `t`.
///
/// Transformed snap kinds and positions must match, and their box must contain them.
pub(crate) fn assert_transform_coherent(geo: &EntityGeometry, t: &Transform2, tol: &Tol) {
    let moved = geo
        .transform(t)
        .expect("Line/Point admiten cualquier afín (nunca Err)");

    let orig = geo.snap_points();
    let tsnaps = moved.snap_points();
    assert_eq!(
        orig.len(),
        tsnaps.len(),
        "transform preserva el número de snaps"
    );
    for (o, m) in orig.iter().zip(tsnaps.iter()) {
        assert_eq!(o.kind, m.kind, "transform preserva la clase de snap");
        assert!(
            approx_pt_eq(t.apply(o.point), m.point, tol),
            "snap transformado incoherente: T({:?}) = {:?} != {:?}",
            o.point,
            t.apply(o.point),
            m.point
        );
    }
    assert_bbox_contains_snaps(&moved, tol);
}

/// Verifies serialization identity and deterministic output.
pub(crate) fn assert_serde_roundtrip(geo: &EntityGeometry) {
    let json = serde_json::to_string(geo).expect("serializa");
    let back: EntityGeometry = serde_json::from_str(&json).expect("deserializa");
    assert_eq!(&back, geo, "roundtrip serde no es identidad");
    // Repeated serialization yields the same bytes.
    let json2 = serde_json::to_string(&back).expect("serializa");
    assert_eq!(json, json2, "serialización no determinista");
}

/// Verifies that finite geometry passes validation.
pub(crate) fn assert_validates(geo: &EntityGeometry, tol: &Tol) {
    assert!(
        geo.validate(tol).is_ok(),
        "geometría finita debería validar"
    );
}

/// Verifies deterministic pure geometry operations.
pub(crate) fn assert_deterministic(geo: &EntityGeometry, probe: Point2, tol: &Tol) {
    assert_eq!(geo.bbox(), geo.bbox(), "bbox no determinista");
    assert_eq!(
        geo.snap_points(),
        geo.snap_points(),
        "snap_points no determinista"
    );
    assert_eq!(
        geo.hit(probe, tol.point_merge),
        geo.hit(probe, tol.point_merge),
        "hit no determinista"
    );
}

/// Runs deterministic checks for one fixture; entity modules test transforms.
pub(crate) fn check_entity(fix: &EntityFixture, tol: &Tol) {
    // The box contains snaps and sampled geometry.
    assert_bbox_contains_snaps(&fix.geo, tol);
    let bb = fix.geo.bbox();
    for p in &fix.on_geometry {
        assert!(
            bb.expand(tol.linear).contains_point(*p),
            "[{}] punto de la geometría {p:?} fuera de la bbox {bb:?}",
            fix.label
        );
    }

    // Geometry points hit and off-geometry points miss.
    for p in &fix.on_geometry {
        assert_hit_positive(&fix.geo, *p, tol.point_merge);
    }
    for p in &fix.off_geometry {
        assert_hit_negative(&fix.geo, *p, tol.point_merge);
    }
    // A deliberately distant point must miss bounded geometry.
    let far = Point2::new(bb.max.x + 1.0e9, bb.max.y + 1.0e9);
    assert_hit_negative(&fix.geo, far, tol.point_merge);

    // Serialization and validation.
    assert_serde_roundtrip(&fix.geo);
    assert_validates(&fix.geo, tol);

    // Determinism.
    let probe = fix.on_geometry.first().copied().unwrap_or(Point2::ORIGIN);
    assert_deterministic(&fix.geo, probe, tol);
}

// ---------------------------------------------------------------------------
// Shared strategy for finite invertible affine transforms.
// ---------------------------------------------------------------------------

prop_compose! {
    /// Rotation, nonuniform scale, optional reflection, and translation with a
    /// determinant kept away from zero.
    pub(crate) fn arb_valid_affine()(
        theta in 0.0f64..std::f64::consts::TAU,
        sx in prop_oneof![-10.0f64..-0.1, 0.1f64..10.0],
        sy in prop_oneof![-10.0f64..-0.1, 0.1f64..10.0],
        tx in -1.0e4f64..1.0e4,
        ty in -1.0e4f64..1.0e4,
    ) -> Transform2 {
        Transform2::rotate(theta)
            .then(Transform2::scale(sx, sy))
            .then(Transform2::translate(Vec2::new(tx, ty)))
    }
}

// ---------------------------------------------------------------------------
// Point registration; line uses its own module.
// ---------------------------------------------------------------------------

mod point {
    use super::{
        EntityFixture, approx_pt_eq, arb_valid_affine, assert_bbox_contains_snaps,
        assert_deterministic, assert_hit_negative, assert_hit_positive, assert_serde_roundtrip,
        assert_transform_coherent, assert_validates, check_entity,
    };
    use af_math::{Point2, Tol};
    use af_model::entity::{EntityGeometry, EntityOps, PointGeo};
    use proptest::prelude::*;

    fn geo(x: f64, y: f64) -> EntityGeometry {
        EntityGeometry::Point(PointGeo::new(Point2::new(x, y)))
    }

    /// Normal, large, and small point fixtures with naturally degenerate boxes.
    fn fixtures(tol: &Tol) -> Vec<EntityFixture> {
        // Near miss just beyond hit tolerance.
        let near = 2.0 * tol.point_merge;
        vec![
            EntityFixture {
                label: "point/normal",
                geo: geo(3.0, 4.0),
                on_geometry: vec![Point2::new(3.0, 4.0)],
                off_geometry: vec![Point2::new(3.0 + near, 4.0), Point2::new(3.0, 5.0)],
            },
            EntityFixture {
                label: "point/huge-1e5",
                geo: geo(1.0e5, -1.0e5),
                on_geometry: vec![Point2::new(1.0e5, -1.0e5)],
                off_geometry: vec![
                    Point2::new(1.0e5 + near, -1.0e5),
                    Point2::new(1.0e5 + 1.0, -1.0e5),
                ],
            },
            EntityFixture {
                label: "point/micro-1e-5",
                geo: geo(1.0e-5, 1.0e-5),
                on_geometry: vec![Point2::new(1.0e-5, 1.0e-5)],
                off_geometry: vec![
                    Point2::new(1.0e-5 + near, 1.0e-5),
                    Point2::new(1.0e-5 + 1.0, 1.0e-5),
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
        fn arb_point()(
            x in -1.0e4f64..1.0e4,
            y in -1.0e4f64..1.0e4,
        ) -> EntityGeometry {
            geo(x, y)
        }
    }

    proptest! {
        // Bounding-box containment.
        #[test]
        fn prop1_bbox_contiene_snaps(g in arb_point()) {
            assert_bbox_contains_snaps(&g, &Tol::default());
        }

        // The node hits; a point one unit away misses.
        #[test]
        fn prop2_hit(g in arb_point()) {
            let tol = Tol::default();
            let pos = g.snap_points()[0].point;
            assert_hit_positive(&g, pos, tol.point_merge);
            assert_hit_negative(&g, Point2::new(pos.x + 1.0, pos.y), tol.point_merge);
        }

        // Transform coherence.
        #[test]
        fn prop3_transform_coherente(g in arb_point(), t in arb_valid_affine()) {
            let tol = Tol::default();
            assert_transform_coherent(&g, &t, &tol);
            // The transformed position remains the transformed snap.
            let moved = g.transform(&t).unwrap();
            prop_assert!(approx_pt_eq(
                t.apply(g.snap_points()[0].point),
                moved.snap_points()[0].point,
                &tol,
            ));
        }

        // Serialization round trip.
        #[test]
        fn prop4_serde_roundtrip(g in arb_point()) {
            assert_serde_roundtrip(&g);
        }

        // Validation.
        #[test]
        fn prop5_validate(g in arb_point()) {
            assert_validates(&g, &Tol::default());
        }

        // Determinism.
        #[test]
        fn prop6_determinismo(g in arb_point()) {
            let tol = Tol::default();
            let probe = g.snap_points()[0].point;
            assert_deterministic(&g, probe, &tol);
        }
    }

    /// Nonfinite coordinates fail validation.
    #[test]
    fn validate_no_finito_es_err() {
        let bad = geo(f64::NAN, 0.0);
        assert!(bad.validate(&Tol::default()).is_err());
    }
}

// ---------------------------------------------------------------------------
// Reserved DXF round-trip hook.
// ---------------------------------------------------------------------------

/// Placeholder for DXF export/import coverage once the I/O crate is available.
#[test]
#[ignore = "DXF round-trip coverage belongs to the af-io-dxf integration suite"]
fn property7_dxf_roundtrip_hook_f8() {
    // Each registered entity should eventually preserve mapped fields across DXF.
}
