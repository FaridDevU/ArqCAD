//! UNDO and REDO history commands.
//!
//! They delegate to [`CommandCtx::undo`] and [`CommandCtx::redo`] without creating
//! a new transaction or history entry.

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec};

/// Returns the UNDO specification with alias `U`.
#[must_use]
pub fn undo_spec() -> CommandSpec {
    CommandSpec::new("UNDO", "Undo", false, undo_exec).alias("U")
}

/// Returns the REDO specification without aliases.
#[must_use]
pub fn redo_spec() -> CommandSpec {
    CommandSpec::new("REDO", "Redo", false, redo_exec)
}

/// Registers UNDO and REDO.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register_builtins(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(undo_spec())?;
    registry.register(redo_spec())?;
    Ok(())
}

/// Undoes the latest transaction.
fn undo_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Capture the label before the entry moves to the redo stack.
    let label = ctx.undo_label().map(str::to_string);
    ctx.undo()?;
    let msg = match label {
        Some(l) => format!("Undo {l}"),
        None => "Undo".to_string(),
    };
    Ok(CommandOutcome::message(msg))
}

/// Redoes the latest undone transaction.
fn redo_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let label = ctx.redo_label().map(str::to_string);
    ctx.redo()?;
    let msg = match label {
        Some(l) => format!("Redo {l}"),
        None => "Redo".to_string(),
    };
    Ok(CommandOutcome::message(msg))
}
