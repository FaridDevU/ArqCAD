//! Deterministic unit tests for exact tables and boundary cases.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use af_math::{
    BBox, MathError, Point2, Tol, Transform2, Vec2,
    angle::{angle_in_sweep, angle_of, normalize_0_2pi, sweep_ccw},
};

/// Test tolerance comfortably above `f64` rounding noise.
fn near(a: f64, b: f64) -> bool {
    Tol::new(1e-9, 1e-9, 1e-9).approx_eq(a, b)
}

fn pt_near(p: Point2, q: Point2) -> bool {
    near(p.x, q.x) && near(p.y, q.y)
}

fn v_near(u: Vec2, w: Vec2) -> bool {
    near(u.x, w.x) && near(u.y, w.y)
}

#[test]
fn rotate_quarter_turn_maps_x_axis_to_y_axis() {
    let r = Transform2::rotate(FRAC_PI_2);
    let out = r.apply(Point2::new(1.0, 0.0));
    assert!(pt_near(out, Point2::new(0.0, 1.0)), "got {out:?}");
}

#[test]
fn perp_is_ccw_quarter_turn() {
    assert!(v_near(Vec2::X.perp(), Vec2::new(0.0, 1.0)));
    assert!(v_near(Vec2::Y.perp(), Vec2::new(-1.0, 0.0)));
}

#[test]
fn then_applies_self_first_then_other() {
    // a.then(b).apply(p) == b.apply(a.apply(p))
    let a = Transform2::rotate(0.7);
    let b = Transform2::translate(Vec2::new(3.0, -2.0));
    let p = Point2::new(5.0, 9.0);
    let chained = a.then(b).apply(p);
    let manual = b.apply(a.apply(p));
    assert!(pt_near(chained, manual));
}

#[test]
fn translate_then_rotate_differs_from_rotate_then_translate() {
    let v = Vec2::new(2.0, 0.0);
    let t = Transform2::translate(v);
    let r = Transform2::rotate(FRAC_PI_2);

    // Translate then rotate: origin -> (2,0) -> 90° rotation -> (0,2).
    let tr = t.then(r).apply(Point2::ORIGIN);
    // Rotate then translate: origin -> origin -> (2,0).
    let rt = r.then(t).apply(Point2::ORIGIN);

    assert!(pt_near(tr, Point2::new(0.0, 2.0)), "tr = {tr:?}");
    assert!(pt_near(rt, Point2::new(2.0, 0.0)), "rt = {rt:?}");
    assert!(!pt_near(tr, rt), "los dos órdenes deberían diferir");
}

#[test]
fn rotate_about_pivot_keeps_pivot_fixed() {
    let pivot = Point2::new(4.0, -1.0);
    let m = Transform2::rotate_about(1.234, pivot);
    assert!(pt_near(m.apply(pivot), pivot));
}

#[test]
fn scale_about_pivot_keeps_pivot_fixed() {
    let pivot = Point2::new(-3.0, 7.5);
    let m = Transform2::scale_about(2.0, 0.5, pivot);
    assert!(pt_near(m.apply(pivot), pivot));
}

#[test]
fn invert_roundtrips_within_linear_tolerance() {
    let m = Transform2::translate(Vec2::new(10.0, -4.0))
        .then(Transform2::rotate(0.9))
        .then(Transform2::scale(2.0, 3.0));
    let inv = m.invert().expect("invertible");
    let p = Point2::new(6.0, -2.0);
    let back = inv.apply(m.apply(p));
    // Well-conditioned matrices recover points within linear tolerance.
    let tol = Tol::default();
    assert!(
        tol.approx_eq(back.x, p.x) && tol.approx_eq(back.y, p.y),
        "back = {back:?}"
    );
}

#[test]
fn invert_of_singular_is_error() {
    let singular = Transform2::scale(1.0, 0.0);
    assert_eq!(singular.invert(), Err(MathError::Singular));
}

#[test]
fn det_and_mirroring() {
    assert!(near(Transform2::identity().det(), 1.0));
    assert!(near(Transform2::scale(2.0, 3.0).det(), 6.0));
    assert!(Transform2::scale(-1.0, 1.0).is_mirroring());
    assert!(!Transform2::rotate(0.5).is_mirroring());
}

