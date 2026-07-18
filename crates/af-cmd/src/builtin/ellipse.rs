//! ELLIPSE (`EL`) creates an ellipse or elliptical arc on the current layer in one
//! transaction using center, major semiaxis, ratio, rotation, and parameter sweep.
//!
//! `center` creates a complete ellipse; `arc` adds counterclockwise start and end
//! parameters in radians.
//!
//! `axisEnd - center` defines the major semiaxis and rotation; `ratio` defines the
//! minor semiaxis. Degenerate geometry is rejected atomically.

use core::f64::consts::TAU;

use af_math::angle::angle_of;
use af_model::entity::{EllipseGeo, EntityGeometry};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_distance, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the ELLIPSE specification with alias `EL`.
#[must_use]
pub fn ellipse_spec() -> CommandSpec {
    CommandSpec::new("ELLIPSE", "Ellipse", true, ellipse_exec)
        .alias("EL")
        .param(ParamSpec::with_default(
            "mode",
            ParamType::Enum(vec!["center".into(), "arc".into()]),
            serde_json::json!("center"),
        ))
        .param(ParamSpec::optional("center", ParamType::Point))
        .param(ParamSpec::optional("axisEnd", ParamType::Point))
        .param(ParamSpec::optional("ratio", ParamType::Distance))
        .param(ParamSpec::optional("startParam", ParamType::Angle))
        .param(ParamSpec::optional("endParam", ParamType::Angle))
}

fn ellipse_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let mode = args.enum_value("mode").unwrap_or("center");
    // Derive major semiaxis and rotation from its endpoint.
    let center = req_point(&args, "center")?;
    let axis_end = req_point(&args, "axisEnd")?;
    let ratio = req_distance(&args, "ratio")?;
    let major = axis_end - center;
    let semi_major = major.norm();
    let rotation = angle_of(major);

    let (start_param, end_param) = match mode {
        // A complete ellipse spans `[0, 2π]`.
        "center" => (0.0, TAU),
        // Elliptical arcs sweep counterclockwise between parameters.
        "arc" => {
            let sp = args
                .angle("startParam")
                .ok_or_else(|| CmdError::MissingParam("startParam".to_string()))?;
            let ep = args
                .angle("endParam")
                .ok_or_else(|| CmdError::MissingParam("endParam".to_string()))?;
            (sp, ep)
        }
        // Registry Enum validation restricts `mode` to the branches above.
        other => {
            return Err(CmdError::Failed(format!(
                "ELLIPSE: modo no soportado '{other}'"
            )));
        }
    };

    let geo = EllipseGeo::new(center, semi_major, ratio, rotation, start_param, end_param);
    let id = create_entity(ctx, "Ellipse", EntityGeometry::Ellipse(geo))?;
    Ok(CommandOutcome::created(vec![id]))
}
