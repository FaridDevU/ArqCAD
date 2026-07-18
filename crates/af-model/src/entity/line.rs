//! Line segment between two points.
//!
//! Affine transforms are exact. Bounding boxes contain endpoints, hits use
//! point-to-segment distance, snaps include endpoints and midpoint, and zero
//! length is valid when coordinates are finite.

// Geometry.

use af_math::{BBox, MathError, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Line segment between `p1` and `p2`.
///
/// JSON representation:
/// `{"type":"line","p1":[0.0,0.0],"p2":[100.0,50.0]}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineGeo {
    /// First endpoint.
    pub p1: Point2,
    /// Second endpoint.
    pub p2: Point2,
}

// Construction.

impl LineGeo {
    /// Creates a segment between two points.
    #[inline]
    #[must_use]
    pub fn new(p1: Point2, p2: Point2) -> Self {
        Self { p1, p2 }
    }
}

// Entity operations.

impl EntityOps for LineGeo {
    /// Bounding box of both endpoints.
    fn bbox(&self) -> BBox {
        BBox::new(self.p1, self.p2)
    }

    /// Applies an exact affine transform to both endpoints.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        Ok(Self {
            p1: t.apply(self.p1),
            p2: t.apply(self.p2),
        })
    }

    /// Point-to-segment distance, or `None` beyond tolerance.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = dist_point_segment(p, self.p1, self.p2);
        (d <= tol).then_some(d)
    }

    /// Endpoint and midpoint snaps; all coincide for a degenerate line.
    fn snap_points(&self) -> SnapVec {
        smallvec![
            SnapPoint::new(self.p1, SnapKind::Endpoint),
            SnapPoint::new(self.p2, SnapKind::Endpoint),
            SnapPoint::new(self.p1.midpoint(self.p2), SnapKind::Midpoint),
        ]
    }

    /// Validates finite endpoints; zero length is allowed.
    fn validate(&self, _tol: &Tol) -> Result<(), GeomIssue> {
        if is_finite(self.p1) && is_finite(self.p2) {
            Ok(())
        } else {
            Err(GeomIssue::NonFinite)
        }
    }
}

// Geometry helpers.

/// Public segment geometry helpers.
impl LineGeo {
    /// Euclidean segment length.
    #[inline]
    #[must_use]
    pub fn length(&self) -> f64 {
        self.p1.dist(self.p2)
    }

    /// Unit direction from `p1` to `p2`.
    ///
    /// # Errors
    /// Returns [`MathError::ZeroVector`] for a degenerate segment.
    #[inline]
    pub fn direction(&self) -> Result<Vec2, MathError> {
        (self.p2 - self.p1).normalize()
    }

    /// Point at linear parameter `t`.
    ///
    /// Values outside `[0, 1]` extrapolate without clamping.
    #[inline]
    #[must_use]
    pub fn point_at(&self, t: f64) -> Point2 {
        self.p1.lerp(self.p2, t)
    }

    /// Segment midpoint.
    #[inline]
    #[must_use]
    pub fn midpoint(&self) -> Point2 {
        self.p1.midpoint(self.p2)
    }
}

/// Whether both point coordinates are finite.
#[inline]
fn is_finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Euclidean distance from `p` to segment `[a, b]`.
///
/// Degenerate segments return distance to their single point.
fn dist_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let ab: Vec2 = b - a;
    let len_sq = ab.norm_sq();
    if len_sq <= 0.0 {
        // Degenerate segment: distance to its sole point.
        return p.dist(a);
    }
    // Clamp the perpendicular projection to the finite segment.
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    p.dist(proj)
}

