//! CLAYER sets the document's current layer in one transaction.
//!
//! It delegates to [`af_model::layers_ops::set_current_layer`], like LAYER's
//! `set-current` operation, and rejects off or frozen layers. It has no alias.

use af_model::layers_ops;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the CLAYER specification without aliases.
#[must_use]
pub fn clayer_spec() -> CommandSpec {
    CommandSpec::new("CLAYER", "Clayer", true, clayer_exec)
        .param(ParamSpec::required("layer", ParamType::LayerRef))
}

/// Registers CLAYER.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(clayer_spec())
}

fn clayer_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = args
        .layer("layer")
        .ok_or_else(|| CmdError::MissingParam("layer".to_string()))?;
    ctx.transact("Clayer", |tx| {
        layers_ops::set_current_layer(tx, layer).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}
