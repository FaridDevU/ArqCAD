//! MATCHPROP (`MA`) copies layer, color, line type, and lineweight from one source
//! entity to a target set in one transaction. Geometry and visibility are unchanged.
//!
//! The entire target set is validated before mutation. The source is read-only and
//! may be on a locked or frozen layer.

use af_model::Layer;
use af_model::TxContext;
use af_model::container::ContainerRef;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the MATCHPROP specification with alias `MA`.
///
/// `source` must contain exactly one ID.
#[must_use]
pub fn matchprop_spec() -> CommandSpec {
    CommandSpec::new("MATCHPROP", "Matchprop", true, matchprop_exec)
        .alias("MA")
        .param(ParamSpec::required("source", ParamType::EntitySet))
        .param(ParamSpec::required("targets", ParamType::EntitySet))
}

/// Registers MATCHPROP.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(matchprop_spec())
}

fn matchprop_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let source = args
        .entity_set("source")
        .ok_or_else(|| CmdError::MissingParam("source".to_string()))?;
    let &[src] = source else {
        return Err(CmdError::Failed(format!(
            "MATCHPROP: 'source' must contain exactly 1 entity ({} given)",
            source.len()
        )));
    };
    let targets: Vec<EntityId> = args
        .entity_set("targets")
        .ok_or_else(|| CmdError::MissingParam("targets".to_string()))?
        .to_vec();

    ctx.transact("Matchprop", |tx| apply_matchprop(tx, src, &targets))?;
    Ok(CommandOutcome::new())
}

/// Atomically copies style properties from `src` to every target.
pub(crate) fn apply_matchprop(
    tx: &mut TxContext<'_>,
    src: EntityId,
    targets: &[EntityId],
) -> Result<(), CmdError> {
    let (src_rec, _) = tx.doc().entity(src).ok_or(CmdError::UnknownEntity(src))?;
    let layer = src_rec.layer;
    let color = src_rec.color;
    let line_type = src_rec.line_type;
    let lineweight = src_rec.lineweight;

    let mut locked: Vec<EntityId> = Vec::new();
    let mut foreign: Vec<EntityId> = Vec::new();
    for &id in targets {
        let (record, container) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        if container != ContainerRef::ModelSpace {
            foreign.push(id);
            continue;
        }
        if tx.doc().layer(record.layer).is_some_and(Layer::is_locked) {
            locked.push(id);
        }
    }
    if !locked.is_empty() {
        return Err(CmdError::Failed(format!(
            "MATCHPROP: target entities on locked layers cannot be changed: [{}]",
            join_ids(&locked)
        )));
    }
    if !foreign.is_empty() {
        return Err(CmdError::Failed(format!(
            "MATCHPROP: only model-space targets are supported; not in model space: [{}]",
            join_ids(&foreign)
        )));
    }

    for &id in targets {
        tx.modify_entity(id, |rec| {
            rec.layer = layer;
            rec.color = color;
            rec.line_type = line_type;
            rec.lineweight = lineweight;
        })?;
    }
    Ok(())
}

/// Formats IDs for diagnostics.
fn join_ids(ids: &[EntityId]) -> String {
    ids.iter()
        .map(|id| id.raw().0.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
