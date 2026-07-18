//! Common entity style properties: color, line weight, and line type.
//!
//! Rendering resolves inherited values through the entity, layer, and block;
//! this module only defines their model representation.

use serde::{Deserialize, Serialize};

use crate::id::StyleId;

/// Valid AutoCAD Color Index in `1..=255` by construction.
///
/// Construction and deserialization use the same validation, so zero cannot be
/// represented. Values above 255 are excluded by `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct AciColor(u8);

impl AciColor {
    /// Creates an ACI color after validating `1..=255`.
    ///
    /// # Errors
    ///
    /// Returns [`ColorError::AciOutOfRange`] when `v` is zero.
    #[inline]
    pub fn new(v: u8) -> Result<Self, ColorError> {
        if (1..=255).contains(&v) {
            Ok(Self(v))
        } else {
            Err(ColorError::AciOutOfRange(v))
        }
    }

    /// Returns the underlying ACI value, always in `1..=255`.
    #[inline]
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for AciColor {
    type Error = ColorError;
    #[inline]
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}

impl From<AciColor> for u8 {
    #[inline]
    fn from(c: AciColor) -> u8 {
        c.0
    }
}

/// Color construction error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorError {
    /// ACI value outside the valid `1..=255` range.
    AciOutOfRange(u8),
}

impl core::fmt::Display for ColorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ColorError::AciOutOfRange(v) => {
                write!(f, "ACI color index {v} out of range 1..=255")
            }
        }
    }
}

impl std::error::Error for ColorError {}

/// Entity color.
///
/// Rendering resolves `ByLayer` and `ByBlock`. `Aci` is a palette index, while
/// `Rgb` stores true color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Color {
    /// Inherits the layer color.
    ByLayer,
    /// Inherits the containing block-reference color.
    ByBlock,
    /// ACI palette index in `1..=255`.
    Aci(AciColor),
    /// True RGB color.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Creates [`Color::Aci`] after validating `1..=255`.
    ///
    /// # Errors
    ///
    /// Returns [`ColorError::AciOutOfRange`] when the index is zero.
    #[inline]
    pub fn aci(v: u8) -> Result<Self, ColorError> {
        Ok(Color::Aci(AciColor::new(v)?))
    }
}

impl Default for Color {
    /// Defaults to `ByLayer` for the current document color and new entities.
    fn default() -> Self {
        Color::ByLayer
    }
}

/// Entity line weight.
///
/// `Mm` stores an explicit value in millimeters. Rendering may currently use a
/// hairline regardless of the stored value.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lineweight {
    /// Inherits layer line weight and serves as the default for new entities.
    #[default]
    ByLayer,
    /// Inherits the containing block-reference line weight.
    ByBlock,
    /// Explicit line weight in millimeters.
    Mm(f32),
}

/// Entity line-type reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineTypeRef {
    /// Inherits the layer line type and serves as the default for new entities.
    #[default]
    ByLayer,
    /// Inherits the containing block-reference line type.
    ByBlock,
    /// Explicit document line style.
    Style(StyleId),
}
