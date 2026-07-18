//! Single point node.
//!
//! Degenerate boxes are valid. Hits use point distance; snap kind is `Node`.

// Geometry.

use af_math::{BBox, Point2, Tol, Transform2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Single node at `position`.
///
/// JSON representation:
/// `{"type":"point","position":[3.0,4.0]}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointGeo {
    /// Node position.
    pub position: Point2,
}

// Construction.

impl PointGeo {
    /// Creates a point at `position`.
    #[inline]
    #[must_use]
    pub fn new(position: Point2) -> Self {
        Self { position }
    }
}

// Entity operations.

impl EntityOps for PointGeo {
    /// Degenerate bounding box at the point.
    fn bbox(&self) -> BBox {
        BBox::from_point(self.position)
    }

    /// Applies an exact position transform.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        Ok(Self {
            position: t.apply(self.position),
        })
    }

    /// Point distance, or `None` beyond tolerance.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = p.dist(self.position);
        (d <= tol).then_some(d)
    }

    /// One `Node` snap at the position.
    fn snap_points(&self) -> SnapVec {
        smallvec![SnapPoint::new(self.position, SnapKind::Node)]
    }

    /// Validates a finite position.
    fn validate(&self, _tol: &Tol) -> Result<(), GeomIssue> {
        if self.position.x.is_finite() && self.position.y.is_finite() {
            Ok(())
        } else {
            Err(GeomIssue::NonFinite)
        }
    }
}

// Unit tests.

#[cfg(test)]
mod tests {
    use super::*;

    use crate::entity::EntityGeometry;

    #[test]
    fn bbox_es_degenerado() {
        let g = PointGeo::new(Point2::new(3.0, 4.0));
        let bb = g.bbox();
        assert_eq!(bb.min, Point2::new(3.0, 4.0));
        assert_eq!(bb.max, Point2::new(3.0, 4.0));
    }

    #[test]
    fn hit_sobre_cerca_lejos() {
        let g = PointGeo::new(Point2::new(0.0, 0.0));
        assert_eq!(g.hit(Point2::new(0.0, 0.0), 0.1), Some(0.0));
        let near = g.hit(Point2::new(0.03, 0.04), 0.1);
        assert!(matches!(near, Some(d) if (d - 0.05).abs() < 1e-12));
        assert_eq!(g.hit(Point2::new(1.0, 1.0), 0.1), None);
    }

    #[test]
    fn snap_es_un_node() {
        let g = PointGeo::new(Point2::new(3.0, 4.0));
        let snaps = g.snap_points();
        assert_eq!(snaps.len(), 1);
        assert_eq!(
            snaps[0],
            SnapPoint::new(Point2::new(3.0, 4.0), SnapKind::Node)
        );
    }

    #[test]
    fn transform_translada() {
        use af_math::Vec2;
        let g = PointGeo::new(Point2::new(1.0, 2.0));
        let m = g
            .transform(&Transform2::translate(Vec2::new(4.0, -1.0)))
            .unwrap();
        assert_eq!(m.position, Point2::new(5.0, 1.0));
    }

    #[test]
    fn validate_finito() {
        let tol = Tol::default();
        assert!(PointGeo::new(Point2::new(0.0, 0.0)).validate(&tol).is_ok());
        let bad = PointGeo::new(Point2::new(0.0, f64::INFINITY));
        assert_eq!(bad.validate(&tol), Err(GeomIssue::NonFinite));
    }

    #[test]
    fn serde_string_exacto() {
        let geo = EntityGeometry::Point(PointGeo::new(Point2::new(3.0, 4.0)));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(json, r#"{"type":"point","position":[3.0,4.0]}"#);
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }

    // Property: bounding boxes contain snap points.

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn bbox_contiene_snaps(x in -1.0e6f64..1.0e6, y in -1.0e6f64..1.0e6) {
            let g = PointGeo::new(Point2::new(x, y));
            let bb = g.bbox();
            for s in g.snap_points() {
                prop_assert!(bb.contains_point(s.point));
            }
        }
    }
}
