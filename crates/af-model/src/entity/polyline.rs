//! Polyline with line and arc segments using LWPOLYLINE semantics.
//!
//! Each [`PolyVertex`] stores a point and outgoing bulge. Zero bulge is a line;
//! nonzero bulge resolves to an exact arc through [`PolylineGeo::segments`].
//!
//! Open polylines have `N-1` segments and ignore the last bulge; closed polylines
//! add the final-to-first segment. Reflections negate bulges. Anisotropic scale is
//! unsupported when effective arc segments exist.

// Geometry.

use af_geom::bulge::{ArcSeg, bulge_to_arc};
use af_math::{BBox, Point2, Tol, Transform2, Vec2};
use serde::{Deserialize, Serialize};

use crate::entity::{EntityOps, GeomIssue, SnapKind, SnapPoint, SnapVec, TransformError};

/// Polyline point plus the bulge of its outgoing segment.
///
/// JSON: `{"pt":[0.0,0.0],"bulge":0.0}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PolyVertex {
    /// Vertex position.
    pub pt: Point2,
    /// Outgoing bulge `tan(Δθ/4)`; positive is CCW and zero is straight.
    pub bulge: f64,
}

impl PolyVertex {
    /// Creates a vertex.
    #[inline]
    #[must_use]
    pub fn new(pt: Point2, bulge: f64) -> Self {
        Self { pt, bulge }
    }
}

/// LWPOLYLINE vertices, closed flag, and optional constant width.
///
/// `width` is world-space geometry, distinct from display/plot lineweight.
///
/// JSON omits zero width for backward-compatible thin-polyline serialization:
/// `{"type":"polyline","vertices":[{"pt":[0.0,0.0],"bulge":0.0}],"closed":true}`;
/// with width: `…,"closed":true,"width":0.5}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolylineGeo {
    /// Vertices in traversal order.
    pub vertices: Vec<PolyVertex>,
    /// Whether the final-to-first closing segment exists.
    pub closed: bool,
    /// Constant world-space width; zero is omitted from JSON.
    #[serde(default, skip_serializing_if = "width_is_zero")]
    pub width: f64,
}

/// Whether width is exactly zero for serialization omission.
#[inline]
fn width_is_zero(w: &f64) -> bool {
    *w == 0.0
}

/// Resolved polyline segment: line or arc.
///
/// Transient result from [`PolylineGeo::segments`], never serialized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SegKind {
    /// Line segment from `a` to `b`.
    Line {
        /// Start endpoint.
        a: Point2,
        /// End endpoint.
        b: Point2,
    },
    /// Circular arc resolved from bulge.
    Arc(ArcSeg),
}

// Construction.

impl PolylineGeo {
    /// Creates a polyline from vertices and a closed flag.
    ///
    /// Validation is deferred to [`EntityOps::validate`].
    #[inline]
    #[must_use]
    pub fn new(vertices: Vec<PolyVertex>, closed: bool) -> Self {
        Self {
            vertices,
            closed,
            width: 0.0,
        }
    }

    /// Sets constant world-space width.
    ///
    /// Chains after [`new`](Self::new); default width remains zero.
    #[inline]
    #[must_use]
    pub fn with_width(mut self, width: f64) -> Self {
        self.width = width;
        self
    }
}

// Entity operations.

impl EntityOps for PolylineGeo {
    /// Segment-box union plus arc centers so every snap is contained.
    fn bbox(&self) -> BBox {
        let mut acc: Option<BBox> = None;
        for seg in self.segments() {
            let mut sb = seg.bbox();
            if let SegKind::Arc(arc) = seg {
                // Include arc centers because they are snaps outside minor-arc boxes.
                sb = sb.union_point(arc.center);
            }
            acc = Some(acc.map_or(sb, |b| b.union(sb)));
        }
        acc.unwrap_or_else(|| {
            // Invalid short polylines still return a box over present vertices.
            BBox::from_points(self.vertices.iter().map(|v| v.pt))
                .unwrap_or_else(|| BBox::from_point(Point2::ORIGIN))
        })
    }

