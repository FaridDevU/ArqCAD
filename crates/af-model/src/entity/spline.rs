//! Cubic spline interpolated through fit points.
//!
//! The curve math lives in [`af_geom::nurbs`].
//!
//! # Contract
//!
//! - `fit_points` and `closed` are the source of truth. Derived control points
//!   are recomputed on demand and are not serialized.
//! - The bounding box is exact and contains every snap point.
//! - Hit testing uses sampled and refined distance through [`FitSpline::nearest`].
//! - Snaps include curve endpoints and one node per fit point.
//! - Any affine transform is applied to the fit points and cannot fail.
//! - Validation requires finite, distinct consecutive fit points: at least two
//!   for an open spline and three for a closed spline.

// Geometry

use af_geom::nurbs::FitSpline;
use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Cubic interpolating spline defined by fit points and a `closed` flag.
///
/// The containing enum provides the internal JSON tag; derived control points
/// are omitted:
/// `{"type":"spline","fitPoints":[[0.0,0.0],[1.0,1.0]],"closed":false}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplineGeo {
    /// Fit points through which the curve passes.
    pub fit_points: Vec<Point2>,
    /// Whether the curve closes periodically from the last point to the first.
    pub closed: bool,
}

// Construction

impl SplineGeo {
    /// Creates a spline from fit points and a `closed` flag.
    ///
    /// Validation remains explicit through [`EntityOps::validate`].
    #[inline]
    #[must_use]
    pub fn new(fit_points: Vec<Point2>, closed: bool) -> Self {
        Self { fit_points, closed }
    }

    /// Builds an evaluable curve, or returns `None` for degenerate geometry.
    #[inline]
    #[must_use]
    pub fn fit_spline(&self) -> Option<FitSpline> {
        FitSpline::from_fit_points(&self.fit_points, self.closed)
    }
}

// Entity operations

impl EntityOps for SplineGeo {
    /// Returns the exact curve bounds, falling back to raw fit-point bounds for
    /// degenerate geometry.
    fn bbox(&self) -> BBox {
        if let Some(sp) = self.fit_spline() {
            sp.bbox()
        } else {
            BBox::from_points(self.fit_points.iter().copied())
                .unwrap_or_else(|| BBox::from_point(Point2::ORIGIN))
        }
    }

    /// Applies any affine transform to the fit points and rebuilds the curve.
    ///
    /// # Errors
    ///
    /// This implementation never returns an error.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        let fit_points = self.fit_points.iter().map(|&p| t.apply(p)).collect();
        Ok(Self {
            fit_points,
            closed: self.closed,
        })
    }

    /// Returns the sampled and refined distance to the curve when within `tol`.
    /// Degenerate geometry falls back to the fit-point polyline.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = match self.fit_spline() {
            Some(sp) => sp.nearest(p).2,
            None => dist_to_fit_polyline(&self.fit_points, self.closed, p),
        };
        (d <= tol).then_some(d)
    }

    /// Returns endpoint snaps and one node snap per fit point.
    fn snap_points(&self) -> SnapVec {
        let mut out = SnapVec::new();
        if let (Some(&first), Some(&last)) = (self.fit_points.first(), self.fit_points.last()) {
            out.push(SnapPoint::new(first, SnapKind::Endpoint));
            out.push(SnapPoint::new(last, SnapKind::Endpoint));
        }
        for &p in &self.fit_points {
            out.push(SnapPoint::new(p, SnapKind::Node));
        }
        out
    }

    /// Validates the count, finiteness, and spacing of fit points.
    ///
    /// # Errors
    ///
    /// Returns [`GeomIssue::TooFewVertices`], [`GeomIssue::NonFinite`], or
    /// [`GeomIssue::CoincidentVertices`] for the first invalid condition.
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        let min = if self.closed { 3 } else { 2 };
        if self.fit_points.len() < min {
            return Err(GeomIssue::TooFewVertices);
        }
        for p in &self.fit_points {
            if !p.x.is_finite() || !p.y.is_finite() {
                return Err(GeomIssue::NonFinite);
            }
        }
        for pair in self.fit_points.windows(2) {
            if tol.points_coincide(pair[0], pair[1]) {
                return Err(GeomIssue::CoincidentVertices);
            }
        }
        if self.closed {
            let first = self.fit_points[0];
            let last = self.fit_points[self.fit_points.len() - 1];
            if tol.points_coincide(first, last) {
                return Err(GeomIssue::CoincidentVertices);
            }
        }
        Ok(())
    }
}

// Geometry helpers

/// Distance from `p` to the fit-point polyline used when no curve can be built.
fn dist_to_fit_polyline(fit: &[Point2], closed: bool, p: Point2) -> f64 {
    match fit.len() {
        0 => f64::INFINITY,
        1 => p.dist(fit[0]),
        _ => {
            let n = fit.len();
            let count = if closed { n } else { n - 1 };
            let mut best = f64::INFINITY;
            for i in 0..count {
                best = best.min(dist_point_segment(p, fit[i], fit[(i + 1) % n]));
            }
            best
        }
    }
}

/// Euclidean distance from `p` to segment `[a, b]`.
fn dist_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let ab: Vec2 = b - a;
    let len_sq = ab.norm_sq();
    if len_sq <= 0.0 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

