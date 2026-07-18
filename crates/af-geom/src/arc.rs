//! Bounding box for a circular arc with counterclockwise sweep.
//!
//! Endpoints alone are insufficient: an arc from 170 to 190 degrees reaches its
//! leftmost point at 180 degrees. Correct bounds include every cardinal angle
//! inside the counterclockwise sweep.

use core::f64::consts::{FRAC_PI_2, PI};

use af_math::angle::angle_in_sweep;
use af_math::{BBox, Point2};

/// Four cardinal angles in radians.
///
/// These angles produce the circle's axis-aligned extrema.
const QUADRANTS: [f64; 4] = [0.0, FRAC_PI_2, PI, PI + FRAC_PI_2];

/// Returns bounds for a circle arc swept counterclockwise from `start_angle` to
/// `end_angle` in radians.
///
/// Bounds include endpoints and cardinal points inside the sweep. A near-zero
/// sweep follows [`af_math::angle::sweep_ccw`] and represents a full circle.
///
/// Entity validation remains separate, so invalid radii or angles propagate.
#[must_use]
pub fn arc_bbox(center: Point2, r: f64, start_angle: f64, end_angle: f64) -> BBox {
    let mut bb = BBox::new(
        point_on_arc(center, r, start_angle),
        point_on_arc(center, r, end_angle),
    );
    for &q in &QUADRANTS {
        if angle_in_sweep(q, start_angle, end_angle) {
            bb = bb.union_point(point_on_arc(center, r, q));
        }
    }
    bb
}

