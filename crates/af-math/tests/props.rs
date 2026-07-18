//! Property-based tests with 1,000 cases for af-math invariants.

use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use proptest::prelude::*;

fn approx(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

/// Finite bounded coordinate avoiding NaN, infinity, and extreme magnitudes.
fn coord() -> impl Strategy<Value = f64> {
    -100.0f64..100.0
}

/// Scale factor bounded away from zero for a non-degenerate matrix.
fn scale_factor() -> impl Strategy<Value = f64> {
    prop_oneof![0.2f64..5.0, -5.0f64..-0.2]
}

/// Non-degenerate translate ∘ rotate ∘ scale transform.
fn nondegenerate_transform() -> impl Strategy<Value = Transform2> {
    (
        -10.0f64..10.0,
        scale_factor(),
        scale_factor(),
        -50.0f64..50.0,
        -50.0f64..50.0,
    )
        .prop_map(|(ang, sx, sy, tx, ty)| {
            Transform2::translate(Vec2::new(tx, ty))
                .then(Transform2::rotate(ang))
                .then(Transform2::scale(sx, sy))
        })
}

fn point() -> impl Strategy<Value = Point2> {
    (coord(), coord()).prop_map(|(x, y)| Point2::new(x, y))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1000, ..ProptestConfig::default() })]

    /// Applying a transform and its inverse recovers the point within linear tolerance.
    #[test]
    fn invert_roundtrip(m in nondegenerate_transform(), p in point()) {
        let inv = m.invert().expect("no degenerada por construcción");
        let back = inv.apply(m.apply(p));
        let eps = Tol::default().linear;
        prop_assert!(approx(back.x, p.x, eps), "x: {} vs {}", back.x, p.x);
        prop_assert!(approx(back.y, p.y, eps), "y: {} vs {}", back.y, p.y);
    }

    /// `M · M⁻¹` approximates identity.
    #[test]
    fn det_of_inverse_is_reciprocal(m in nondegenerate_transform()) {
        let inv = m.invert().unwrap();
        prop_assert!(approx(m.det() * inv.det(), 1.0, 1e-6));
    }

    /// A bounding-box union contains both boxes and all source points.
    #[test]
    fn union_contains_both(
        p0 in point(), p1 in point(), p2 in point(), p3 in point()
    ) {
        let a = BBox::new(p0, p1);
        let b = BBox::new(p2, p3);
        let u = a.union(b);
        prop_assert!(u.contains_bbox(a));
        prop_assert!(u.contains_bbox(b));
        for p in [p0, p1, p2, p3] {
            prop_assert!(u.contains_point(p));
        }
    }

    /// Rotation preserves distance.
    #[test]
    fn rotation_preserves_distance(a in point(), b in point(), ang in -10.0f64..10.0) {
        let r = Transform2::rotate(ang);
        let before = a.dist(b);
        let after = r.apply(a).dist(r.apply(b));
        prop_assert!(approx(before, after, 1e-6), "{before} vs {after}");
    }

    /// Translation preserves distance.
    #[test]
    fn translation_preserves_distance(a in point(), b in point(), v in (coord(), coord())) {
        let t = Transform2::translate(Vec2::new(v.0, v.1));
        prop_assert!(approx(a.dist(b), t.apply(a).dist(t.apply(b)), 1e-9));
    }

    /// Normalizing a nonzero vector produces unit length.
    #[test]
    fn normalize_yields_unit(v in (coord(), coord())) {
        let vec = Vec2::new(v.0, v.1);
        if vec.norm() > 1e-3 {
            let u = vec.normalize().expect("norma > tol");
            prop_assert!(approx(u.norm(), 1.0, 1e-9));
        }
    }

    /// `from_points` produces a box containing every input point.
    #[test]
    fn from_points_contains_all(pts in proptest::collection::vec(point(), 1..20)) {
        let bb = BBox::from_points(pts.iter().copied()).unwrap();
        for p in &pts {
            prop_assert!(bb.contains_point(*p));
        }
    }

    /// Rotating the X axis by θ produces direction angle θ.
    #[test]
    fn angle_of_matches_rotation(ang in -3.0f64..3.0) {
        let dir = Transform2::rotate(ang).apply_vec(Vec2::X);
        let recovered = af_math::angle::angle_of(dir);
        let tol = Tol::default();
        prop_assert!(tol.angles_eq(ang, recovered));
    }
}
