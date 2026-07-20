//! 2D tangent-circle centers and contact points.

use af_math::{Point2, Tol, Vec2};

use crate::intersect::{LineX, circle_circle, line_circle, line_line};
use crate::project::{perp_foot_line, project_on_circle};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TangentCurve {
    Line { a: Point2, b: Point2 },
    Circle { center: Point2, radius: f64 },
}

#[derive(Debug, Clone, Copy)]
enum Locus {
    Line { a: Point2, b: Point2 },
    Circle { center: Point2, radius: f64 },
}

fn offset_loci(curve: &TangentCurve, r: f64, tol: &Tol) -> Vec<Locus> {
    match *curve {
        TangentCurve::Line { a, b } => {
            let d = b - a;
            let len = d.norm();
            if len <= tol.point_merge {
                return Vec::new();
            }
            let off: Vec2 = (d / len).perp() * r;
            vec![
                Locus::Line {
                    a: a + off,
                    b: b + off,
                },
                Locus::Line {
                    a: a - off,
                    b: b - off,
                },
            ]
        }
        TangentCurve::Circle { center, radius } => {
            if !radius.is_finite() || radius <= tol.point_merge {
                return Vec::new();
            }
            let mut loci = vec![Locus::Circle {
                center,
                radius: radius + r,
            }];
            let inner = (radius - r).abs();
            if inner > tol.point_merge {
                loci.push(Locus::Circle {
                    center,
                    radius: inner,
                });
            }
            loci
        }
    }
}

fn intersect_loci(l1: &Locus, l2: &Locus, out: &mut Vec<Point2>) {
    match (l1, l2) {
        (Locus::Line { a: a1, b: b1 }, Locus::Line { a: a2, b: b2 }) => {
            if let LineX::Point(h) = line_line(*a1, *b1, *a2, *b2) {
                out.push(h.point);
            }
        }
        (Locus::Line { a, b }, Locus::Circle { center, radius })
        | (Locus::Circle { center, radius }, Locus::Line { a, b }) => {
            out.extend(
                line_circle(*a, *b, *center, *radius)
                    .into_iter()
                    .map(|h| h.point),
            );
        }
        (
            Locus::Circle {
                center: c1,
                radius: r1,
            },
            Locus::Circle {
                center: c2,
                radius: r2,
            },
        ) => {
            out.extend(
                circle_circle(*c1, *r1, *c2, *r2)
                    .into_iter()
                    .map(|h| h.point),
            );
        }
    }
}

fn dedup_points(pts: Vec<Point2>, tol: &Tol) -> Vec<Point2> {
    let mut out: Vec<Point2> = Vec::with_capacity(pts.len());
    for p in pts {
        if !out.iter().any(|q| tol.points_coincide(p, *q)) {
            out.push(p);
        }
    }
    out
}

#[must_use]
pub fn tangent_circle_centers(c1: &TangentCurve, c2: &TangentCurve, r: f64) -> Vec<Point2> {
    let tol = Tol::default();
    if !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    let loci1 = offset_loci(c1, r, &tol);
    let loci2 = offset_loci(c2, r, &tol);
    let mut pts = Vec::new();
    for l1 in &loci1 {
        for l2 in &loci2 {
            intersect_loci(l1, l2, &mut pts);
        }
    }
    dedup_points(pts, &tol)
}

#[must_use]
pub fn tangent_point_on(curve: &TangentCurve, center: Point2) -> Point2 {
    match *curve {
        TangentCurve::Line { a, b } => perp_foot_line(center, a, b).map_or(a, |(foot, _)| foot),
        TangentCurve::Circle { center: o, radius } => {
            project_on_circle(center, o, radius).unwrap_or_else(|| Point2::new(o.x + radius, o.y))
        }
    }
}

/// Returns the contact between `curve` and a tangent candidate circle.
///
/// Lines use the perpendicular foot. Circles use the radical-axis foot, which
/// also selects the opposite-side contact when the candidate encloses the source.
#[must_use]
pub fn tangent_contact_point(
    curve: &TangentCurve,
    candidate_center: Point2,
    candidate_radius: f64,
) -> Option<Point2> {
    let tol = Tol::default();
    if !finite_point(candidate_center)
        || !candidate_radius.is_finite()
        || candidate_radius <= tol.point_merge
    {
        return None;
    }

    let contact = match *curve {
        TangentCurve::Line { a, b } => perp_foot_line(candidate_center, a, b)?.0,
        TangentCurve::Circle { center, radius } => {
            if !finite_point(center) || !radius.is_finite() || radius <= tol.point_merge {
                return None;
            }
            let delta = candidate_center - center;
            let distance = delta.x.hypot(delta.y);
            if !distance.is_finite() || distance <= tol.point_merge {
                return None;
            }
            let along = (distance * distance + radius * radius
                - candidate_radius * candidate_radius)
                / (2.0 * distance);
            center + delta * (along / distance)
        }
    };
    finite_point(contact).then_some(contact)
}