#[test]
fn is_uniform_detects_equal_axis_scales() {
    let tol = Tol::default();
    assert!(Transform2::rotate(1.1).is_uniform(&tol));
    assert!(Transform2::scale(2.0, 2.0).is_uniform(&tol));
    assert!(Transform2::scale(2.0, -2.0).is_uniform(&tol));
    assert!(!Transform2::scale(2.0, 3.0).is_uniform(&tol));
}

#[test]
fn normalize_zero_vector_is_error() {
    assert_eq!(Vec2::ZERO.normalize(), Err(MathError::ZeroVector));
}

#[test]
fn reflect_about_line_keeps_axis_points_fixed_and_flips_orientation() {
    let p1 = Point2::new(1.0, 1.0);
    let p2 = Point2::new(4.0, 3.0);
    let m = Transform2::reflect_about_line(p1, p2).expect("axis is well-defined");
    assert!(pt_near(m.apply(p1), p1));
    assert!(pt_near(m.apply(p2), p2));
    assert!(near(m.det(), -1.0), "det = {}", m.det());
    assert!(m.is_mirroring());
    // Applying the same reflection twice is identity.
    let p = Point2::new(-2.0, 6.0);
    assert!(pt_near(m.apply(m.apply(p)), p));
}

#[test]
fn reflect_about_x_axis_flips_y() {
    let m = Transform2::reflect_about_line(Point2::ORIGIN, Point2::new(1.0, 0.0)).unwrap();
    let out = m.apply(Point2::new(3.0, 5.0));
    assert!(pt_near(out, Point2::new(3.0, -5.0)), "got {out:?}");
}

#[test]
fn reflect_about_degenerate_axis_is_error() {
    let p = Point2::new(2.0, 2.0);
    assert_eq!(
        Transform2::reflect_about_line(p, p),
        Err(MathError::ZeroVector)
    );
}

#[test]
fn normalize_unit_length() {
    let u = Vec2::new(3.0, 4.0).normalize().unwrap();
    assert!(near(u.norm(), 1.0));
    assert!(near(u.x, 0.6) && near(u.y, 0.8));
}

#[test]
fn vec_dot_cross() {
    let a = Vec2::new(1.0, 0.0);
    let b = Vec2::new(0.0, 1.0);
    assert!(near(a.dot(b), 0.0));
    assert!(near(a.cross(b), 1.0));
    assert!(near(b.cross(a), -1.0));
}

#[test]
fn point_vec_arithmetic() {
    let p = Point2::new(1.0, 2.0);
    let q = Point2::new(4.0, 6.0);
    assert!(v_near(q - p, Vec2::new(3.0, 4.0)));
    assert!(near(p.dist(q), 5.0));
    assert!(pt_near(p + Vec2::new(3.0, 4.0), q));
    assert!(pt_near(p.midpoint(q), Point2::new(2.5, 4.0)));
}

#[test]
fn bbox_normalizes_and_measures() {
    let bb = BBox::new(Point2::new(3.0, 5.0), Point2::new(-1.0, 2.0));
    assert!(pt_near(bb.min, Point2::new(-1.0, 2.0)));
    assert!(pt_near(bb.max, Point2::new(3.0, 5.0)));
    assert!(near(bb.width(), 4.0));
    assert!(near(bb.height(), 3.0));
    assert!(pt_near(bb.center(), Point2::new(1.0, 3.5)));
}

#[test]
fn bbox_from_points_and_containment() {
    let pts = [
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 1.0),
        Point2::new(-2.0, 3.0),
    ];
    let bb = BBox::from_points(pts).unwrap();
    assert!(pt_near(bb.min, Point2::new(-2.0, 0.0)));
    assert!(pt_near(bb.max, Point2::new(4.0, 3.0)));
    assert!(bb.contains_point(Point2::new(0.0, 0.0)));
    assert!(bb.contains_point(Point2::new(-2.0, 3.0))); // Corner on the boundary.
    assert!(!bb.contains_point(Point2::new(5.0, 0.0)));
}

