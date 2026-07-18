//! Parametric ellipses and elliptical arcs using DXF conventions.

use core::f64::consts::PI;

use af_math::angle::{angle_in_sweep, sweep_ccw};
use af_math::{BBox, Point2, Vec2};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pub center: Point2,
    pub semi_major: f64,
    pub semi_minor: f64,
    pub rotation: f64,
    pub start_param: f64,
    pub end_param: f64,
}

impl Ellipse {
    #[inline]
    #[must_use]
    pub fn new(
        center: Point2,
        semi_major: f64,
        semi_minor: f64,
        rotation: f64,
        start_param: f64,
        end_param: f64,
    ) -> Self {
        Self {
            center,
            semi_major,
            semi_minor,
            rotation,
            start_param,
            end_param,
        }
    }

    #[inline]
    #[must_use]
    pub fn major_axis(&self) -> Vec2 {
        let (s, c) = self.rotation.sin_cos();
        Vec2::new(self.semi_major * c, self.semi_major * s)
    }

    #[inline]
    #[must_use]
    pub fn minor_axis(&self) -> Vec2 {
        let (s, c) = self.rotation.sin_cos();
        Vec2::new(-self.semi_minor * s, self.semi_minor * c)
    }

    #[inline]
    #[must_use]
    pub fn point_at(&self, t: f64) -> Point2 {
        let (st, ct) = t.sin_cos();
        self.center + self.major_axis() * ct + self.minor_axis() * st
    }

    #[inline]
    #[must_use]
    pub fn start_point(&self) -> Point2 {
        self.point_at(self.start_param)
    }

    #[inline]
    #[must_use]
    pub fn end_point(&self) -> Point2 {
        self.point_at(self.end_param)
    }

    #[inline]
    #[must_use]
    pub fn sweep(&self) -> f64 {
        sweep_ccw(self.start_param, self.end_param)
    }

    #[inline]
    #[must_use]
    pub fn midpoint(&self) -> Point2 {
        self.point_at(self.start_param + self.sweep() * 0.5)
    }

    fn aabb_extreme_params(&self) -> [f64; 4] {
        let (s, c) = self.rotation.sin_cos();
        let (a, b) = (self.semi_major, self.semi_minor);
        let tx = (-b * s).atan2(a * c);
        let ty = (b * c).atan2(a * s);
        [tx, tx + PI, ty, ty + PI]
    }

    #[must_use]
    pub fn bbox(&self) -> BBox {
        let mut bb = BBox::new(self.start_point(), self.end_point());
        for &t in &self.aabb_extreme_params() {
            if angle_in_sweep(t, self.start_param, self.end_param) {
                bb = bb.union_point(self.point_at(t));
            }
        }
        bb
    }

    #[must_use]
    pub fn distance_to(&self, p: Point2) -> f64 {
        let sweep = self.sweep();
        const N: usize = 48;
        let mut best_t = self.start_param;
        let mut best_d2 = f64::INFINITY;
        for i in 0..=N {
            let t = self.start_param + sweep * (i as f64) / (N as f64);
            let d2 = (self.point_at(t) - p).norm_sq();
            if d2 < best_d2 {
                best_d2 = d2;
                best_t = t;
            }
        }
        let (lo, hi) = (self.start_param, self.start_param + sweep);
        let mut t = best_t;
        for _ in 0..12 {
            let (st, ct) = t.sin_cos();
            let maj = self.major_axis();
            let min = self.minor_axis();
            let pt = self.center + maj * ct + min * st;
            let d1 = min * ct - maj * st;
            let d2v = -(maj * ct + min * st);
            let diff = pt - p;
            let f = diff.dot(d1);
            let fp = d1.norm_sq() + diff.dot(d2v);
            if fp.abs() < 1e-14 {
                break;
            }
            let next = (t - f / fp).clamp(lo, hi);
            if (next - t).abs() < 1e-15 {
                t = next;
                break;
            }
            t = next;
        }
        let refined = (self.point_at(t) - p).norm();
        refined.min(best_d2.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    fn axis_aligned() -> Ellipse {
        Ellipse::new(Point2::ORIGIN, 3.0, 1.0, 0.0, 0.0, TAU)
    }

    #[test]
    fn point_at_vertices_ejes() {
        let e = axis_aligned();
        assert!(close_pt(e.point_at(0.0), Point2::new(3.0, 0.0)));
        assert!(close_pt(e.point_at(FRAC_PI_2), Point2::new(0.0, 1.0)));
        assert!(close_pt(e.point_at(PI), Point2::new(-3.0, 0.0)));
    }

    #[test]
    fn bbox_alineada_es_center_mas_menos_semiejes() {
        let bb = axis_aligned().bbox();
        assert!(close(bb.min.x, -3.0) && close(bb.max.x, 3.0));
        assert!(close(bb.min.y, -1.0) && close(bb.max.y, 1.0));
    }

    #[test]
    fn bbox_rotada_90_intercambia_extension() {
        let e = Ellipse::new(Point2::ORIGIN, 3.0, 1.0, FRAC_PI_2, 0.0, TAU);
        let bb = e.bbox();
        assert!(close(bb.min.x, -1.0) && close(bb.max.x, 1.0));
        assert!(close(bb.min.y, -3.0) && close(bb.max.y, 3.0));
    }

    #[test]
    fn bbox_rotada_45_contiene_la_curva_muestreada() {
        let e = Ellipse::new(Point2::new(2.0, -1.0), 4.0, 1.5, FRAC_PI_4, 0.0, TAU);
        let bb = e.bbox();
        for i in 0..=720 {
            let t = TAU * f64::from(i) / 720.0;
            assert!(
                bb.expand(1e-9).contains_point(e.point_at(t)),
                "punto fuera de la bbox rotada"
            );
        }
    }

    #[test]
    fn bbox_arco_no_incluye_vertices_fuera_del_barrido() {
        let e = Ellipse::new(Point2::ORIGIN, 2.0, 1.0, 0.0, 0.0, FRAC_PI_2);
        let bb = e.bbox();
        assert!(close(bb.min.x, 0.0) && close(bb.max.x, 2.0));
        assert!(close(bb.min.y, 0.0) && close(bb.max.y, 1.0));
    }

    #[test]
    fn distance_sobre_la_curva_es_cero() {
        let e = Ellipse::new(Point2::new(1.0, 2.0), 5.0, 2.0, FRAC_PI_4, 0.3, 4.0);
        for i in 0..=20 {
            let t = e.start_param + e.sweep() * f64::from(i) / 20.0;
            let on = e.point_at(t);
            assert!(e.distance_to(on) < 1e-7, "distancia sobre la curva no ~0");
        }
    }

    #[test]
    fn distance_al_centro_es_semieje_menor() {
        let e = axis_aligned();
        assert!(close(e.distance_to(Point2::ORIGIN), 1.0));
    }

    #[test]
    fn distance_arco_fuera_del_barrido_gana_un_extremo() {
        let e = Ellipse::new(Point2::ORIGIN, 2.0, 1.0, 0.0, -FRAC_PI_2, FRAC_PI_2);
        let d = e.distance_to(Point2::new(-5.0, 0.0));
        let end_d = Point2::new(-5.0, 0.0).dist(Point2::new(0.0, 1.0));
        assert!(close(d, end_d), "esperado extremo {end_d}, dio {d}");
    }
}
