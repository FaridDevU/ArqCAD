//! Shared formatting for query-command lengths, angles, coordinates, colors, and areas.
//!
//! Linear values use document units and `LUPREC` through
//! [`af_model::units::format_linear`]. Angles are displayed in degrees with the
//! same precision.

use std::f64::consts::TAU;

use af_math::{Point2, Vec2};
use af_model::Document;
use af_model::entity::Color;
use af_model::units::{LinearUnit, format_angle_deg, format_linear};

/// Returns the lowercase short name of a linear unit.
pub(crate) fn linear_unit_name(u: LinearUnit) -> &'static str {
    match u {
        LinearUnit::Mm => "mm",
        LinearUnit::Cm => "cm",
        LinearUnit::M => "m",
        LinearUnit::In => "in",
        LinearUnit::Ft => "ft",
        LinearUnit::Unitless => "unitless",
    }
}

/// Formats a length or coordinate using document units and precision.
pub(crate) fn fmt_len(doc: &Document, v: f64) -> String {
    format_linear(v, doc.units(), doc.linear_precision())
}

/// Formats a point as `x,y` with document precision.
pub(crate) fn fmt_pt(doc: &Document, p: Point2) -> String {
    format!("{},{}", fmt_len(doc, p.x), fmt_len(doc, p.y))
}

/// Formats radians as degrees normalized to `[0, 360)`.
pub(crate) fn fmt_angle(doc: &Document, rad: f64) -> String {
    format!(
        "{}°",
        format_angle_deg(rad.rem_euclid(TAU), doc.linear_precision())
    )
}

/// Returns a stable entity-color name for LIST.
pub(crate) fn color_name(c: Color) -> String {
    match c {
        Color::ByLayer => "BYLAYER".to_string(),
        Color::ByBlock => "BYBLOCK".to_string(),
        Color::Aci(a) => format!("ACI {}", a.get()),
        Color::Rgb(r, g, b) => format!("RGB({r},{g},{b})"),
    }
}

/// Returns the `[0, π]` angle at `v`, or `None` for a degenerate ray.
pub(crate) fn vertex_angle(v: Point2, a: Point2, b: Point2) -> Option<f64> {
    let u: Vec2 = a - v;
    let w: Vec2 = b - v;
    let (nu, nw) = (u.norm(), w.norm());
    if nu <= f64::EPSILON || nw <= f64::EPSILON {
        return None;
    }
    // `atan2(|u×w|, u·w)` is robust and always lies in `[0, π]`.
    let cross = (u.x * w.y - u.y * w.x).abs();
    let dot = u.x * w.x + u.y * w.y;
    Some(cross.atan2(dot))
}
