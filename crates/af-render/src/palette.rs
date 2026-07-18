//! ACI-to-RGBA palette conversion.
//!
//! # Scope
//!
//! The nine standard ACI colors (1 through 9) are mapped exactly. They cover the
//! primary and secondary colors, white, and two grays, including layer 0's ACI 7
//! default. ACI 10 through 255 currently use a deterministic provisional gray
//! derived from the index.
//!
//! `ByLayer` and `ByBlock` are not indices and are resolved by `af-render::build`
//! against the layer or block context.

use crate::Rgba;

/// Opaque-white fallback when no concrete color can be resolved.
pub(crate) const FALLBACK: Rgba = Rgba::new(255, 255, 255, 255);

/// Converts an ACI index (`1..=255`) to opaque [`Rgba`].
///
/// Values 1 through 9 are exact; 10 through 255 use the provisional gray map.
/// The model prevents ACI 0, but it also falls back to the gray map if received.
pub(crate) fn aci_to_rgba(aci: u8) -> Rgba {
    match aci {
        1 => Rgba::new(255, 0, 0, 255),     // Red.
        2 => Rgba::new(255, 255, 0, 255),   // Yellow.
        3 => Rgba::new(0, 255, 0, 255),     // Green.
        4 => Rgba::new(0, 255, 255, 255),   // Cyan.
        5 => Rgba::new(0, 0, 255, 255),     // Blue.
        6 => Rgba::new(255, 0, 255, 255),   // magenta
        7 => Rgba::new(255, 255, 255, 255), // White, or black on a light background.
        8 => Rgba::new(128, 128, 128, 255), // Dark gray.
        9 => Rgba::new(192, 192, 192, 255), // Light gray.
        // ponytail: A deterministic gray ramp is sufficient until full ACI
        // fidelity is required by broader DXF interoperability.
        other => Rgba::new(other, other, other, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colores_estandar_exactos() {
        assert_eq!(aci_to_rgba(1), Rgba::new(255, 0, 0, 255));
        assert_eq!(aci_to_rgba(5), Rgba::new(0, 0, 255, 255));
        // ACI 7 is layer 0's default color.
        assert_eq!(aci_to_rgba(7), Rgba::new(255, 255, 255, 255));
        assert_eq!(aci_to_rgba(9), Rgba::new(192, 192, 192, 255));
    }

    #[test]
    fn rango_alto_es_gris_determinista() {
        assert_eq!(aci_to_rgba(100), Rgba::new(100, 100, 100, 255));
        assert_eq!(aci_to_rgba(100), aci_to_rgba(100)); // Deterministic.
    }
}