    /// Transforms vertices; reflections negate bulge.
    ///
    /// # Errors
    /// Returns [`TransformError::NonUniformScaleUnsupported`] for anisotropic arc scale.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        let tol = Tol::default();
        let has_arc = self.vertices.iter().any(|v| v.bulge.abs() > tol.linear);
        if has_arc && !t.is_uniform(&tol) {
            return Err(TransformError::NonUniformScaleUnsupported);
        }
        let mirror = t.is_mirroring();
        let vertices = self
            .vertices
            .iter()
            .map(|v| PolyVertex {
                pt: t.apply(v.pt),
                bulge: if mirror { -v.bulge } else { v.bulge },
            })
            .collect();
        // Width is geometry and scales by the isotropic determinant factor.
        // Anisotropic scaling of wide straight-only input is approximate.
        let width = if self.width == 0.0 {
            0.0
        } else {
            self.width * t.det().abs().sqrt()
        };
        Ok(Self {
            vertices,
            closed: self.closed,
            width,
        })
    }

    /// Minimum distance to any segment, or `None` beyond tolerance.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        for seg in self.segments() {
            let d = seg.distance_to(p);
            best = Some(best.map_or(d, |b: f64| b.min(d)));
        }
        best.filter(|&d| d <= tol)
    }

    /// Vertex endpoints, segment midpoints, and arc centers.
    fn snap_points(&self) -> SnapVec {
        let mut out = SnapVec::new();
        for v in &self.vertices {
            out.push(SnapPoint::new(v.pt, SnapKind::Endpoint));
        }
        for seg in self.segments() {
            out.push(SnapPoint::new(seg.midpoint(), SnapKind::Midpoint));
            if let SegKind::Arc(arc) = seg {
                out.push(SnapPoint::new(arc.center, SnapKind::Center));
            }
        }
        out
    }

    /// Validates at least two finite, noncoincident consecutive vertices.
    ///
    /// # Errors
    /// Returns the corresponding [`GeomIssue`] for count, finiteness, or coincidence.
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        if self.vertices.len() < 2 {
            return Err(GeomIssue::TooFewVertices);
        }
        if !self.width.is_finite() {
            return Err(GeomIssue::NonFinite);
        }
        for v in &self.vertices {
            if !v.pt.x.is_finite() || !v.pt.y.is_finite() || !v.bulge.is_finite() {
                return Err(GeomIssue::NonFinite);
            }
        }
        for pair in self.vertices.windows(2) {
            if tol.points_coincide(pair[0].pt, pair[1].pt) {
                return Err(GeomIssue::CoincidentVertices);
            }
        }
        if self.closed {
            // The closing segment also makes final and first vertices consecutive.
            let first = self.vertices[0].pt;
            let last = self.vertices[self.vertices.len() - 1].pt;
            if tol.points_coincide(first, last) {
                return Err(GeomIssue::CoincidentVertices);
            }
        }
        Ok(())
    }
}

// Geometry helpers.

impl PolylineGeo {
    /// Iterates resolved segments.
    ///
    /// Open polylines yield `N-1` segments and closed ones `N`. Invalid/near-zero
    /// arc definitions fall back to [`SegKind::Line`].
    pub fn segments(&self) -> impl Iterator<Item = SegKind> + '_ {
        let n = self.vertices.len();
        let count = if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        };
        (0..count).map(move |i| {
            let a = self.vertices[i].pt;
            let b = self.vertices[(i + 1) % n].pt;
            match bulge_to_arc(a, b, self.vertices[i].bulge) {
                Ok(arc) => SegKind::Arc(arc),
                Err(_) => SegKind::Line { a, b },
            }
        })
    }

    /// Total segment length.
    #[must_use]
    pub fn length(&self) -> f64 {
        self.segments().map(SegKind::length).sum()
    }

    /// Whether explicitly closed or endpoints coincide within tolerance.
    #[must_use]
    pub fn is_closed_effective(&self) -> bool {
        if self.closed {
            return true;
        }
        if self.vertices.len() < 2 {
            return false;
        }
        let first = self.vertices[0].pt;
        let last = self.vertices[self.vertices.len() - 1].pt;
        Tol::default().points_coincide(first, last)
    }
}

