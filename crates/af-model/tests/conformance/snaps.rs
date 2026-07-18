//! Exact `snap_points()` coverage for the Entity Conformance Suite.
//!
//! Tests verify exact snap positions, kinds, and order on simple fixtures, plus
//! the cross-entity property that nonsemantic snaps lie on real geometry.
//!
//! A semicircular polyline also covers exact snaps for a bulged arc segment.

use af_math::{Point2, Tol};
use af_model::entity::{
    CircleGeo, EntityGeometry, EntityOps, LineGeo, PointGeo, PolyVertex, PolylineGeo, SnapKind,
    SnapPoint,
};
use proptest::prelude::*;

use super::assert_hit_positive;

// ---------------------------------------------------------------------------
// Exact snap sets, positions, and order on simple fixtures.
// ---------------------------------------------------------------------------

/// Line snaps: both endpoints, then midpoint.
#[test]
fn line_snaps_exactos() {
    let g = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)));
    let snaps: Vec<SnapPoint> = g.snap_points().into_iter().collect();
    assert_eq!(
        snaps,
        vec![
            SnapPoint::new(Point2::new(0.0, 0.0), SnapKind::Endpoint),
            SnapPoint::new(Point2::new(10.0, 0.0), SnapKind::Endpoint),
            SnapPoint::new(Point2::new(5.0, 0.0), SnapKind::Midpoint),
        ]
    );
}

/// Circle snaps: center, then east, north, west, and south quadrants.
#[test]
fn circle_snaps_exactos() {
    let g = EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 10.0));
    let snaps: Vec<SnapPoint> = g.snap_points().into_iter().collect();
    assert_eq!(
        snaps,
        vec![
            SnapPoint::new(Point2::new(0.0, 0.0), SnapKind::Center),
            SnapPoint::new(Point2::new(10.0, 0.0), SnapKind::Quadrant),
            SnapPoint::new(Point2::new(0.0, 10.0), SnapKind::Quadrant),
            SnapPoint::new(Point2::new(-10.0, 0.0), SnapKind::Quadrant),
            SnapPoint::new(Point2::new(0.0, -10.0), SnapKind::Quadrant),
        ]
    );
}

/// Point snap: one node at its position.
#[test]
fn point_snap_exacto() {
    let g = EntityGeometry::Point(PointGeo::new(Point2::new(3.0, 4.0)));
    let snaps: Vec<SnapPoint> = g.snap_points().into_iter().collect();
    assert_eq!(
        snaps,
        vec![SnapPoint::new(Point2::new(3.0, 4.0), SnapKind::Node)]
    );
}

/// Open straight polyline snaps: vertices, then segment midpoints, with no center.
#[test]
fn polyline_recta_snaps_exactos() {
    let g = EntityGeometry::Polyline(PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 10.0), 0.0),
            PolyVertex::new(Point2::new(0.0, 10.0), 0.0),
        ],
        false,
    ));
    let snaps: Vec<SnapPoint> = g.snap_points().into_iter().collect();
    assert_eq!(
        snaps,
        vec![
            SnapPoint::new(Point2::new(0.0, 0.0), SnapKind::Endpoint),
            SnapPoint::new(Point2::new(10.0, 0.0), SnapKind::Endpoint),
            SnapPoint::new(Point2::new(10.0, 10.0), SnapKind::Endpoint),
            SnapPoint::new(Point2::new(0.0, 10.0), SnapKind::Endpoint),
            SnapPoint::new(Point2::new(5.0, 0.0), SnapKind::Midpoint),
            SnapPoint::new(Point2::new(10.0, 5.0), SnapKind::Midpoint),
            SnapPoint::new(Point2::new(5.0, 10.0), SnapKind::Midpoint),
        ]
    );
}

