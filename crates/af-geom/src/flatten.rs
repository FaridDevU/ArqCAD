//! Chord-error flattening for arcs, circles, and ellipses.

use core::f64::consts::TAU;

use af_math::Point2;

use crate::bulge::ArcSeg;
use crate::ellipse::Ellipse;

// ponytail: Cap segments to bound memory at extreme chord-error ratios.
const MAX_SEGMENTS: usize = 100_000;

#[must_use]
pub fn arc_segment_count(radius: f64, sweep: f64, chord_err: f64) -> usize {
    let usable = radius.is_finite()
        && radius > 0.0
        && chord_err.is_finite()
        && chord_err > 0.0
        && sweep.is_finite()
        && sweep > 0.0;
    if !usable {
        return 1;
    }
    let ratio = 1.0 - chord_err / radius;
    if ratio <= -1.0 {
        return 1;
    }
    let theta_max = 2.0 * ratio.acos();
    let n = (sweep / theta_max).ceil();
    if n.is_finite() && n >= 1.0 {
        (n as usize).min(MAX_SEGMENTS)
    } else {
        MAX_SEGMENTS
    }
}

#[must_use]
pub fn flatten_arc(arc: &ArcSeg, chord_err: f64) -> Vec<Point2> {
    let sweep = arc.sweep();
    let n = arc_segment_count(arc.radius, sweep, chord_err);
    let mut pts = Vec::with_capacity(n + 1);
    pts.push(arc.start_point());
    for i in 1..n {
        let a = arc.start_angle + sweep * (i as f64) / (n as f64);
        pts.push(arc.point_at(a));
    }
    pts.push(arc.end_point());
    pts
}

#[must_use]
pub fn flatten_circle(center: Point2, radius: f64, chord_err: f64) -> Vec<Point2> {
    let n = arc_segment_count(radius, TAU, chord_err).max(3);
    let mut pts = Vec::with_capacity(n + 1);
    let first = Point2::new(center.x + radius, center.y);
    pts.push(first);
    for i in 1..n {
        let a = TAU * (i as f64) / (n as f64);
        let (s, c) = a.sin_cos();
        pts.push(Point2::new(center.x + radius * c, center.y + radius * s));
    }
    pts.push(first);
    pts
}

#[must_use]
pub fn ellipse_segment_count(semi_major: f64, sweep: f64, chord_err: f64) -> usize {
    let usable = semi_major.is_finite()
        && semi_major > 0.0
        && chord_err.is_finite()
        && chord_err > 0.0
        && sweep.is_finite()
        && sweep > 0.0;
    if !usable {
        return 1;
    }
    let dt_max = (8.0 * chord_err / semi_major).sqrt();
    if !dt_max.is_finite() || dt_max <= 0.0 {
        return MAX_SEGMENTS;
    }
    let n = (sweep / dt_max).ceil();
    if n.is_finite() && n >= 1.0 {
        (n as usize).min(MAX_SEGMENTS)
    } else {
        MAX_SEGMENTS
    }
}

