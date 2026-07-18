//! Pure 2D intersections with curve parameters for trim and extend operations.

use af_math::angle::angle_of;
use af_math::{Point2, Tol};

use crate::bulge::{ArcSeg, bulge_to_arc};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub point: Point2,
    pub t1: f64,
    pub t2: f64,
}

impl Hit {
    #[inline]
    #[must_use]
    fn swapped(self) -> Self {
        Self {
            point: self.point,
            t1: self.t2,
            t2: self.t1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineX {
    Point(Hit),
    Parallel,
    Collinear,
}

#[must_use]
pub fn line_line(a1: Point2, b1: Point2, a2: Point2, b2: Point2) -> LineX {
    let tol = Tol::default();
    let d1 = b1 - a1;
    let d2 = b2 - a2;
    let n1 = d1.norm();
    let n2 = d2.norm();
    if n1 <= tol.point_merge || n2 <= tol.point_merge {
        return LineX::Parallel;
    }
    let denom = d1.cross(d2);
    if denom.abs() <= tol.linear * n1 * n2 {
        let perp = (a2 - a1).cross(d1).abs() / n1;
        return if perp <= tol.point_merge {
            LineX::Collinear
        } else {
            LineX::Parallel
        };
    }
    let w = a2 - a1;
    let t1 = w.cross(d2) / denom;
    let t2 = w.cross(d1) / denom;
    LineX::Point(Hit {
        point: a1 + d1 * t1,
        t1,
        t2,
    })
}

#[must_use]
pub fn line_circle(a: Point2, b: Point2, center: Point2, radius: f64) -> Vec<Hit> {
    let tol = Tol::default();
    let d = b - a;
    let dlen = d.norm();
    if dlen <= tol.point_merge || !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    let dir = d / dlen;
    let f = center - a;
    let proj = f.dot(dir);
    let foot = a + dir * proj;
    let perp_dist = center.dist(foot);
    if perp_dist > radius + tol.point_merge {
        return Vec::new();
    }
    let half = (radius * radius - perp_dist * perp_dist).max(0.0).sqrt();
    if half <= tol.point_merge {
        return vec![hit_lc(foot, proj, dlen, center)];
    }
    [-half, half]
        .into_iter()
        .map(|s| {
            let len = proj + s;
            hit_lc(a + dir * len, len, dlen, center)
        })
        .collect()
}

#[inline]
fn hit_lc(p: Point2, len: f64, dlen: f64, center: Point2) -> Hit {
    Hit {
        point: p,
        t1: len / dlen,
        t2: angle_of(p - center),
    }
}

#[must_use]
pub fn circle_circle(c1: Point2, r1: f64, c2: Point2, r2: f64) -> Vec<Hit> {
    let tol = Tol::default();
    if !r1.is_finite() || !r2.is_finite() || r1 <= 0.0 || r2 <= 0.0 {
        return Vec::new();
    }
    let d = c2 - c1;
    let dist = d.norm();
    if dist <= tol.point_merge {
        return Vec::new();
    }
    if dist > r1 + r2 + tol.point_merge || dist < (r1 - r2).abs() - tol.point_merge {
        return Vec::new();
    }
    let dir = d / dist;
    let a = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let mid = c1 + dir * a;
    if h <= tol.point_merge {
        return vec![hit_cc(mid, c1, c2)];
    }
    let perp = dir.perp();
    [-h, h]
        .into_iter()
        .map(|s| hit_cc(mid + perp * s, c1, c2))
        .collect()
}

#[inline]
fn hit_cc(p: Point2, c1: Point2, c2: Point2) -> Hit {
    Hit {
        point: p,
        t1: angle_of(p - c1),
        t2: angle_of(p - c2),
    }
}

#[must_use]
pub fn line_arc(a: Point2, b: Point2, arc: &ArcSeg) -> Vec<Hit> {
    line_circle(a, b, arc.center, arc.radius)
}

#[must_use]
pub fn circle_arc(center: Point2, radius: f64, arc: &ArcSeg) -> Vec<Hit> {
    circle_circle(center, radius, arc.center, arc.radius)
}

#[must_use]
pub fn arc_arc(a1: &ArcSeg, a2: &ArcSeg) -> Vec<Hit> {
    circle_circle(a1.center, a1.radius, a2.center, a2.radius)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegGeom {
    Straight { a: Point2, b: Point2 },
    Arc(ArcSeg),
}

#[must_use]
pub fn resolve_poly_seg(a: Point2, b: Point2, bulge: f64) -> SegGeom {
    match bulge_to_arc(a, b, bulge) {
        Ok(arc) => SegGeom::Arc(arc),
        Err(_) => SegGeom::Straight { a, b },
    }
}

#[must_use]
pub fn seg_seg(s1: &SegGeom, s2: &SegGeom) -> Vec<Hit> {
    match (s1, s2) {
        (SegGeom::Straight { a: a1, b: b1 }, SegGeom::Straight { a: a2, b: b2 }) => {
            match line_line(*a1, *b1, *a2, *b2) {
                LineX::Point(h) => vec![h],
                LineX::Parallel | LineX::Collinear => Vec::new(),
            }
        }
        (SegGeom::Straight { a, b }, SegGeom::Arc(arc)) => line_arc(*a, *b, arc),
        (SegGeom::Arc(arc), SegGeom::Straight { a, b }) => line_arc(*a, *b, arc)
            .into_iter()
            .map(Hit::swapped)
            .collect(),
        (SegGeom::Arc(a1), SegGeom::Arc(a2)) => arc_arc(a1, a2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::angle::angle_in_sweep;
    use core::f64::consts::{FRAC_PI_2, PI};

    const TOL: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= TOL
    }

    fn close_pt(p: Point2, q: Point2) -> bool {
        close(p.x, q.x) && close(p.y, q.y)
    }

    #[test]
    fn line_line_cruce_dentro_de_ambos_segmentos() {
        let x = line_line(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 0.0),
        );
        match x {
            LineX::Point(h) => {
                assert!(close_pt(h.point, Point2::new(0.5, 0.5)));
                assert!(close(h.t1, 0.5) && close(h.t2, 0.5));
            }
            other => panic!("esperaba un cruce, got {other:?}"),
        }
    }

    #[test]
    fn line_line_cruce_en_la_prolongacion() {
        let x = line_line(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 1.0),
            Point2::new(2.0, 2.0),
        );
        match x {
            LineX::Point(h) => {
                assert!(close_pt(h.point, Point2::new(2.0, 0.0)));
                assert!(h.t1 > 1.0, "t1={} debería estar en la prolongación", h.t1);
                assert!(h.t2 < 0.0, "t2={} debería estar en la prolongación", h.t2);
            }
            other => panic!("esperaba un cruce, got {other:?}"),
        }
    }

    #[test]
    fn line_line_paralelas_distintas() {
        let x = line_line(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 1.0),
        );
        assert_eq!(x, LineX::Parallel);
    }

    #[test]
    fn line_line_colineales() {
        let x = line_line(
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(3.0, 0.0),
        );
        assert_eq!(x, LineX::Collinear);
    }

    #[test]
    fn line_line_degenerada_es_paralela() {
        let x = line_line(
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(1.0, 1.0),
        );
        assert_eq!(x, LineX::Parallel);
    }

    #[test]
    fn line_circle_secante_dos_puntos() {
        let hits = line_circle(
            Point2::new(-2.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::ORIGIN,
            1.0,
        );
        assert_eq!(hits.len(), 2);
        assert!(close_pt(hits[0].point, Point2::new(-1.0, 0.0)));
        assert!(close_pt(hits[1].point, Point2::new(1.0, 0.0)));
        assert!(hits[0].t1 < hits[1].t1);
        assert!(close(hits[0].t2, PI) && close(hits[1].t2, 0.0));
    }

    #[test]
    fn line_circle_tangente_un_punto() {
        let hits = line_circle(
            Point2::new(-2.0, 1.0),
            Point2::new(2.0, 1.0),
            Point2::ORIGIN,
            1.0,
        );
        assert_eq!(hits.len(), 1);
        assert!(close_pt(hits[0].point, Point2::new(0.0, 1.0)));
        assert!(close(hits[0].t2, FRAC_PI_2));
    }

    #[test]
    fn line_circle_sin_corte() {
        let hits = line_circle(
            Point2::new(-2.0, 2.0),
            Point2::new(2.0, 2.0),
            Point2::ORIGIN,
            1.0,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn circle_circle_secante_dos_puntos() {
        let hits = circle_circle(Point2::ORIGIN, 1.0, Point2::new(1.0, 0.0), 1.0);
        assert_eq!(hits.len(), 2);
        let s3 = 3.0_f64.sqrt() / 2.0;
        assert!(close_pt(hits[0].point, Point2::new(0.5, -s3)));
        assert!(close_pt(hits[1].point, Point2::new(0.5, s3)));
    }

    #[test]
    fn circle_circle_tangente_externa() {
        let hits = circle_circle(Point2::ORIGIN, 1.0, Point2::new(2.0, 0.0), 1.0);
        assert_eq!(hits.len(), 1);
        assert!(close_pt(hits[0].point, Point2::new(1.0, 0.0)));
    }

    #[test]
    fn circle_circle_tangente_interna() {
        let hits = circle_circle(Point2::ORIGIN, 2.0, Point2::new(1.0, 0.0), 1.0);
        assert_eq!(hits.len(), 1);
        assert!(close_pt(hits[0].point, Point2::new(2.0, 0.0)));
    }

    #[test]
    fn circle_circle_disjuntos_y_concentricos_vacios() {
        assert!(circle_circle(Point2::ORIGIN, 1.0, Point2::new(5.0, 0.0), 1.0).is_empty());
        assert!(circle_circle(Point2::ORIGIN, 1.0, Point2::ORIGIN, 1.0).is_empty());
        assert!(circle_circle(Point2::ORIGIN, 3.0, Point2::new(0.5, 0.0), 1.0).is_empty());
    }

    #[test]
    fn line_arc_devuelve_corte_dentro_y_fuera_del_barrido() {
        let arc = bulge_to_arc(Point2::new(1.0, 0.0), Point2::new(-1.0, 0.0), 1.0).unwrap();
        assert!(close(arc.radius, 1.0));
        let hits = line_arc(Point2::new(0.0, -2.0), Point2::new(0.0, 2.0), &arc);
        assert_eq!(hits.len(), 2);
        let inside: Vec<_> = hits
            .iter()
            .filter(|h| angle_in_sweep(h.t2, arc.start_angle, arc.end_angle))
            .collect();
        assert_eq!(inside.len(), 1);
        assert!(close_pt(inside[0].point, Point2::new(0.0, 1.0)));
    }

    #[test]
    fn arc_arc_reusa_circulos_soporte() {
        let a1 = bulge_to_arc(Point2::new(1.0, 0.0), Point2::new(-1.0, 0.0), 1.0).unwrap();
        let a2 = bulge_to_arc(Point2::new(2.0, 0.0), Point2::new(0.0, 0.0), 1.0).unwrap();
        let hits = arc_arc(&a1, &a2);
        assert_eq!(hits.len(), 2);
        for h in &hits {
            assert!(close(h.point.dist(a1.center), a1.radius));
            assert!(close(h.point.dist(a2.center), a2.radius));
        }
    }

    #[test]
    fn seg_seg_recto_recto() {
        let s1 = resolve_poly_seg(Point2::new(0.0, 0.0), Point2::new(2.0, 2.0), 0.0);
        let s2 = resolve_poly_seg(Point2::new(0.0, 2.0), Point2::new(2.0, 0.0), 0.0);
        assert!(matches!(s1, SegGeom::Straight { .. }));
        let hits = seg_seg(&s1, &s2);
        assert_eq!(hits.len(), 1);
        assert!(close_pt(hits[0].point, Point2::new(1.0, 1.0)));
    }

    #[test]
    fn seg_seg_recto_arco_intercambia_parametros() {
        let recto = resolve_poly_seg(Point2::new(0.0, -2.0), Point2::new(0.0, 2.0), 0.0);
        let arco = resolve_poly_seg(Point2::new(1.0, 0.0), Point2::new(-1.0, 0.0), 1.0);
        let ra = seg_seg(&recto, &arco);
        let ar = seg_seg(&arco, &recto);
        assert_eq!(ra.len(), ar.len());
        for (x, y) in ra.iter().zip(ar.iter()) {
            assert!(close_pt(x.point, y.point));
            assert!(close(x.t1, y.t2) && close(x.t2, y.t1));
        }
    }

    #[test]
    fn propiedad_line_circle_puntos_sobre_ambas_curvas() {
        let lines = [
            (Point2::new(-3.0, 0.5), Point2::new(4.0, 0.5)),
            (Point2::new(-2.0, -1.0), Point2::new(3.0, 2.0)),
            (Point2::new(0.0, -5.0), Point2::new(0.1, 5.0)),
        ];
        let circles = [
            (Point2::ORIGIN, 1.0),
            (Point2::new(1.0, -0.5), 2.0),
            (Point2::new(-2.0, 3.0), 1.5),
        ];
        for (a, b) in lines {
            for (c, r) in circles {
                for h in line_circle(a, b, c, r) {
                    let on_line = a + (b - a) * h.t1;
                    assert!(close_pt(on_line, h.point), "t1 no reproduce el punto");
                    assert!(close(h.point.dist(c), r), "no está en el círculo");
                    let (s, cs) = h.t2.sin_cos();
                    assert!(close_pt(Point2::new(c.x + r * cs, c.y + r * s), h.point));
                }
            }
        }
    }

    #[test]
    fn propiedad_circle_circle_puntos_sobre_ambas_curvas() {
        let cases = [
            (Point2::ORIGIN, 1.0, Point2::new(1.0, 0.0), 1.0),
            (Point2::ORIGIN, 2.0, Point2::new(3.0, 0.0), 2.0),
            (Point2::new(-1.0, 1.0), 1.5, Point2::new(1.0, -0.5), 2.0),
        ];
        for (c1, r1, c2, r2) in cases {
            for h in circle_circle(c1, r1, c2, r2) {
                assert!(close(h.point.dist(c1), r1), "no está en el círculo 1");
                assert!(close(h.point.dist(c2), r2), "no está en el círculo 2");
                assert!(close(h.t1, angle_of(h.point - c1)));
                assert!(close(h.t2, angle_of(h.point - c2)));
            }
        }
    }
}
