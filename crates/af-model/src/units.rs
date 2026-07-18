//! Document units plus linear/angular display formatting and parsing.
//!
//! # Units never convert geometry
//! [`Units`] is interpretation metadata like DXF `$INSUNITS`. Coordinates remain
//! raw drawing-unit `f64` values; changing units never rescales stored geometry.
//!
//! Internal angles use radians; display helpers use decimal degrees.
//!
//! `Tol` is re-exported from `af-math`.

use std::fmt;

use serde::{Deserialize, Serialize};

// Re-export `Tol` so model consumers need not depend directly on `af-math`.
pub use af_math::Tol;

/// Physical interpretation of one drawing unit.
///
/// Serializes as a lowercase string and defaults to [`LinearUnit::Mm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinearUnit {
    /// Millimeters; the default.
    #[default]
    Mm,
    /// Centimeters.
    Cm,
    /// Meters.
    M,
    /// Inches.
    In,
    /// Feet.
    Ft,
    /// Unitless drawing with no millimeter conversion factor.
    Unitless,
}

impl LinearUnit {
    /// Informational conversion factor to millimeters.
    ///
    /// Returns `None` for [`LinearUnit::Unitless`] and never mutates geometry.
    #[inline]
    #[must_use]
    pub const fn to_mm_factor(self) -> Option<f64> {
        match self {
            LinearUnit::Mm => Some(1.0),
            LinearUnit::Cm => Some(10.0),
            LinearUnit::M => Some(1000.0),
            LinearUnit::In => Some(25.4),
            LinearUnit::Ft => Some(304.8),
            LinearUnit::Unitless => None,
        }
    }

    /// Corresponding DXF `$INSUNITS` code.
    ///
    /// Mapping: `unitless=0`, `in=1`, `ft=2`, `mm=4`, `cm=5`, `m=6`.
    #[inline]
    #[must_use]
    pub const fn dxf_insunits_code(self) -> u8 {
        match self {
            LinearUnit::Unitless => 0,
            LinearUnit::In => 1,
            LinearUnit::Ft => 2,
            LinearUnit::Mm => 4,
            LinearUnit::Cm => 5,
            LinearUnit::M => 6,
        }
    }
}

/// Document unit interpretation metadata.
///
/// Changing `linear` reinterprets values without changing coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Units {
    /// Linear unit for lengths and coordinates.
    pub linear: LinearUnit,
}

/// Error parsing a user-entered linear value or angle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseValueError {
    /// Input uses a comma decimal separator, reserved for coordinate separation.
    CommaDecimalSeparator(String),
    /// Input is not a plain number; unit suffixes are unsupported.
    NotANumber(String),
    /// Input parses to a nonfinite value.
    NotFinite(String),
}

impl fmt::Display for ParseValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseValueError::CommaDecimalSeparator(s) => write!(
                f,
                "'{s}': la coma no es un separador decimal válido (está reservada para separar coordenadas); usa un punto, p. ej. '7.25'"
            ),
            ParseValueError::NotANumber(s) => write!(
                f,
                "'{s}' no es un número válido (se espera un decimal simple con punto, sin sufijos de unidad, p. ej. '7.25')"
            ),
            ParseValueError::NotFinite(s) => {
                write!(
                    f,
                    "'{s}' no es un valor finito (NaN/infinito no son válidos)"
                )
            }
        }
    }
}

impl std::error::Error for ParseValueError {}

/// Formats a drawing-unit linear value with `decimals` places.
///
/// `units` does not convert `v`; output has no unit suffix.
///
/// Normalizes rounded negative zero to positive zero.
#[must_use]
pub fn format_linear(v: f64, _units: Units, decimals: u8) -> String {
    format_fixed(v, decimals)
}

/// Formats a radian angle as decimal degrees.
#[must_use]
pub fn format_angle_deg(rad: f64, decimals: u8) -> String {
    format_fixed(rad.to_degrees(), decimals)
}

/// Formats `v` while normalizing rounded negative zero.
fn format_fixed(v: f64, decimals: u8) -> String {
    let s = format!("{:.*}", decimals as usize, v);
    match s.strip_prefix('-') {
        // Remaining characters represent negative zero.
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s,
    }
}

/// Parses a plain decimal linear value without unit suffixes.
///
/// # Errors
/// Returns [`ParseValueError`] for comma decimals, invalid numbers, or nonfinite values.
pub fn parse_linear(s: &str) -> Result<f64, ParseValueError> {
    let trimmed = s.trim();
    if trimmed.contains(',') {
        return Err(ParseValueError::CommaDecimalSeparator(s.to_string()));
    }
    let v: f64 = trimmed
        .parse()
        .map_err(|_| ParseValueError::NotANumber(s.to_string()))?;
    if !v.is_finite() {
        return Err(ParseValueError::NotFinite(s.to_string()));
    }
    Ok(v)
}

/// Parses a decimal-degree angle and returns radians.
///
/// # Errors
/// Uses the same validation as [`parse_linear`].
pub fn parse_angle_deg(s: &str) -> Result<f64, ParseValueError> {
    parse_linear(s).map(f64::to_radians)
}