// Unit tests

#[cfg(test)]
mod tests {
    use super::*;

    use crate::entity::EntityGeometry;

    fn fit(v: &[(f64, f64)]) -> Vec<Point2> {
        v.iter().map(|&(x, y)| Point2::new(x, y)).collect()
    }

    fn s_curve(closed: bool) -> SplineGeo {
        SplineGeo::new(
            fit(&[(0.0, 0.0), (1.0, 2.0), (3.0, -1.0), (4.0, 1.0), (6.0, 0.0)]),
            closed,
        )
    }

    #[test]
    fn bbox_contiene_todos_los_snaps_y_la_curva() {
        let g = s_curve(false);
        let bb = g.bbox();
        for s in g.snap_points() {
            assert!(
                bb.expand(1e-9).contains_point(s.point),
                "snap {:?} fuera",
                s.point
            );
        }
        // Sample the curve as well.
        let sp = g.fit_spline().unwrap();
        let (t0, t1) = sp.param_range();
        for k in 0..=200 {
            let p = sp.eval(t0 + (t1 - t0) * (k as f64) / 200.0);
            assert!(
                bb.expand(1e-9).contains_point(p),
                "curva {p:?} fuera de {bb:?}"
            );
        }
    }

    #[test]
    fn hit_sobre_la_curva_y_fuera() {
        let g = s_curve(false);
        let sp = g.fit_spline().unwrap();
        let (t0, t1) = sp.param_range();
        let on = sp.eval(0.5 * (t0 + t1));
        assert!(g.hit(on, 1e-6).unwrap() < 1e-6);
        // A point clearly outside the curve.
        assert_eq!(g.hit(Point2::new(100.0, 100.0), 1e-6), None);
    }

    #[test]
    fn hit_en_los_puntos_de_ajuste() {
        let g = s_curve(false);
        for &p in &g.fit_points {
            assert!(
                g.hit(p, 1e-6).is_some(),
                "el punto de ajuste {p:?} debe acertar"
            );
        }
    }

    #[test]
    fn snaps_endpoints_y_nodes() {
        let g = s_curve(false);
        let snaps = g.snap_points();
        let n_end = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Endpoint)
            .count();
        let n_node = snaps.iter().filter(|s| s.kind == SnapKind::Node).count();
        assert_eq!((n_end, n_node), (2, 5));
        // Endpoints at the first and last fit points.
        assert_eq!(snaps[0].point, Point2::new(0.0, 0.0));
        assert_eq!(snaps[1].point, Point2::new(6.0, 0.0));
    }

    #[test]
    fn transform_traslada_los_puntos_de_ajuste() {
        let g = s_curve(false);
        let m = g
            .transform(&Transform2::translate(Vec2::new(3.0, -2.0)))
            .unwrap();
        for (a, b) in g.fit_points.iter().zip(m.fit_points.iter()) {
            assert_eq!(*b, Point2::new(a.x + 3.0, a.y - 2.0));
        }
    }

    #[test]
    fn transform_admite_escala_no_uniforme_sin_error() {
        // Splines transform their fit points and rebuild without failing.
        let g = s_curve(false);
        assert!(g.transform(&Transform2::scale(2.0, 5.0)).is_ok());
    }

    #[test]
    fn transform_coherente_en_los_snaps_para_cualquier_afin() {
        let g = s_curve(false);
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
        assert!(s_curve(false).validate(&tol).is_ok());
        assert!(s_curve(true).validate(&tol).is_ok());
        // Too few points.
        assert_eq!(
            SplineGeo::new(fit(&[(0.0, 0.0)]), false).validate(&tol),
            Err(GeomIssue::TooFewVertices)
        );
        // Closed splines require at least three points.
        assert_eq!(
            SplineGeo::new(fit(&[(0.0, 0.0), (1.0, 1.0)]), true).validate(&tol),
            Err(GeomIssue::TooFewVertices)
        );
        // Non-finite coordinate.
        assert_eq!(
            SplineGeo::new(fit(&[(0.0, 0.0), (f64::NAN, 1.0)]), false).validate(&tol),
            Err(GeomIssue::NonFinite)
        );
        // Coincident consecutive points.
        assert_eq!(
            SplineGeo::new(fit(&[(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)]), false).validate(&tol),
            Err(GeomIssue::CoincidentVertices)
        );
    }

    #[test]
    fn serde_string_exacto_y_roundtrip() {
        let geo = EntityGeometry::Spline(SplineGeo::new(
            fit(&[(0.0, 0.0), (1.0, 2.0), (3.0, 0.0)]),
            true,
        ));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"spline","fitPoints":[[0.0,0.0],[1.0,2.0],[3.0,0.0]],"closed":true}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }

    #[test]
    fn cerrada_bbox_contiene_la_curva() {
        let g = SplineGeo::new(fit(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]), true);
        let bb = g.bbox();
        let sp = g.fit_spline().unwrap();
        let (t0, t1) = sp.param_range();
        for k in 0..=300 {
            let p = sp.eval(t0 + (t1 - t0) * (k as f64) / 300.0);
            assert!(
                bb.expand(1e-9).contains_point(p),
                "curva cerrada fuera de bbox"
            );
        }
    }
}
