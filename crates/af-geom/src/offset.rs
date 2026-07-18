//! Signed parallel offsets for lines, circles, arcs, and polylines.
//! Polyline offsets do not resolve self-intersections or separated-neighbor gaps.

use af_math::angle::angle_of;
use af_math::{Point2, Tol};

use crate::bulge::{ArcSeg, arc_to_bulge, bulge_to_arc};
use crate::intersect::{Hit, LineX, circle_circle, line_circle, line_line};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetError {
    Degenerate,
    TooFewVertices,
}

impl core::fmt::Display for OffsetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            OffsetError::Degenerate => "offset collapses the curve (zero-length segment or radius)",
            OffsetError::TooFewVertices => "polyline needs at least two vertices to offset",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for OffsetError {}

pub fn offset_line(a: Point2, b: Point2, d: f64) -> Result<(Point2, Point2), OffsetError> {
    let dir = (b - a).normalize().map_err(|_| OffsetError::Degenerate)?;
    let n = dir.perp() * d;
    Ok((a + n, b + n))
}

pub fn offset_circle(radius: f64, d: f64) -> Result<f64, OffsetError> {
    let r = radius + d;
    if !r.is_finite() || r <= Tol::default().point_merge {
        return Err(OffsetError::Degenerate);
    }
    Ok(r)
}

pub fn offset_arc(arc: &ArcSeg, d: f64) -> Result<ArcSeg, OffsetError> {
    let radius = offset_circle(arc.radius, d)?;
    Ok(ArcSeg {
        center: arc.center,
        radius,
        start_angle: arc.start_angle,
        end_angle: arc.end_angle,
    })
}

#[derive(Debug, Clone, Copy)]
enum OSeg {
    Line {
        a: Point2,
        b: Point2,
    },
    Arc {
        center: Point2,
        radius: f64,
        a: Point2,
        b: Point2,
        ccw: bool,
    },
}

impl OSeg {
    fn start(&self) -> Point2 {
        match self {
            OSeg::Line { a, .. } | OSeg::Arc { a, .. } => *a,
        }
    }
    fn end(&self) -> Point2 {
        match self {
            OSeg::Line { b, .. } | OSeg::Arc { b, .. } => *b,
        }
    }
    fn set_start(&mut self, p: Point2) {
        match self {
            OSeg::Line { a, .. } | OSeg::Arc { a, .. } => *a = p,
        }
    }
    fn set_end(&mut self, p: Point2) {
        match self {
            OSeg::Line { b, .. } | OSeg::Arc { b, .. } => *b = p,
        }
    }
    fn bulge(&self) -> f64 {
        match self {
            OSeg::Line { .. } => 0.0,
            OSeg::Arc {
                center,
                radius,
                a,
                b,
                ccw,
            } => {
                let (sa, ea) = if *ccw {
                    (angle_of(*a - *center), angle_of(*b - *center))
                } else {
                    (angle_of(*b - *center), angle_of(*a - *center))
                };
                let arc = ArcSeg {
                    center: *center,
                    radius: *radius,
                    start_angle: sa,
                    end_angle: ea,
                };
                arc_to_bulge(*a, *b, &arc)
            }
        }
    }
}

fn offset_seg(a: Point2, b: Point2, bulge: f64, d: f64) -> Result<OSeg, OffsetError> {
    let tol = Tol::default();
    if bulge.abs() <= tol.linear {
        let (a2, b2) = offset_line(a, b, d)?;
        return Ok(OSeg::Line { a: a2, b: b2 });
    }
    let arc = bulge_to_arc(a, b, bulge).map_err(|_| OffsetError::Degenerate)?;
    let sign = if bulge > 0.0 { 1.0 } else { -1.0 };
    let radius = offset_circle(arc.radius, -sign * d)?;
    let scale = radius / arc.radius;
    let a2 = arc.center + (a - arc.center) * scale;
    let b2 = arc.center + (b - arc.center) * scale;
    Ok(OSeg::Arc {
        center: arc.center,
        radius,
        a: a2,
        b: b2,
        ccw: bulge > 0.0,
    })
}

fn join(prev: &OSeg, next: &OSeg, near: Point2) -> Point2 {
    match (prev, next) {
        (OSeg::Line { a: pa, b: pb }, OSeg::Line { a: na, b: nb }) => {
            match line_line(*pa, *pb, *na, *nb) {
                LineX::Point(h) => h.point,
                LineX::Parallel | LineX::Collinear => near,
            }
        }
        (OSeg::Line { a, b }, OSeg::Arc { center, radius, .. })
        | (OSeg::Arc { center, radius, .. }, OSeg::Line { a, b }) => {
            nearest(line_circle(*a, *b, *center, *radius), near)
        }
        (
            OSeg::Arc {
                center: c1,
                radius: r1,
                ..
            },
            OSeg::Arc {
                center: c2,
                radius: r2,
                ..
            },
        ) => nearest(circle_circle(*c1, *r1, *c2, *r2), near),
    }
}

