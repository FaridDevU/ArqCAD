//! PLINE (`PL`) creates a polyline on the current layer in one transaction.
//!
//! A typed `Path` supplies vertices and optional segment bulges. `closed` adds the
//! final-to-first segment; geometry resolution remains in the geometry/model layer.

use af_model::entity::{EntityGeometry, PolyVertex, PolylineGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::create_entity;
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the PLINE specification with alias `PL`.
#[must_use]
pub fn pline_spec() -> CommandSpec {
    CommandSpec::new("PLINE", "Polyline", true, pline_exec)
        .alias("PL")
        .param(ParamSpec::required("vertices", ParamType::Path))
        .param(ParamSpec::optional("closed", ParamType::Flag))
}

fn pline_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let path = args
        .path("vertices")
        .ok_or_else(|| CmdError::MissingParam("vertices".to_string()))?;
    if path.len() < 2 {
        return Err(CmdError::Failed(
            "PLINE: se requieren al menos 2 vértices".to_string(),
        ));
    }
    let vertices = path
        .iter()
        .map(|&(pt, bulge)| PolyVertex::new(pt, bulge))
        .collect();
    let closed = args.flag("closed");
    let geo = EntityGeometry::Polyline(PolylineGeo::new(vertices, closed));
    let id = create_entity(ctx, "Polyline", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}
