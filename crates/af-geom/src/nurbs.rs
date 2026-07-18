//! C2 cubic interpolation through spline fit points.

use af_math::{BBox, Point2, Vec2};

// ponytail: Cap adaptive subdivision depth to bound memory.
const MAX_FLATTEN_DEPTH: u32 = 12;

const NEAREST_SAMPLES_PER_SEG: usize = 24;

const BBOX_SAFETY_SAMPLES: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct FitSpline {
    knots: Vec<f64>,
    pts: Vec<Point2>,
    mx: Vec<f64>,
    my: Vec<f64>,
}

impl FitSpline {
    #[must_use]
    pub fn from_fit_points(fit: &[Point2], closed: bool) -> Option<Self> {
        if closed {
            if fit.len() < 3 {
                return None;
            }
        } else if fit.len() < 2 {
            return None;
        }
        for p in fit {
            if !p.x.is_finite() || !p.y.is_finite() {
                return None;
            }
        }

        let mut pts: Vec<Point2> = fit.to_vec();
        if closed {
            pts.push(fit[0]);
        }

        let mut knots = Vec::with_capacity(pts.len());
        knots.push(0.0);
        for i in 1..pts.len() {
            let h = pts[i].dist(pts[i - 1]);
            if h <= 0.0 {
                return None;
            }
            knots.push(knots[i - 1] + h);
        }

        let h: Vec<f64> = (0..pts.len() - 1)
            .map(|i| knots[i + 1] - knots[i])
            .collect();
        let px: Vec<f64> = pts.iter().map(|p| p.x).collect();
        let py: Vec<f64> = pts.iter().map(|p| p.y).collect();

        let (mx, my) = if closed {
            (periodic_moments(&h, &px)?, periodic_moments(&h, &py)?)
        } else {
            (natural_moments(&h, &px), natural_moments(&h, &py))
        };

        Some(Self { knots, pts, mx, my })
    }

