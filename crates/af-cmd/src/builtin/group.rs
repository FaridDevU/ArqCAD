//! Group commands: GROUP (`G`) creates a named group, UNGROUP dissolves one, and
//! GROUPEDIT adds or removes members. Successful changes commit one transaction.
//!
//! Group changes use reversible [`TxContext`](af_model::TxContext) operations.
//! Names must be nonempty and unique.
//!
//! ponytail: reuse `layers_ops::FORBIDDEN_NAME_CHARS` if groups gain DXF export.

use af_model::Group;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the GROUP specification with alias `G`.
#[must_use]
pub fn group_spec() -> CommandSpec {
    CommandSpec::new("GROUP", "Group", true, group_exec)
        .alias("G")
        .param(ParamSpec::required("name", ParamType::Text))
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Returns the UNGROUP specification.
#[must_use]
pub fn ungroup_spec() -> CommandSpec {
    CommandSpec::new("UNGROUP", "Ungroup", true, ungroup_exec)
        .param(ParamSpec::required("name", ParamType::Text))
}

/// Returns the GROUPEDIT specification.
#[must_use]
pub fn groupedit_spec() -> CommandSpec {
    CommandSpec::new("GROUPEDIT", "Group Edit", true, groupedit_exec)
        .param(ParamSpec::required("name", ParamType::Text))
        .param(ParamSpec::optional("add", ParamType::EntitySet))
        .param(ParamSpec::optional("remove", ParamType::EntitySet))
}

/// Registers GROUP, UNGROUP, and GROUPEDIT.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(group_spec())?;
    registry.register(ungroup_spec())?;
    registry.register(groupedit_spec())?;
    Ok(())
}

/// Deduplicates while preserving first-appearance order.
fn dedup(ids: &[EntityId]) -> Vec<EntityId> {
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn group_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let name = args
        .text("name")
        .ok_or_else(|| CmdError::MissingParam("name".to_string()))?
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(CmdError::Failed("group name must not be empty".to_string()));
    }
    let members = dedup(
        args.entity_set("entities")
            .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?,
    );
    ctx.transact("Group", |tx| -> Result<(), CmdError> {
        tx.add_group_raw(
            Group::new(af_model::id::ObjectId::NIL.into(), name).with_members(members),
        )?;
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}

fn ungroup_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let name = args
        .text("name")
        .ok_or_else(|| CmdError::MissingParam("name".to_string()))?;
    let gid = ctx
        .document()
        .group_by_name(name)
        .map(Group::id)
        .ok_or_else(|| CmdError::Failed(format!("no group named {name:?}")))?;
    ctx.transact("Ungroup", |tx| {
        tx.remove_group_raw(gid).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

fn groupedit_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let name = args
        .text("name")
        .ok_or_else(|| CmdError::MissingParam("name".to_string()))?;
    let add: Vec<EntityId> = args
        .entity_set("add")
        .map(<[_]>::to_vec)
        .unwrap_or_default();
    let remove: Vec<EntityId> = args
        .entity_set("remove")
        .map(<[_]>::to_vec)
        .unwrap_or_default();

    let group = ctx
        .document()
        .group_by_name(name)
        .cloned()
        .ok_or_else(|| CmdError::Failed(format!("no group named {name:?}")))?;
    let gid = group.id();

    // Drop stale members, preserve surviving order, append additions, then remove requests.
    let doc = ctx.document();
    let mut members: Vec<EntityId> = group
        .members()
        .iter()
        .copied()
        .filter(|m| doc.entity(*m).is_some())
        .collect();
    for id in dedup(&add) {
        if !members.contains(&id) {
            members.push(id);
        }
    }
    members.retain(|m| !remove.contains(m));

    if members == group.members() {
        return Err(CmdError::Failed("group membership unchanged".to_string()));
    }
    ctx.transact("Group Edit", |tx| {
        tx.modify_group_raw(gid, group.with_members(members))
            .map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}
