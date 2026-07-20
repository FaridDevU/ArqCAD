//! Ellipse or elliptical arc using center, semimajor axis, ratio, rotation, and
//! a parameter sweep.
//!
//! It follows DXF ELLIPSE conventions: radians, CCW parameter sweep, and
//! `semi_minor = semi_major * ratio`. Bounding boxes include the center snap.
//! Conformal transforms are supported; reflections preserve CCW storage.
//!
//! Rendering owns tessellation of the exact model.

use core::f64::consts::{FRAC_PI_2, PI};

use af_geom::ellipse::Ellipse;
use af_math::angle::{angle_of, normalize_0_2pi};
use af_math::{BBox, Point2, Tol, Transform2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Eccentric-anomaly parameters for the four axis vertices.
const QUADRANT_PARAMS: [f64; 4] = [0.0, FRAC_PI_2, PI, PI + FRAC_PI_2];

/// Ellipse or elliptical arc with a CCW parameter sweep.
///
/// JSON representation:
/// `{"type":"ellipse","center":[0.0,0.0],"semiMajor":3.0,"ratio":0.5,"rotation":0.0,"startParam":0.0,"endParam":6.283185307179586}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EllipseGeo {
    /// Ellipse center.
    pub center: Point2,
    /// Positive semimajor axis `a`.
    pub semi_major: f64,
    /// Minor-to-major ratio `b/a` in `(0, 1]`.
    pub ratio: f64,
    /// Semimajor-axis rotation in radians CCW from `+X`.
    pub rotation: f64,
    /// Start eccentric anomaly.
    pub start_param: f64,
    /// End eccentric anomaly reached by sweeping CCW.
    pub end_param: f64,
}

// Construction.

impl EllipseGeo {
    /// Creates an elliptical arc or full ellipse for an approximately zero sweep.
    ///
    /// Validation is deferred to [`EntityOps::validate`].
    #[inline]
    #[must_use]
    pub fn new(
        center: Point2,
        semi_major: f64,
        ratio: f64,
        rotation: f64,
        start_param: f64,
        end_param: f64,
    ) -> Self {
        Self {
            center,
            semi_major,
            ratio,
            rotation,
            start_param,
            end_param,
        }
    }
}

// Entity operations.

impl EntityOps for EllipseGeo {
    /// Exact elliptical-arc box united with center.
    fn bbox(&self) -> BBox {
        self.ellipse().bbox().union_point(self.center)
    }

    /// Applies a conformal affine transform.
    ///
    /// Reflections reorder negated parameters to retain CCW representation.
    ///
    /// # Errors
    /// Returns [`TransformError::NonUniformScaleUnsupported`] for anisotropic scale.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        let tol = Tol::default();
        if !t.is_uniform(&tol) {
            return Err(TransformError::NonUniformScaleUnsupported);
        }
        let (scale, _) = t.scale_factors();
        let new_major = t.apply_vec(self.ellipse().major_axis());
        let (start_param, end_param) = if t.is_mirroring() {
            (-self.end_param, -self.start_param)
        } else {
            (self.start_param, self.end_param)
        };
        Ok(Self {
            center: t.apply(self.center),
            semi_major: self.semi_major * scale,
            ratio: self.ratio,
            rotation: angle_of(new_major),
            start_param,
            end_param,
        })
    }

    /// Approximate curve distance, or `None` beyond tolerance.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = self.ellipse().distance_to(p);
        (d <= tol).then_some(d)
    }

    /// Endpoints, midpoint, center, and interior axis-vertex snaps.
    fn snap_points(&self) -> SnapVec {
        let e = self.ellipse();
        let mut out: SnapVec = smallvec![
            SnapPoint::new(e.start_point(), SnapKind::Endpoint),
            SnapPoint::new(e.end_point(), SnapKind::Endpoint),
            SnapPoint::new(e.midpoint(), SnapKind::Midpoint),
            SnapPoint::new(self.center, SnapKind::Center),
        ];
        let sweep = e.sweep();
        let atol = Tol::default().angle;
        for &q in &QUADRANT_PARAMS {
            // Strict interior excludes quadrant snaps that duplicate endpoints.
            let off = normalize_0_2pi(q - self.start_param);
            if off > atol && off < sweep - atol {
                out.push(SnapPoint::new(e.point_at(q), SnapKind::Quadrant));
            }
        }
        out
    }

    /// Validates finite values and nondegenerate semiaxes.
    ///
    /// # Errors
    /// Returns [`GeomIssue::NonFinite`], [`GeomIssue::InvalidAxisRatio`], or
    /// [`GeomIssue::DegenerateRadius`].
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        if !is_finite(self.center)
            || !self.semi_major.is_finite()
            || !self.ratio.is_finite()
            || !self.rotation.is_finite()
            || !self.start_param.is_finite()
            || !self.end_param.is_finite()
        {
            return Err(GeomIssue::NonFinite);
        }
        if self.ratio <= 0.0 || self.ratio > 1.0 {
            return Err(GeomIssue::InvalidAxisRatio);
        }
        if self.semi_major <= tol.point_merge || self.semi_minor() <= tol.point_merge {
            return Err(GeomIssue::DegenerateRadius);
        }
        Ok(())
    }
}

