//! Shared pre-mutation validation for editing commands.
//!
//! Every ID must exist in model space on an unlocked layer. Any invalid member
//! rejects the entire set before mutation.

use std::collections::BTreeSet;

use af_model::entity::EntityRecord;
use af_model::id::{EntityId, LayerId};
use af_model::{ContainerRef, Layer, TxContext};

use crate::spec::CmdError;

/// Returns cloned records for editable `ids` without mutating the document.
///
/// # Errors
/// Returns [`CmdError::UnknownEntity`] for missing IDs or [`CmdError::Failed`] for
/// non-model-space entities and locked layers.
pub(crate) fn validate_editable(
    tx: &TxContext<'_>,
    op: &str,
    ids: &[EntityId],
) -> Result<Vec<(EntityId, EntityRecord)>, CmdError> {
    let mut locked: Vec<EntityId> = Vec::new();
    let mut foreign: Vec<EntityId> = Vec::new();
    let mut records: Vec<(EntityId, EntityRecord)> = Vec::with_capacity(ids.len());

    for &id in ids {
        let (record, container) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        if container != ContainerRef::ModelSpace {
            foreign.push(id);
            continue;
        }
        if tx.doc().layer(record.layer).is_some_and(Layer::is_locked) {
            locked.push(id);
            continue;
        }
        records.push((id, record.clone()));
    }

    if !locked.is_empty() {
        return Err(CmdError::Failed(format!(
            "{op}: entities on locked layers cannot be used: [{}]",
            join_ids(&locked)
        )));
    }
    if !foreign.is_empty() {
        return Err(CmdError::Failed(format!(
            "{op}: only model-space entities are supported; not in model space: [{}]",
            join_ids(&foreign)
        )));
    }
    Ok(records)
}

/// Returns distinct layer IDs referenced by `ids` in stable ID order.
///
/// Unlike [`validate_editable`], this read-only lookup allows entities outside
/// model space and on locked layers because object-layer commands change layers,
/// not the referenced entities.
///
/// # Errors
/// Returns [`CmdError::UnknownEntity`] when an ID is missing.
pub(crate) fn distinct_layers(
    tx: &TxContext<'_>,
    ids: &[EntityId],
) -> Result<Vec<LayerId>, CmdError> {
    let mut seen: BTreeSet<LayerId> = BTreeSet::new();
    for &id in ids {
        let (record, _container) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        seen.insert(record.layer);
    }
    Ok(seen.into_iter().collect())
}

/// Formats IDs for diagnostics.
pub(crate) fn join_ids(ids: &[EntityId]) -> String {
    ids.iter()
        .map(|id| id.raw().0.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
