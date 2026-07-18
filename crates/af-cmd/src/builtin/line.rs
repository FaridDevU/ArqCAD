//! LINE (`L`) creates a model-space line between two points on the current layer,
//! using current document style properties, in exactly one transaction.
//!
//! The registry supplies validated endpoints. Locked, frozen, or off current layers
//! fail. Zero-length lines are allowed but produce an outcome warning.

use af_model::Layer;
use af_model::container::ContainerRef;
use af_model::entity::{EntityGeometry, EntityRecord, LineGeo};
use af_model::id::{EntityId, ObjectId};

use crate::args::ParsedArgs;
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the LINE specification with alias `L`.
#[must_use]
pub fn line_spec() -> CommandSpec {
    CommandSpec::new("LINE", "Line", true, line_exec)
        .alias("L")
        .param(ParamSpec::required("p1", ParamType::Point))
        .param(ParamSpec::required("p2", ParamType::Point))
}

/// Creates `p1` to `p2` on the current layer in one transaction.
fn line_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Required points were validated; keep defensive missing-parameter errors.
    let p1 = args
        .point("p1")
        .ok_or_else(|| CmdError::MissingParam("p1".to_string()))?;
    let p2 = args
        .point("p2")
        .ok_or_else(|| CmdError::MissingParam("p2".to_string()))?;

    // Resolve the current layer rather than assuming layer `0`.
    let layer_id = ctx.document().current_layer();
    if let Some(layer) = ctx.document().layer(layer_id)
        && let Some(reason) = uneditable_reason(layer)
    {
        return Err(CmdError::Failed(format!(
            "cannot draw on layer '{}': {reason}",
            layer.name()
        )));
    }

    // New lines inherit current color, line type, and lineweight.
    let record = EntityRecord::new(
        ObjectId::NIL.into(),
        layer_id,
        ctx.document().current_color(),
        ctx.document().current_line_type(),
        ctx.document().current_lineweight(),
        EntityGeometry::Line(LineGeo::new(p1, p2)),
    );

    let id = ctx.transact("Line", |tx| -> Result<EntityId, CmdError> {
        Ok(tx.add_entity(ContainerRef::ModelSpace, record)?)
    })?;

    let mut outcome = CommandOutcome::created(vec![id]);
    if p1 == p2 {
        outcome.message = Some("warning: zero-length line (p1 equals p2)".to_string());
    }
    Ok(outcome)
}

/// Returns why `layer` rejects new entities, prioritizing locked, frozen, then off.
pub(crate) fn uneditable_reason(layer: &Layer) -> Option<&'static str> {
    if layer.is_locked() {
        Some("layer is locked")
    } else if layer.is_frozen() {
        Some("layer is frozen")
    } else if layer.is_off() {
        Some("layer is off")
    } else {
        None
    }
}
