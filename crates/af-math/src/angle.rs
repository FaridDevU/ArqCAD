//! Angle utilities using radians and positive counterclockwise orientation.
//!
//! A `start → end` sweep always travels counterclockwise.

use core::f64::consts::TAU;

use crate::{Tol, Vec2};

/// Normalizes an angle to `[0, 2π)`.
#[inline]
#[must_use]
pub fn normalize_0_2pi(a: f64) -> f64 {
    let r = a.rem_euclid(TAU);
    // Collapse a rounded exact `TAU` back to zero.
    if r >= TAU { 0.0 } else { r }
}

/// Returns vector direction as `atan2(y, x)` in `(-π, π]`.
///
/// A zero vector returns `0.0`, following `atan2(0, 0)`.
#[inline]
#[must_use]
pub fn angle_of(v: Vec2) -> f64 {
    v.y.atan2(v.x)
}

/// Returns the counterclockwise sweep from `start` to `end` in `(0, 2π]`.
///
/// Coincident endpoints modulo `2π` represent a full turn, not zero.
#[inline]
#[must_use]
pub fn sweep_ccw(start: f64, end: f64) -> f64 {
    let d = normalize_0_2pi(end - start);
    if d <= 0.0 { TAU } else { d }
}

/// Returns whether `a` lies within the counterclockwise `start → end` sweep.
///
/// Endpoints are included within [`Tol::default()`]`.angle`. A full turn accepts
/// any angle.
#[inline]
#[must_use]
pub fn angle_in_sweep(a: f64, start: f64, end: f64) -> bool {
    let tol = Tol::default().angle;
    let sweep = sweep_ccw(start, end);
    let offset = normalize_0_2pi(a - start);
    // `offset ≈ 2π` covers values just before `start` across the wrap.
    offset <= sweep + tol || offset >= TAU - tol
}

/// Returns the minimum wrapped angular difference in `[0, π]`.
///
/// Used for angle comparisons across the `2π` wrap.
#[inline]
pub(crate) fn angular_gap(a: f64, b: f64) -> f64 {
    let d = normalize_0_2pi(a - b);
    d.min(TAU - d)
}
