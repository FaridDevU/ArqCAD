#![forbid(unsafe_code)]
//! # af-math — ArcCAD 2D math kernel
//!
//! Provides core geometry types, angle utilities, and canonical [`Tol`] values.
//! It contains only pure deterministic primitives and knows nothing about
//! entities, IDs, documents, or rendering.
//!
//! ## Conventions
//!
//! - Scalars are `f64`; this API is 2D and not generic over scalar types.
//! - Angles use radians.
//! - Counterclockwise is positive, with Y pointing up.
//! - `Transform2` uses affine matrix `[[a, b, tx], [c, d, ty]]`.
//! - `A.then(B)` applies `A` first and `B` second, equivalent to `B ∘ A`.
//! - Approximate comparisons use [`Tol`] instead of direct floating-point equality.
//!
//! ## Determinism and degenerate cases
//!
//! There is no global state. External-data constructors leave finite-value
//! validation to the model, while degenerate operations such as
//! [`Vec2::normalize`] and [`Transform2::invert`] return [`MathError`].

pub mod angle;
mod bbox;
mod point;
mod tol;
mod transform;
mod vec;

pub use bbox::BBox;
pub use point::Point2;
pub use tol::Tol;
pub use transform::Transform2;
pub use vec::Vec2;

/// Errors from operations that can degenerate on finite input.
///
/// These prevent silent `NaN` or infinity results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MathError {
    /// Attempted to normalize a near-zero vector.
    ZeroVector,
    /// Attempted to invert a near-singular transform.
    Singular,
}

impl core::fmt::Display for MathError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            MathError::ZeroVector => "cannot normalize a near-zero-length vector",
            MathError::Singular => "cannot invert a singular (near-zero determinant) transform",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for MathError {}
