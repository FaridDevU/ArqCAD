//! Pure 2D projection helpers for calculated snaps.

use af_math::angle::{angle_in_sweep, angle_of};
use af_math::{Point2, Tol, Vec2};

use crate::bulge::ArcSeg;

#[must_use]
pub fn perp_foot_line(p: Point2, a: Point2, b: Point2) -> Option<(Point2, f64)> {
    let ab: Vec2 = b - a;
    let len_sq = ab.norm_sq();
    if len_sq <= Tol::default().point_merge {
        return None;
    }
    let t = (p - a).dot(ab) / len_sq;
    Some((a + ab * t, t))
}

#[must_use]
pub fn nearest_on_segment(p: Point2, a: Point2, b: Point2) -> Point2 {
    match perp_foot_line(p, a, b) {
        Some((_, t)) => a + (b - a) * t.clamp(0.0, 1.0),
        None => a,
    }
}

#[must_use]
pub fn project_on_circle(p: Point2, center: Point2, radius: f64) -> Option<Point2> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let v = p - center;
    let d = v.norm();
    if d <= Tol::default().point_merge {
        return None;
    }
    Some(center + v * (radius / d))
}

#[must_use]
pub fn nearest_on_arc(p: Point2, arc: &ArcSeg) -> Point2 {
    let v = p - arc.center;
    if angle_in_sweep(angle_of(v), arc.start_angle, arc.end_angle) {
        project_on_circle(p, arc.center, arc.radius).unwrap_or_else(|| arc.start_point())
    } else {
        let s = arc.start_point();
        let e = arc.end_point();
        if p.dist_sq(s) <= p.dist_sq(e) { s } else { e }
    }
}

#[must_use]
pub fn tangent_points(p: Point2, center: Point2, radius: f64) -> Vec<Point2> {
    let tol = Tol::default();
    if !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    let v = p - center;
    let d = v.norm();
    if d < radius - tol.point_merge {
        return Vec::new();
    }
    let u = v / d;
    if d <= radius + tol.point_merge {
        return vec![center + u * radius];
    }
    let cos_a = (radius / d).clamp(-1.0, 1.0);
    let sin_a = (1.0 - cos_a * cos_a).max(0.0).sqrt();
    let rot = |s: f64| {
        Point2::new(
            center.x + radius * (cos_a * u.x - s * u.y),
            center.y + radius * (s * u.x + cos_a * u.y),
        )
    };
    vec![rot(sin_a), rot(-sin_a)]
}

