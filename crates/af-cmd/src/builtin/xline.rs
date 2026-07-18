//! XLINE (`XL`) and RAY create infinite construction curves on the current layer
//! in one transaction.
//!
//! XLINE modes are two points, point plus angle, horizontal, and vertical.
//!
//! RAY starts at `origin` and points toward `through`.
//!
//! Directions are stored as unit vectors. Degenerate directions fail atomically.

use af_math::Vec2;
use af_model::entity::{EntityGeometry, RayGeo, XlineGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the XLINE specification with alias `XL`.
#[must_use]
pub fn xline_spec() -> CommandSpec {
    CommandSpec::new("XLINE", "Xline", true, xline_exec)
        .alias("XL")
        .param(ParamSpec::with_default(
            "mode",
            ParamType::Enum(vec![
                "points".into(),
                "ang".into(),
                "hor".into(),
                "ver".into(),
            ]),
            serde_json::json!("points"),
        ))
        .param(ParamSpec::optional("p1", ParamType::Point))
        .param(ParamSpec::optional("p2", ParamType::Point))
        .param(ParamSpec::optional("angle", ParamType::Angle))
}

/// Returns the RAY specification.
#[must_use]
pub fn ray_spec() -> CommandSpec {
    CommandSpec::new("RAY", "Ray", true, ray_exec)
        .param(ParamSpec::required("origin", ParamType::Point))
        .param(ParamSpec::required("through", ParamType::Point))
}

fn xline_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let base = req_point(&args, "p1")?;
    // Registry validation inserts the canonical default mode.
    let dir = match args.enum_value("mode").unwrap_or("points") {
        "points" => req_point(&args, "p2")? - base,
        "ang" => {
            let a = args
                .angle("angle")
                .ok_or_else(|| CmdError::MissingParam("angle".to_string()))?;
            Vec2::new(a.cos(), a.sin())
        }
        "hor" => Vec2::X,
        "ver" => Vec2::Y,
        // Registry Enum validation restricts `mode`.
        other => {
            return Err(CmdError::Failed(format!(
                "XLINE: modo no soportado '{other}'"
            )));
        }
    };
    // Preserve a zero vector so transaction geometry validation rejects it atomically.
    let dir = dir.normalize().unwrap_or(dir);
    let id = create_entity(
        ctx,
        "Xline",
        EntityGeometry::Xline(XlineGeo::new(base, dir)),
    )?;
    Ok(CommandOutcome::created(vec![id]))
}

fn ray_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let origin = req_point(&args, "origin")?;
    let through = req_point(&args, "through")?;
    let dir = through - origin;
    let dir = dir.normalize().unwrap_or(dir);
    let id = create_entity(ctx, "Ray", EntityGeometry::Ray(RayGeo::new(origin, dir)))?;
    Ok(CommandOutcome::created(vec![id]))
}
