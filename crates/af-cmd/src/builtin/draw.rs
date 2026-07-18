//! Shared drawing-command path: resolve an editable current layer, apply current
//! document style properties, and create one entity in one transaction.
//!
//! Registry validation handles types and ranges; the `req_*` helpers assert the
//! parameters required by each active variant.

use af_math::Point2;
use af_model::Layer;
use af_model::container::ContainerRef;
use af_model::entity::{EntityGeometry, EntityRecord};
use af_model::id::{EntityId, LayerId, ObjectId};

use crate::args::ParsedArgs;
use crate::spec::{CmdError, CommandCtx};

/// Returns the current layer when it is editable and visible.
///
/// Locked, frozen, and off layers are rejected explicitly.
pub(crate) fn editable_current_layer(ctx: &CommandCtx<'_>) -> Result<LayerId, CmdError> {
    let layer_id = ctx.document().current_layer();
    if let Some(layer) = ctx.document().layer(layer_id)
        && let Some(reason) = uneditable_reason(layer)
    {
        return Err(CmdError::Failed(format!(
            "cannot draw on layer '{}': {reason}",
            layer.name()
        )));
    }
    Ok(layer_id)
}

/// Returns why `layer` cannot be drawn on, prioritizing locked, frozen, then off.
fn uneditable_reason(layer: &Layer) -> Option<&'static str> {
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

/// Creates one model-space entity on the current layer with current document style
/// properties in a transaction labeled `label`.
///
/// Layer or geometry failures roll back atomically and count no transaction.
pub(crate) fn create_entity(
    ctx: &mut CommandCtx<'_>,
    label: &str,
    geometry: EntityGeometry,
) -> Result<EntityId, CmdError> {
    let layer = editable_current_layer(ctx)?;
    let record = EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        ctx.document().current_color(),
        ctx.document().current_line_type(),
        ctx.document().current_lineweight(),
        geometry,
    );
    ctx.transact(label, |tx| -> Result<EntityId, CmdError> {
        Ok(tx.add_entity(ContainerRef::ModelSpace, record)?)
    })
}

/// Returns a required point for the active command variant.
pub(crate) fn req_point(args: &ParsedArgs, name: &str) -> Result<Point2, CmdError> {
    args.point(name)
        .ok_or_else(|| CmdError::MissingParam(name.to_string()))
}

/// Returns a required distance for the active command variant.
pub(crate) fn req_distance(args: &ParsedArgs, name: &str) -> Result<f64, CmdError> {
    args.distance(name)
        .ok_or_else(|| CmdError::MissingParam(name.to_string()))
}
