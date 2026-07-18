//! Signed area of closed contours using shoelace terms plus bulged arc segments.
//!
//! Counterclockwise contours are positive. Consumers can take the absolute value
//! for physical area while retaining the sign for contour composition.
//!
//! Arcs are not flattened. Their exact circular-segment contribution is
//! `(r^2 / 2) * (theta - sin(theta))`, where `theta = 4 * atan(bulge)`.

use af_math::{Point2, Tol};

/// Signed shoelace area of the polygon formed by `pts`, positive counterclockwise.
///
/// Fewer than three vertices return 0.0. This function treats every edge as straight.
#[must_use]
pub fn polygon_signed_area(pts: &[Point2]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut acc = 0.0;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        acc += a.x * b.y - b.x * a.y;
    }
    acc * 0.5
}

/// Signed circular-segment area between chord `a -> b` and its bulge arc.
///
/// Positive bulges arc left of `a -> b`. Degenerate chords and near-zero bulges
/// return 0.0.
#[must_use]
pub fn bulge_segment_area(a: Point2, b: Point2, bulge: f64) -> f64 {
    let tol = Tol::default();
    let chord = a.dist(b);
    if chord <= tol.point_merge || bulge.abs() <= tol.linear || !bulge.is_finite() {
        return 0.0;
    }
    let theta = 4.0 * bulge.atan(); // Signed sweep: bulge > 0 means CCW and theta > 0.
    let half = theta * 0.5;
    // r = chord / (2 * sin(theta / 2)); squaring removes the bulge sign.
    let r = chord / (2.0 * half.sin());
    0.5 * r * r * (theta - theta.sin())
}

/// Signed area of a closed polyline where vertex `i` carries the bulge for the
/// segment to `i + 1`.
///
/// Adds every circular-segment contribution to the vertex shoelace area.
#[must_use]
pub fn closed_polyline_signed_area(verts: &[(Point2, f64)]) -> f64 {
    if verts.is_empty() {
        return 0.0;
    }
    let pts: Vec<Point2> = verts.iter().map(|(p, _)| *p).collect();
    let mut area = polygon_signed_area(&pts);
    let n = verts.len();
    for i in 0..n {
        let (a, bulge) = verts[i];
        let (b, _) = verts[(i + 1) % n];
        area += bulge_segment_area(a, b, bulge);
    }
    area
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::PI;

    const TOL: f64 = 1e-9;

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    #[test]
    fn shoelace_cuadrado_ccw_y_cw() {
        let ccw = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)];
        assert!((polygon_signed_area(&ccw) - 100.0).abs() < TOL);
        // The clockwise square has the same magnitude with a negative sign.
        let cw = [p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0)];
        assert!((polygon_signed_area(&cw) + 100.0).abs() < TOL);
    }

    #[test]
    fn shoelace_triangulo() {
        let t = [p(0.0, 0.0), p(4.0, 0.0), p(0.0, 3.0)];
        assert!((polygon_signed_area(&t) - 6.0).abs() < TOL); // Base times height divided by two.
    }

    #[test]
    fn menos_de_tres_vertices_no_encierra_area() {
        assert_eq!(polygon_signed_area(&[]), 0.0);
        assert_eq!(polygon_signed_area(&[p(1.0, 1.0)]), 0.0);
        assert_eq!(polygon_signed_area(&[p(0.0, 0.0), p(5.0, 0.0)]), 0.0);
    }

    #[test]
    fn segmento_circular_semidisco() {
        // A bulge-1 chord from (0, 0) to (2, 0) encloses a pi/2 semicircle.
        let area = bulge_segment_area(p(0.0, 0.0), p(2.0, 0.0), 1.0);
        assert!((area - PI / 2.0).abs() < TOL, "got {area}");
        // A negative bulge reverses the sign.
        let neg = bulge_segment_area(p(0.0, 0.0), p(2.0, 0.0), -1.0);
        assert!((neg + PI / 2.0).abs() < TOL);
    }

    #[test]
    fn tramo_recto_o_cuerda_degenerada_no_aporta() {
        assert_eq!(bulge_segment_area(p(0.0, 0.0), p(5.0, 0.0), 0.0), 0.0);
        assert_eq!(bulge_segment_area(p(1.0, 1.0), p(1.0, 1.0), 1.0), 0.0);
    }

    #[test]
    fn polilinea_cerrada_semicirculo_sobre_diametro() {
        // Two vertices with one bulged face and a straight closure enclose pi/2.
        let verts = [(p(0.0, 0.0), 1.0), (p(2.0, 0.0), 0.0)];
        let area = closed_polyline_signed_area(&verts);
        assert!((area - PI / 2.0).abs() < TOL, "got {area}");
    }

    #[test]
    fn polilinea_cerrada_circulo_completo_por_dos_bulges() {
        // Two bulge-1 semicircles form a unit circle with area pi.
        let verts = [(p(0.0, 0.0), 1.0), (p(2.0, 0.0), 1.0)];
        let area = closed_polyline_signed_area(&verts);
        assert!((area - PI).abs() < TOL, "got {area}");
    }

    #[test]
    fn polilinea_cerrada_cuadrado_con_lado_abombado() {
        // An outward bulge on a CCW 10x10 square adds its circular segment exactly.
        let side = 10.0;
        let bulge = 0.4; // Minor arc.
        let seg = bulge_segment_area(p(side, 0.0), p(side, side), bulge);
        let verts = [
            (p(0.0, 0.0), 0.0),
            (p(side, 0.0), bulge), // Bulged right side.
            (p(side, side), 0.0),
            (p(0.0, side), 0.0),
        ];
        let area = closed_polyline_signed_area(&verts);
        assert!((area - (100.0 + seg)).abs() < TOL, "got {area}, seg {seg}");
        assert!(seg > 0.0, "el bombeo hacia afuera añade área");
    }
}
