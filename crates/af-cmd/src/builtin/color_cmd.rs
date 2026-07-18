//! COLOR (`COL`) sets CECOLOR, used when drawing commands do not specify a color.
//!
//! `CO` remains reserved for COPY.
//!
//! # Current color
//!
//! `Document::current_color` is backward-compatible through `#[serde(default)]`
//! and changes through a reversible document-property operation. Drawing commands
//! read it when creating entities.
//!
//! Values are parsed by [`crate::builtin::style_value::parse_color`].

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::style_value::parse_color;

/// Returns the COLOR specification with alias `COL`.
#[must_use]
pub fn color_spec() -> CommandSpec {
    CommandSpec::new("COLOR", "Color", true, color_exec)
        .alias("COL")
        .param(ParamSpec::required("color", ParamType::Text))
}

/// Registers COLOR.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(color_spec())
}

fn color_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let raw = args
        .text("color")
        .ok_or_else(|| CmdError::MissingParam("color".to_string()))?;
    let color = parse_color(raw)?;
    ctx.transact("Color", |tx| -> Result<(), CmdError> {
        tx.set_current_color(color);
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}
