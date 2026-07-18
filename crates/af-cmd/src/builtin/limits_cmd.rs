//! LIMITS sets document drawing bounds (`LIMMIN` and `LIMMAX`) and reports them.
//!
//! Bounds are persistent, undoable document metadata and use one transaction.
//! They do not clip geometry.

use af_model::Limits;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::report::fmt_pt;

/// Returns the LIMITS specification without aliases.
#[must_use]
pub fn limits_spec() -> CommandSpec {
    CommandSpec::new("LIMITS", "Limits", true, limits_exec)
        .param(ParamSpec::required("min", ParamType::Point))
        .param(ParamSpec::required("max", ParamType::Point))
}

/// Registers LIMITS.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(limits_spec())
}

fn limits_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let min = args
        .point("min")
        .ok_or_else(|| CmdError::MissingParam("min".to_string()))?;
    let max = args
        .point("max")
        .ok_or_else(|| CmdError::MissingParam("max".to_string()))?;

    let limits = Limits { min, max };
    ctx.transact("Limits", |tx| {
        tx.set_limits(limits);
        Ok::<(), CmdError>(())
    })?;

    let doc = ctx.document();
    let lim = doc.limits();
    Ok(CommandOutcome::message(format!(
        "Limits: {} - {}",
        fmt_pt(doc, lim.min),
        fmt_pt(doc, lim.max),
    )))
}
