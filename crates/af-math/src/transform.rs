//! 2D affine transform using a `2×3` matrix.

use crate::{MathError, Point2, Tol, Vec2};

/// 2D affine transform represented by `[[a, b, tx], [c, d, ty]]`.
///
/// Applied as `p' = M · p`: `x' = a·x + b·y + tx`, `y' = c·x + d·y + ty`.
/// Vectors use only the linear part.
///
/// [`Transform2::then`] applies `self` first and `other` second.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Transform2 {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Transform2 {
    #[inline]
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform2 {
    /// Identity transform.
    #[inline]
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Creates a transform from its six matrix coefficients.
    #[inline]
    #[must_use]
    pub const fn from_rows(a: f64, b: f64, tx: f64, c: f64, d: f64, ty: f64) -> Self {
        Self { a, b, c, d, tx, ty }
    }

    /// Pure translation by `v`.
    #[inline]
    #[must_use]
    pub const fn translate(v: Vec2) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: v.x,
            ty: v.y,
        }
    }

    /// Counterclockwise rotation by `rad` around the origin.
    #[inline]
    #[must_use]
    pub fn rotate(rad: f64) -> Self {
        let (s, co) = rad.sin_cos();
        Self {
            a: co,
            b: -s,
            c: s,
            d: co,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Counterclockwise rotation by `rad` around `pivot`.
    #[inline]
    #[must_use]
    pub fn rotate_about(rad: f64, pivot: Point2) -> Self {
        let to_origin = Self::translate(-pivot.to_vec());
        let back = Self::translate(pivot.to_vec());
        to_origin.then(Self::rotate(rad)).then(back)
    }

    /// Non-uniform `(sx, sy)` scaling about the origin.
    #[inline]
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// `(sx, sy)` scaling about `pivot`.
    #[inline]
    #[must_use]
    pub fn scale_about(sx: f64, sy: f64, pivot: Point2) -> Self {
        let to_origin = Self::translate(-pivot.to_vec());
        let back = Self::translate(pivot.to_vec());
        to_origin.then(Self::scale(sx, sy)).then(back)
    }

    /// Reflection across the line through `p1` and `p2`, with determinant `-1`.
    ///
    /// # Errors
    /// Returns [`MathError::ZeroVector`] when the axis points are too close.
    #[inline]
    pub fn reflect_about_line(p1: Point2, p2: Point2) -> Result<Self, MathError> {
        let u = (p2 - p1).normalize()?;
        // Reflection about an origin line with unit direction `u = (ux, uy)`.
        let (ux, uy) = (u.x, u.y);
        let a = ux * ux - uy * uy;
        let b = 2.0 * ux * uy;
        let linear = Self::from_rows(a, b, 0.0, b, -a, 0.0);
        let to_origin = Self::translate(-p1.to_vec());
        let back = Self::translate(p1.to_vec());
        Ok(to_origin.then(linear).then(back))
    }

    /// Composes by applying `self` first and `other` second.
    ///
    /// Equivalent to matrix `other · self`, so
    /// `a.then(b).apply(p) == b.apply(a.apply(p))`.
    #[inline]
    #[must_use]
    pub fn then(self, other: Self) -> Self {
        // other · self
        Self {
            a: other.a * self.a + other.b * self.c,
            b: other.a * self.b + other.b * self.d,
            c: other.c * self.a + other.d * self.c,
            d: other.c * self.b + other.d * self.d,
            tx: other.a * self.tx + other.b * self.ty + other.tx,
            ty: other.c * self.tx + other.d * self.ty + other.ty,
        }
    }

    /// Applies the transform to a point, including translation.
    #[inline]
    #[must_use]
    pub fn apply(self, p: Point2) -> Point2 {
        Point2 {
            x: self.a * p.x + self.b * p.y + self.tx,
            y: self.c * p.x + self.d * p.y + self.ty,
        }
    }

    /// Applies only the linear part to a vector.
    #[inline]
    #[must_use]
    pub fn apply_vec(self, v: Vec2) -> Vec2 {
        Vec2 {
            x: self.a * v.x + self.b * v.y,
            y: self.c * v.x + self.d * v.y,
        }
    }

    /// Determinant of the linear part, `a·d − b·c`.
    #[inline]
    #[must_use]
    pub fn det(self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    /// Returns the inverse transform.
    ///
    /// # Errors
    /// Returns [`MathError::Singular`] when the determinant is within tolerance.
    #[inline]
    pub fn invert(self) -> Result<Self, MathError> {
        self.invert_eps(Tol::default().linear)
    }

    /// Like [`Transform2::invert`] with explicit determinant threshold `eps`.
    ///
    /// # Errors
    /// Returns [`MathError::Singular`] when `|det()| <= eps`.
    #[inline]
    pub fn invert_eps(self, eps: f64) -> Result<Self, MathError> {
        let det = self.det();
        if det.abs() <= eps {
            return Err(MathError::Singular);
        }
        let inv_det = 1.0 / det;
        let a = self.d * inv_det;
        let b = -self.b * inv_det;
        let c = -self.c * inv_det;
        let d = self.a * inv_det;
        // Inverse translation: -(L⁻¹ · t).
        let tx = -(a * self.tx + b * self.ty);
        let ty = -(c * self.tx + d * self.ty);
        Ok(Self { a, b, c, d, tx, ty })
    }

    /// Scale factors along each axis: `(‖X column‖, ‖Y column‖)`.
    #[inline]
    #[must_use]
    pub fn scale_factors(self) -> (f64, f64) {
        (self.a.hypot(self.c), self.b.hypot(self.d))
    }

    /// Returns whether scale is uniform within `tol`.
    #[inline]
    #[must_use]
    pub fn is_uniform(self, tol: &Tol) -> bool {
        let (sx, sy) = self.scale_factors();
        tol.approx_eq(sx, sy)
    }

    /// Returns whether the transform reverses orientation.
    #[inline]
    #[must_use]
    pub fn is_mirroring(self) -> bool {
        self.det() < 0.0
    }
}
