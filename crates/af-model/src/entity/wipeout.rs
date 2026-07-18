//! `Wipeout` is a closed masking polygon that hides geometry below it in draw order.
//! It consists only of straight segments and is rendered as
//! [`af_render::PrimGeom::MaskPolygon`].
//!
//! - **Source of truth:** `points`. The last vertex connects to the first; no
//!   `closed` flag or duplicate endpoint is stored.
//! - **bbox:** bounds all vertices and snap points.
//! - **hit:** minimum distance to the closed boundary. The interior is not a hit.
//! - **snaps:** one `Endpoint` per vertex and one `Midpoint` per edge.
//! - **transform:** accepts any affine transform and applies it to every vertex.
//! - **validate:** requires at least three vertices and finite coordinates.

use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Closed masking polygon defined by its vertices.
///
/// JSON is internally tagged by the containing enum. The polygon is implicitly
/// closed without repeating the first vertex:
/// `{"type":"wipeout","points":[[0.0,0.0],[1.0,0.0],[1.0,1.0]]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WipeoutGeo {
    /// Closed polygon vertices in traversal order.
    pub points: Vec<Point2>,
}

impl WipeoutGeo {
    /// Creates a wipeout from the given vertices.
    ///
    /// Validation is deferred to [`EntityOps::validate`].
    #[inline]
    #[must_use]
    pub fn new(points: Vec<Point2>) -> Self {
        Self { points }
    }
}

impl EntityOps for WipeoutGeo {
    /// Returns the bounds of all vertices and snap points.
    fn bbox(&self) -> BBox {
        BBox::from_points(self.points.iter().copied())
            .unwrap_or_else(|| BBox::from_point(Point2::ORIGIN))
    }

    /// Applies any affine transform to all vertices.
    ///
    /// # Errors
    ///
    /// This implementation never returns an error; `Result` is required by the trait.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        let points = self.points.iter().map(|&p| t.apply(p)).collect();
        Ok(Self { points })
    }

    /// Returns the minimum distance to the closed boundary, or `None` beyond `tol`.
    /// The polygon interior is not a hit.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = dist_to_closed_polygon(&self.points, p);
        (d <= tol).then_some(d)
    }

    /// Returns one `Endpoint` per vertex and one `Midpoint` per closed edge.
    fn snap_points(&self) -> SnapVec {
        let mut out = SnapVec::new();
        for &p in &self.points {
            out.push(SnapPoint::new(p, SnapKind::Endpoint));
        }
        let n = self.points.len();
        for i in 0..n {
            let a = self.points[i];
            let b = self.points[(i + 1) % n];
            out.push(SnapPoint::new(a.midpoint(b), SnapKind::Midpoint));
        }
        out
    }

    /// Validates that the mask has at least three finite vertices.
    ///
    /// # Errors
    ///
    /// Returns [`GeomIssue::TooFewVertices`] for fewer than three vertices and
    /// [`GeomIssue::NonFinite`] for non-finite coordinates.
    fn validate(&self, _tol: &Tol) -> Result<(), GeomIssue> {
        if self.points.len() < 3 {
            return Err(GeomIssue::TooFewVertices);
        }
        for p in &self.points {
            if !p.x.is_finite() || !p.y.is_finite() {
                return Err(GeomIssue::NonFinite);
            }
        }
        Ok(())
    }
}

/// Returns the distance from `p` to the boundary of the closed polygon `pts`.
/// Degenerate input yields the distance to one point or infinity for no vertices.
fn dist_to_closed_polygon(pts: &[Point2], p: Point2) -> f64 {
    match pts.len() {
        0 => f64::INFINITY,
        1 => p.dist(pts[0]),
        n => {
            let mut best = f64::INFINITY;
            for i in 0..n {
                best = best.min(dist_point_segment(p, pts[i], pts[(i + 1) % n]));
            }
            best
        }
    }
}

