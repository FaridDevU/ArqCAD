//! Polyline bulge semantics and exact derived circular arcs.

use af_math::angle::{angle_in_sweep, angle_of, normalize_0_2pi, sweep_ccw};
use af_math::{BBox, Point2, Tol};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArcSeg {
    pub center: Point2,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulgeError {
    NonFinite,
    DegenerateChord,
    StraightSegment,
}

impl core::fmt::Display for BulgeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            BulgeError::NonFinite => "bulge or endpoint is not finite",
            BulgeError::DegenerateChord => "arc chord endpoints coincide",
            BulgeError::StraightSegment => "bulge is ~0: the segment is straight, not an arc",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for BulgeError {}

pub fn bulge_to_arc(a: Point2, b: Point2, bulge: f64) -> Result<ArcSeg, BulgeError> {
    if !bulge.is_finite() || !is_finite(a) || !is_finite(b) {
        return Err(BulgeError::NonFinite);
    }
    let tol = Tol::default();
    let chord = b - a;
    let chord_len = chord.norm();
    if chord_len <= tol.point_merge {
        return Err(BulgeError::DegenerateChord);
    }
    if bulge.abs() <= tol.linear {
        return Err(BulgeError::StraightSegment);
    }

    let mid = a.midpoint(b);
    let half_chord = chord_len * 0.5;
    let perp = chord.perp() / chord_len;
    let apothem = half_chord * (1.0 - bulge * bulge) / (2.0 * bulge);
    let center = mid + perp * apothem;
    let radius = center.dist(a);

    let ang_a = angle_of(a - center);
    let ang_b = angle_of(b - center);
    let (start_angle, end_angle) = if bulge > 0.0 {
        (ang_a, ang_b)
    } else {
        (ang_b, ang_a)
    };

    Ok(ArcSeg {
        center,
        radius,
        start_angle,
        end_angle,
    })
}

#[must_use]
pub fn arc_to_bulge(a: Point2, b: Point2, arc: &ArcSeg) -> f64 {
    let sweep = sweep_ccw(arc.start_angle, arc.end_angle);
    let magnitude = (sweep * 0.25).tan().abs();
    let start = arc.start_point();
    if start.dist(a) <= start.dist(b) {
        magnitude
    } else {
        -magnitude
    }
}

#[inline]
fn seg_travel_angle(arc: &ArcSeg, bulge: f64, f: f64) -> f64 {
    let sweep = arc.sweep();
    if bulge >= 0.0 {
        arc.start_angle + f * sweep
    } else {
        arc.end_angle - f * sweep
    }
}

#[must_use]
pub fn seg_point_at(a: Point2, b: Point2, bulge: f64, f: f64) -> Point2 {
    match bulge_to_arc(a, b, bulge) {
        Ok(arc) => arc.point_at(seg_travel_angle(&arc, bulge, f)),
        Err(_) => a + (b - a) * f,
    }
}

#[must_use]
pub fn seg_angle_fraction(a: Point2, b: Point2, bulge: f64, theta: f64) -> Option<f64> {
    let arc = bulge_to_arc(a, b, bulge).ok()?;
    let sweep = arc.sweep();
    let off = if bulge >= 0.0 {
        normalize_0_2pi(theta - arc.start_angle)
    } else {
        normalize_0_2pi(arc.end_angle - theta)
    };
    Some(off / sweep)
}

#[must_use]
pub fn split_bulge_segment(
    a: Point2,
    b: Point2,
    bulge: f64,
    f0: f64,
    f1: f64,
) -> (Point2, Point2, f64) {
    match bulge_to_arc(a, b, bulge) {
        Ok(arc) => {
            let sweep = arc.sweep();
            let p0 = arc.point_at(seg_travel_angle(&arc, bulge, f0));
            let p1 = arc.point_at(seg_travel_angle(&arc, bulge, f1));
            let sub_bulge = ((f1 - f0) * sweep * 0.25).tan().copysign(bulge);
            (p0, p1, sub_bulge)
        }
        Err(_) => (a + (b - a) * f0, a + (b - a) * f1, 0.0),
    }
}

impl ArcSeg {
    #[inline]
    #[must_use]
    pub fn point_at(&self, angle: f64) -> Point2 {
        let (s, c) = angle.sin_cos();
        Point2::new(
            self.center.x + self.radius * c,
            self.center.y + self.radius * s,
        )
    }

    #[inline]
    #[must_use]
    pub fn start_point(&self) -> Point2 {
        self.point_at(self.start_angle)
    }

    #[inline]
    #[must_use]
    pub fn end_point(&self) -> Point2 {
        self.point_at(self.end_angle)
    }

    #[inline]
    #[must_use]
    pub fn sweep(&self) -> f64 {
        sweep_ccw(self.start_angle, self.end_angle)
    }

    #[inline]
    #[must_use]
    pub fn length(&self) -> f64 {
        self.radius * self.sweep()
    }

    #[inline]
    #[must_use]
    pub fn midpoint(&self) -> Point2 {
        self.point_at(self.start_angle + self.sweep() * 0.5)
    }

    #[must_use]
    pub fn bbox(&self) -> BBox {
        crate::arc::arc_bbox(self.center, self.radius, self.start_angle, self.end_angle)
    }

    #[must_use]
    pub fn distance_to(&self, p: Point2) -> f64 {
        let v = p - self.center;
        if angle_in_sweep(angle_of(v), self.start_angle, self.end_angle) {
            (v.norm() - self.radius).abs()
        } else {
            p.dist(self.start_point()).min(p.dist(self.end_point()))
        }
    }
}

#[inline]
fn is_finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_8, PI, TAU};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn semicirculo_ccw_bulge_positivo() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, 1.0).unwrap();
        assert!(close_pt(arc.center, Point2::new(1.0, 0.0)));
        assert!(close(arc.radius, 1.0));
        assert!(close(arc.sweep(), PI));
        assert!(close_pt(arc.midpoint(), Point2::new(1.0, -1.0)));
        assert!(close_pt(arc.start_point(), a));
        assert!(close_pt(arc.end_point(), b));
    }

    #[test]
    fn semicirculo_cw_bulge_negativo() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, -1.0).unwrap();
        assert!(close_pt(arc.center, Point2::new(1.0, 0.0)));
        assert!(close(arc.radius, 1.0));
        assert!(close(arc.sweep(), PI));
        assert!(close_pt(arc.midpoint(), Point2::new(1.0, 1.0)));
    }

    #[test]
    fn cuarto_de_circulo_bulge_tan_pi_8() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, FRAC_PI_8.tan()).unwrap();
        assert!(close(arc.sweep(), FRAC_PI_2));
        assert!(close(arc.radius, 2.0_f64.sqrt()));
        assert!(close_pt(arc.center, Point2::new(1.0, 1.0)));
        assert!(close_pt(
            arc.midpoint(),
            Point2::new(1.0, 1.0 - 2.0_f64.sqrt())
        ));
    }

    #[test]
    fn arco_mayor_bulge_mayor_que_uno() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, 2.0).unwrap();
        assert!(arc.sweep() > PI);
        assert!(close(arc.sweep(), 4.0 * 2.0_f64.atan()));
        assert!(close(arc.radius, 1.25));
        assert!(close_pt(arc.center, Point2::new(1.0, -0.75)));
    }

    #[test]
    fn signo_opuesto_refleja_el_arco() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(4.0, 2.0);
        let pos = bulge_to_arc(a, b, 0.5).unwrap();
        let neg = bulge_to_arc(a, b, -0.5).unwrap();
        assert!(close(pos.radius, neg.radius));
        assert!(close(pos.sweep(), neg.sweep()));
        let mid = a.midpoint(b);
        let dp = pos.midpoint() - mid;
        let dn = neg.midpoint() - mid;
        assert!(close(dp.norm(), dn.norm()));
        assert!(close_pt(pos.midpoint(), mid - (neg.midpoint() - mid)));
    }

    #[test]
    fn round_trip_bulge_arc_bulge() {
        let a = Point2::new(-3.0, 1.5);
        let b = Point2::new(5.0, -2.0);
        for &beta in &[0.2_f64, 1.0, FRAC_PI_8.tan(), 2.0, -0.4, -1.0, -1.7] {
            let arc = bulge_to_arc(a, b, beta).unwrap();
            let back = arc_to_bulge(a, b, &arc);
            assert!(close(back, beta), "round-trip β={beta} -> {back}");
        }
    }

    #[test]
    fn cuerda_degenerada_es_error() {
        let a = Point2::new(1.0, 1.0);
        assert_eq!(bulge_to_arc(a, a, 0.5), Err(BulgeError::DegenerateChord));
    }

    #[test]
    fn bulge_cero_es_tramo_recto() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(10.0, 0.0);
        assert_eq!(bulge_to_arc(a, b, 0.0), Err(BulgeError::StraightSegment));
    }

    #[test]
    fn bulge_no_finito_es_error() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(10.0, 0.0);
        assert_eq!(bulge_to_arc(a, b, f64::NAN), Err(BulgeError::NonFinite));
        assert_eq!(
            bulge_to_arc(Point2::new(f64::INFINITY, 0.0), b, 0.5),
            Err(BulgeError::NonFinite)
        );
    }

    #[test]
    fn bbox_semicirculo_incluye_el_vertice_no_la_cuerda() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, 1.0).unwrap();
        let bb = arc.bbox();
        assert!(close(bb.min.x, 0.0) && close(bb.max.x, 2.0));
        assert!(close(bb.min.y, -1.0) && close(bb.max.y, 0.0));
        assert!(bb.height() > 0.0, "la bbox del arco no es la de la cuerda");
    }

    #[test]
    fn bbox_arco_menor_no_incluye_cuadrantes_externos() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, FRAC_PI_8.tan()).unwrap();
        let bb = arc.bbox();
        assert!(close(bb.min.y, 1.0 - 2.0_f64.sqrt()));
        assert!(close(bb.max.y, 0.0));
    }

    #[test]
    fn distancia_punto_arco() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let arc = bulge_to_arc(a, b, 1.0).unwrap();
        assert!(arc.distance_to(Point2::new(1.0, -1.0)) < 1e-9);
        assert!(close(arc.distance_to(arc.center), 1.0));
        assert!(close(arc.distance_to(Point2::new(1.0, -0.75)), 0.25));
        let d = arc.distance_to(Point2::new(3.0, 0.0));
        assert!(close(d, 1.0));
    }

    #[test]
    fn barrido_siempre_en_rango() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(7.0, -3.0);
        for &beta in &[0.01_f64, 0.9, 1.0, 3.0, -0.3, -2.5] {
            let arc = bulge_to_arc(a, b, beta).unwrap();
            let s = arc.sweep();
            assert!(s > 0.0 && s < TAU, "barrido fuera de rango: {s}");
        }
    }

    #[test]
    fn seg_point_at_recto_es_interpolacion_lineal() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(10.0, 4.0);
        assert!(close_pt(seg_point_at(a, b, 0.0, 0.0), a));
        assert!(close_pt(seg_point_at(a, b, 0.0, 1.0), b));
        assert!(close_pt(
            seg_point_at(a, b, 0.0, 0.25),
            Point2::new(2.5, 1.0)
        ));
    }

    #[test]
    fn seg_point_at_arco_respeta_extremos_y_sentido() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        assert!(close_pt(seg_point_at(a, b, 1.0, 0.0), a));
        assert!(close_pt(seg_point_at(a, b, 1.0, 1.0), b));
        assert!(close_pt(
            seg_point_at(a, b, 1.0, 0.5),
            Point2::new(1.0, -1.0)
        ));
        assert!(close_pt(
            seg_point_at(a, b, -1.0, 0.5),
            Point2::new(1.0, 1.0)
        ));
        assert!(close_pt(seg_point_at(a, b, -1.0, 0.0), a));
        assert!(close_pt(seg_point_at(a, b, -1.0, 1.0), b));
    }

    #[test]
    fn seg_angle_fraction_inversa_de_seg_point_at() {
        let a = Point2::new(-1.0, 2.0);
        let b = Point2::new(3.0, 1.0);
        for &beta in &[0.4_f64, 1.0, 1.8, -0.6, -1.3] {
            let arc = bulge_to_arc(a, b, beta).unwrap();
            for &f in &[0.0_f64, 0.2, 0.5, 0.75, 1.0] {
                let theta = angle_of(seg_point_at(a, b, beta, f) - arc.center);
                let back = seg_angle_fraction(a, b, beta, theta).unwrap();
                assert!(close(back, f), "β={beta} f={f} -> {back}");
            }
        }
        assert_eq!(seg_angle_fraction(a, b, 0.0, 0.0), None);
    }

    #[test]
    fn split_bulge_segment_recto() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(8.0, 0.0);
        let (p0, p1, sub) = split_bulge_segment(a, b, 0.0, 0.25, 0.75);
        assert!(close_pt(p0, Point2::new(2.0, 0.0)));
        assert!(close_pt(p1, Point2::new(6.0, 0.0)));
        assert_eq!(sub, 0.0);
    }

    #[test]
    fn split_bulge_segment_arco_mitad_conserva_signo_y_circulo() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let full = bulge_to_arc(a, b, 1.0).unwrap();
        let (p0, p1, sub) = split_bulge_segment(a, b, 1.0, 0.0, 0.5);
        assert!(close_pt(p0, a));
        assert!(close_pt(p1, Point2::new(1.0, -1.0)));
        assert!(close(sub, (PI / 8.0).tan()));
        let sub_arc = bulge_to_arc(p0, p1, sub).unwrap();
        assert!(close_pt(sub_arc.center, full.center));
        assert!(close(sub_arc.radius, full.radius));
    }

    #[test]
    fn split_bulge_segment_arco_negativo_conserva_signo() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        let (_p0, _p1, sub) = split_bulge_segment(a, b, -1.0, 0.25, 0.75);
        assert!(sub < 0.0);
        assert!(close(sub, -(PI / 8.0).tan()));
    }

    #[test]
    fn split_bulge_segment_propiedad_puntos_sobre_el_circulo() {
        let a = Point2::new(-2.0, 1.0);
        let b = Point2::new(4.0, -1.0);
        for &beta in &[0.5_f64, 1.2, -0.9, -1.6] {
            let arc = bulge_to_arc(a, b, beta).unwrap();
            for &(f0, f1) in &[(0.1_f64, 0.4_f64), (0.0, 0.6), (0.3, 1.0)] {
                let (p0, p1, sub) = split_bulge_segment(a, b, beta, f0, f1);
                assert!(close(p0.dist(arc.center), arc.radius));
                assert!(close(p1.dist(arc.center), arc.radius));
                let expected = ((f1 - f0) * arc.sweep() * 0.25).tan().abs();
                assert!(close(sub.abs(), expected));
                assert_eq!(sub.is_sign_negative(), beta.is_sign_negative());
            }
        }
    }
}
