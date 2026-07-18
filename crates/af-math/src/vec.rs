//! Free 2D vector representing displacement or direction.

use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use crate::{MathError, Tol};

/// 2D `f64` displacement or direction, not an anchored point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    /// Zero vector `(0, 0)`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    /// `+X` basis vector `(1, 0)`.
    pub const X: Self = Self { x: 1.0, y: 0.0 };
    /// `+Y` basis vector `(0, 1)`.
    pub const Y: Self = Self { x: 0.0, y: 1.0 };

    /// Creates a vector from its components.
    #[inline]
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Dot product `self · other`.
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Scalar 2D cross product: `self.x·other.y − self.y·other.x`.
    ///
    /// Positive when `other` lies counterclockwise from `self`.
    #[inline]
    #[must_use]
    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// Perpendicular vector rotated 90° counterclockwise:
    /// `(x, y) → (−y, x)`.
    #[inline]
    #[must_use]
    pub fn perp(self) -> Self {
        Self {
            x: -self.y,
            y: self.x,
        }
    }

    /// Euclidean length.
    #[inline]
    #[must_use]
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }

    /// Squared length, avoiding a square root.
    #[inline]
    #[must_use]
    pub fn norm_sq(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// Returns a unit vector in the same direction.
    ///
    /// # Errors
    /// Returns [`MathError::ZeroVector`] when the norm is within default tolerance.
    #[inline]
    pub fn normalize(self) -> Result<Self, MathError> {
        self.normalize_eps(Tol::default().linear)
    }

    /// Like [`Vec2::normalize`] with explicit norm threshold `eps`.
    ///
    /// # Errors
    /// Returns [`MathError::ZeroVector`] when `norm() <= eps`.
    #[inline]
    pub fn normalize_eps(self, eps: f64) -> Result<Self, MathError> {
        let n = self.norm();
        if n <= eps {
            return Err(MathError::ZeroVector);
        }
        Ok(Self {
            x: self.x / n,
            y: self.y / n,
        })
    }
}

impl Add for Vec2 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Vec2 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Neg for Vec2 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl Mul<f64> for Vec2 {
    type Output = Self;
    #[inline]
    fn mul(self, s: f64) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
        }
    }
}

impl Mul<Vec2> for f64 {
    type Output = Vec2;
    #[inline]
    fn mul(self, v: Vec2) -> Vec2 {
        Vec2 {
            x: self * v.x,
            y: self * v.y,
        }
    }
}

impl Div<f64> for Vec2 {
    type Output = Self;
    #[inline]
    fn div(self, s: f64) -> Self {
        Self {
            x: self.x / s,
            y: self.y / s,
        }
    }
}

impl AddAssign for Vec2 {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl SubAssign for Vec2 {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl MulAssign<f64> for Vec2 {
    #[inline]
    fn mul_assign(&mut self, s: f64) {
        self.x *= s;
        self.y *= s;
    }
}

impl DivAssign<f64> for Vec2 {
    #[inline]
    fn div_assign(&mut self, s: f64) {
        self.x /= s;
        self.y /= s;
    }
}