#[test]
fn bbox_from_empty_is_none() {
    let empty: [Point2; 0] = [];
    assert!(BBox::from_points(empty).is_none());
}

#[test]
fn bbox_intersects_and_contains_bbox() {
    let a = BBox::new(Point2::ORIGIN, Point2::new(10.0, 10.0));
    let inside = BBox::new(Point2::new(2.0, 2.0), Point2::new(4.0, 4.0));
    let touching = BBox::new(Point2::new(10.0, 0.0), Point2::new(12.0, 5.0));
    let apart = BBox::new(Point2::new(20.0, 20.0), Point2::new(25.0, 25.0));

    assert!(a.contains_bbox(inside));
    assert!(a.intersects(inside));
    assert!(a.intersects(touching)); // Boundary contact counts.
    assert!(!a.intersects(apart));
    assert!(!a.contains_bbox(touching));
}

#[test]
fn bbox_degenerate_detection() {
    let line = BBox::new(Point2::ORIGIN, Point2::new(0.0, 5.0));
    let area = BBox::new(Point2::ORIGIN, Point2::new(5.0, 5.0));
    assert!(line.is_degenerate());
    assert!(!area.is_degenerate());
}

#[test]
fn bbox_union_and_expand() {
    let a = BBox::new(Point2::ORIGIN, Point2::new(2.0, 2.0));
    let b = BBox::new(Point2::new(5.0, -1.0), Point2::new(6.0, 1.0));
    let u = a.union(b);
    assert!(pt_near(u.min, Point2::new(0.0, -1.0)));
    assert!(pt_near(u.max, Point2::new(6.0, 2.0)));

    let e = a.expand(1.0);
    assert!(pt_near(e.min, Point2::new(-1.0, -1.0)));
    assert!(pt_near(e.max, Point2::new(3.0, 3.0)));
}

#[test]
fn angle_normalize_range() {
    assert!(near(normalize_0_2pi(0.0), 0.0));
    assert!(near(normalize_0_2pi(-FRAC_PI_2), 3.0 * FRAC_PI_2));
    assert!(near(normalize_0_2pi(TAU + 0.5), 0.5));
    // Always in `[0, 2π)`.
    assert!((0.0..TAU).contains(&normalize_0_2pi(-1000.0)));
}

#[test]
fn angle_of_directions() {
    assert!(near(angle_of(Vec2::new(1.0, 0.0)), 0.0));
    assert!(near(angle_of(Vec2::new(0.0, 1.0)), FRAC_PI_2));
    assert!(near(angle_of(Vec2::new(-1.0, 0.0)), PI));
}

#[test]
fn sweep_ccw_examples() {
    // `sweep_ccw(3π/2, π/2) = π`.
    assert!(near(sweep_ccw(3.0 * FRAC_PI_2, FRAC_PI_2), PI));
    assert!(near(sweep_ccw(0.0, FRAC_PI_2), FRAC_PI_2));
    // A zero sweep represents a full turn.
    assert!(near(sweep_ccw(1.0, 1.0), TAU));
}

#[test]
fn angle_in_sweep_borders() {
    let start = 0.0;
    let end = FRAC_PI_2;
    // Interior.
    assert!(angle_in_sweep(FRAC_PI_2 / 2.0, start, end));
    // Boundaries include tolerance.
    assert!(angle_in_sweep(start, start, end));
    assert!(angle_in_sweep(end, start, end));
    assert!(angle_in_sweep(start - 1e-12, start, end));
    assert!(angle_in_sweep(end + 1e-12, start, end));
    // Outside.
    assert!(!angle_in_sweep(PI, start, end));
    assert!(!angle_in_sweep(-FRAC_PI_2, start, end));
    // A full turn contains every angle.
    assert!(angle_in_sweep(PI, 2.0, 2.0));
}

#[test]
fn tol_angles_eq_wraps() {
    let tol = Tol::default();
    assert!(tol.angles_eq(0.0, TAU));
    assert!(tol.angles_eq(0.1, 0.1 + TAU));
    assert!(!tol.angles_eq(0.0, 0.1));
}
