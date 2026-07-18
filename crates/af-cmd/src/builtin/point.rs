//! POINT (`PO`) creates a point entity at `position` on the current layer in one transaction.

use af_model::entity::{EntityGeometry, PointGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the POINT specification with alias `PO`.
#[must_use]
pub fn point_spec() -> CommandSpec {
    CommandSpec::new("POINT", "Point", true, point_exec)
        .alias("PO")
        .param(ParamSpec::required("position", ParamType::Point))
}

fn point_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let position = req_point(&args, "position")?;
    let geo = EntityGeometry::Point(PointGeo::new(position));
    let id = create_entity(ctx, "Point", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}
