//! Circular arc defined by center, radius, and a counterclockwise sweep.
//!
//! Angles use radians. Bounding boxes include arc extrema and center so all snaps
//! are contained. Hits measure distance to the curve. Conformal transforms are
//! supported; reflections swap endpoints to retain CCW storage.
//!
//! Rendering can tessellate the exact model through `af-geom`.

// Geometry.

use core::f64::consts::{FRAC_PI_2, PI};

use af_geom::arc::arc_bbox;
use af_geom::bulge::ArcSeg;
use af_math::angle::{angle_of, normalize_0_2pi};
use af_math::{BBox, Point2, Tol, Transform2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Axis-crossing angles in radians.
const QUADRANTS: [f64; 4] = [0.0, FRAC_PI_2, PI, PI + FRAC_PI_2];

/// Circular arc with a CCW radian sweep from `start_angle` to `end_angle`.
///
/// JSON representation:
/// `{"type":"arc","center":[0.0,0.0],"radius":5.0,"startAngle":0.0,"endAngle":1.5}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcGeo {
    /// Supporting-circle center.
    pub center: Point2,
    /// Positive radius.
    pub radius: f64,
    /// Start angle CCW from `+X`.
    pub start_angle: f64,
    /// End angle reached by sweeping CCW.
    pub end_angle: f64,
}

// Construction.

impl ArcGeo {
    /// Creates an arc with a CCW radian sweep.
    ///
    /// Validation is deferred to [`EntityOps::validate`].
    #[inline]
    #[must_use]
    pub fn new(center: Point2, radius: f64, start_angle: f64, end_angle: f64) -> Self {
        Self {
            center,
            radius,
            start_angle,
            end_angle,
        }
    }
}

// Entity operations.

impl EntityOps for ArcGeo {
    /// Tight arc box united with center to contain the center snap.
    fn bbox(&self) -> BBox {
        arc_bbox(self.center, self.radius, self.start_angle, self.end_angle)
            .union_point(self.center)
    }

    /// Applies a conformal affine transform.
    ///
    /// Reflections swap transformed endpoints to retain CCW representation.
    ///
    /// # Errors
    /// Returns [`TransformError::NonUniformScaleUnsupported`] for anisotropic scale.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        let tol = Tol::default();
        if !t.is_uniform(&tol) {
            return Err(TransformError::NonUniformScaleUnsupported);
        }
        let (scale, _) = t.scale_factors();
        let seg = self.arc_seg();
        let new_center = t.apply(self.center);
        let ang_start = angle_of(t.apply(seg.start_point()) - new_center);
        let ang_end = angle_of(t.apply(seg.end_point()) - new_center);
        // Reflection reverses orientation, so swap endpoints for CCW storage.
        let (start_angle, end_angle) = if t.is_mirroring() {
            (ang_end, ang_start)
        } else {
            (ang_start, ang_end)
        };
        Ok(Self {
            center: new_center,
            radius: self.radius * scale,
            start_angle,
            end_angle,
        })
    }

    /// Hit distance to the arc curve, or `None` beyond tolerance.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = self.arc_seg().distance_to(p);
        (d <= tol).then_some(d)
    }

    /// Endpoints, arc midpoint, center, and interior quadrant snaps.
    fn snap_points(&self) -> SnapVec {
        let seg = self.arc_seg();
        let mut out: SnapVec = smallvec![
            SnapPoint::new(seg.start_point(), SnapKind::Endpoint),
            SnapPoint::new(seg.end_point(), SnapKind::Endpoint),
            SnapPoint::new(seg.midpoint(), SnapKind::Midpoint),
            SnapPoint::new(self.center, SnapKind::Center),
        ];
        let sweep = seg.sweep();
        let atol = Tol::default().angle;
        for &q in &QUADRANTS {
            // Strict interior excludes quadrant snaps that duplicate endpoints.
            let off = normalize_0_2pi(q - self.start_angle);
            if off > atol && off < sweep - atol {
                out.push(SnapPoint::new(seg.point_at(q), SnapKind::Quadrant));
            }
        }
        out
    }

    /// Validates finite values and a nondegenerate radius.
    ///
    /// # Errors
    /// Returns [`GeomIssue::NonFinite`] or [`GeomIssue::DegenerateRadius`].
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        if !is_finite(self.center)
            || !self.radius.is_finite()
            || !self.start_angle.is_finite()
            || !self.end_angle.is_finite()
        {
            return Err(GeomIssue::NonFinite);
        }
        if self.radius <= tol.point_merge {
            return Err(GeomIssue::DegenerateRadius);
        }
        Ok(())
    }
}

