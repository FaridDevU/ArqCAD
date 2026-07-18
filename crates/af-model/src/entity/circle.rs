//! Circle defined by center and radius.
//!
//! Hits measure distance to the curve, not the disk. Snaps include center and four
//! quadrants. Conformal affine transforms are supported; anisotropic scale is not.
//!
//! The model remains exact; rendering owns tessellation.

// Geometry.

use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};
use smallvec::smallvec;

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Circle defined by `center` and `radius`.
///
/// JSON representation:
/// `{"type":"circle","center":[5.0,5.0],"radius":2.5}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleGeo {
    /// Circle center.
    pub center: Point2,
    /// Positive radius.
    pub radius: f64,
}

// Construction.

impl CircleGeo {
    /// Creates a circle from `center` and `radius`.
    ///
    /// Validation is deferred to [`EntityOps::validate`].
    #[inline]
    #[must_use]
    pub fn new(center: Point2, radius: f64) -> Self {
        Self { center, radius }
    }
}

// Entity operations.

impl EntityOps for CircleGeo {
    /// Bounding box from `center ± radius`.
    fn bbox(&self) -> BBox {
        let r = Vec2::new(self.radius, self.radius);
        BBox::new(self.center - r, self.center + r)
    }

    /// Applies an affine transform that preserves circular geometry.
    ///
    /// Uniform scale and reflection preserve the circle; anisotropic scale does not.
    ///
    /// # Errors
    /// Returns [`TransformError::NonUniformScaleUnsupported`] for anisotropic scale.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        if !t.is_uniform(&Tol::default()) {
            return Err(TransformError::NonUniformScaleUnsupported);
        }
        // Column norm keeps radius nonnegative, including under reflection.
        let (scale, _) = t.scale_factors();
        Ok(Self {
            center: t.apply(self.center),
            radius: self.radius * scale,
        })
    }

    /// Hit distance to the curve, or `None` beyond tolerance.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let d = (p.dist(self.center) - self.radius).abs();
        (d <= tol).then_some(d)
    }

    /// Center and four quadrant snaps.
    fn snap_points(&self) -> SnapVec {
        let c = self.center;
        let r = self.radius;
        smallvec![
            SnapPoint::new(c, SnapKind::Center),
            SnapPoint::new(Point2::new(c.x + r, c.y), SnapKind::Quadrant),
            SnapPoint::new(Point2::new(c.x, c.y + r), SnapKind::Quadrant),
            SnapPoint::new(Point2::new(c.x - r, c.y), SnapKind::Quadrant),
            SnapPoint::new(Point2::new(c.x, c.y - r), SnapKind::Quadrant),
        ]
    }

    /// Validates finite values and a nondegenerate radius.
    ///
    /// # Errors
    /// Returns [`GeomIssue::NonFinite`] or [`GeomIssue::DegenerateRadius`].
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        if !is_finite(self.center) || !self.radius.is_finite() {
            return Err(GeomIssue::NonFinite);
        }
        if self.radius <= tol.point_merge {
            return Err(GeomIssue::DegenerateRadius);
        }
        Ok(())
    }
}

// Geometry helpers.

/// Public exact circle geometry helpers.
impl CircleGeo {
    /// Circumference length: `2πr`.
    #[inline]
    #[must_use]
    pub fn circumference(&self) -> f64 {
        core::f64::consts::TAU * self.radius
    }