    #[inline]
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.pts.len() - 1
    }

    #[inline]
    #[must_use]
    pub fn param_range(&self) -> (f64, f64) {
        (self.knots[0], self.knots[self.knots.len() - 1])
    }

    fn seg_index(&self, t: f64) -> usize {
        let last = self.segment_count() - 1;
        if t <= self.knots[0] {
            return 0;
        }
        if t >= self.knots[last + 1] {
            return last;
        }
        match self
            .knots
            .binary_search_by(|k| k.partial_cmp(&t).unwrap_or(core::cmp::Ordering::Less))
        {
            Ok(i) => i.min(last),
            Err(i) => i - 1,
        }
    }

    #[must_use]
    pub fn eval(&self, t: f64) -> Point2 {
        let i = self.seg_index(t);
        let (a, b, h) = self.frac(i, t);
        Point2::new(
            hermite(
                a,
                b,
                h,
                self.pts[i].x,
                self.pts[i + 1].x,
                self.mx[i],
                self.mx[i + 1],
            ),
            hermite(
                a,
                b,
                h,
                self.pts[i].y,
                self.pts[i + 1].y,
                self.my[i],
                self.my[i + 1],
            ),
        )
    }

    #[must_use]
    pub fn deriv(&self, t: f64) -> Vec2 {
        let i = self.seg_index(t);
        let (a, b, h) = self.frac(i, t);
        Vec2::new(
            hermite_d(
                a,
                b,
                h,
                self.pts[i].x,
                self.pts[i + 1].x,
                self.mx[i],
                self.mx[i + 1],
            ),
            hermite_d(
                a,
                b,
                h,
                self.pts[i].y,
                self.pts[i + 1].y,
                self.my[i],
                self.my[i + 1],
            ),
        )
    }

    fn deriv2(&self, t: f64) -> Vec2 {
        let i = self.seg_index(t);
        let (a, b, _h) = self.frac(i, t);
        Vec2::new(
            a * self.mx[i] + b * self.mx[i + 1],
            a * self.my[i] + b * self.my[i + 1],
        )
    }

    #[inline]
    fn frac(&self, i: usize, t: f64) -> (f64, f64, f64) {
        let h = self.knots[i + 1] - self.knots[i];
        let tc = t.clamp(self.knots[i], self.knots[i + 1]);
        let b = (tc - self.knots[i]) / h;
        (1.0 - b, b, h)
    }

    #[must_use]
    pub fn flatten(&self, chord_err: f64) -> Vec<Point2> {
        let err = if chord_err.is_finite() && chord_err > 0.0 {
            chord_err
        } else {
            f64::MIN_POSITIVE
        };
        let mut out = vec![self.pts[0]];
        for i in 0..self.segment_count() {
            self.flatten_seg(i, self.knots[i], self.knots[i + 1], err, 0, &mut out);
        }
        out
    }

    fn flatten_seg(&self, i: usize, ta: f64, tb: f64, err: f64, depth: u32, out: &mut Vec<Point2>) {
        let pa = self.eval_seg(i, ta);
        let pb = self.eval_seg(i, tb);
        let tm = 0.5 * (ta + tb);
        let pm = self.eval_seg(i, tm);
        if depth >= MAX_FLATTEN_DEPTH || dist_point_segment(pm, pa, pb) <= err {
            out.push(pb);
        } else {
            self.flatten_seg(i, ta, tm, err, depth + 1, out);
            self.flatten_seg(i, tm, tb, err, depth + 1, out);
        }
    }

    fn eval_seg(&self, i: usize, t: f64) -> Point2 {
        let (a, b, h) = self.frac(i, t);
        Point2::new(
            hermite(
                a,
                b,
                h,
                self.pts[i].x,
                self.pts[i + 1].x,
                self.mx[i],
                self.mx[i + 1],
            ),
            hermite(
                a,
                b,
                h,
                self.pts[i].y,
                self.pts[i + 1].y,
                self.my[i],
                self.my[i + 1],
            ),
        )
    }

    #[must_use]
    pub fn bbox(&self) -> BBox {
        let mut ts: Vec<f64> = self.knots.clone();
        for i in 0..self.segment_count() {
            let h = self.knots[i + 1] - self.knots[i];
            self.push_extrema(
                i,
                h,
                self.mx[i],
                self.mx[i + 1],
                self.pts[i].x,
                self.pts[i + 1].x,
                &mut ts,
            );
            self.push_extrema(
                i,
                h,
                self.my[i],
                self.my[i + 1],
                self.pts[i].y,
                self.pts[i + 1].y,
                &mut ts,
            );
            for s in 1..=BBOX_SAFETY_SAMPLES {
                ts.push(self.knots[i] + h * (s as f64) / (BBOX_SAFETY_SAMPLES as f64 + 1.0));
            }
        }
        BBox::from_points(ts.iter().map(|&t| self.eval(t)))
            .unwrap_or_else(|| BBox::from_point(self.pts[0]))
    }

    #[allow(clippy::too_many_arguments)]
    fn push_extrema(
        &self,
        i: usize,
        h: f64,
        m0: f64,
        m1: f64,
        p0: f64,
        p1: f64,
        ts: &mut Vec<f64>,
    ) {
        let a = (m1 - m0) / (2.0 * h);
        let b = m0;
        let c = (p1 - p0) / h - h * (2.0 * m0 + m1) / 6.0;
        for tau in quad_roots_in(a, b, c, h) {
            ts.push(self.knots[i] + tau);
        }
    }

    #[must_use]
    pub fn nearest(&self, q: Point2) -> (f64, Point2, f64) {
        let (t0, t1) = self.param_range();
        let samples = (self.segment_count() * NEAREST_SAMPLES_PER_SEG).max(2);
        let step = (t1 - t0) / (samples as f64);
        let mut best_t = t0;
        let mut best_d2 = f64::INFINITY;
        for k in 0..=samples {
            let t = t0 + step * (k as f64);
            let d2 = self.eval(t).dist_sq(q);
            if d2 < best_d2 {
                best_d2 = d2;
                best_t = t;
            }
        }
        let lo = (best_t - step).max(t0);
        let hi = (best_t + step).min(t1);
        let mut t = golden_min(lo, hi, |t| self.eval(t).dist_sq(q));
        for _ in 0..6 {
            let c = self.eval(t);
            let d1 = self.deriv(t);
            let d2 = self.deriv2(t);
            let r = c - q;
            let g1 = r.dot(d1);
            let g2 = d1.dot(d1) + r.dot(d2);
            if g2.abs() <= f64::MIN_POSITIVE {
                break;
            }
            t = (t - g1 / g2).clamp(t0, t1);
        }
        let p = self.eval(t);
        (t, p, p.dist(q))
    }
}

