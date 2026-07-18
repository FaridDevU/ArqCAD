//! Object visibility commands: ISOLATEOBJECTS hides unselected model-space
//! entities, HIDEOBJECTS hides selected entities, and UNISOLATEOBJECTS shows all.
//! Successful changes commit exactly one transaction; no-op requests fail.
//!
//! # Visibility storage
//!
//! Commands toggle the existing `EntityRecord::visible` flag through
//! `modify_entity`; rendering, selection, and extents already respect it.
//!
//! ISOLATEOBJECTS and UNISOLATEOBJECTS scan model space; HIDEOBJECTS changes the
//! validated IDs directly.

use af_model::TxContext;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the ISOLATEOBJECTS specification.
#[must_use]
pub fn isolate_spec() -> CommandSpec {
    CommandSpec::new("ISOLATEOBJECTS", "Isolate Objects", true, isolate_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Returns the HIDEOBJECTS specification.
#[must_use]
pub fn hide_spec() -> CommandSpec {
    CommandSpec::new("HIDEOBJECTS", "Hide Objects", true, hide_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Returns the parameterless UNISOLATEOBJECTS specification.
#[must_use]
pub fn unisolate_spec() -> CommandSpec {
    CommandSpec::new(
        "UNISOLATEOBJECTS",
        "End Object Isolation",
        true,
        unisolate_exec,
    )
}

/// Registers ISOLATEOBJECTS, HIDEOBJECTS, and UNISOLATEOBJECTS.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(isolate_spec())?;
    registry.register(hide_spec())?;
    registry.register(unisolate_spec())?;
    Ok(())
}

/// Sets visibility to `value` for `ids` in `tx`.
fn set_visible(tx: &mut TxContext<'_>, ids: &[EntityId], value: bool) -> Result<(), CmdError> {
    for &id in ids {
        tx.modify_entity(id, |rec| rec.visible = value)?;
    }
    Ok(())
}

fn hide_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    // Change only visible entities to avoid an empty transaction.
    let doc = ctx.document();
    let to_hide: Vec<EntityId> = ids
        .iter()
        .copied()
        .filter(|id| doc.entity(*id).is_some_and(|(rec, _)| rec.visible))
        .collect();
    if to_hide.is_empty() {
        return Err(CmdError::Failed(
            "nothing to hide (already hidden)".to_string(),
        ));
    }
    ctx.transact("Hide Objects", |tx| set_visible(tx, &to_hide, false))?;
    Ok(CommandOutcome::new())
}

fn isolate_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let keep: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    // Hide unselected model-space entities and reveal the isolated set.
    let doc = ctx.document();
    let mut to_hide: Vec<EntityId> = Vec::new();
    let mut to_show: Vec<EntityId> = Vec::new();
    for rec in doc.model_space().iter_records() {
        let keep_it = keep.contains(&rec.id);
        if keep_it && !rec.visible {
            to_show.push(rec.id);
        } else if !keep_it && rec.visible {
            to_hide.push(rec.id);
        }
    }
    if to_hide.is_empty() && to_show.is_empty() {
        return Err(CmdError::Failed("nothing to isolate".to_string()));
    }
    ctx.transact("Isolate Objects", |tx| {
        set_visible(tx, &to_hide, false)?;
        set_visible(tx, &to_show, true)
    })?;
    Ok(CommandOutcome::new())
}

fn unisolate_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Reveal every hidden model-space entity.
    let doc = ctx.document();
    let to_show: Vec<EntityId> = doc
        .model_space()
        .iter()
        .filter(|rec| !rec.visible)
        .map(|rec| rec.id)
        .collect();
    if to_show.is_empty() {
        return Err(CmdError::Failed("no hidden objects to show".to_string()));
    }
    ctx.transact("End Object Isolation", |tx| set_visible(tx, &to_show, true))?;
    Ok(CommandOutcome::new())
}