    /// Point at angle `rad`, measured CCW from `+X`.
    #[inline]
    #[must_use]
    pub fn point_at_angle(&self, rad: f64) -> Point2 {
        let (s, c) = rad.sin_cos();
        Point2::new(
            self.center.x + self.radius * c,
            self.center.y + self.radius * s,
        )
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

    fn sample() -> CircleGeo {
        CircleGeo::new(Point2::new(0.0, 0.0), 5.0)
    }

    #[test]
    fn bbox_es_caja_center_mas_menos_r() {
        let c = CircleGeo::new(Point2::new(2.0, -3.0), 4.0);
        let bb = c.bbox();
        assert_eq!(bb.min, Point2::new(-2.0, -7.0));
        assert_eq!(bb.max, Point2::new(6.0, 1.0));
    }

    #[test]
    fn hit_centro_no_acierta() {
        // Center distance to the curve equals the radius.
        let c = sample();
        assert_eq!(c.hit(Point2::new(0.0, 0.0), 0.1), None);
    }

    #[test]
    fn hit_sobre_anillo_es_cero() {
        let c = sample();
        // East quadrant lies exactly on the curve.
        assert_eq!(c.hit(Point2::new(5.0, 0.0), 1e-9), Some(0.0));
        // A 45-degree point also lies on the curve.
        let p = c.point_at_angle(FRAC_PI_4);
        let d = c.hit(p, 1e-6).expect("45° está sobre el anillo");
        assert!(d < 1e-9);
    }

    #[test]
    fn hit_dentro_cerca_del_anillo_segun_tol() {
        let c = sample();
        // A point 0.05 inside has curve distance 0.05.
        let inner = c.hit(Point2::new(4.95, 0.0), 0.1);
        assert!(matches!(inner, Some(d) if (d - 0.05).abs() < 1e-12));
        // A tighter tolerance rejects the same point.
        assert_eq!(c.hit(Point2::new(4.95, 0.0), 0.01), None);
    }

    #[test]
    fn hit_fuera_es_none() {
        let c = sample();
        // Far outside the curve.
        assert_eq!(c.hit(Point2::new(20.0, 0.0), 0.1), None);
    }

    #[test]
    fn snaps_son_center_y_cuatro_quadrants() {
        let c = CircleGeo::new(Point2::new(1.0, 2.0), 3.0);
        let snaps = c.snap_points();
        assert_eq!(snaps.len(), 5);
        assert_eq!(
            snaps[0],
            SnapPoint::new(Point2::new(1.0, 2.0), SnapKind::Center)
        );
        assert_eq!(
            snaps[1],
            SnapPoint::new(Point2::new(4.0, 2.0), SnapKind::Quadrant)
        );
        assert_eq!(
            snaps[2],
            SnapPoint::new(Point2::new(1.0, 5.0), SnapKind::Quadrant)
        );
        assert_eq!(
            snaps[3],
            SnapPoint::new(Point2::new(-2.0, 2.0), SnapKind::Quadrant)
        );
        assert_eq!(
            snaps[4],
            SnapPoint::new(Point2::new(1.0, -1.0), SnapKind::Quadrant)
        );
    }

    #[test]
    fn transform_translate() {
        let c = sample();
        let m = c
            .transform(&Transform2::translate(Vec2::new(3.0, -2.0)))
            .unwrap();
        assert_eq!(m.center, Point2::new(3.0, -2.0));
        assert_eq!(m.radius, 5.0);
    }

    #[test]
    fn transform_rotate_conserva_radio_y_center_dist() {
        let c = CircleGeo::new(Point2::new(3.0, 0.0), 5.0);
        let m = c.transform(&Transform2::rotate(FRAC_PI_2)).unwrap();
        // Rotation moves the center and preserves radius.
        assert!(m.center.x.abs() < 1e-12 && (m.center.y - 3.0).abs() < 1e-12);
        assert!((m.radius - 5.0).abs() < 1e-12);
    }

    #[test]
    fn transform_escala_uniforme_2x_duplica_radio() {
        let c = CircleGeo::new(Point2::new(1.0, 1.0), 2.5);
        let m = c.transform(&Transform2::scale(2.0, 2.0)).unwrap();
        assert_eq!(m.center, Point2::new(2.0, 2.0));
        assert_eq!(m.radius, 5.0);
    }

    #[test]
    fn transform_espejo_es_valido_y_conserva_radio() {
        // Reflection across Y is a conformal transform.
        let c = CircleGeo::new(Point2::new(4.0, 1.0), 3.0);
        let m = c.transform(&Transform2::scale(-1.0, 1.0)).unwrap();
        assert_eq!(m.center, Point2::new(-4.0, 1.0));
        assert_eq!(m.radius, 3.0);
    }

    #[test]
    fn transform_no_uniforme_es_err() {
        let c = sample();
        assert_eq!(
            c.transform(&Transform2::scale(2.0, 3.0)),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    #[test]
    fn validate_radio_valido_ok() {
        let tol = Tol::default();
        assert!(sample().validate(&tol).is_ok());
    }

    #[test]
    fn validate_radio_cero_o_negativo_es_degenerado() {
        let tol = Tol::default();
        let zero = CircleGeo::new(Point2::new(0.0, 0.0), 0.0);
        assert_eq!(zero.validate(&tol), Err(GeomIssue::DegenerateRadius));
        let neg = CircleGeo::new(Point2::new(0.0, 0.0), -2.0);
        assert_eq!(neg.validate(&tol), Err(GeomIssue::DegenerateRadius));
        // Radius below merge tolerance is degenerate.
        let tiny = CircleGeo::new(Point2::new(0.0, 0.0), tol.point_merge / 2.0);
        assert_eq!(tiny.validate(&tol), Err(GeomIssue::DegenerateRadius));
    }

    #[test]
    fn validate_no_finito_es_err() {
        let tol = Tol::default();
        let bad_center = CircleGeo::new(Point2::new(f64::NAN, 0.0), 1.0);
        assert_eq!(bad_center.validate(&tol), Err(GeomIssue::NonFinite));
        let bad_radius = CircleGeo::new(Point2::new(0.0, 0.0), f64::INFINITY);
        assert_eq!(bad_radius.validate(&tol), Err(GeomIssue::NonFinite));
    }

    /// Exact serialized string including `.0` for integral floating-point values.
    #[test]
    fn serde_string_exacto() {
        let geo = EntityGeometry::Circle(CircleGeo::new(Point2::new(5.0, 5.0), 2.5));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(json, r#"{"type":"circle","center":[5.0,5.0],"radius":2.5}"#);

        // Round trip.
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }

    #[test]
    fn circumference_es_2pi_r() {
        let c = sample();
        assert!((c.circumference() - TAU * 5.0).abs() < 1e-12);
    }

    #[test]
    fn point_at_angle_cuadrantes() {
        let c = sample();
        let e = c.point_at_angle(0.0);
        assert!((e.x - 5.0).abs() < 1e-12 && e.y.abs() < 1e-12);
        let n = c.point_at_angle(FRAC_PI_2);
        assert!(n.x.abs() < 1e-12 && (n.y - 5.0).abs() < 1e-12);
    }

    // Property: bounding boxes contain snap points.

    use proptest::prelude::*;

    prop_compose! {
        fn arb_circle()(
            cx in -1.0e6f64..1.0e6,
            cy in -1.0e6f64..1.0e6,
            r in 1.0e-3f64..1.0e6,
        ) -> CircleGeo {
            CircleGeo::new(Point2::new(cx, cy), r)
        }
    }

    proptest! {
        #[test]
        fn bbox_contiene_snaps(c in arb_circle()) {
            let bb = c.bbox();
            for s in c.snap_points() {
                // Quadrants lie on inclusive box borders.
                prop_assert!(bb.expand(1e-9).contains_point(s.point));
            }
        }
    }
}
