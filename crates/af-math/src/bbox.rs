//! Axis-aligned bounding box (AABB).

use crate::{Point2, Tol, Vec2};

/// Axis-aligned bounding box.
///
/// [`BBox::new`] normalizes corners so `min <= max` on both axes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BBox {
    pub min: Point2,
    pub max: Point2,
}

impl BBox {
    /// Builds a normalized box from any two corners.
    #[inline]
    #[must_use]
    pub fn new(a: Point2, b: Point2) -> Self {
        Self {
            min: Point2::new(a.x.min(b.x), a.y.min(b.y)),
            max: Point2::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    /// Degenerate box containing one point.
    #[inline]
    #[must_use]
    pub fn from_point(p: Point2) -> Self {
        Self { min: p, max: p }
    }

    /// Returns the smallest box containing every input point.
    ///
    /// Returns `None` for an empty iterator.
    #[must_use]
    pub fn from_points<I>(points: I) -> Option<Self>
    where
        I: IntoIterator<Item = Point2>,
    {
        let mut iter = points.into_iter();
        let first = iter.next()?;
        let mut bb = Self::from_point(first);
        for p in iter {
            bb = bb.union_point(p);
        }
        Some(bb)
    }

    /// Returns the smallest box containing `self` and `other`.
    #[inline]
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Point2::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Point2::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// Returns the smallest box containing `self` and point `p`.
    #[inline]
    #[must_use]
    pub fn union_point(self, p: Point2) -> Self {
        Self {
            min: Point2::new(self.min.x.min(p.x), self.min.y.min(p.y)),
            max: Point2::new(self.max.x.max(p.x), self.max.y.max(p.y)),
        }
    }

    /// Expands each side by `margin`, or contracts it when negative, then normalizes.
    #[inline]
    #[must_use]
    pub fn expand(self, margin: f64) -> Self {
        let m = Vec2::new(margin, margin);
        Self::new(self.min - m, self.max + m)
    }

    /// Returns whether the point lies inside or on the boundary.
    #[inline]
    #[must_use]
    pub fn contains_point(self, p: Point2) -> bool {
        (self.min.x..=self.max.x).contains(&p.x) && (self.min.y..=self.max.y).contains(&p.y)
    }

    /// Returns whether `other` is fully contained, including boundaries.
    #[inline]
    #[must_use]
    pub fn contains_bbox(self, other: Self) -> bool {
        self.min.x <= other.min.x
            && self.min.y <= other.min.y
            && other.max.x <= self.max.x
            && other.max.y <= self.max.y
    }

    /// Returns whether the boxes overlap or touch.
    #[inline]
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
    }

    /// Returns the geometric center.
    #[inline]
    #[must_use]
    pub fn center(self) -> Point2 {
        self.min.midpoint(self.max)
    }

    /// Returns non-negative `(width, height)` as a vector.
    #[inline]
    #[must_use]
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    /// Width along X.
    #[inline]
    #[must_use]
    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }

    /// Height along Y.
    #[inline]
    #[must_use]
    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    /// Returns whether width or height is within the default linear tolerance.
    #[inline]
    #[must_use]
    pub fn is_degenerate(self) -> bool {
        let tol = Tol::default();
        tol.approx_zero(self.width()) || tol.approx_zero(self.height())
    }
}