#[must_use]
pub fn flatten_ellipse(e: &Ellipse, chord_err: f64) -> Vec<Point2> {
    let sweep = e.sweep();
    let semi_major = e.semi_major.abs().max(e.semi_minor.abs());
    let n = ellipse_segment_count(semi_major, sweep, chord_err);
    let mut pts = Vec::with_capacity(n + 1);
    pts.push(e.start_point());
    for i in 1..n {
        let t = e.start_param + sweep * (i as f64) / (n as f64);
        pts.push(e.point_at(t));
    }
    pts.push(e.end_point());
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bulge::bulge_to_arc;
    use core::f64::consts::PI;

    fn sagitta(r: f64, theta: f64) -> f64 {
        r * (1.0 - (theta * 0.5).cos())
    }

    #[test]
    fn count_degenerados_devuelven_uno() {
        assert_eq!(arc_segment_count(0.0, PI, 0.1), 1);
        assert_eq!(arc_segment_count(-1.0, PI, 0.1), 1);
        assert_eq!(arc_segment_count(1.0, PI, 0.0), 1);
        assert_eq!(arc_segment_count(1.0, PI, -0.1), 1);
        assert_eq!(arc_segment_count(1.0, f64::NAN, 0.1), 1);
        assert_eq!(arc_segment_count(1.0, 0.0, 0.1), 1);
    }

    #[test]
    fn count_chord_err_gigante_es_un_segmento() {
        assert_eq!(arc_segment_count(1.0, PI, 2.0), 1);
        assert_eq!(arc_segment_count(1.0, PI, 5.0), 1);
    }

    #[test]
    fn count_monotono_en_chord_err() {
        let radius = 10.0;
        let sweep = PI;
        let mut prev = 0;
        for k in 1..=8 {
            let chord_err = 10.0 / f64::from(1 << k);
            let n = arc_segment_count(radius, sweep, chord_err);
            assert!(n >= prev, "n={n} < prev={prev} para chord_err={chord_err}");
            prev = n;
        }
    }

    #[test]
    fn count_respeta_la_cota() {
        let cases = [
            (1.0, PI, 0.1),
            (10.0, PI * 1.5, 0.05),
            (0.5, TAU, 0.01),
            (100.0, 0.3, 0.001),
        ];
        for (r, sweep, chord_err) in cases {
            let n = arc_segment_count(r, sweep, chord_err);
            let theta = sweep / (n as f64);
            let e = sagitta(r, theta);
            assert!(
                e <= chord_err + 1e-12,
                "sagitta {e} > chord_err {chord_err} (r={r}, sweep={sweep}, n={n})"
            );
        }
    }

    #[test]
    fn flatten_arc_extremos_exactos() {
        let arc = bulge_to_arc(Point2::new(0.0, 0.0), Point2::new(2.0, 0.0), 1.0).unwrap();
        let pts = flatten_arc(&arc, 0.01);
        assert!(pts.len() >= 2);
        assert_eq!(pts[0], arc.start_point());
        assert_eq!(*pts.last().unwrap(), arc.end_point());
    }

    #[test]
    fn flatten_arc_puntos_sobre_la_curva_y_dentro_de_la_cota() {
        let arc = bulge_to_arc(Point2::new(-3.0, 1.0), Point2::new(4.0, 2.5), 0.7).unwrap();
        let chord_err = 0.02;
        let pts = flatten_arc(&arc, chord_err);
        for p in &pts {
            let d = p.dist(arc.center);
            assert!(
                (d - arc.radius).abs() < 1e-9,
                "punto fuera del círculo: {d}"
            );
        }
        for w in pts.windows(2) {
            let mid_chord = w[0].midpoint(w[1]);
            let err = (arc.radius - mid_chord.dist(arc.center)).abs();
            assert!(err <= chord_err + 1e-9, "error cordal {err} > {chord_err}");
        }
    }

    #[test]
    fn flatten_circle_cerrado_y_minimo_tres_segmentos() {
        let pts = flatten_circle(Point2::new(2.0, -1.0), 3.0, 100.0);
        assert_eq!(pts.len(), 4, "3 segmentos ⇒ 4 puntos");
        assert_eq!(*pts.last().unwrap(), pts[0]);
        assert_eq!(pts[0], Point2::new(5.0, -1.0));
    }

    #[test]
    fn flatten_ellipse_extremos_exactos_y_dentro_de_la_cota() {
        use crate::ellipse::Ellipse;
        use core::f64::consts::{FRAC_PI_4, TAU};
        let e = Ellipse::new(Point2::new(1.0, -2.0), 4.0, 1.5, FRAC_PI_4, 0.0, TAU);
        let chord_err = 0.01;
        let pts = flatten_ellipse(&e, chord_err);
        assert_eq!(pts[0], e.start_point());
        assert_eq!(*pts.last().unwrap(), e.end_point());
        let n = pts.len() - 1;
        let sweep = e.sweep();
        for i in 0..n {
            let t_mid = e.start_param + sweep * (i as f64 + 0.5) / (n as f64);
            let curve_mid = e.point_at(t_mid);
            let chord_mid = pts[i].midpoint(pts[i + 1]);
            assert!(
                chord_mid.dist(curve_mid) <= chord_err + 1e-9,
                "error cordal excede la cota"
            );
        }
    }

    #[test]
    fn flatten_circle_respeta_la_cota() {
        let center = Point2::new(0.0, 0.0);
        let radius = 5.0;
        let chord_err = 0.01;
        let pts = flatten_circle(center, radius, chord_err);
        for p in &pts {
            assert!((p.dist(center) - radius).abs() < 1e-9);
        }
        for w in pts.windows(2) {
            let err = (radius - w[0].midpoint(w[1]).dist(center)).abs();
            assert!(err <= chord_err + 1e-9, "error cordal {err} > {chord_err}");
        }
    }
}