#[inline]
fn finite_point(point: Point2) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::SQRT_2;

    const TOL: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= TOL
    }
    fn close_pt(p: Point2, q: Point2) -> bool {
        close(p.x, q.x) && close(p.y, q.y)
    }

    fn perp_dist_line(p: Point2, a: Point2, b: Point2) -> f64 {
        let d = b - a;
        let len = d.norm();
        (p - a).cross(d / len).abs()
    }

    fn tangency_residual(curve: &TangentCurve, p: Point2, r: f64) -> f64 {
        match *curve {
            TangentCurve::Line { a, b } => (perp_dist_line(p, a, b) - r).abs(),
            TangentCurve::Circle { center, radius } => {
                let d = p.dist(center);
                (d - (radius + r)).abs().min((d - (radius - r).abs()).abs())
            }
        }
    }

    fn dist_to_curve(curve: &TangentCurve, t: Point2) -> f64 {
        match *curve {
            TangentCurve::Line { a, b } => perp_dist_line(t, a, b),
            TangentCurve::Circle { center, radius } => (t.dist(center) - radius).abs(),
        }
    }

    #[test]
    fn lineas_perpendiculares_r1_da_4_centros() {
        let lx = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(1.0, 0.0),
        };
        let ly = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(0.0, 1.0),
        };
        let centers = tangent_circle_centers(&lx, &ly, 1.0);
        assert_eq!(centers.len(), 4);
        for &(sx, sy) in &[(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            assert!(
                centers.iter().any(|c| close_pt(*c, Point2::new(sx, sy))),
                "falta el centro ({sx}, {sy})"
            );
        }
        for c in &centers {
            assert!(close(tangency_residual(&lx, *c, 1.0), 0.0));
            assert!(close(tangency_residual(&ly, *c, 1.0), 0.0));
        }
    }

    #[test]
    fn linea_tangente_externa_a_circulo() {
        let line = TangentCurve::Line {
            a: Point2::new(-1.0, 0.0),
            b: Point2::new(3.0, 0.0),
        };
        let circle = TangentCurve::Circle {
            center: Point2::new(0.0, 4.0),
            radius: 2.0,
        };
        let centers = tangent_circle_centers(&line, &circle, 1.0);
        assert_eq!(centers.len(), 1);
        assert!(close_pt(centers[0], Point2::new(0.0, 1.0)));
        assert!(close(tangency_residual(&line, centers[0], 1.0), 0.0));
        assert!(close(centers[0].dist(Point2::new(0.0, 4.0)), 3.0));
    }

    #[test]
    fn dos_circulos_separados() {
        let c1 = TangentCurve::Circle {
            center: Point2::new(0.0, 0.0),
            radius: 2.0,
        };
        let c2 = TangentCurve::Circle {
            center: Point2::new(4.0, 0.0),
            radius: 2.0,
        };
        let centers = tangent_circle_centers(&c1, &c2, 1.0);
        assert_eq!(centers.len(), 4);
        let s5 = 5.0_f64.sqrt();
        for &want in &[
            Point2::new(2.0, s5),
            Point2::new(2.0, -s5),
            Point2::new(3.0, 0.0),
            Point2::new(1.0, 0.0),
        ] {
            assert!(
                centers.iter().any(|c| close_pt(*c, want)),
                "falta el centro {want:?}"
            );
        }
        for c in &centers {
            assert!(close(tangency_residual(&c1, *c, 1.0), 0.0));
            assert!(close(tangency_residual(&c2, *c, 1.0), 0.0));
        }
    }

    #[test]
    fn radio_no_positivo_da_vacio() {
        let lx = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(1.0, 0.0),
        };
        let ly = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(0.0, 1.0),
        };
        assert!(tangent_circle_centers(&lx, &ly, 0.0).is_empty());
        assert!(tangent_circle_centers(&lx, &ly, -1.0).is_empty());
        assert!(tangent_circle_centers(&lx, &ly, f64::NAN).is_empty());
    }

    #[test]
    fn curvas_coincidentes_da_vacio() {
        let line = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(1.0, 0.0),
        };
        assert!(tangent_circle_centers(&line, &line, 1.0).is_empty());
        let circle = TangentCurve::Circle {
            center: Point2::new(2.0, 1.0),
            radius: 3.0,
        };
        assert!(tangent_circle_centers(&circle, &circle, 1.0).is_empty());
    }

    #[test]
    fn tangent_point_on_linea_es_pie_perpendicular() {
        let line = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(4.0, 0.0),
        };
        let center = Point2::new(1.0, 1.0);
        let t = tangent_point_on(&line, center);
        assert!(close_pt(t, Point2::new(1.0, 0.0)));
        assert!(close(dist_to_curve(&line, t), 0.0));
        assert!(close(center.dist(t), 1.0));
    }

    #[test]
    fn tangent_point_on_circulo_yace_en_circunferencia() {
        let circle = TangentCurve::Circle {
            center: Point2::new(0.0, 0.0),
            radius: 2.0,
        };
        let t = tangent_point_on(&circle, Point2::new(3.0, 0.0));
        assert!(close_pt(t, Point2::new(2.0, 0.0)));
        assert!(close(dist_to_curve(&circle, t), 0.0));
        let t2 = tangent_point_on(&circle, Point2::new(3.0, 3.0));
        assert!(close(t2.dist(Point2::new(0.0, 0.0)), 2.0));
        assert!(close_pt(t2, Point2::new(SQRT_2, SQRT_2)));
    }

    #[test]
    fn tangent_contact_point_cubre_las_tres_tangencias_circulares() {
        for (source_radius, candidate_x, candidate_radius, expected_x) in [
            (2.0, 3.0, 1.0, 2.0),
            (5.0, 3.0, 2.0, 5.0),
            (1.0, 2.0, 3.0, -1.0),
        ] {
            let source = TangentCurve::Circle {
                center: Point2::ORIGIN,
                radius: source_radius,
            };
            assert!(close_pt(
                tangent_contact_point(&source, Point2::new(candidate_x, 0.0), candidate_radius,)
                    .expect("contacto circular"),
                Point2::new(expected_x, 0.0),
            ));
        }
    }

    #[test]
    fn tangent_contact_point_linea_y_entradas_invalidas() {
        let line = TangentCurve::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(4.0, 0.0),
        };
        assert_eq!(
            tangent_contact_point(&line, Point2::new(1.0, 1.0), 1.0),
            Some(Point2::new(1.0, 0.0))
        );
        let degenerate_line = TangentCurve::Line {
            a: Point2::ORIGIN,
            b: Point2::ORIGIN,
        };
        let circle = TangentCurve::Circle {
            center: Point2::ORIGIN,
            radius: 2.0,
        };
        assert!(tangent_contact_point(&degenerate_line, Point2::new(1.0, 0.0), 1.0).is_none());
        assert!(tangent_contact_point(&circle, Point2::new(3.0, 0.0), 0.0).is_none());
        assert!(tangent_contact_point(&circle, Point2::new(3.0, 0.0), -1.0).is_none());
        assert!(tangent_contact_point(&circle, Point2::new(3.0, 0.0), f64::NAN).is_none());
        assert!(tangent_contact_point(&circle, Point2::new(f64::INFINITY, 0.0), 1.0).is_none());
        assert!(tangent_contact_point(&circle, Point2::ORIGIN, 1.0).is_none());
        let invalid_circle = TangentCurve::Circle {
            center: Point2::ORIGIN,
            radius: 0.0,
        };
        assert!(tangent_contact_point(&invalid_circle, Point2::new(1.0, 0.0), 1.0).is_none());
        let huge = TangentCurve::Circle {
            center: Point2::ORIGIN,
            radius: f64::MAX,
        };
        assert!(tangent_contact_point(&huge, Point2::new(1.0, 0.0), f64::MAX).is_none());
    }

    #[test]
    fn propiedad_centros_equidistan_r_de_ambas_curvas() {
        let curves = [
            TangentCurve::Line {
                a: Point2::new(0.0, 0.0),
                b: Point2::new(1.0, 0.0),
            },
            TangentCurve::Line {
                a: Point2::new(0.0, 0.0),
                b: Point2::new(0.0, 1.0),
            },
            TangentCurve::Line {
                a: Point2::new(-1.0, -1.0),
                b: Point2::new(2.0, 1.0),
            },
            TangentCurve::Circle {
                center: Point2::new(0.0, 0.0),
                radius: 1.0,
            },
            TangentCurve::Circle {
                center: Point2::new(3.0, 0.0),
                radius: 2.0,
            },
            TangentCurve::Circle {
                center: Point2::new(-1.0, 2.5),
                radius: 1.5,
            },
        ];
        let radii = [0.5, 1.0, 2.0];
        let mut total = 0usize;
        for (i, c1) in curves.iter().enumerate() {
            for c2 in curves.iter().skip(i + 1) {
                for &r in &radii {
                    for center in tangent_circle_centers(c1, c2, r) {
                        total += 1;
                        assert!(
                            tangency_residual(c1, center, r) <= 1e-6,
                            "residual c1 alto para r={r}"
                        );
                        assert!(
                            tangency_residual(c2, center, r) <= 1e-6,
                            "residual c2 alto para r={r}"
                        );
                        assert!(dist_to_curve(c1, tangent_point_on(c1, center)) <= 1e-6);
                        assert!(dist_to_curve(c2, tangent_point_on(c2, center)) <= 1e-6);
                    }
                }
            }
        }
        assert!(total > 20, "esperaba varios centros, hubo {total}");
    }
}