// Unit tests.

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::FRAC_PI_2;

    use crate::entity::EntityGeometry;

    fn sample() -> LineGeo {
        LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0))
    }

    #[test]
    fn bbox_es_caja_de_extremos() {
        let l = LineGeo::new(Point2::new(2.0, 5.0), Point2::new(-1.0, 3.0));
        let bb = l.bbox();
        assert_eq!(bb.min, Point2::new(-1.0, 3.0));
        assert_eq!(bb.max, Point2::new(2.0, 5.0));
    }

    #[test]
    fn hit_sobre_cerca_y_lejos() {
        let l = sample();
        let tol = 0.1;
        // Midpoint lies on the segment.
        assert_eq!(l.hit(Point2::new(5.0, 0.0), tol), Some(0.0));
        // A nearby point returns its actual distance.
        let near = l.hit(Point2::new(5.0, 0.05), tol);
        assert!(matches!(near, Some(d) if (d - 0.05).abs() < 1e-12));
        // A distant point is outside tolerance.
        assert_eq!(l.hit(Point2::new(5.0, 1.0), tol), None);
        // Beyond an endpoint, distance is measured to that endpoint.
        let beyond = l.hit(Point2::new(13.0, 4.0), 100.0);
        assert!(matches!(beyond, Some(d) if (d - 5.0).abs() < 1e-12));
    }

    #[test]
    fn hit_linea_degenerada_es_distancia_al_punto() {
        let l = LineGeo::new(Point2::new(1.0, 1.0), Point2::new(1.0, 1.0));
        assert_eq!(l.hit(Point2::new(1.0, 1.0), 0.01), Some(0.0));
        let d = l.hit(Point2::new(4.0, 5.0), 100.0);
        assert!(matches!(d, Some(v) if (v - 5.0).abs() < 1e-12));
    }

    #[test]
    fn snaps_son_end_end_mid() {
        let l = sample();
        let snaps = l.snap_points();
        assert_eq!(snaps.len(), 3);
        assert_eq!(
            snaps[0],
            SnapPoint::new(Point2::new(0.0, 0.0), SnapKind::Endpoint)
        );
        assert_eq!(
            snaps[1],
            SnapPoint::new(Point2::new(10.0, 0.0), SnapKind::Endpoint)
        );
        assert_eq!(
            snaps[2],
            SnapPoint::new(Point2::new(5.0, 0.0), SnapKind::Midpoint)
        );
    }

    #[test]
    fn snaps_de_linea_degenerada_coinciden() {
        let l = LineGeo::new(Point2::new(2.0, 3.0), Point2::new(2.0, 3.0));
        let snaps = l.snap_points();
        assert!(snaps.iter().all(|s| s.point == Point2::new(2.0, 3.0)));
    }

    #[test]
    fn transform_translate() {
        let l = sample();
        let t = Transform2::translate(Vec2::new(3.0, -2.0));
        let m = l.transform(&t).unwrap();
        assert_eq!(m.p1, Point2::new(3.0, -2.0));
        assert_eq!(m.p2, Point2::new(13.0, -2.0));
    }

    #[test]
    fn transform_rotate_90() {
        let l = sample();
        let t = Transform2::rotate(FRAC_PI_2);
        let m = l.transform(&t).unwrap();
        // Rotate `(10, 0)` 90 degrees CCW to `(0, 10)`.
        assert!((m.p1.x).abs() < 1e-12 && (m.p1.y).abs() < 1e-12);
        assert!((m.p2.x).abs() < 1e-12 && (m.p2.y - 10.0).abs() < 1e-12);
    }

    #[test]
    fn transform_mirror_en_x() {
        // Reflect across the Y axis.
        let l = sample();
        let t = Transform2::scale(-1.0, 1.0);
        let m = l.transform(&t).unwrap();
        assert_eq!(m.p1, Point2::new(0.0, 0.0));
        assert_eq!(m.p2, Point2::new(-10.0, 0.0));
    }

    #[test]
    fn validate_finito_ok_y_no_finito_err() {
        let tol = Tol::default();
        assert!(sample().validate(&tol).is_ok());
        // Zero length is valid.
        let zero = LineGeo::new(Point2::new(1.0, 1.0), Point2::new(1.0, 1.0));
        assert!(zero.validate(&tol).is_ok());
        // NaN is nonfinite.
        let bad = LineGeo::new(Point2::new(f64::NAN, 0.0), Point2::new(1.0, 1.0));
        assert_eq!(bad.validate(&tol), Err(GeomIssue::NonFinite));
    }

    /// Exact serialized example string.
    ///
    /// `serde_json` renders integral `f64` values with `.0`.
    #[test]
    fn serde_string_exacto() {
        let geo = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(json, r#"{"type":"line","p1":[0.0,0.0],"p2":[1.0,1.0]}"#);

        // Round trip.
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }

    // Geometry helper tests.

    #[test]
    fn length_de_segmento_y_degenerado() {
        assert!((sample().length() - 10.0).abs() < 1e-12);
        // 3-4-5.
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(3.0, 4.0));
        assert!((l.length() - 5.0).abs() < 1e-12);
        // Degenerate length is exactly zero.
        let deg = LineGeo::new(Point2::new(2.0, 2.0), Point2::new(2.0, 2.0));
        assert_eq!(deg.length(), 0.0);
    }

    #[test]
    fn direction_unitaria_y_error_si_degenerado() {
        // `(10, 0)` normalizes exactly to `(1, 0)`.
        assert_eq!(sample().direction().unwrap(), Vec2::new(1.0, 0.0));
        // Degenerate direction is undefined.
        let deg = LineGeo::new(Point2::new(5.0, 5.0), Point2::new(5.0, 5.0));
        assert_eq!(deg.direction(), Err(MathError::ZeroVector));
    }

    #[test]
    fn point_at_extremos_y_medio() {
        let l = sample();
        assert_eq!(l.point_at(0.0), Point2::new(0.0, 0.0));
        assert_eq!(l.point_at(1.0), Point2::new(10.0, 0.0));
        assert_eq!(l.point_at(0.5), Point2::new(5.0, 0.0));
    }

    #[test]
    fn midpoint_es_point_at_0_5() {
        let l = LineGeo::new(Point2::new(-2.0, 4.0), Point2::new(6.0, 10.0));
        assert_eq!(l.midpoint(), l.point_at(0.5));
        assert_eq!(l.midpoint(), Point2::new(2.0, 7.0));
    }

    /// Exact endpoints hit at zero distance.
    #[test]
    fn hit_en_extremos_exactos_es_cero() {
        let l = sample();
        assert_eq!(l.hit(l.p1, Tol::default().linear), Some(0.0));
        assert_eq!(l.hit(l.p2, Tol::default().linear), Some(0.0));
    }

    /// Reflections preserve length.
    #[test]
    fn transform_mirror_conserva_length() {
        let l = LineGeo::new(Point2::new(1.0, 2.0), Point2::new(7.0, 10.0));
        let before = l.length();
        // Reflection across Y preserves distances.
        let m = l.transform(&Transform2::scale(-1.0, 1.0)).unwrap();
        assert!((m.length() - before).abs() < 1e-12);
    }

    // Property: bounding boxes contain snap points.

    use proptest::prelude::*;

    prop_compose! {
        fn arb_line()(
            x1 in -1.0e6f64..1.0e6,
            y1 in -1.0e6f64..1.0e6,
            x2 in -1.0e6f64..1.0e6,
            y2 in -1.0e6f64..1.0e6,
        ) -> LineGeo {
            LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2))
        }
    }

    proptest! {
        #[test]
        fn bbox_contiene_snaps(l in arb_line()) {
            let bb = l.bbox();
            for s in l.snap_points() {
                // Allow minimal midpoint rounding tolerance.
                prop_assert!(bb.expand(1e-9).contains_point(s.point));
            }
        }
    }
}
