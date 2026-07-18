//! UNITS (`-UNITS`) sets document linear display precision (`LUPREC`) and reports it.
//!
//! Precision is persistent, undoable document metadata in range `0..=8`, changed
//! in exactly one transaction.
//!
//! The command is a setter rather than a separate view operation; its outcome
//! message reports the resulting format.

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::report::linear_unit_name;

/// Maximum supported linear precision.
const MAX_LUPREC: u64 = 8;

/// Returns the UNITS specification with alias `-UNITS`.
#[must_use]
pub fn units_spec() -> CommandSpec {
    CommandSpec::new("UNITS", "Units", true, units_exec)
        .alias("-UNITS")
        .param(ParamSpec::required("precision", ParamType::Count))
}

/// Registers UNITS.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(units_spec())
}

fn units_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let precision = args
        .count("precision")
        .ok_or_else(|| CmdError::MissingParam("precision".to_string()))?;
    if precision > MAX_LUPREC {
        return Err(CmdError::OutOfRange {
            param: "precision".to_string(),
            message: format!("la precisión lineal (LUPREC) debe estar en 0..={MAX_LUPREC}"),
        });
    }
    let precision = precision as u8;

    ctx.transact("Units", |tx| {
        tx.set_linear_precision(precision);
        Ok::<(), CmdError>(())
    })?;

    let doc = ctx.document();
    Ok(CommandOutcome::message(format!(
        "Linear units: {}   Precision: {} decimals",
        linear_unit_name(doc.units().linear),
        doc.linear_precision(),
    )))
}