/// Returns the Euclidean distance from `p` to segment `[a, b]`.
fn dist_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let ab: Vec2 = b - a;
    let len_sq = ab.norm_sq();
    if len_sq <= 0.0 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::entity::EntityGeometry;

    fn pts(v: &[(f64, f64)]) -> Vec<Point2> {
        v.iter().map(|&(x, y)| Point2::new(x, y)).collect()
    }

    /// Returns a closed square mask.
    fn square() -> WipeoutGeo {
        WipeoutGeo::new(pts(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]))
    }

    #[test]
    fn bbox_contiene_todos_los_snaps() {
        let g = square();
        let bb = g.bbox();
        for s in g.snap_points() {
            assert!(
                bb.expand(1e-9).contains_point(s.point),
                "snap {:?} fuera",
                s.point
            );
        }
    }

    #[test]
    fn hit_sobre_el_borde_y_no_en_el_interior() {
        let g = square();
        // On an edge.
        assert_eq!(g.hit(Point2::new(5.0, 0.0), 1e-6), Some(0.0));
        // On the closing edge (0,10)->(0,0).
        assert_eq!(g.hit(Point2::new(0.0, 5.0), 1e-6), Some(0.0));
        // The interior is not a hit.
        assert_eq!(g.hit(Point2::new(5.0, 5.0), 1e-6), None);
        // Far from every edge.
        assert_eq!(g.hit(Point2::new(50.0, 50.0), 1e-6), None);
    }

    #[test]
    fn snaps_endpoints_y_midpoints_de_aristas() {
        let g = square();
        let snaps = g.snap_points();
        let n_end = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Endpoint)
            .count();
        let n_mid = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Midpoint)
            .count();
        // Four vertices and four closed edges.
        assert_eq!((n_end, n_mid), (4, 4));
        // The midpoint of the closing edge (0,10)->(0,0) is (0,5).
        assert!(
            snaps
                .iter()
                .any(|s| s.kind == SnapKind::Midpoint && s.point == Point2::new(0.0, 5.0)),
            "falta el midpoint de la arista de cierre"
        );
    }

    #[test]
    fn transform_traslada_todos_los_vertices() {
        let g = square();
        let m = g
            .transform(&Transform2::translate(Vec2::new(3.0, -2.0)))
            .unwrap();
        for (a, b) in g.points.iter().zip(m.points.iter()) {
            assert_eq!(*b, Point2::new(a.x + 3.0, a.y - 2.0));
        }
    }

    #[test]
    fn transform_admite_escala_no_uniforme_sin_error() {
        // A straight-edge wipeout accepts non-uniform scaling.
        let g = square();
        assert!(g.transform(&Transform2::scale(2.0, 5.0)).is_ok());
    }

    #[test]
    fn transform_coherente_en_los_snaps_para_cualquier_afin() {
        let g = square();
        let t = Transform2::rotate(0.7)
            .then(Transform2::scale(2.0, 3.0))
            .then(Transform2::translate(Vec2::new(10.0, -4.0)));
        let m = g.transform(&t).unwrap();
        let os = g.snap_points();
        let ms = m.snap_points();
        assert_eq!(os.len(), ms.len());
        for (o, n) in os.iter().zip(ms.iter()) {
            assert_eq!(o.kind, n.kind);
            let expect = t.apply(o.point);
            assert!(n.point.dist(expect) < 1e-9, "snap incoherente");
        }
    }

    #[test]
    fn validate_ok_y_casos_invalidos() {
        let tol = Tol::default();
        assert!(square().validate(&tol).is_ok());
        assert!(
            WipeoutGeo::new(pts(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]))
                .validate(&tol)
                .is_ok()
        );
        // Fewer than three vertices.
        assert_eq!(
            WipeoutGeo::new(pts(&[(0.0, 0.0), (1.0, 1.0)])).validate(&tol),
            Err(GeomIssue::TooFewVertices)
        );
        // Non-finite coordinate.
        assert_eq!(
            WipeoutGeo::new(pts(&[(0.0, 0.0), (f64::NAN, 1.0), (1.0, 1.0)])).validate(&tol),
            Err(GeomIssue::NonFinite)
        );
    }

    #[test]
    fn serde_string_exacto_y_roundtrip() {
        let geo =
            EntityGeometry::Wipeout(WipeoutGeo::new(pts(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)])));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"wipeout","points":[[0.0,0.0],[1.0,0.0],[1.0,1.0]]}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }
}
