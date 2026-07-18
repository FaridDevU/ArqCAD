//! Revision-cloud bulge vertices generated from a closed contour.

use af_math::{Point2, Tol};

use crate::area::polygon_signed_area;

const CLOUD_BULGE: f64 = 0.5;

const MAX_REVCLOUD_SEGMENTS: usize = 4096;

#[must_use]
pub fn revcloud_vertices(contour: &[Point2], arc_len: f64) -> Vec<(Point2, f64)> {
    if contour.len() < 3 || !arc_len.is_finite() || arc_len <= 0.0 {
        return Vec::new();
    }
    let tol = Tol::default();
    let mut n = contour.len();
    if tol.points_coincide(contour[0], contour[n - 1]) {
        n -= 1;
    }
    if n < 3
        || contour[..n]
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return Vec::new();
    }

    let mut total = 0usize;
    for i in 0..n {
        let len = contour[i].dist(contour[(i + 1) % n]);
        if !len.is_finite() || len <= tol.point_merge {
            return Vec::new();
        }
        let segs = (len / arc_len).round().max(1.0);
        if !segs.is_finite() || segs > MAX_REVCLOUD_SEGMENTS as f64 {
            return Vec::new();
        }
        let Some(next_total) = total.checked_add(segs as usize) else {
            return Vec::new();
        };
        if next_total > MAX_REVCLOUD_SEGMENTS {
            return Vec::new();
        }
        total = next_total;
    }

    let area = polygon_signed_area(&contour[..n]);
    if !area.is_finite() {
        return Vec::new();
    }
    let outward = if area >= 0.0 {
        CLOUD_BULGE
    } else {
        -CLOUD_BULGE
    };
    let mut out: Vec<(Point2, f64)> = Vec::with_capacity(total);
    for i in 0..n {
        let a = contour[i];
        let b = contour[(i + 1) % n];
        let len = a.dist(b);
        let segs = ((len / arc_len).round() as usize).max(1);
        for k in 0..segs {
            let t = k as f64 / segs as f64;
            out.push((a.lerp(b, t), outward));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bulge::bulge_to_arc;

    fn rect(w: f64, h: f64) -> Vec<Point2> {
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(w, 0.0),
            Point2::new(w, h),
            Point2::new(0.0, h),
        ]
    }

    #[test]
    fn todos_los_tramos_son_arcos_con_orientacion_consistente() {
        let verts = revcloud_vertices(&rect(10.0, 6.0), 2.0);
        assert!(verts.len() >= 3);
        let signo = verts[0].1.signum();
        for i in 0..verts.len() {
            let (a, bulge) = verts[i];
            let (b, _) = verts[(i + 1) % verts.len()];
            assert!(
                bulge_to_arc(a, b, bulge).is_ok(),
                "el tramo {i} debe ser un arco válido"
            );
            assert!(bulge.abs() > 0.0, "bulge {i} no nulo");
            assert_eq!(bulge.signum(), signo, "orientación consistente en {i}");
        }
    }

    #[test]
    fn cuenta_de_arcos_por_longitud_de_cuerda() {
        let verts = revcloud_vertices(&rect(10.0, 6.0), 2.0);
        assert_eq!(verts.len(), 16);
        let pocos = revcloud_vertices(&rect(10.0, 6.0), 1_000.0);
        assert_eq!(pocos.len(), 4);
    }

    #[test]
    fn cierre_a_a_equivale_al_contorno_sin_repeticion() {
        let open = rect(10.0, 6.0);
        let mut explicit = open.clone();
        explicit.push(open[0]);
        assert_eq!(
            revcloud_vertices(&explicit, 2.0),
            revcloud_vertices(&open, 2.0)
        );
    }

    #[test]
    fn cap_4096_acepta_limite_y_rechaza_exceso() {
        assert_eq!(
            revcloud_vertices(&rect(1024.0, 1024.0), 1.0).len(),
            MAX_REVCLOUD_SEGMENTS
        );
        assert!(revcloud_vertices(&rect(1025.0, 1024.0), 1.0).is_empty());
    }

    #[test]
    fn tramo_degenerado_no_terminal_es_error() {
        let contour = vec![
            Point2::ORIGIN,
            Point2::ORIGIN,
            Point2::new(4.0, 0.0),
            Point2::new(0.0, 4.0),
        ];
        assert!(revcloud_vertices(&contour, 1.0).is_empty());

        let mut double_close = rect(4.0, 4.0);
        double_close.extend([Point2::ORIGIN, Point2::ORIGIN]);
        assert!(revcloud_vertices(&double_close, 1.0).is_empty());
    }

    #[test]
    fn combado_hacia_afuera_en_ccw_y_cw() {
        let ccw = revcloud_vertices(&rect(4.0, 4.0), 4.0);
        assert!(ccw.iter().all(|&(_, b)| b > 0.0));
        let mut cw = rect(4.0, 4.0);
        cw.reverse();
        let cw = revcloud_vertices(&cw, 4.0);
        assert!(cw.iter().all(|&(_, b)| b < 0.0));
    }

    #[test]
    fn fuera_de_contrato_devuelve_vacio() {
        assert!(revcloud_vertices(&[Point2::ORIGIN, Point2::new(1.0, 0.0)], 1.0).is_empty());
        assert!(revcloud_vertices(&rect(4.0, 4.0), 0.0).is_empty());
        assert!(revcloud_vertices(&rect(4.0, 4.0), f64::NAN).is_empty());
    }
}