#[inline]
fn hermite(a: f64, b: f64, h: f64, p0: f64, p1: f64, m0: f64, m1: f64) -> f64 {
    a * p0 + b * p1 + (h * h / 6.0) * ((a * a * a - a) * m0 + (b * b * b - b) * m1)
}

#[inline]
fn hermite_d(a: f64, b: f64, h: f64, p0: f64, p1: f64, m0: f64, m1: f64) -> f64 {
    (p1 - p0) / h + (h / 6.0) * (-(3.0 * a * a - 1.0) * m0 + (3.0 * b * b - 1.0) * m1)
}

fn natural_moments(h: &[f64], p: &[f64]) -> Vec<f64> {
    let n = h.len();
    let mut m = vec![0.0; n + 1];
    if n < 2 {
        return m;
    }
    let k = n - 1;
    let mut sub = vec![0.0; k];
    let mut diag = vec![0.0; k];
    let mut sup = vec![0.0; k];
    let mut rhs = vec![0.0; k];
    for j in 0..k {
        let i = j + 1;
        sub[j] = h[i - 1];
        diag[j] = 2.0 * (h[i - 1] + h[i]);
        sup[j] = h[i];
        rhs[j] = 6.0 * ((p[i + 1] - p[i]) / h[i] - (p[i] - p[i - 1]) / h[i - 1]);
    }
    let x = thomas(&sub, &diag, &sup, &rhs);
    m[1..=k].copy_from_slice(&x);
    m
}

fn periodic_moments(h: &[f64], p: &[f64]) -> Option<Vec<f64>> {
    let n = h.len();
    if n < 3 {
        return None;
    }
    let mut sub = vec![0.0; n];
    let mut diag = vec![0.0; n];
    let mut sup = vec![0.0; n];
    let mut rhs = vec![0.0; n];
    for i in 0..n {
        let hp = h[(i + n - 1) % n];
        let hi = h[i];
        sub[i] = hp;
        diag[i] = 2.0 * (hp + hi);
        sup[i] = hi;
        let next = p[i + 1];
        let cur = p[i];
        let prev = if i == 0 { p[n - 1] } else { p[i - 1] };
        rhs[i] = 6.0 * ((next - cur) / hi - (cur - prev) / hp);
    }
    let alpha = sup[n - 1];
    let beta = sub[0];
    let mut m = cyclic_solve(&sub, &diag, &sup, &rhs, alpha, beta)?;
    m.push(m[0]);
    Some(m)
}

fn thomas(sub: &[f64], diag: &[f64], sup: &[f64], rhs: &[f64]) -> Vec<f64> {
    let k = diag.len();
    let mut c = vec![0.0; k];
    let mut d = vec![0.0; k];
    c[0] = sup[0] / diag[0];
    d[0] = rhs[0] / diag[0];
    for i in 1..k {
        let denom = diag[i] - sub[i] * c[i - 1];
        c[i] = sup[i] / denom;
        d[i] = (rhs[i] - sub[i] * d[i - 1]) / denom;
    }
    let mut x = vec![0.0; k];
    x[k - 1] = d[k - 1];
    for i in (0..k - 1).rev() {
        x[i] = d[i] - c[i] * x[i + 1];
    }
    x
}

fn cyclic_solve(
    sub: &[f64],
    diag: &[f64],
    sup: &[f64],
    rhs: &[f64],
    alpha: f64,
    beta: f64,
) -> Option<Vec<f64>> {
    let n = diag.len();
    if n < 2 {
        return None;
    }
    let gamma = -diag[0];
    if gamma == 0.0 {
        return None;
    }
    let mut bb = diag.to_vec();
    bb[0] = diag[0] - gamma;
    bb[n - 1] = diag[n - 1] - alpha * beta / gamma;
    let x = thomas(sub, &bb, sup, rhs);
    let mut u = vec![0.0; n];
    u[0] = gamma;
    u[n - 1] = alpha;
    let z = thomas(sub, &bb, sup, &u);
    let denom = 1.0 + z[0] + beta * z[n - 1] / gamma;
    if denom == 0.0 {
        return None;
    }
    let fact = (x[0] + beta * x[n - 1] / gamma) / denom;
    Some((0..n).map(|i| x[i] - fact * z[i]).collect())
}

