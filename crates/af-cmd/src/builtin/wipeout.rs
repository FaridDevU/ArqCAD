//! WIPEOUT creates a closed masking polygon on the current layer in one transaction.
//!
//! Only typed `Path` positions are used; bulges are ignored. The polygon closes
//! implicitly from its last vertex to its first and requires at least three points.
//!
//! Frame-mode changes and conversion from an existing polyline are unsupported.

use af_model::entity::{EntityGeometry, WipeoutGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::create_entity;
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the WIPEOUT specification.
#[must_use]
pub fn wipeout_spec() -> CommandSpec {
    CommandSpec::new("WIPEOUT", "Wipeout", true, wipeout_exec)
        .param(ParamSpec::optional("points", ParamType::Path))
        .param(ParamSpec::optional("frames", ParamType::Flag))
        .param(ParamSpec::optional("polyline", ParamType::EntitySet))
}

fn wipeout_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Frame visibility is system state rather than entity geometry.
    if args.flag("frames") {
        return Err(CmdError::Failed(
            "WIPEOUT: el modo Frames (marcos) está diferido".to_string(),
        ));
    }
    // Existing-polyline conversion is not implemented.
    if args
        .entity_set("polyline")
        .is_some_and(|ids| !ids.is_empty())
    {
        return Err(CmdError::Failed(
            "WIPEOUT: la conversión desde polilínea está diferida".to_string(),
        ));
    }

    let path = args
        .path("points")
        .ok_or_else(|| CmdError::MissingParam("points".to_string()))?;
    if path.len() < 3 {
        return Err(CmdError::Failed(
            "WIPEOUT: se requieren al menos 3 puntos para el polígono de máscara".to_string(),
        ));
    }
    // Masks use positions only and close implicitly without duplicating the first vertex.
    let points = path.iter().map(|&(pt, _bulge)| pt).collect();
    let geo = EntityGeometry::Wipeout(WipeoutGeo::new(points));
    let id = create_entity(ctx, "Wipeout", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}
