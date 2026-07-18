//! ARC (`A`) creates a circular arc on the current layer in one transaction. Arcs
//! are always stored as counterclockwise sweeps.
//!
//! `mode="3p"` uses start, on-arc, and end points. `mode="cse"` uses center and
//! start plus either an end point or an end angle in radians.

use af_geom::circle::circumcircle;
use af_math::Point2;
use af_math::angle::{angle_in_sweep, angle_of};
use af_model::entity::{ArcGeo, EntityGeometry};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the ARC specification with alias `A`.
#[must_use]
pub fn arc_spec() -> CommandSpec {
    CommandSpec::new("ARC", "Arc", true, arc_exec)
        .alias("A")
        .param(ParamSpec::with_default(
            "mode",
            ParamType::Enum(vec!["3p".into(), "cse".into()]),
            serde_json::json!("3p"),
        ))
        .param(ParamSpec::optional("p1", ParamType::Point))
        .param(ParamSpec::optional("p2", ParamType::Point))
        .param(ParamSpec::optional("p3", ParamType::Point))
        .param(ParamSpec::optional("center", ParamType::Point))
        .param(ParamSpec::optional("start", ParamType::Point))
        .param(ParamSpec::optional("end", ParamType::Point))
        .param(ParamSpec::optional("endAngle", ParamType::Angle))
}

fn arc_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let mode = args.enum_value("mode").unwrap_or("3p");
    let geo = match mode {
        "3p" => arc_from_3p(&args)?,
        "cse" => arc_from_cse(&args)?,
        other => {
            return Err(CmdError::Failed(format!(
                "ARC: modo no soportado '{other}'"
            )));
        }
    };
    let id = create_entity(ctx, "Arc", EntityGeometry::Arc(geo))?;
    Ok(CommandOutcome::created(vec![id]))
}

/// Builds the three-point arc whose counterclockwise sweep contains `p2`.
fn arc_from_3p(args: &ParsedArgs) -> Result<ArcGeo, CmdError> {
    let p1 = req_point(args, "p1")?;
    let p2 = req_point(args, "p2")?;
    let p3 = req_point(args, "p3")?;
    arc_from_three_points(p1, p2, p3)
}

/// Shared three-point arc geometry used by ARC and ADDSELECTED.
pub(crate) fn arc_from_three_points(
    p1: Point2,
    p2: Point2,
    p3: Point2,
) -> Result<ArcGeo, CmdError> {
    let (center, radius) = circumcircle(p1, p2, p3)
        .ok_or_else(|| CmdError::Failed("ARC 3P: los tres puntos son colineales".to_string()))?;
    let a1 = angle_of(p1 - center);
    let a2 = angle_of(p2 - center);
    let a3 = angle_of(p3 - center);
    let (start_angle, end_angle) = if angle_in_sweep(a2, a1, a3) {
        (a1, a3)
    } else {
        (a3, a1)
    };
    Ok(ArcGeo::new(center, radius, start_angle, end_angle))
}

/// Builds a counterclockwise center-start-end arc.
fn arc_from_cse(args: &ParsedArgs) -> Result<ArcGeo, CmdError> {
    let center = req_point(args, "center")?;
    let start = req_point(args, "start")?;
    let radius = center.dist(start);
    let start_angle = angle_of(start - center);
    let end_angle = if let Some(end) = args.point("end") {
        angle_of(end - center)
    } else if let Some(a) = args.angle("endAngle") {
        a
    } else {
        return Err(CmdError::MissingParam("end".to_string()));
    };
    Ok(ArcGeo::new(center, radius, start_angle, end_angle))
}