/// Semicircular bulged-polyline snaps for center `(0,0)`, radius 10, sweep `0 → π`.
///
/// The midpoint lies on the curve at `(0,10)`, not at the chord midpoint `(0,0)`.
#[test]
fn polyline_arco_semicircular_snaps_exactos_c0_r10() {
    let g = EntityGeometry::Polyline(PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(10.0, 0.0), 1.0),
            PolyVertex::new(Point2::new(-10.0, 0.0), 0.0),
        ],
        false,
    ));
    let snaps: Vec<SnapPoint> = g.snap_points().into_iter().collect();
    assert_eq!(snaps.len(), 4);
    assert_eq!(
        snaps[0],
        SnapPoint::new(Point2::new(10.0, 0.0), SnapKind::Endpoint)
    );
    assert_eq!(
        snaps[1],
        SnapPoint::new(Point2::new(-10.0, 0.0), SnapKind::Endpoint)
    );
    assert_eq!(snaps[2].kind, SnapKind::Midpoint);
    assert!(
        (snaps[2].point.x).abs() < 1e-9 && (snaps[2].point.y - 10.0).abs() < 1e-9,
        "midpoint del arco debe caer en (0,10) (EN la curva), no en la cuerda: {:?}",
        snaps[2].point
    );
    assert_eq!(
        snaps[3],
        SnapPoint::new(Point2::new(0.0, 0.0), SnapKind::Center)
    );
}

// ---------------------------------------------------------------------------
// Every nonsemantic snap lies on real geometry within tolerance.
// ---------------------------------------------------------------------------

/// Requires a hit for every snap other than semantic `Center` and `Node` snaps.
fn assert_curve_snaps_hit(geo: &EntityGeometry, tol: &Tol) {
    for s in geo.snap_points() {
        if matches!(s.kind, SnapKind::Center | SnapKind::Node) {
            continue;
        }
        assert_hit_positive(geo, s.point, tol.point_merge);
    }
}

prop_compose! {
    fn arb_line()(
        x1 in -1.0e4f64..1.0e4,
        y1 in -1.0e4f64..1.0e4,
        x2 in -1.0e4f64..1.0e4,
        y2 in -1.0e4f64..1.0e4,
    ) -> EntityGeometry {
        EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2)))
    }
}

prop_compose! {
    fn arb_circle()(
        cx in -1.0e4f64..1.0e4,
        cy in -1.0e4f64..1.0e4,
        r in 1.0e-2f64..1.0e4,
    ) -> EntityGeometry {
        EntityGeometry::Circle(CircleGeo::new(Point2::new(cx, cy), r))
    }
}

/// Valid polyline with separated vertices and straight or arc bulges.
fn arb_polyline() -> impl Strategy<Value = EntityGeometry> {
    (2usize..=5)
        .prop_flat_map(|n| {
            (
                prop::collection::vec(-100.0f64..100.0, n),
                prop::collection::vec(-100.0f64..100.0, n),
                prop::collection::vec(prop_oneof![Just(0.0f64), -1.5f64..1.5], n),
                any::<bool>(),
            )
        })
        .prop_map(|(xs, ys, bulges, closed)| {
            let verts = xs
                .iter()
                .zip(ys.iter())
                .zip(bulges.iter())
                .enumerate()
                .map(|(i, ((x, y), b))| PolyVertex::new(Point2::new(x + i as f64 * 300.0, *y), *b))
                .collect();
            EntityGeometry::Polyline(PolylineGeo::new(verts, closed))
        })
}

proptest! {
    #[test]
    fn prop_line_snaps_en_la_geometria(g in arb_line()) {
        assert_curve_snaps_hit(&g, &Tol::default());
    }

    #[test]
    fn prop_circle_snaps_en_la_geometria(g in arb_circle()) {
        assert_curve_snaps_hit(&g, &Tol::default());
    }

    #[test]
    fn prop_polyline_snaps_en_la_geometria(g in arb_polyline()) {
        assert_curve_snaps_hit(&g, &Tol::default());
    }
}
