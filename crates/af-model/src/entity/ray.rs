//! Infinite ray from `origin` along `direction`.
//!
//! A large finite proxy using [`crate::entity::INFINITE_HALF_LEN`] provides boxes.
//!
//! Hit distance is perpendicular ahead of the origin and distance to the origin
//! behind it. The origin is the sole endpoint snap.

use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{
    EntityOps, GeomIssue, INFINITE_HALF_LEN, SnapKind, SnapPoint, SnapVec, TransformError,
};

/// Ray from `origin` infinitely forward along `direction`.
///
/// Direction magnitude carries no length meaning.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RayGeo {
    /// Ray endpoint.
    pub origin: Point2,
    /// Nonzero forward direction.
    pub direction: Vec2,
}

impl RayGeo {
    /// Creates a ray with the given origin and direction.
    #[inline]
    #[must_use]
    pub fn new(origin: Point2, direction: Vec2) -> Self {
        Self { origin, direction }
    }

    /// Creates a ray from `a` toward `b`; equal points are degenerate.
    #[inline]
    #[must_use]
    pub fn through(a: Point2, b: Point2) -> Self {
        Self {
            origin: a,
            direction: b - a,
        }
    }

    /// Unit direction, or `None` when degenerate.
    #[inline]
    #[must_use]
    pub fn unit_direction(&self) -> Option<Vec2> {
        self.direction.normalize().ok()
    }

    /// Endpoints of the finite proxy segment.
    #[inline]
    #[must_use]
    pub fn endpoints(&self) -> (Point2, Point2) {
        let u = self.unit_direction().unwrap_or(Vec2::X) * INFINITE_HALF_LEN;
        (self.origin, self.origin + u)
    }
}

impl EntityOps for RayGeo {
    /// Large finite proxy box containing the origin snap.
    fn bbox(&self) -> BBox {
        let (a, b) = self.endpoints();
        BBox::new(a, b)
    }

    /// Applies an exact affine transform.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        Ok(Self {
            origin: t.apply(self.origin),
            direction: t.apply_vec(self.direction),
        })
    }

    /// Ray distance, or `None` beyond tolerance or for a degenerate direction.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let u = self.unit_direction()?;
        let w = p - self.origin;
        let d = if w.dot(u) >= 0.0 {
            w.cross(u).abs()
        } else {
            p.dist(self.origin)
        };
        (d <= tol).then_some(d)
    }

    /// Sole `Endpoint` snap at the origin.
    fn snap_points(&self) -> SnapVec {
        smallvec![SnapPoint::new(self.origin, SnapKind::Endpoint)]
    }

    /// Validates finite values and a nonzero direction.
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        if !is_finite(self.origin) || !self.direction.x.is_finite() || !self.direction.y.is_finite()
        {
            return Err(GeomIssue::NonFinite);
        }
        if self.direction.norm() <= tol.linear {
            return Err(GeomIssue::ZeroDirection);
        }
        Ok(())
    }
}

/// Whether both point coordinates are finite.
#[inline]
fn is_finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityGeometry;

    fn horiz() -> RayGeo {
        RayGeo::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))
    }

    #[test]
    fn hit_solo_hacia_adelante() {
        let r = horiz();
        // A forward point on the support line hits at any distance.
        assert_eq!(r.hit(Point2::new(1.0e8, 0.0), 1e-6), Some(0.0));
        // Behind the origin, distance is measured to the endpoint.
        let d = r.hit(Point2::new(-5.0, 0.0), 100.0);
        assert!(matches!(d, Some(v) if (v - 5.0).abs() < 1e-12));
        // A point behind and beyond tolerance does not hit.
        assert_eq!(r.hit(Point2::new(-5.0, 0.0), 1.0), None);
        // Ahead, perpendicular distance is 1.0.
        let dp = r.hit(Point2::new(3.0, 1.0), 2.0);
        assert!(matches!(dp, Some(v) if (v - 1.0).abs() < 1e-12));
    }

    #[test]
    fn snap_es_el_origen_endpoint() {
        let r = RayGeo::through(Point2::new(2.0, 3.0), Point2::new(6.0, 3.0));
        let snaps = r.snap_points();
        assert_eq!(snaps.len(), 1);
        assert_eq!(
            snaps[0],
            SnapPoint::new(Point2::new(2.0, 3.0), SnapKind::Endpoint)
        );
        assert!(r.bbox().contains_point(Point2::new(2.0, 3.0)));
    }

    #[test]
    fn validate_direccion_nula_y_no_finita() {
        let tol = Tol::default();
        assert!(horiz().validate(&tol).is_ok());
        let zero = RayGeo::new(Point2::ORIGIN, Vec2::ZERO);
        assert_eq!(zero.validate(&tol), Err(GeomIssue::ZeroDirection));
        let nan = RayGeo::new(Point2::new(0.0, f64::INFINITY), Vec2::X);
        assert_eq!(nan.validate(&tol), Err(GeomIssue::NonFinite));
    }

    #[test]
    fn serde_string_exacto() {
        let geo = EntityGeometry::Ray(RayGeo::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0)));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"ray","origin":[0.0,0.0],"direction":{"x":1.0,"y":0.0}}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }
}