fn quad_roots_in(a: f64, b: f64, c: f64, h: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut push = |t: f64| {
        if t > 0.0 && t < h {
            out.push(t);
        }
    };
    if a.abs() <= 1e-14 * (1.0 + b.abs() + c.abs()) {
        if b.abs() > f64::MIN_POSITIVE {
            push(-c / b);
        }
        return out;
    }
    let disc = b * b - 4.0 * a * c;
    if disc >= 0.0 {
        let s = disc.sqrt();
        push((-b + s) / (2.0 * a));
        push((-b - s) / (2.0 * a));
    }
    out
}

fn golden_min(mut lo: f64, mut hi: f64, f: impl Fn(f64) -> f64) -> f64 {
    const INV_PHI: f64 = 0.618_033_988_749_895;
    let mut c = hi - (hi - lo) * INV_PHI;
    let mut d = lo + (hi - lo) * INV_PHI;
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..64 {
        if fc < fd {
            hi = d;
            d = c;
            fd = fc;
            c = hi - (hi - lo) * INV_PHI;
            fc = f(c);
        } else {
            lo = c;
            c = d;
            fc = fd;
            d = lo + (hi - lo) * INV_PHI;
            fd = f(d);
        }
    }
    0.5 * (lo + hi)
}

fn dist_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let ab = b - a;
    let len_sq = ab.norm_sq();
    if len_sq <= 0.0 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts(v: &[(f64, f64)]) -> Vec<Point2> {
        v.iter().map(|&(x, y)| Point2::new(x, y)).collect()
    }

    fn s_curve() -> Vec<Point2> {
        pts(&[(0.0, 0.0), (1.0, 2.0), (3.0, -1.0), (4.0, 1.0), (6.0, 0.0)])
    }

    #[test]
    fn interpola_los_puntos_de_ajuste_abierta() {
        let fit = s_curve();
        let sp = FitSpline::from_fit_points(&fit, false).unwrap();
        for (i, &p) in fit.iter().enumerate() {
            let got = sp.eval(sp.knots[i]);
            assert!(got.dist(p) < 1e-9, "no interpola P{i}: {got:?} vs {p:?}");
        }
    }

    #[test]
    fn dos_puntos_es_recta() {
        let fit = pts(&[(0.0, 0.0), (10.0, 4.0)]);
        let sp = FitSpline::from_fit_points(&fit, false).unwrap();
        let (t0, t1) = sp.param_range();
        let mid = sp.eval(0.5 * (t0 + t1));
        assert!(mid.dist(Point2::new(5.0, 2.0)) < 1e-12);
    }

    #[test]
    fn c1_en_nudos_interiores_abierta() {
        let sp = FitSpline::from_fit_points(&s_curve(), false).unwrap();
        let eps = 1e-6;
        for i in 1..sp.segment_count() {
            let t = sp.knots[i];
            let left = (sp.eval(t) - sp.eval(t - eps)) * (1.0 / eps);
            let right = (sp.eval(t + eps) - sp.eval(t)) * (1.0 / eps);
            assert!(
                (left - right).norm() < 1e-3,
                "discontinuidad C1 en nudo {i}: {left:?} vs {right:?}"
            );
        }
    }

    #[test]
    fn deriv_analitica_coincide_con_diferencia_finita() {
        let sp = FitSpline::from_fit_points(&s_curve(), false).unwrap();
        let (t0, t1) = sp.param_range();
        let eps = 1e-6;
        for k in 1..20 {
            let t = t0 + (t1 - t0) * (k as f64) / 20.0;
            let fd = (sp.eval(t + eps) - sp.eval(t - eps)) * (1.0 / (2.0 * eps));
            let an = sp.deriv(t);
            assert!((fd - an).norm() < 1e-4, "deriv mal en t={t}");
        }
    }

    #[test]
    fn cerrada_interpola_y_es_c1_en_la_costura() {
        let fit = pts(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        let sp = FitSpline::from_fit_points(&fit, true).unwrap();
        for (i, &p) in fit.iter().enumerate() {
            assert!(
                sp.eval(sp.knots[i]).dist(p) < 1e-9,
                "cerrada no interpola P{i}"
            );
        }
        let (t0, t1) = sp.param_range();
        assert!(sp.eval(t1).dist(fit[0]) < 1e-9);
        assert!(sp.eval(t0).dist(fit[0]) < 1e-9);
        let eps = 1e-6;
        let incoming = (sp.eval(t1) - sp.eval(t1 - eps)) * (1.0 / eps);
        let outgoing = (sp.eval(t0 + eps) - sp.eval(t0)) * (1.0 / eps);
        assert!(
            (incoming - outgoing).norm() < 1e-3,
            "costura no es C1: {incoming:?} vs {outgoing:?}"
        );
    }

    #[test]
    fn bbox_contiene_muestras_densas() {
        let sp = FitSpline::from_fit_points(&s_curve(), false).unwrap();
        let bb = sp.bbox();
        let (t0, t1) = sp.param_range();
        for k in 0..=500 {
            let t = t0 + (t1 - t0) * (k as f64) / 500.0;
            let p = sp.eval(t);
            assert!(
                bb.expand(1e-9).contains_point(p),
                "punto {p:?} de la curva fuera de la bbox {bb:?}"
            );
        }
    }

    #[test]
    fn nearest_sobre_la_curva_es_casi_cero() {
        let sp = FitSpline::from_fit_points(&s_curve(), false).unwrap();
        let (t0, t1) = sp.param_range();
        for k in 1..15 {
            let t = t0 + (t1 - t0) * (k as f64) / 15.0;
            let on = sp.eval(t);
            let (_tn, _p, d) = sp.nearest(on);
            assert!(d < 1e-7, "nearest sobre la curva d={d} en t={t}");
        }
    }

    #[test]
    fn nearest_de_punto_lejano_es_grande() {
        let sp = FitSpline::from_fit_points(&s_curve(), false).unwrap();
        let (_t, _p, d) = sp.nearest(Point2::new(1000.0, 1000.0));
        assert!(d > 100.0);
    }

    #[test]
    fn flatten_extremos_exactos_y_dentro_de_cota() {
        let sp = FitSpline::from_fit_points(&s_curve(), false).unwrap();
        let chord = 0.01;
        let poly = sp.flatten(chord);
        assert!(poly.len() >= 2);
        assert_eq!(poly[0], sp.pts[0]);
        assert_eq!(*poly.last().unwrap(), *sp.pts.last().unwrap());
        for w in poly.windows(2) {
            let mid = w[0].midpoint(w[1]);
            let (_t, _p, d) = sp.nearest(mid);
            assert!(d <= chord + 1e-9, "cuerda con error {d} > {chord}");
        }
    }

    #[test]
    fn degenerados_devuelven_none() {
        assert!(FitSpline::from_fit_points(&pts(&[(0.0, 0.0)]), false).is_none());
        assert!(FitSpline::from_fit_points(&pts(&[(0.0, 0.0), (1.0, 0.0)]), true).is_none());
        assert!(
            FitSpline::from_fit_points(&pts(&[(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)]), false)
                .is_none()
        );
        assert!(FitSpline::from_fit_points(&pts(&[(0.0, 0.0), (f64::NAN, 1.0)]), false).is_none());
    }

    #[test]
    fn similaridad_transforma_la_curva_exactamente() {
        let fit = s_curve();
        let sp = FitSpline::from_fit_points(&fit, false).unwrap();
        let moved: Vec<Point2> = fit
            .iter()
            .map(|p| Point2::new(p.x * 2.0 + 5.0, p.y * 2.0 - 3.0))
            .collect();
        let sp2 = FitSpline::from_fit_points(&moved, false).unwrap();
        let (t0, t1) = sp.param_range();
        let (u0, u1) = sp2.param_range();
        for k in 0..=20 {
            let f = (k as f64) / 20.0;
            let p = sp.eval(t0 + (t1 - t0) * f);
            let q = sp2.eval(u0 + (u1 - u0) * f);
            let expect = Point2::new(p.x * 2.0 + 5.0, p.y * 2.0 - 3.0);
            assert!(q.dist(expect) < 1e-7, "similaridad no conmuta en f={f}");
        }
    }
}
