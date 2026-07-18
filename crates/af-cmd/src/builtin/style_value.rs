//! Free-form property value parsing for color, line type, lineweight, and layer.
//!
//! Shared by COLOR, CHPROP, and LAYER's color operation for command-line literals.

use af_model::Document;
use af_model::entity::{Color, LineTypeRef, Lineweight};
use af_model::id::{LayerId, ObjectId};

use crate::spec::CmdError;

/// Parses `BYLAYER`, `BYBLOCK`, ACI `1..=255`, or RGB `r,g,b`.
///
/// # Errors
/// Returns [`CmdError::Failed`] when `raw` matches no accepted form.
pub(crate) fn parse_color(raw: &str) -> Result<Color, CmdError> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("bylayer") {
        return Ok(Color::ByLayer);
    }
    if s.eq_ignore_ascii_case("byblock") {
        return Ok(Color::ByBlock);
    }
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() == 3 {
        let mut rgb = [0u8; 3];
        for (i, p) in parts.iter().enumerate() {
            rgb[i] = p.trim().parse::<u8>().map_err(|_| {
                CmdError::Failed(format!(
                    "invalid color '{raw}': component '{p}' must be an integer 0..=255"
                ))
            })?;
        }
        return Ok(Color::Rgb(rgb[0], rgb[1], rgb[2]));
    }
    let aci: u8 = s.parse().map_err(|_| {
        CmdError::Failed(format!(
            "invalid color '{raw}' (expected BYLAYER, BYBLOCK, an ACI index 1..=255, or an r,g,b triplet)"
        ))
    })?;
    Color::aci(aci).map_err(|e| CmdError::Failed(e.to_string()))
}

/// Parses `BYLAYER`, `BYBLOCK`, or a finite nonnegative millimeter lineweight.
///
/// # Errors
/// Returns [`CmdError::Failed`] when `raw` matches no accepted form.
pub(crate) fn parse_lineweight(raw: &str) -> Result<Lineweight, CmdError> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("bylayer") {
        return Ok(Lineweight::ByLayer);
    }
    if s.eq_ignore_ascii_case("byblock") {
        return Ok(Lineweight::ByBlock);
    }
    let mm: f32 = s.parse().map_err(|_| {
        CmdError::Failed(format!(
            "invalid lineweight '{raw}' (expected BYLAYER, BYBLOCK, or a number in mm)"
        ))
    })?;
    if !mm.is_finite() || mm < 0.0 {
        return Err(CmdError::Failed(format!(
            "invalid lineweight '{raw}': must be finite and >= 0"
        )));
    }
    Ok(Lineweight::Mm(mm))
}

/// Parses `BYLAYER`, `BYBLOCK`, or an existing case-insensitive line type name.
///
/// # Errors
/// Returns [`CmdError::Failed`] for an unknown line type.
pub(crate) fn parse_line_type(doc: &Document, raw: &str) -> Result<LineTypeRef, CmdError> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("bylayer") {
        return Ok(LineTypeRef::ByLayer);
    }
    if s.eq_ignore_ascii_case("byblock") {
        return Ok(LineTypeRef::ByBlock);
    }
    doc.line_types()
        .find(|lt| lt.name().eq_ignore_ascii_case(s))
        .map(|lt| LineTypeRef::Style(lt.id()))
        .ok_or_else(|| CmdError::Failed(format!("unknown line type '{raw}'")))
}

/// Resolves a layer by case-insensitive name or numeric text ID.
///
/// # Errors
/// Returns [`CmdError::UnknownLayer`] when no layer matches `raw`.
pub(crate) fn parse_layer_ref(doc: &Document, raw: &str) -> Result<LayerId, CmdError> {
    let s = raw.trim();
    if let Ok(n) = s.parse::<u64>() {
        let id: LayerId = ObjectId(n).into();
        if doc.layer(id).is_some() {
            return Ok(id);
        }
    }
    doc.layer_by_name(s)
        .map(af_model::Layer::id)
        .ok_or_else(|| CmdError::UnknownLayer(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_color_bylayer_byblock_case_insensitive() {
        assert_eq!(parse_color("bylayer").unwrap(), Color::ByLayer);
        assert_eq!(parse_color("BYBLOCK").unwrap(), Color::ByBlock);
    }

    #[test]
    fn parse_color_aci() {
        assert_eq!(parse_color("5").unwrap(), Color::aci(5).unwrap());
    }

    #[test]
    fn parse_color_aci_zero_is_err() {
        assert!(parse_color("0").is_err());
    }

    #[test]
    fn parse_color_rgb_triplet() {
        assert_eq!(parse_color("255,0,128").unwrap(), Color::Rgb(255, 0, 128));
    }

    #[test]
    fn parse_color_garbage_is_err() {
        assert!(parse_color("not-a-color").is_err());
    }

    #[test]
    fn parse_lineweight_variants() {
        assert_eq!(parse_lineweight("BYLAYER").unwrap(), Lineweight::ByLayer);
        assert_eq!(parse_lineweight("byblock").unwrap(), Lineweight::ByBlock);
        assert_eq!(parse_lineweight("0.5").unwrap(), Lineweight::Mm(0.5));
        assert!(parse_lineweight("-1").is_err());
        assert!(parse_lineweight("nope").is_err());
    }
}
