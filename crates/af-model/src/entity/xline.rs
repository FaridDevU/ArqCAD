//! `Xline` is an infinite construction line through `point` along `direction`.
//!
//! DXF `XLINE` semantics define a line without endpoints. When bounded geometry is
//! required for indexing, rendering, or snapping, the model materializes a large
//! segment using [`crate::entity::INFINITE_HALF_LEN`].
//!
//! - `bbox` bounds the materialized segment and includes the base-point snap.
//! - `hit` measures perpendicular distance to the infinite line.
//! - `snap_points` returns the base point as a `Node`.
//! - `transform` accepts any affine transform.
//! - `validate` requires finite coordinates and a nonzero direction.

use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{
    EntityOps, GeomIssue, INFINITE_HALF_LEN, SnapKind, SnapPoint, SnapVec, TransformError,
};

/// Infinite line through `point` along `direction`.
///
/// JSON is internally tagged by the containing enum:
/// `{"type":"xline","point":[0.0,0.0],"direction":{"x":1.0,"y":0.0}}`.
/// The magnitude of `direction` is irrelevant because it defines orientation,
/// not length.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XlineGeo {
    /// Base point on the line.
    pub point: Point2,
    /// Nonzero line direction.
    pub direction: Vec2,
}

impl XlineGeo {
    /// Creates a line through `point` along `direction`.
    #[inline]
    #[must_use]
    pub fn new(point: Point2, direction: Vec2) -> Self {
        Self { point, direction }
    }

    /// Creates a line through `a` and `b` with direction `b - a`.
    /// Equal points produce a zero direction rejected by validation.
    #[inline]
    #[must_use]
    pub fn through(a: Point2, b: Point2) -> Self {
        Self {
            point: a,
            direction: b - a,
        }
    }

    /// Returns the unit direction, or `None` for a degenerate direction.
    #[inline]
    #[must_use]
    pub fn unit_direction(&self) -> Option<Vec2> {
        self.direction.normalize().ok()
    }

    /// Returns the materialized segment endpoints at ± [`INFINITE_HALF_LEN`].
    /// A degenerate direction defensively falls back to `+X`.
    #[inline]
    #[must_use]
    pub fn endpoints(&self) -> (Point2, Point2) {
        let u = self.unit_direction().unwrap_or(Vec2::X) * INFINITE_HALF_LEN;
        (self.point - u, self.point + u)
    }
}

impl EntityOps for XlineGeo {
    /// Returns the finite bounds of the materialized segment.
    fn bbox(&self) -> BBox {
        let (a, b) = self.endpoints();
        BBox::new(a, b)
    }

    /// Applies an affine transform to the point and its linear part to the direction.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        Ok(Self {
            point: t.apply(self.point),
            direction: t.apply_vec(self.direction),
        })
    }

    /// Returns perpendicular distance to the infinite line, or `None` beyond `tol`.
    /// A degenerate direction also returns `None`.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let u = self.unit_direction()?;
        let d = (p - self.point).cross(u).abs();
        (d <= tol).then_some(d)
    }

    /// Returns the base point as the only `Node` snap.
    fn snap_points(&self) -> SnapVec {
        smallvec![SnapPoint::new(self.point, SnapKind::Node)]
    }

    /// Validates finite coordinates and a nonzero direction.
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        if !is_finite(self.point) || !self.direction.x.is_finite() || !self.direction.y.is_finite()
        {
            return Err(GeomIssue::NonFinite);
        }
        if self.direction.norm() <= tol.linear {
            return Err(GeomIssue::ZeroDirection);
        }
        Ok(())
    }
}

/// Returns `true` when both point coordinates are finite.
#[inline]
fn is_finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityGeometry;
    use core::f64::consts::FRAC_PI_2;

    fn horiz() -> XlineGeo {
        XlineGeo::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))
    }

    #[test]
    fn hit_es_distancia_a_la_recta_infinita() {
        let x = horiz();
        // A distant point on the infinite line is still a hit.
        assert_eq!(x.hit(Point2::new(1.0e8, 0.0), 1e-6), Some(0.0));
        // Perpendicular distance is 1.0.
        let d = x.hit(Point2::new(3.0, 1.0), 2.0);
        assert!(matches!(d, Some(v) if (v - 1.0).abs() < 1e-12));
        // Outside tolerance.
        assert_eq!(x.hit(Point2::new(3.0, 1.0), 0.5), None);
    }

    #[test]
    fn bbox_contiene_el_snap_base() {
        let x = XlineGeo::through(Point2::new(2.0, 3.0), Point2::new(5.0, 9.0));
        assert!(x.bbox().contains_point(Point2::new(2.0, 3.0)));
        let snaps = x.snap_points();
        assert_eq!(snaps.len(), 1);
        assert_eq!(
            snaps[0],
            SnapPoint::new(Point2::new(2.0, 3.0), SnapKind::Node)
        );
    }

    #[test]
    fn transform_rota_la_direccion() {
        let x = horiz();
        let m = x.transform(&Transform2::rotate(FRAC_PI_2)).unwrap();
        assert!(m.point.x.abs() < 1e-12 && m.point.y.abs() < 1e-12);
        // Rotating (1,0) by 90° produces the vertical direction (0,1).
        assert!(m.direction.x.abs() < 1e-12 && (m.direction.y - 1.0).abs() < 1e-12);
    }

    #[test]
    fn validate_direccion_nula_y_no_finita() {
        let tol = Tol::default();
        assert!(horiz().validate(&tol).is_ok());
        let zero = XlineGeo::new(Point2::ORIGIN, Vec2::ZERO);
        assert_eq!(zero.validate(&tol), Err(GeomIssue::ZeroDirection));
        let deg = XlineGeo::through(Point2::new(4.0, 4.0), Point2::new(4.0, 4.0));
        assert_eq!(deg.validate(&tol), Err(GeomIssue::ZeroDirection));
        let nan = XlineGeo::new(Point2::new(f64::NAN, 0.0), Vec2::X);
        assert_eq!(nan.validate(&tol), Err(GeomIssue::NonFinite));
    }

    #[test]
    fn serde_string_exacto() {
        let geo = EntityGeometry::Xline(XlineGeo::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0)));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"xline","point":[0.0,0.0],"direction":{"x":1.0,"y":0.0}}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }
}
