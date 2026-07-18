//! 2D point anchored in drawing space.

use core::ops::{Add, AddAssign, Sub, SubAssign};

use crate::Vec2;

/// 2D `f64` point anchored to an origin, unlike [`Vec2`].
///
/// # Serialization
/// `Point2` serializes exactly as tuple `[x, y]`, not an object.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point2 {
    pub x: f64,
    pub y: f64,
}

impl Point2 {
    /// Origin `(0, 0)`.
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    /// Creates a point from its coordinates.
    #[inline]
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Returns the position vector from the origin.
    #[inline]
    #[must_use]
    pub fn to_vec(self) -> Vec2 {
        Vec2 {
            x: self.x,
            y: self.y,
        }
    }

    /// Returns Euclidean distance to `other`.
    #[inline]
    #[must_use]
    pub fn dist(self, other: Self) -> f64 {
        (self - other).norm()
    }

    /// Returns squared distance to `other`.
    #[inline]
    #[must_use]
    pub fn dist_sq(self, other: Self) -> f64 {
        (self - other).norm_sq()
    }

    /// Returns the midpoint between `self` and `other`.
    #[inline]
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        self.lerp(other, 0.5)
    }

    /// Linearly interpolates from `self` at `t = 0` to `other` at `t = 1`.
    #[inline]
    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        self + (other - self) * t
    }
}

impl Sub for Point2 {
    type Output = Vec2;
    /// Returns displacement `self − other`.
    #[inline]
    fn sub(self, rhs: Self) -> Vec2 {
        Vec2 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Add<Vec2> for Point2 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Vec2) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub<Vec2> for Point2 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Vec2) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl AddAssign<Vec2> for Point2 {
    #[inline]
    fn add_assign(&mut self, rhs: Vec2) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl SubAssign<Vec2> for Point2 {
    #[inline]
    fn sub_assign(&mut self, rhs: Vec2) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

// --- Serde uses exact tuple `[x, y]`, gated by the `serde` feature. ---

#[cfg(feature = "serde")]
impl serde::Serialize for Point2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        [self.x, self.y].serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Point2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let [x, y] = <[f64; 2]>::deserialize(deserializer)?;
        Ok(Self { x, y })
    }
}
