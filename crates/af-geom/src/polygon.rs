//! Vertices for regular polygons and axis-aligned rectangles.
//!
//! Both functions return an open counterclockwise ring without repeating the
//! first vertex. Closure is implicit and document validation remains elsewhere.

use core::f64::consts::TAU;

use af_math::Point2;

/// Returns vertices for a regular polygon inscribed in the given circle, starting
/// at `start_angle` radians counterclockwise from positive X.
///
/// Returns an empty vector when `n` falls outside 3 through 1024.
///
/// Circumscribed polygons use the same calculation after scaling radius by
/// `1 / cos(pi / n)`, so they do not need a separate function.
#[must_use]
pub fn regular_polygon_vertices(
    center: Point2,
    radius: f64,
    n: usize,
    start_angle: f64,
) -> Vec<Point2> {
    if !(3..=1024).contains(&n) {
        return Vec::new();
    }
    let step = TAU / n as f64;
    (0..n)
        .map(|k| {
            let (s, c) = (start_angle + step * k as f64).sin_cos();
            Point2::new(center.x + radius * c, center.y + radius * s)
        })
        .collect()
}

/// Returns four counterclockwise vertices for the axis-aligned rectangle whose
/// opposite corners are `p1` and `p2` in either order.
///
/// Order is lower-left, lower-right, upper-right, upper-left. Degenerate input
/// still returns four vertices.
#[must_use]
pub fn rectangle_vertices(p1: Point2, p2: Point2) -> [Point2; 4] {
    let (min_x, max_x) = (p1.x.min(p2.x), p1.x.max(p2.x));
    let (min_y, max_y) = (p1.y.min(p2.y), p1.y.max(p2.y));
    [
        Point2::new(min_x, min_y),
        Point2::new(max_x, min_y),
        Point2::new(max_x, max_y),
        Point2::new(min_x, max_y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-9;

    /// Signed shoelace area; positive means counterclockwise.
    fn signed_area(v: &[Point2]) -> f64 {
        v.iter()
            .zip(v.iter().cycle().skip(1))
            .map(|(a, b)| a.x * b.y - b.x * a.y)
            .sum::<f64>()
            * 0.5
    }

    #[test]
    fn poligono_lados_iguales_y_ccw() {
        for n in [3usize, 4, 5, 6, 12] {
            let v = regular_polygon_vertices(Point2::new(2.0, -1.0), 3.0, n, 0.3);
            assert_eq!(v.len(), n);
            // Every vertex lies on the source circle.
            for p in &v {
                assert!((p.dist(Point2::new(2.0, -1.0)) - 3.0).abs() <= TOL);
            }
            // All sides are equal within tolerance.
            let side0 = v[0].dist(v[1]);
            for k in 0..n {
                let side = v[k].dist(v[(k + 1) % n]);
                assert!((side - side0).abs() <= TOL, "lado {k} desigual: {side}");
            }
            // Counterclockwise orientation.
            assert!(signed_area(&v) > 0.0, "el polígono no es CCW (n={n})");
        }
    }

    #[test]
    fn poligono_start_angle_coloca_el_primer_vertice() {
        // A zero start angle places the first vertex along positive X.
        let v = regular_polygon_vertices(Point2::ORIGIN, 5.0, 4, 0.0);
        assert!((v[0].x - 5.0).abs() <= TOL && v[0].y.abs() <= TOL);
    }

    #[test]
    fn poligono_fuera_de_rango_es_vacio() {
        assert!(regular_polygon_vertices(Point2::ORIGIN, 1.0, 2, 0.0).is_empty());
        assert!(regular_polygon_vertices(Point2::ORIGIN, 1.0, 1025, 0.0).is_empty());
    }

    #[test]
    fn rectangulo_ccw_cerrado_y_lados_opuestos_iguales() {
        // Reversing opposite corners does not change the result.
        let v = rectangle_vertices(Point2::new(3.0, 4.0), Point2::new(-1.0, -2.0));
        assert_eq!(
            v,
            [
                Point2::new(-1.0, -2.0),
                Point2::new(3.0, -2.0),
                Point2::new(3.0, 4.0),
                Point2::new(-1.0, 4.0),
            ]
        );
        // Counterclockwise.
        assert!(signed_area(&v) > 0.0);
        // Opposite sides have equal width and height.
        assert!((v[0].dist(v[1]) - v[2].dist(v[3])).abs() <= TOL);
        assert!((v[1].dist(v[2]) - v[3].dist(v[0])).abs() <= TOL);
        // Width is 4 and height is 6.
        assert!((v[0].dist(v[1]) - 4.0).abs() <= TOL);
        assert!((v[1].dist(v[2]) - 6.0).abs() <= TOL);
    }

    #[test]
    fn rectangulo_orden_de_esquinas_indiferente() {
        let a = rectangle_vertices(Point2::new(0.0, 0.0), Point2::new(2.0, 1.0));
        let b = rectangle_vertices(Point2::new(2.0, 1.0), Point2::new(0.0, 0.0));
        let c = rectangle_vertices(Point2::new(0.0, 1.0), Point2::new(2.0, 0.0));
        assert_eq!(a, b);
        assert_eq!(a, c);
    }
}
