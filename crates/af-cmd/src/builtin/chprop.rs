//! CHPROP changes one property (`layer`, `color`, `linetype`, or `lineweight`) on
//! an entity set in one atomic transaction.
//!
//! It has no standard PGP alias.
//!
//! The command validates the entire set before mutation. It parses the free-form
//! value through [`crate::builtin::style_value`] and never changes geometry or visibility.

use af_model::Layer;
use af_model::TxContext;
use af_model::container::ContainerRef;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::style_value::{parse_color, parse_layer_ref, parse_line_type, parse_lineweight};

/// Properties supported by CHPROP.
const PROPS: &[&str] = &["layer", "color", "linetype", "lineweight"];

/// Returns the CHPROP specification without aliases.
#[must_use]
pub fn chprop_spec() -> CommandSpec {
    CommandSpec::new("CHPROP", "Chprop", true, chprop_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required(
            "prop",
            ParamType::Enum(PROPS.iter().map(|s| (*s).to_string()).collect()),
        ))
        .param(ParamSpec::required("value", ParamType::Text))
}

/// Registers CHPROP.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(chprop_spec())
}

fn chprop_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let prop = args
        .enum_value("prop")
        .ok_or_else(|| CmdError::MissingParam("prop".to_string()))?
        .to_string();
    let value = args
        .text("value")
        .ok_or_else(|| CmdError::MissingParam("value".to_string()))?
        .to_string();

    ctx.transact("Chprop", |tx| apply_chprop(tx, &ids, &prop, &value))?;
    Ok(CommandOutcome::new())
}

/// Applies `prop = value` to all `ids` atomically.
///
/// Validation rejects unknown IDs, non-model-space entities, and locked layers
/// before any entity is changed.
pub(crate) fn apply_chprop(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    prop: &str,
    value: &str,
) -> Result<(), CmdError> {
    let mut locked: Vec<EntityId> = Vec::new();
    let mut foreign: Vec<EntityId> = Vec::new();
    for &id in ids {
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
            "CHPROP: entities on locked layers cannot be changed: [{}]",
            join_ids(&locked)
        )));
    }
    if !foreign.is_empty() {
        return Err(CmdError::Failed(format!(
            "CHPROP: only model-space entities can be changed; not in model space: [{}]",
            join_ids(&foreign)
        )));
    }

    match prop {
        "layer" => {
            let target = parse_layer_ref(tx.doc(), value)?;
            for &id in ids {
                tx.modify_entity(id, |rec| rec.layer = target)?;
            }
        }
        "color" => {
            let color = parse_color(value)?;
            for &id in ids {
                tx.modify_entity(id, |rec| rec.color = color)?;
            }
        }
        "linetype" => {
            let line_type = parse_line_type(tx.doc(), value)?;
            for &id in ids {
                tx.modify_entity(id, |rec| rec.line_type = line_type)?;
            }
        }
        "lineweight" => {
            let lineweight = parse_lineweight(value)?;
            for &id in ids {
                tx.modify_entity(id, |rec| rec.lineweight = lineweight)?;
            }
        }
        other => unreachable!("prop fuera del schema Enum de CHPROP: {other}"),
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
