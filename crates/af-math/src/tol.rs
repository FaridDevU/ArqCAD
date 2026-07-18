//! Canonical tolerances and approximate comparisons.
//!
//! [`Tol`] is the single source of default tolerances for geometry logic.

use crate::Point2;

/// Absolute tolerances in drawing units and radians.
///
/// Canonical defaults:
/// `linear = 1e-8`, `point_merge = 1e-6`, `angle = 1e-9`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tol {
    /// General linear tolerance for lengths, determinants, and scalars.
    pub linear: f64,
    /// Distance below which points are considered coincident.
    pub point_merge: f64,
    /// Angular tolerance in radians.
    pub angle: f64,
}

impl Default for Tol {
    #[inline]
    fn default() -> Self {
        Self {
            linear: 1e-8,
            point_merge: 1e-6,
            angle: 1e-9,
        }
    }
}

impl Tol {
    /// Creates explicit tolerance values.
    #[inline]
    #[must_use]
    pub const fn new(linear: f64, point_merge: f64, angle: f64) -> Self {
        Self {
            linear,
            point_merge,
            angle,
        }
    }

    /// Returns `true` when `a` and `b` differ by at most `linear`.
    #[inline]
    #[must_use]
    pub fn approx_eq(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.linear
    }

    /// Returns whether `x` is within `linear` of zero.
    #[inline]
    #[must_use]
    pub fn approx_zero(&self, x: f64) -> bool {
        x.abs() <= self.linear
    }

    /// Returns whether `p` and `q` are within `point_merge` distance.
    #[inline]
    #[must_use]
    pub fn points_coincide(&self, p: Point2, q: Point2) -> bool {
        p.dist(q) <= self.point_merge
    }

    /// Returns whether `a` and `b` represent the same wrapped angle.
    #[inline]
    #[must_use]
    pub fn angles_eq(&self, a: f64, b: f64) -> bool {
        crate::angle::angular_gap(a, b) <= self.angle
    }
}
