//! Serialization tests, compiled only when the `serde` feature is enabled.
#![cfg(feature = "serde")]

use af_math::{BBox, Point2, Tol, Transform2, Vec2};

/// Exact `f64` equality without direct `==` comparison.
fn bits_eq(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

#[test]
fn point2_serializes_as_exact_tuple() {
    let p = Point2::new(1.5, -2.0);
    let json = serde_json::to_string(&p).unwrap();
    // Point2 serializes exactly as `[x,y]`.
    assert_eq!(json, "[1.5,-2.0]");
}

#[test]
fn point2_roundtrip_is_bit_exact() {
    let p = Point2::new(1.5, -2.0);
    let json = serde_json::to_string(&p).unwrap();
    let back: Point2 = serde_json::from_str(&json).unwrap();
    assert!(bits_eq(back.x, p.x) && bits_eq(back.y, p.y));
}

#[test]
fn point2_deserializes_from_tuple() {
    let p: Point2 = serde_json::from_str("[3.25, -7.5]").unwrap();
    assert!(bits_eq(p.x, 3.25) && bits_eq(p.y, -7.5));
}

#[test]
fn point2_rejects_object_form() {
    // The disk format is a tuple; an `{x,y}` object must not deserialize.
    let r: Result<Point2, _> = serde_json::from_str(r#"{"x":1.0,"y":2.0}"#);
    assert!(r.is_err());
}

#[test]
fn vec2_roundtrip() {
    let v = Vec2::new(-4.0, 0.125);
    let json = serde_json::to_string(&v).unwrap();
    let back: Vec2 = serde_json::from_str(&json).unwrap();
    assert!(bits_eq(back.x, v.x) && bits_eq(back.y, v.y));
}

#[test]
fn transform2_roundtrip() {
    let m = Transform2::from_rows(1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let json = serde_json::to_string(&m).unwrap();
    let back: Transform2 = serde_json::from_str(&json).unwrap();
    assert!(
        bits_eq(back.a, m.a)
            && bits_eq(back.b, m.b)
            && bits_eq(back.c, m.c)
            && bits_eq(back.d, m.d)
            && bits_eq(back.tx, m.tx)
            && bits_eq(back.ty, m.ty)
    );
}

#[test]
fn bbox_roundtrip_uses_point_tuples() {
    let bb = BBox::new(Point2::new(-1.0, -2.0), Point2::new(3.0, 4.0));
    let json = serde_json::to_string(&bb).unwrap();
    // `min` and `max` inherit Point2's tuple representation.
    assert_eq!(json, r#"{"min":[-1.0,-2.0],"max":[3.0,4.0]}"#);
    let back: BBox = serde_json::from_str(&json).unwrap();
    assert!(bits_eq(back.min.x, bb.min.x) && bits_eq(back.max.y, bb.max.y));
}

#[test]
fn tol_roundtrip() {
    let t = Tol::default();
    let json = serde_json::to_string(&t).unwrap();
    let back: Tol = serde_json::from_str(&json).unwrap();
    assert!(
        bits_eq(back.linear, t.linear)
            && bits_eq(back.point_merge, t.point_merge)
            && bits_eq(back.angle, t.angle)
    );
}
