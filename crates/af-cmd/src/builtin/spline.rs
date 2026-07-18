//! SPLINE (`SPL`) creates an interpolating cubic spline through fit points on the
//! current layer in one transaction.
//!
//! Only `Path` positions become fit points; bulges are ignored. `closed` creates a
//! periodic C2-continuous spline. Interpolation and flattening live in geometry/model code.

use af_model::entity::{EntityGeometry, SplineGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::create_entity;
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the SPLINE specification with alias `SPL`.
#[must_use]
pub fn spline_spec() -> CommandSpec {
    CommandSpec::new("SPLINE", "Spline", true, spline_exec)
        .alias("SPL")
        .param(ParamSpec::required("points", ParamType::Path))
        .param(ParamSpec::optional("closed", ParamType::Flag))
}

fn spline_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let path = args
        .path("points")
        .ok_or_else(|| CmdError::MissingParam("points".to_string()))?;
    let closed = args.flag("closed");
    let min = if closed { 3 } else { 2 };
    if path.len() < min {
        return Err(CmdError::Failed(format!(
            "SPLINE: se requieren al menos {min} puntos de ajuste{}",
            if closed { " (cerrada)" } else { "" }
        )));
    }
    // Splines use fit-point positions, not path bulges.
    let fit_points = path.iter().map(|&(pt, _bulge)| pt).collect();
    let geo = EntityGeometry::Spline(SplineGeo::new(fit_points, closed));
    let id = create_entity(ctx, "Spline", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}