#[must_use]
// ponytail: Uses straight vertices; include bulge arc area if that precision becomes necessary.
pub fn polygon_centroid(verts: &[Point2]) -> Option<Point2> {
    if verts.len() < 3 {
        return None;
    }
    let mut area2 = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..verts.len() {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let cross = a.x * b.y - b.x * a.y;
        area2 += cross;
        cx += (a.x + b.x) * cross;
        cy += (a.y + b.y) * cross;
    }
    if area2.abs() <= Tol::default().point_merge {
        let n = verts.len() as f64;
        let (sx, sy) = verts
            .iter()
            .fold((0.0, 0.0), |(sx, sy), v| (sx + v.x, sy + v.y));
        return Some(Point2::new(sx / n, sy / n));
    }
    Some(Point2::new(cx / (3.0 * area2), cy / (3.0 * area2)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bulge::bulge_to_arc;
    use core::f64::consts::FRAC_PI_2;

    const TOL: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= TOL
    }
    fn close_pt(p: Point2, q: Point2) -> bool {
        close(p.x, q.x) && close(p.y, q.y)
    }

    #[test]
    fn perp_foot_cae_en_el_pie_con_producto_punto_nulo() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(4.0, 0.0);
        let (foot, t) = perp_foot_line(Point2::new(1.0, 3.0), a, b).unwrap();
        assert!(close_pt(foot, Point2::new(1.0, 0.0)));
        assert!(close(t, 0.25));
        let dir = b - a;
        assert!(close((Point2::new(1.0, 3.0) - foot).dot(dir), 0.0));
    }

    #[test]
    fn perp_foot_en_la_prolongacion_da_t_fuera_de_0_1() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(1.0, 0.0);
        let (_, t) = perp_foot_line(Point2::new(3.0, 2.0), a, b).unwrap();
        assert!(t > 1.0, "t={t} debería caer en la prolongación");
        assert!(perp_foot_line(a, a, a).is_none());
    }

    #[test]
    fn nearest_on_segment_clampa_a_extremos() {
        let a = Point2::new(0.0, 0.0);
        let b = Point2::new(2.0, 0.0);
        assert!(close_pt(
            nearest_on_segment(Point2::new(1.0, 5.0), a, b),
            Point2::new(1.0, 0.0)
        ));
        assert!(close_pt(
            nearest_on_segment(Point2::new(-3.0, 1.0), a, b),
            a
        ));
    }

    #[test]
    fn nearest_on_circle_yace_en_el_anillo() {
        let c = Point2::new(0.0, 0.0);
        let n = project_on_circle(Point2::new(4.0, 0.0), c, 1.0).unwrap();
        assert!(close_pt(n, Point2::new(1.0, 0.0)));
        assert!(close(n.dist(c), 1.0));
        assert!(project_on_circle(c, c, 1.0).is_none());
    }

    #[test]
    fn nearest_on_arc_dentro_y_fuera_del_barrido() {
        let arc = ArcSeg {
            center: Point2::ORIGIN,
            radius: 1.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        };
        let n = nearest_on_arc(Point2::new(2.0, 2.0), &arc);
        assert!(close(n.dist(arc.center), 1.0));
        assert!(close_pt(n, Point2::new(0.5f64.sqrt(), 0.5f64.sqrt())));
        let e = nearest_on_arc(Point2::new(2.0, -5.0), &arc);
        assert!(close_pt(e, Point2::new(1.0, 0.0)));
    }

    #[test]
    fn tangentes_desde_exterior_son_perpendiculares_al_radio() {
        let c = Point2::ORIGIN;
        let p = Point2::new(2.0, 0.0);
        let pts = tangent_points(p, c, 1.0);
        assert_eq!(pts.len(), 2);
        let s3 = 3.0f64.sqrt() / 2.0;
        for t in &pts {
            assert!(close(t.dist(c), 1.0));
            assert!(close((*t - c).dot(*t - p), 0.0));
        }
        assert!(pts.iter().any(|t| close_pt(*t, Point2::new(0.5, s3))));
        assert!(pts.iter().any(|t| close_pt(*t, Point2::new(0.5, -s3))));
    }

    #[test]
    fn tangente_interior_vacia_sobre_circulo_una() {
        let c = Point2::ORIGIN;
        assert!(tangent_points(Point2::new(0.2, 0.0), c, 1.0).is_empty());
        let on = tangent_points(Point2::new(1.0, 0.0), c, 1.0);
        assert_eq!(on.len(), 1);
        assert!(close_pt(on[0], Point2::new(1.0, 0.0)));
    }

    #[test]
    fn centroide_cuadrado_unidad() {
        let sq = [
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ];
        let g = polygon_centroid(&sq).unwrap();
        assert!(close_pt(g, Point2::new(0.5, 0.5)));
        let cw = [sq[0], sq[3], sq[2], sq[1]];
        assert!(close_pt(
            polygon_centroid(&cw).unwrap(),
            Point2::new(0.5, 0.5)
        ));
    }

    #[test]
    fn centroide_triangulo_es_media_de_vertices() {
        let tri = [
            Point2::new(0.0, 0.0),
            Point2::new(3.0, 0.0),
            Point2::new(0.0, 3.0),
        ];
        assert!(close_pt(
            polygon_centroid(&tri).unwrap(),
            Point2::new(1.0, 1.0)
        ));
        assert!(polygon_centroid(&tri[..2]).is_none());
    }

    #[test]
    fn centroide_colineal_degenera_a_la_media() {
        let line = [
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(4.0, 0.0),
        ];
        assert!(close_pt(
            polygon_centroid(&line).unwrap(),
            Point2::new(2.0, 0.0)
        ));
    }

    #[test]
    fn arc_tangent_reusa_soporte() {
        let arc = bulge_to_arc(Point2::new(1.0, 0.0), Point2::new(-1.0, 0.0), 1.0).unwrap();
        let pts = tangent_points(Point2::new(0.0, 3.0), arc.center, arc.radius);
        assert_eq!(pts.len(), 2);
        for t in &pts {
            assert!(close(t.dist(arc.center), arc.radius));
        }
    }
}