impl SegKind {
    /// Tight segment box including arc quadrants.
    #[must_use]
    pub fn bbox(self) -> BBox {
        match self {
            SegKind::Line { a, b } => BBox::new(a, b),
            SegKind::Arc(arc) => arc.bbox(),
        }
    }

    /// Segment midpoint.
    #[must_use]
    pub fn midpoint(self) -> Point2 {
        match self {
            SegKind::Line { a, b } => a.midpoint(b),
            SegKind::Arc(arc) => arc.midpoint(),
        }
    }

    /// Segment length.
    #[must_use]
    pub fn length(self) -> f64 {
        match self {
            SegKind::Line { a, b } => a.dist(b),
            SegKind::Arc(arc) => arc.length(),
        }
    }

    /// Euclidean distance from `p` to the segment geometry.
    #[must_use]
    pub fn distance_to(self, p: Point2) -> f64 {
        match self {
            SegKind::Line { a, b } => dist_point_segment(p, a, b),
            SegKind::Arc(arc) => arc.distance_to(p),
        }
    }
}

/// Euclidean distance from `p` to segment `[a, b]`.
fn dist_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let ab: Vec2 = b - a;
    let len_sq = ab.norm_sq();
    if len_sq <= 0.0 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

// Unit tests.

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_8, PI};

    use crate::entity::EntityGeometry;

    fn v(x: f64, y: f64, bulge: f64) -> PolyVertex {
        PolyVertex::new(Point2::new(x, y), bulge)
    }

    /// Straight square, open or closed.
    fn square(closed: bool) -> PolylineGeo {
        PolylineGeo::new(
            vec![
                v(0.0, 0.0, 0.0),
                v(10.0, 0.0, 0.0),
                v(10.0, 10.0, 0.0),
                v(0.0, 10.0, 0.0),
            ],
            closed,
        )
    }

    #[test]
    fn segments_abierta_produce_n_menos_1() {
        assert_eq!(square(false).segments().count(), 3);
    }

    #[test]
    fn segments_cerrada_produce_n() {
        assert_eq!(square(true).segments().count(), 4);
        // Final segment closes the square.
        let segs: Vec<_> = square(true).segments().collect();
        assert_eq!(
            segs[3],
            SegKind::Line {
                a: Point2::new(0.0, 10.0),
                b: Point2::new(0.0, 0.0)
            }
        );
    }

    #[test]
    fn bulge_del_ultimo_vertice_abierta_se_ignora() {
        // Inputs differ only in the unused last bulge of an open polyline.
        let con_bulge = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 1.0)],
            false,
        );
        let sin_bulge = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0)],
            false,
        );
        let a: Vec<_> = con_bulge.segments().collect();
        let b: Vec<_> = sin_bulge.segments().collect();
        assert_eq!(
            a, b,
            "el bulge del último vértice de una abierta no debe afectar"
        );
    }

    #[test]
    fn bulge_del_ultimo_vertice_cerrada_si_se_usa() {
        // In a closed polyline, the last bulge defines the closing segment.
        let poly = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(2.0, 2.0, 1.0)],
            true,
        );
        let segs: Vec<_> = poly.segments().collect();
        assert!(
            matches!(segs[2], SegKind::Arc(_)),
            "el tramo de cierre es un arco"
        );
    }

    #[test]
    fn bbox_tramo_semicirculo_no_es_la_cuerda() {
        // Bulge 1 forms a semicircle on a horizontal chord.
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 0.0)], false);
        let bb = poly.bbox();
        // The arc extends below the zero-height chord.
        assert!(
            bb.height() > 0.9,
            "bbox del arco debe superar a la de la cuerda"
        );
        assert!((bb.min.y + 1.0).abs() < 1e-9);
        assert!(bb.max.y.abs() < 1e-9);
    }

    #[test]
    fn bbox_contiene_centro_de_arco_menor() {
        // A minor arc's center lies outside the sweep but remains inside the box.
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, FRAC_PI_8.tan()), v(2.0, 0.0, 0.0)], false);
        let bb = poly.bbox();
        let arc = match poly.segments().next().unwrap() {
            SegKind::Arc(a) => a,
            SegKind::Line { .. } => panic!("se esperaba un arco"),
        };
        assert!(bb.expand(1e-9).contains_point(arc.center));
        // Every snap lies in the bounding box.
        for s in poly.snap_points() {
            assert!(
                bb.expand(1e-9).contains_point(s.point),
                "snap {:?} fuera",
                s.point
            );
        }
    }

    #[test]
    fn hit_sobre_tramo_curvo() {
        // Lower unit semicircle centered at `(1, 0)`.
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 0.0)], false);
        assert!(poly.hit(Point2::new(1.0, -1.0), 1e-6).unwrap() < 1e-9);
        // The center is not on the curve.
        assert_eq!(poly.hit(Point2::new(1.0, 0.0), 1e-6), None);
        // A distant point does not hit.
        assert_eq!(poly.hit(Point2::new(50.0, 50.0), 1e-6), None);
    }

    #[test]
    fn hit_sobre_tramo_recto() {
        let poly = square(false);
        assert_eq!(poly.hit(Point2::new(5.0, 0.0), 1e-6), Some(0.0));
        assert_eq!(poly.hit(Point2::new(10.0, 5.0), 1e-6), Some(0.0));
    }

    #[test]
    fn snaps_cuenta_endpoints_midpoints_y_centros() {
        // One arc segment yields endpoints, midpoint, and center.
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 0.0)], false);
        let snaps = poly.snap_points();
        let n_center = snaps.iter().filter(|s| s.kind == SnapKind::Center).count();
        let n_mid = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Midpoint)
            .count();
        let n_end = snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Endpoint)
            .count();
        assert_eq!((n_end, n_mid, n_center), (2, 1, 1));
        // An open straight square has four endpoints and three midpoints.
        let sq = square(false).snap_points();
        assert_eq!(sq.iter().filter(|s| s.kind == SnapKind::Center).count(), 0);
        assert_eq!(
            sq.iter().filter(|s| s.kind == SnapKind::Endpoint).count(),
            4
        );
        assert_eq!(
            sq.iter().filter(|s| s.kind == SnapKind::Midpoint).count(),
            3
        );
    }

    #[test]
    fn mirror_invierte_el_signo_del_bulge() {
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 0.6), v(4.0, 0.0, -0.3)], false);
        // Reflect across the Y axis.
        let m = poly.transform(&Transform2::scale(-1.0, 1.0)).unwrap();
        assert_eq!(m.vertices[0].bulge, -0.6);
        assert_eq!(m.vertices[1].bulge, 0.3);
    }

    #[test]
    fn mirror_dos_veces_es_identidad() {
        let poly = PolylineGeo::new(
            vec![v(1.0, 2.0, 0.7), v(4.0, -1.0, -0.9), v(0.0, 5.0, 1.3)],
            true,
        );
        let mirror = Transform2::scale(-1.0, 1.0);
        let twice = poly.transform(&mirror).unwrap().transform(&mirror).unwrap();
        assert_eq!(twice, poly);
    }

    #[test]
    fn escala_no_uniforme_con_bulge_es_err() {
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 0.0)], false);
        assert_eq!(
            poly.transform(&Transform2::scale(2.0, 3.0)),
            Err(TransformError::NonUniformScaleUnsupported)
        );
    }

    #[test]
    fn escala_no_uniforme_sin_bulge_es_ok() {
        // Anisotropic scale is exact for straight-only polylines.
        let poly = square(true);
        let m = poly.transform(&Transform2::scale(2.0, 3.0)).unwrap();
        assert_eq!(m.vertices[1].pt, Point2::new(20.0, 0.0));
        assert_eq!(m.vertices[2].pt, Point2::new(20.0, 30.0));
    }

    #[test]
    fn escala_uniforme_conserva_magnitud_de_bulge() {
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 0.8), v(4.0, 0.0, 0.0)], false);
        let m = poly.transform(&Transform2::scale(3.0, 3.0)).unwrap();
        assert_eq!(m.vertices[0].bulge, 0.8);
    }

    #[test]
    fn validate_ok_y_casos_invalidos() {
        let tol = Tol::default();
        assert!(square(true).validate(&tol).is_ok());
        // Fewer than two vertices.
        let one = PolylineGeo::new(vec![v(0.0, 0.0, 0.0)], false);
        assert_eq!(one.validate(&tol), Err(GeomIssue::TooFewVertices));
        // Nonfinite value.
        let nan = PolylineGeo::new(vec![v(0.0, 0.0, 0.0), v(f64::NAN, 0.0, 0.0)], false);
        assert_eq!(nan.validate(&tol), Err(GeomIssue::NonFinite));
        let nan_bulge = PolylineGeo::new(vec![v(0.0, 0.0, f64::INFINITY), v(1.0, 0.0, 0.0)], false);
        assert_eq!(nan_bulge.validate(&tol), Err(GeomIssue::NonFinite));
        // Coincident consecutive vertices.
        let dup = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0)],
            false,
        );
        assert_eq!(dup.validate(&tol), Err(GeomIssue::CoincidentVertices));
        // Closed polyline with coincident endpoints.
        let closed_dup = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(0.0, 0.0, 0.0)],
            true,
        );
        assert_eq!(
            closed_dup.validate(&tol),
            Err(GeomIssue::CoincidentVertices)
        );
        // Open coincident endpoints are not consecutive and remain valid.
        let open_ends = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(0.0, 0.0, 0.0)],
            false,
        );
        assert!(open_ends.validate(&tol).is_ok());
    }

    #[test]
    fn length_recto_y_arco() {
        // Open square has three length-10 sides.
        assert!((square(false).length() - 30.0).abs() < 1e-9);
        // Unit semicircle length is π.
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 0.0)], false);
        assert!((poly.length() - PI).abs() < 1e-9);
    }

    #[test]
    fn is_closed_effective() {
        assert!(square(true).is_closed_effective());
        assert!(!square(false).is_closed_effective());
        // An open polyline with coincident endpoints is effectively closed.
        let ring = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(5.0, 0.0, 0.0), v(0.0, 0.0, 0.0)],
            false,
        );
        assert!(ring.is_closed_effective());
    }

    #[test]
    fn serde_string_exacto_y_roundtrip() {
        let geo = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.4142)],
            true,
        ));
        let json = serde_json::to_string(&geo).unwrap();
        assert_eq!(
            json,
            r#"{"type":"polyline","vertices":[{"pt":[0.0,0.0],"bulge":0.0},{"pt":[10.0,0.0],"bulge":0.4142}],"closed":true}"#
        );
        let back: EntityGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, geo);
    }

    #[test]
    fn width_cero_se_omite_pero_nonzero_se_serializa() {
        // Zero width preserves legacy JSON.
        let fina = PolylineGeo::new(vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)], false);
        let json = serde_json::to_string(&fina).unwrap();
        assert_eq!(
            json,
            r#"{"vertices":[{"pt":[0.0,0.0],"bulge":0.0},{"pt":[1.0,0.0],"bulge":0.0}],"closed":false}"#
        );
        // Nonzero width serializes and round-trips exactly.
        let ancha = fina.clone().with_width(0.5);
        let json = serde_json::to_string(&ancha).unwrap();
        assert_eq!(
            json,
            r#"{"vertices":[{"pt":[0.0,0.0],"bulge":0.0},{"pt":[1.0,0.0],"bulge":0.0}],"closed":false,"width":0.5}"#
        );
        assert_eq!(serde_json::from_str::<PolylineGeo>(&json).unwrap(), ancha);
        // Missing width deserializes to zero.
        let back: PolylineGeo = serde_json::from_str(
            r#"{"vertices":[{"pt":[0.0,0.0],"bulge":0.0},{"pt":[1.0,0.0],"bulge":0.0}],"closed":false}"#,
        )
        .unwrap();
        assert_eq!(back.width, 0.0);
    }

    #[test]
    fn transform_escala_el_grosor_de_forma_isotropa() {
        // Uniform scale multiplies geometric width.
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 1.0)], true).with_width(0.4);
        let m = poly.transform(&Transform2::scale(3.0, 3.0)).unwrap();
        assert!((m.width - 1.2).abs() < 1e-12);
        // Translation preserves width.
        let t = poly
            .transform(&Transform2::translate(Vec2::new(5.0, -2.0)))
            .unwrap();
        assert_eq!(t.width, 0.4);
        // Reflection preserves width magnitude.
        let mir = poly.transform(&Transform2::scale(-1.0, 1.0)).unwrap();
        assert!((mir.width - 0.4).abs() < 1e-12);
    }

    #[test]
    fn validate_rechaza_grosor_no_finito() {
        let poly = PolylineGeo::new(vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)], false)
            .with_width(f64::INFINITY);
        assert_eq!(poly.validate(&Tol::default()), Err(GeomIssue::NonFinite));
    }

    /// Deserialization preserves coincident vertices for validation to report.
    #[test]
    fn deserializacion_no_normaliza() {
        let json = r#"{"type":"polyline","vertices":[{"pt":[0.0,0.0],"bulge":0.0},{"pt":[0.0,0.0],"bulge":0.0},{"pt":[1.0,1.0],"bulge":0.0}],"closed":false}"#;
        let geo: EntityGeometry = serde_json::from_str(json).unwrap();
        let EntityGeometry::Polyline(p) = &geo else {
            panic!("se esperaba polyline");
        };
        assert_eq!(
            p.vertices.len(),
            3,
            "no debe fusionar vértices coincidentes"
        );
        assert_eq!(
            p.validate(&Tol::default()),
            Err(GeomIssue::CoincidentVertices)
        );
    }

    // Property: bounding boxes contain snap points.

    use proptest::prelude::*;

    prop_compose! {
        fn arb_polyline()(
            n in 2usize..=5,
        )(
            xs in prop::collection::vec(-100.0f64..100.0, n),
            ys in prop::collection::vec(-100.0f64..100.0, n),
            bulges in prop::collection::vec(prop_oneof![Just(0.0f64), -1.5f64..1.5], n),
            closed in any::<bool>(),
        ) -> PolylineGeo {
            // Separate vertices in X so generated geometry always validates.
            let verts = xs
                .iter()
                .zip(ys.iter())
                .zip(bulges.iter())
                .enumerate()
                .map(|(i, ((x, y), b))| PolyVertex::new(Point2::new(x + i as f64 * 300.0, *y), *b))
                .collect();
            PolylineGeo::new(verts, closed)
        }
    }

    proptest! {
        #[test]
        fn prop_bbox_contiene_snaps(poly in arb_polyline()) {
            let bb = poly.bbox();
            for s in poly.snap_points() {
                prop_assert!(bb.expand(1e-6).contains_point(s.point), "snap {:?} fuera de {bb:?}", s.point);
            }
        }

        #[test]
        fn prop_valida_ok(poly in arb_polyline()) {
            prop_assert!(poly.validate(&Tol::default()).is_ok());
        }
    }
}