fn nearest(hits: Vec<Hit>, near: Point2) -> Point2 {
    hits.into_iter()
        .map(|h| h.point)
        .min_by(|p, q| {
            p.dist_sq(near)
                .partial_cmp(&q.dist_sq(near))
                .unwrap_or(core::cmp::Ordering::Equal)
        })
        .unwrap_or(near)
}

pub fn offset_polyline(
    verts: &[(Point2, f64)],
    closed: bool,
    d: f64,
) -> Result<Vec<(Point2, f64)>, OffsetError> {
    let n = verts.len();
    if n < 2 {
        return Err(OffsetError::TooFewVertices);
    }
    let seg_count = if closed { n } else { n - 1 };

    let mut segs: Vec<OSeg> = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        let (a, bulge) = verts[i];
        let (b, _) = verts[(i + 1) % n];
        segs.push(offset_seg(a, b, bulge, d)?);
    }

    let junctions: Vec<usize> = if closed {
        (0..n).collect()
    } else {
        (1..seg_count).collect()
    };
    for k in junctions {
        let prev = (k + seg_count - 1) % seg_count;
        let cur = k % seg_count;
        let near = segs[prev].end();
        let v = join(&segs[prev], &segs[cur], near);
        segs[prev].set_end(v);
        segs[cur].set_start(v);
    }

    let mut out: Vec<(Point2, f64)> = segs.iter().map(|s| (s.start(), s.bulge())).collect();
    if !closed {
        out.push((segs[seg_count - 1].end(), 0.0));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, PI};

    const TOL: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= TOL
    }

    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn offset_line_izquierda_positiva() {
        let (a, b) = offset_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), 2.0).unwrap();
        assert!(close(a.x, 0.0) && close(a.y, 2.0));
        assert!(close(b.x, 10.0) && close(b.y, 2.0));
    }

    #[test]
    fn offset_line_negativa_al_otro_lado() {
        let (a, b) = offset_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), -3.0).unwrap();
        assert!(close(a.y, -3.0) && close(b.y, -3.0));
    }

    #[test]
    fn offset_line_degenerada_es_error() {
        let p = Point2::new(1.0, 1.0);
        assert_eq!(offset_line(p, p, 1.0), Err(OffsetError::Degenerate));
    }

    #[test]
    fn propiedad_offset_line_mantiene_distancia() {
        let a = Point2::new(-2.0, 1.0);
        let b = Point2::new(5.0, 4.0);
        for &d in &[0.5_f64, 3.0, -1.5] {
            let (a2, b2) = offset_line(a, b, d).unwrap();
            let dir = (b - a).normalize().unwrap();
            let perp = (a2 - a).cross(dir).abs();
            assert!(close(perp, d.abs()), "perp {perp} != |d| {}", d.abs());
            assert!(close((b2 - a2).cross(dir), 0.0));
        }
    }

    #[test]
    fn offset_circle_afuera_y_adentro() {
        assert!(close(offset_circle(5.0, 2.0).unwrap(), 7.0));
        assert!(close(offset_circle(5.0, -2.0).unwrap(), 3.0));
    }

    #[test]
    fn offset_circle_colapso_es_error() {
        assert_eq!(offset_circle(5.0, -5.0), Err(OffsetError::Degenerate));
        assert_eq!(offset_circle(5.0, -6.0), Err(OffsetError::Degenerate));
    }

    #[test]
    fn offset_arc_conserva_centro_y_barrido() {
        let arc = ArcSeg {
            center: Point2::new(1.0, 2.0),
            radius: 4.0,
            start_angle: 0.0,
            end_angle: FRAC_PI_2,
        };
        let off = offset_arc(&arc, 1.5).unwrap();
        assert_eq!(off.center, arc.center);
        assert!(close(off.radius, 5.5));
        assert!(close(off.start_angle, arc.start_angle));
        assert!(close(off.end_angle, arc.end_angle));
    }

    #[test]
    fn offset_polyline_recta_en_l_une_la_esquina() {
        let verts = [
            (Point2::new(0.0, 0.0), 0.0),
            (Point2::new(10.0, 0.0), 0.0),
            (Point2::new(10.0, 10.0), 0.0),
        ];
        let out = offset_polyline(&verts, false, 1.0).unwrap();
        assert_eq!(out.len(), 3);
        assert!(close(out[0].0.x, 0.0) && close(out[0].0.y, 1.0));
        assert!(close(out[1].0.x, 9.0) && close(out[1].0.y, 1.0));
        assert!(close(out[2].0.x, 9.0) && close(out[2].0.y, 10.0));
        assert!(out.iter().all(|(_, b)| close(*b, 0.0)));
    }

    #[test]
    fn propiedad_offset_polyline_con_bulge_mantiene_distancia() {
        let bulge = 1.0;
        let verts = [
            (Point2::new(0.0, 0.0), 0.0),
            (Point2::new(4.0, 0.0), bulge),
            (Point2::new(4.0, 4.0), 0.0),
        ];
        let d = 0.75;
        let arc0 = bulge_to_arc(verts[1].0, verts[2].0, bulge).unwrap();
        let out = offset_polyline(&verts, false, d).unwrap();
        assert_eq!(out.len(), 3);

        assert!(close(out[0].0.y, d), "recta no se desplazó a +y por d");

        assert!(out[1].1.abs() > 0.1, "el tramo curvo perdió su bulge");
        let arc1 = bulge_to_arc(out[1].0, out[2].0, out[1].1).unwrap();
        assert!(
            close_pt(arc1.center, arc0.center),
            "el arco desplazado no es concéntrico"
        );
        assert!(
            close((arc1.radius - arc0.radius).abs(), d),
            "radio no cambió en |d|: {} vs {}",
            arc1.radius,
            arc0.radius
        );
    }

    #[test]
    fn offset_polyline_cerrada_cuadrado_encoge_hacia_dentro() {
        let verts = [
            (Point2::new(0.0, 0.0), 0.0),
            (Point2::new(4.0, 0.0), 0.0),
            (Point2::new(4.0, 4.0), 0.0),
            (Point2::new(0.0, 4.0), 0.0),
        ];
        let out = offset_polyline(&verts, true, 1.0).unwrap();
        assert_eq!(out.len(), 4);
        assert!(close(out[0].0.x, 1.0) && close(out[0].0.y, 1.0));
        assert!(close(out[1].0.x, 3.0) && close(out[1].0.y, 1.0));
        assert!(close(out[2].0.x, 3.0) && close(out[2].0.y, 3.0));
        assert!(close(out[3].0.x, 1.0) && close(out[3].0.y, 3.0));
    }

    #[test]
    fn offset_polyline_pocos_vertices_es_error() {
        let verts = [(Point2::new(0.0, 0.0), 0.0)];
        assert_eq!(
            offset_polyline(&verts, false, 1.0),
            Err(OffsetError::TooFewVertices)
        );
    }

    #[test]
    fn offset_polyline_arco_colapsado_es_error() {
        let verts = [(Point2::new(0.0, 0.0), 1.0), (Point2::new(2.0, 0.0), 0.0)];
        assert_eq!(
            offset_polyline(&verts, false, 2.0),
            Err(OffsetError::Degenerate)
        );
    }

    #[test]
    fn offset_polyline_semicirculo_signo_correcto() {
        let verts = [(Point2::new(0.0, 0.0), 1.0), (Point2::new(2.0, 0.0), 0.0)];
        let arc0 = bulge_to_arc(verts[0].0, verts[1].0, 1.0).unwrap();
        let out = offset_polyline(&verts, false, -0.5).unwrap();
        let arc1 = bulge_to_arc(out[0].0, out[1].0, out[0].1).unwrap();
        assert!(close(arc1.radius, arc0.radius + 0.5));
        assert!(close_pt(arc1.center, arc0.center));
    }

    #[test]
    fn propiedad_muestra_arco_a_distancia_d() {
        let verts = [(Point2::new(0.0, 0.0), 0.5), (Point2::new(3.0, 1.0), 0.0)];
        let arc0 = bulge_to_arc(verts[0].0, verts[1].0, 0.5).unwrap();
        for &d in &[0.3_f64, -0.4] {
            let out = offset_polyline(&verts, false, d).unwrap();
            let arc1 = bulge_to_arc(out[0].0, out[1].0, out[0].1).unwrap();
            let mid = arc1.midpoint();
            assert!(
                close(arc0.distance_to(mid), d.abs()),
                "muestra a {} != |d| {}",
                arc0.distance_to(mid),
                d.abs()
            );
        }
    }

    #[test]
    fn offset_polyline_arco_semicirculo_completo_barrido() {
        let verts = [(Point2::new(0.0, 0.0), 1.0), (Point2::new(4.0, 0.0), 0.0)];
        let out = offset_polyline(&verts, false, -1.0).unwrap();
        assert!(close(out[0].1.abs(), 1.0), "bulge {} != 1", out[0].1.abs());
        assert!(out[0].1 > 0.0);
        let _ = PI;
    }
}
