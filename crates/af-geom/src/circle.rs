//! Builds a circumcircle from three points for CIRCLE 3P and ARC 3P. This pure
//! geometry does not mutate documents and rejects collinear input.

use af_math::{Point2, Tol};

/// Returns the circumcircle `(center, radius)` equidistant from three points.
///
/// Returns `None` when the points are collinear within relative linear tolerance.
///
/// The closed-form circumcenter calculation translates coordinates to the first
/// point to reduce numerical error.
#[must_use]
pub fn circumcircle(a: Point2, b: Point2, c: Point2) -> Option<(Point2, f64)> {
    let tol = Tol::default();
    let ab = b - a;
    let ac = c - a;
    let cross = ab.cross(ac);
    // Use a scale-relative cross-product test for near-collinearity.
    if cross.abs() <= tol.linear * ab.norm() * ac.norm() {
        return None;
    }
    let d = 2.0 * cross;
    let ab2 = ab.norm_sq();
    let ac2 = ac.norm_sq();
    // Circumcenter relative to a, with b' = ab and c' = ac.
    let ux = (ac.y * ab2 - ab.y * ac2) / d;
    let uy = (ab.x * ac2 - ac.x * ab2) / d;
    let center = Point2::new(a.x + ux, a.y + uy);
    Some((center, center.dist(a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= TOL
    }

    fn close_pt(p: Point2, q: Point2) -> bool {
        close(p.x, q.x) && close(p.y, q.y)
    }

    #[test]
    fn tres_puntos_sobre_el_circulo_unidad() {
        let (c, r) = circumcircle(
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
        )
        .expect("no colineales");
        assert!(close_pt(c, Point2::ORIGIN));
        assert!(close(r, 1.0));
    }

    #[test]
    fn triangulo_rectangulo_centro_en_la_hipotenusa() {
        // For a right angle at the origin, the hypotenuse midpoint is the center.
        let (c, r) = circumcircle(
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(0.0, 2.0),
        )
        .expect("no colineales");
        assert!(close_pt(c, Point2::new(1.0, 1.0)));
        assert!(close(r, 2.0_f64.sqrt()));
    }

    #[test]
    fn equidistante_de_los_tres_vertices() {
        let (a, b, d) = (
            Point2::new(-3.0, 2.0),
            Point2::new(5.0, 1.0),
            Point2::new(1.0, 7.0),
        );
        let (c, r) = circumcircle(a, b, d).expect("no colineales");
        assert!(close(c.dist(a), r));
        assert!(close(c.dist(b), r));
        assert!(close(c.dist(d), r));
    }

    #[test]
    fn colineales_es_none() {
        assert!(
            circumcircle(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(2.0, 0.0),
            )
            .is_none()
        );
        // Diagonal collinearity, including duplicate points.
        assert!(
            circumcircle(
                Point2::new(0.0, 0.0),
                Point2::new(2.0, 2.0),
                Point2::new(0.0, 0.0),
            )
            .is_none()
        );
    }
}