// Geometry helpers.

impl ArcGeo {
    /// Converts to an `af-geom` [`ArcSeg`].
    #[inline]
    #[must_use]
    pub fn arc_seg(&self) -> ArcSeg {
        ArcSeg {
            center: self.center,
            radius: self.radius,
            start_angle: self.start_angle,
            end_angle: self.end_angle,
        }
    }

    /// CCW angular sweep in `(0, 2π]`.
    #[inline]
    #[must_use]
    pub fn sweep(&self) -> f64 {
        self.arc_seg().sweep()
    }

    /// Arc length: radius times sweep.
    #[inline]
    #[must_use]
    pub fn length(&self) -> f64 {
        self.arc_seg().length()
    }
}

/// Whether both point coordinates are finite.
#[inline]
fn is_finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

// Unit tests.

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    use crate::entity::EntityGeometry;
    use af_math::Vec2;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    /// Northeast quarter arc centered at the origin.
    fn quarter() -> ArcGeo {
        ArcGeo::new(Point2::ORIGIN, 1.0, 0.0, FRAC_PI_2)
    }

    #[test]
    fn bbox_cuarto_ne_no_alcanza_ejes_negativos_pero_incluye_centro() {
        let bb = quarter().bbox();
        // The center forces the quarter-arc minimum to the origin.
        assert!(close(bb.min.x, 0.0) && close(bb.min.y, 0.0));
        assert!(close(bb.max.x, 1.0) && close(bb.max.y, 1.0));
    }

    #[test]
    fn bbox_semicirculo_superior_incluye_y_mas_r_y_centro() {
        // The upper semicircle reaches +radius and includes the center.
        let arc = ArcGeo::new(Point2::ORIGIN, 2.0, 0.0, PI);
        let bb = arc.bbox();
        assert!(close(bb.min.x, -2.0) && close(bb.max.x, 2.0));
        assert!(close(bb.min.y, 0.0) && close(bb.max.y, 2.0));
    }

    #[test]
    fn hit_sobre_la_curva_y_centro_no_acierta() {
        let arc = quarter();
        // The 45-degree midpoint lies on the curve.
        let mid = arc.arc_seg().midpoint();
        assert!(arc.hit(mid, 1e-6).unwrap() < 1e-9);
        // The center is one radius from the curve.
        assert_eq!(arc.hit(Point2::ORIGIN, 0.5), None);
        // A supporting-circle point outside the sweep does not hit.
        let outside = Point2::new(0.0, -1.0);
        assert_eq!(arc.hit(outside, 0.5), None);
    }

    #[test]
    fn snaps_cuarto_son_end_end_mid_center_sin_quadrant() {
        // Endpoint quadrants are not duplicated.
        let snaps = quarter().snap_points();
        let n_end = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Endpoint)
            .count();
        let n_mid = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Midpoint)
            .count();
        let n_center = snaps.iter().filter(|s| s.kind == SnapKind::Center).count();
        let n_quad = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Quadrant)
            .count();
        assert_eq!((n_end, n_mid, n_center, n_quad), (2, 1, 1, 0));
    }

    #[test]
    fn snaps_incluyen_quadrant_interior() {
        // A 45-to-135-degree sweep contains the 90-degree quadrant internally.
        let arc = ArcGeo::new(Point2::ORIGIN, 3.0, FRAC_PI_4, PI - FRAC_PI_4);
        let quads: Vec<_> = arc
            .snap_points()
            .into_iter()
            .filter(|s| s.kind == SnapKind::Quadrant)
            .collect();
        assert_eq!(quads.len(), 1);
        assert!(close_pt(quads[0].point, Point2::new(0.0, 3.0)));
    }

    #[test]
    fn bbox_contiene_todos_los_snaps() {
        // Arc crossing negative X with an offset center and large radius.
        let arc = ArcGeo::new(
            Point2::new(4.0, -2.0),
            10.0,
            170f64.to_radians(),
            190f64.to_radians(),
        );
        let bb = arc.bbox();
        for s in arc.snap_points() {
            assert!(
                bb.expand(1e-9).contains_point(s.point),
                "snap {:?} fuera de {bb:?}",
                s.point
            );
        }
    }

    #[test]
    fn transform_translate() {
        let arc = quarter();
        let m = arc
            .transform(&Transform2::translate(Vec2::new(3.0, -2.0)))
            .unwrap();
        assert!(close_pt(m.center, Point2::new(3.0, -2.0)));
        assert!(close(m.radius, 1.0));
        // Translation preserves sweep and angles.
        assert!(close(m.sweep(), FRAC_PI_2));
    }

    #[test]
    fn transform_rotate_desplaza_los_angulos() {
        // A 90-degree rotation shifts the quarter-arc angles accordingly.
        let m = quarter().transform(&Transform2::rotate(FRAC_PI_2)).unwrap();
        assert!(close(m.sweep(), FRAC_PI_2));
        assert!(close_pt(m.arc_seg().start_point(), Point2::new(0.0, 1.0)));
        assert!(close_pt(m.arc_seg().end_point(), Point2::new(-1.0, 0.0)));
    }

    #[test]
    fn transform_escala_uniforme_duplica_radio() {
        let arc = ArcGeo::new(Point2::new(1.0, 1.0), 2.5, 0.0, FRAC_PI_2);
        let m = arc.transform(&Transform2::scale(2.0, 2.0)).unwrap();
        assert!(close_pt(m.center, Point2::new(2.0, 2.0)));
        assert!(close(m.radius, 5.0));
    }

    #[test]
    fn transform_espejo_invierte_orientacion_conserva_curva() {
        // Reflection across Y moves the first-quadrant arc into the second.
        let arc = quarter();
        let orig_mid = arc.arc_seg().midpoint();
        let m = arc.transform(&Transform2::scale(-1.0, 1.0)).unwrap();
        assert!(close(m.radius, 1.0));
        assert!(close(m.sweep(), FRAC_PI_2));
        // The reflected midpoint matches reflection of the original midpoint.
        let reflected_mid = Point2::new(-orig_mid.x, orig_mid.y);
        assert!(close_pt(m.arc_seg().midpoint(), reflected_mid));
    }

    #[test]
    fn transform_no_uniforme_es_err() {
        assert_eq!(
            quarter().transform(&Transform2::scale(2.0, 3.0)),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    #[test]
    fn validate_ok_y_casos_invalidos() {
        let tol = Tol::default();
        assert!(quarter().validate(&tol).is_ok());
        // Degenerate radius.
        let zero = ArcGeo::new(Point2::ORIGIN, 0.0, 0.0, FRAC_PI_2);
        assert_eq!(zero.validate(&tol), Err(GeomIssue::DegenerateRadius));
        // Nonfinite center, radius, and angles.
        let bad_c = ArcGeo::new(Point2::new(f64::NAN, 0.0), 1.0, 0.0, FRAC_PI_2);
        assert_eq!(bad_c.validate(&tol), Err(GeomIssue::NonFinite));
        let bad_r = ArcGeo::new(Point2::ORIGIN, f64::INFINITY, 0.0, FRAC_PI_2);
        assert_eq!(bad_r.validate(&tol), Err(GeomIssue::NonFinite));
        let bad_a = ArcGeo::new(Point2::ORIGIN, 1.0, f64::NAN, FRAC_PI_2);
        assert_eq!(bad_a.validate(&tol), Err(GeomIssue::NonFinite));
    }

    #[test]
    fn serde_string_exacto_y_roundtrip() {
        let geo = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 5.0, 0.0, 1.5));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"arc","center":[0.0,0.0],"radius":5.0,"startAngle":0.0,"endAngle":1.5}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }

    #[test]
    fn length_es_radio_por_barrido() {
        // A radius-2 semicircle has length 2π.
        let arc = ArcGeo::new(Point2::ORIGIN, 2.0, 0.0, PI);
        assert!(close(arc.length(), 2.0 * PI));
    }

    // Property: bounding boxes contain snap points.

    use proptest::prelude::*;

    prop_compose! {
        fn arb_arc()(
            cx in -1.0e6f64..1.0e6,
            cy in -1.0e6f64..1.0e6,
            r in 1.0e-3f64..1.0e6,
            start in 0.0f64..core::f64::consts::TAU,
            sweep in 0.05f64..(core::f64::consts::TAU - 0.05),
        ) -> ArcGeo {
            ArcGeo::new(Point2::new(cx, cy), r, start, start + sweep)
        }
    }

    proptest! {
        #[test]
        fn bbox_contiene_snaps(arc in arb_arc()) {
            let bb = arc.bbox();
            for s in arc.snap_points() {
                // Scale tolerance with coordinates because trigonometric error grows.
                let slack = 1e-9 * (1.0 + arc.radius);
                prop_assert!(
                    bb.expand(slack).contains_point(s.point),
                    "snap {:?} fuera de {bb:?}", s.point
                );
            }
        }
    }
}