/// Point on the arc at `angle` counterclockwise from positive X.
#[inline]
fn point_on_arc(center: Point2, r: f64, angle: f64) -> Point2 {
    let (s, c) = angle.sin_cos();
    Point2::new(center.x + r * c, center.y + r * s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::angle::sweep_ccw;

    /// Combined absolute and relative slack covers trigonometric rounding.
    const EPS: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= EPS * (1.0 + a.abs().max(b.abs()))
    }

    /// Compares bounds component by component with expected extrema.
    fn assert_box(bb: BBox, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        assert!(close(bb.min.x, min_x), "min.x: {} != {min_x}", bb.min.x);
        assert!(close(bb.min.y, min_y), "min.y: {} != {min_y}", bb.min.y);
        assert!(close(bb.max.x, max_x), "max.x: {} != {max_x}", bb.max.x);
        assert!(close(bb.max.y, max_y), "max.y: {} != {max_y}", bb.max.y);
    }

    fn at(center: Point2, r: f64, deg: f64) -> Point2 {
        point_on_arc(center, r, deg.to_radians())
    }

    fn bb(cx: f64, cy: f64, r: f64, sd: f64, ed: f64) -> BBox {
        arc_bbox(Point2::new(cx, cy), r, sd.to_radians(), ed.to_radians())
    }

    // ===== Exact arc table =====

    #[test]
    fn cuarto_ne_no_alcanza_los_ejes_negativos() {
        // 0 to 90 degrees excludes negative X and Y extrema.
        assert_box(bb(0.0, 0.0, 1.0, 0.0, 90.0), 0.0, 0.0, 1.0, 1.0);
    }

    #[test]
    fn semicirculo_superior_incluye_y_mas_r() {
        // 0 to 180 degrees includes top and left extrema.
        assert_box(bb(0.0, 0.0, 1.0, 0.0, 180.0), -1.0, 0.0, 1.0, 1.0);
    }

    #[test]
    fn semicirculo_izquierdo_incluye_x_menos_r() {
        // 90 to 270 degrees includes top, left, and bottom extrema.
        assert_box(bb(0.0, 0.0, 1.0, 90.0, 270.0), -1.0, -1.0, 0.0, 1.0);
    }

    #[test]
    fn arco_170_190_incluye_x_menos_r() {
        // Canonical case: 180 degrees is leftmost but is not an endpoint.
        let got = bb(0.0, 0.0, 1.0, 170.0, 190.0);
        assert!(close(got.min.x, -1.0), "no alcanzó x=−r: {}", got.min.x);
        assert_box(
            got,
            -1.0,
            at(Point2::ORIGIN, 1.0, 190.0).y,
            at(Point2::ORIGIN, 1.0, 170.0).x,
            at(Point2::ORIGIN, 1.0, 170.0).y,
        );
    }

    #[test]
    fn arco_350_10_cruza_cero_incluye_x_mas_r() {
        // A sweep crossing zero includes the positive-X extreme.
        let got = bb(0.0, 0.0, 1.0, 350.0, 10.0);
        assert!(close(got.max.x, 1.0), "no alcanzó x=+r: {}", got.max.x);
        assert_box(
            got,
            at(Point2::ORIGIN, 1.0, 350.0).x,
            at(Point2::ORIGIN, 1.0, 350.0).y,
            1.0,
            at(Point2::ORIGIN, 1.0, 10.0).y,
        );
    }

    #[test]
    fn centro_desplazado_semicirculo_izquierdo() {
        // An offset-center 90-to-270 sweep occupies the circle's left half.
        assert_box(bb(10.0, 20.0, 5.0, 90.0, 270.0), 5.0, 15.0, 10.0, 25.0);
    }

    #[test]
    fn tres_cuartos_llena_la_caja_del_circulo() {
        // 0 to 270 degrees touches all four extrema and yields full-circle bounds.
        assert_box(bb(0.0, 0.0, 1.0, 0.0, 270.0), -1.0, -1.0, 1.0, 1.0);
    }

    #[test]
    fn barrido_nulo_es_circulo_completo() {
        // Equal start and end represent a full circle.
        assert_box(bb(0.0, 0.0, 1.0, 0.0, 0.0), -1.0, -1.0, 1.0, 1.0);
    }

    #[test]
    fn cuna_superior_solo_incluye_cuadrante_90() {
        // A 45-to-135 sweep includes the top but not the bottom extreme.
        let c = Point2::new(2.0, -3.0);
        assert_box(
            bb(2.0, -3.0, 4.0, 45.0, 135.0),
            at(c, 4.0, 135.0).x,
            at(c, 4.0, 45.0).y,
            at(c, 4.0, 45.0).x,
            1.0,
        );
    }

    // ===== Property: bounds contain the sampled arc and stay within the circle =====

    #[test]
    fn bbox_contiene_el_arco_muestreado_y_esta_en_el_circulo() {
        let cases = [
            (Point2::new(0.0, 0.0), 1.0, 0.0_f64, 90.0_f64),
            (Point2::new(0.0, 0.0), 1.0, 170.0, 190.0),
            (Point2::new(0.0, 0.0), 1.0, 350.0, 10.0),
            (Point2::new(-5.0, 7.0), 3.5, 200.0, 40.0),
            (Point2::new(2.0, -3.0), 4.0, 45.0, 135.0),
            (Point2::new(0.0, 0.0), 2.0, 0.0, 359.0),
        ];
        for (center, r, sd, ed) in cases {
            let (s, e) = (sd.to_radians(), ed.to_radians());
            let arc = arc_bbox(center, r, s, e);
            let sweep = sweep_ccw(s, e);
            let n = 400;
            for i in 0..=n {
                let a = s + sweep * f64::from(i) / f64::from(n);
                let p = point_on_arc(center, r, a);
                assert!(
                    arc.expand(1e-9).contains_point(p),
                    "punto del arco {p:?} fuera de la caja {arc:?}"
                );
            }
            // Arc bounds stay within full-circle bounds.
            let full = arc_bbox(center, r, 0.0, 0.0);
            assert!(
                full.expand(1e-9).contains_bbox(arc),
                "arco {arc:?} no cabe en el círculo {full:?}"
            );
        }
    }
}