// Geometry helpers.

impl EllipseGeo {
    /// Semiminor axis `b = semi_major * ratio`.
    #[inline]
    #[must_use]
    pub fn semi_minor(&self) -> f64 {
        self.semi_major * self.ratio
    }

    /// Converts to an `af-geom` [`Ellipse`].
    #[inline]
    #[must_use]
    pub fn ellipse(&self) -> Ellipse {
        Ellipse::new(
            self.center,
            self.semi_major,
            self.semi_minor(),
            self.rotation,
            self.start_param,
            self.end_param,
        )
    }

    /// CCW parameter sweep in `(0, 2π]`.
    #[inline]
    #[must_use]
    pub fn sweep(&self) -> f64 {
        self.ellipse().sweep()
    }

    /// Arc length via composite Simpson integration, exact in the circular limit.
    #[must_use]
    pub fn length(&self) -> f64 {
        const STEPS: usize = 4096;
        let semi_minor = self.semi_minor();
        let sweep = self.sweep();
        if (self.semi_major - semi_minor).abs() <= f64::EPSILON * self.semi_major {
            return sweep * self.semi_major;
        }

        let step = sweep / STEPS as f64;
        let speed = |parameter: f64| {
            (self.semi_major * parameter.sin()).hypot(semi_minor * parameter.cos())
        };
        let mut sum = speed(self.start_param) + speed(self.start_param + sweep);
        for index in 1..STEPS {
            let parameter = self.start_param + step * index as f64;
            sum += if index % 2 == 0 { 2.0 } else { 4.0 } * speed(parameter);
        }
        step * sum / 3.0
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
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

    use crate::entity::EntityGeometry;
    use af_math::Vec2;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    /// Full unrotated ellipse with `a=3`, `b=1.5`.
    fn full() -> EllipseGeo {
        EllipseGeo::new(Point2::ORIGIN, 3.0, 0.5, 0.0, 0.0, TAU)
    }

    #[test]
    fn bbox_alineada_incluye_semiejes_y_centro() {
        let bb = full().bbox();
        assert!(close(bb.min.x, -3.0) && close(bb.max.x, 3.0));
        assert!(close(bb.min.y, -1.5) && close(bb.max.y, 1.5));
    }

    #[test]
    fn hit_sobre_la_curva_y_centro_no_acierta() {
        let e = full();
        // Semimajor vertex lies on the curve.
        assert!(e.hit(Point2::new(3.0, 0.0), 1e-6).unwrap() < 1e-9);
        // Center curve distance equals the semiminor axis.
        assert_eq!(e.hit(Point2::ORIGIN, 0.5), None);
    }

    #[test]
    fn transform_rotate_gira_el_semieje_mayor() {
        // Rotating an unrotated ellipse updates its rotation by 90 degrees.
        let m = full().transform(&Transform2::rotate(FRAC_PI_2)).unwrap();
        assert!(close(m.semi_major, 3.0));
        assert!(close(m.ratio, 0.5));
        assert!(close(normalize_0_2pi(m.rotation), FRAC_PI_2));
    }

    #[test]
    fn transform_escala_uniforme_duplica_semieje() {
        let m = full().transform(&Transform2::scale(2.0, 2.0)).unwrap();
        assert!(close(m.semi_major, 6.0));
        assert!(close(m.semi_minor(), 3.0));
        assert!(close(m.ratio, 0.5));
    }

    #[test]
    fn transform_espejo_conserva_la_curva() {
        // Reflection across Y reflects the arc midpoint.
        let e = EllipseGeo::new(Point2::ORIGIN, 2.0, 0.5, 0.0, 0.0, FRAC_PI_2);
        let orig_mid = e.ellipse().midpoint();
        let m = e.transform(&Transform2::scale(-1.0, 1.0)).unwrap();
        assert!(close(m.semi_major, 2.0));
        assert!(close(m.sweep(), FRAC_PI_2));
        let reflected_mid = Point2::new(-orig_mid.x, orig_mid.y);
        assert!(close_pt(m.ellipse().midpoint(), reflected_mid));
    }

    #[test]
    fn transform_no_uniforme_es_err() {
        assert_eq!(
            full().transform(&Transform2::scale(2.0, 3.0)),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    #[test]
    fn transform_translate() {
        let m = full()
            .transform(&Transform2::translate(Vec2::new(4.0, -1.0)))
            .unwrap();
        assert!(close_pt(m.center, Point2::new(4.0, -1.0)));
        assert!(close(m.semi_major, 3.0));
    }

    #[test]
    fn snaps_arco_cuarto_incluye_center_y_endpoints() {
        // Quarter-ellipse axis vertices coincide with endpoints, so no duplicates.
        let e = EllipseGeo::new(Point2::ORIGIN, 2.0, 0.5, 0.0, 0.0, FRAC_PI_2);
        let snaps = e.snap_points();
        let n_center = snaps.iter().filter(|s| s.kind == SnapKind::Center).count();
        let n_quad = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Quadrant)
            .count();
        assert_eq!((n_center, n_quad), (1, 0));
    }

    #[test]
    fn bbox_contiene_todos_los_snaps() {
        let e = EllipseGeo::new(Point2::new(4.0, -2.0), 6.0, 0.4, FRAC_PI_4, 0.5, 4.5);
        let bb = e.bbox();
        for s in e.snap_points() {
            assert!(
                bb.expand(1e-9).contains_point(s.point),
                "snap {:?} fuera de {bb:?}",
                s.point
            );
        }
    }

    #[test]
    fn validate_ok_y_casos_invalidos() {
        let tol = Tol::default();
        assert!(full().validate(&tol).is_ok());
        // Degenerate semimajor axis.
        let zero = EllipseGeo::new(Point2::ORIGIN, 0.0, 0.5, 0.0, 0.0, TAU);
        assert_eq!(zero.validate(&tol), Err(GeomIssue::DegenerateRadius));
        // Ratio has its own model invariant before semiaxis degeneracy.
        for ratio in [0.0, -0.5, 1.000_000_1] {
            let invalid = EllipseGeo::new(Point2::ORIGIN, 3.0, ratio, 0.0, 0.0, TAU);
            assert_eq!(invalid.validate(&tol), Err(GeomIssue::InvalidAxisRatio));
        }
        assert!(
            EllipseGeo::new(Point2::ORIGIN, 3.0, 1.0, 0.0, 0.0, TAU)
                .validate(&tol)
                .is_ok()
        );
        let tiny = EllipseGeo::new(Point2::ORIGIN, 3.0, f64::MIN_POSITIVE, 0.0, 0.0, TAU);
        assert_eq!(tiny.validate(&tol), Err(GeomIssue::DegenerateRadius));
        // Nonfinite value.
        let bad = EllipseGeo::new(Point2::new(f64::NAN, 0.0), 3.0, 0.5, 0.0, 0.0, TAU);
        assert_eq!(bad.validate(&tol), Err(GeomIssue::NonFinite));
        let bad_ratio = EllipseGeo::new(Point2::ORIGIN, 3.0, f64::NAN, 0.0, 0.0, TAU);
        assert_eq!(bad_ratio.validate(&tol), Err(GeomIssue::NonFinite));
    }

    #[test]
    fn length_completa_arco_y_limite_circular() {
        let full = EllipseGeo::new(Point2::ORIGIN, 40.0, 0.5, 0.0, 0.0, TAU);
        let quarter = EllipseGeo::new(Point2::ORIGIN, 40.0, 0.5, FRAC_PI_4, 0.0, FRAC_PI_2);
        let circular = EllipseGeo::new(Point2::ORIGIN, 20.0, 1.0, 0.0, 0.0, FRAC_PI_2);
        assert!((full.length() - 193.768_964_410_963).abs() < 1e-9);
        assert!((quarter.length() - 48.442_241_102_741).abs() < 1e-9);
        assert!((circular.length() - 10.0 * PI).abs() < 1e-12);
    }

    #[test]
    fn serde_string_exacto_y_roundtrip() {
        let geo = EntityGeometry::Ellipse(EllipseGeo::new(Point2::ORIGIN, 3.0, 0.5, 0.0, 0.0, 1.5));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"ellipse","center":[0.0,0.0],"semiMajor":3.0,"ratio":0.5,"rotation":0.0,"startParam":0.0,"endParam":1.5}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }
}
