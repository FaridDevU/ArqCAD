//! [`ChangeSet`] is emitted by each committed transaction and undo/redo so the
//! spatial index, renderer, and UI share one synchronization path.
//!
//! It summarizes the net effect of applying a [`Transaction`] for a [`Cause`]
//! by folding operations and deduplicating by ID:
//!
//! - An entity created and removed in one transaction is omitted.
//! - An entity created then modified appears only in `added`.
//! - An entity modified then removed appears only in `removed`.
//! - A modification with no net state change is omitted.
//!
//! Layer-table changes use the same rules in `layers_changed`. Changing the
//! current layer is a document-property change (`doc_changed`).
//!
//! Vectors are returned in deterministic ascending ID order.
//!
//! # Direction by cause
//!
//! [`Cause::Do`] and [`Cause::Redo`] fold forward. [`Cause::Undo`] folds reversed
//! operations backward, matching [`apply_inverse`](crate::tx::apply_inverse).
//!
//! This session event is not serialized.

use std::collections::HashMap;
use std::hash::Hash;

use crate::entity::EntityRecord;
use crate::id::{EntityId, LayerId};
use crate::layers::Layer;
use crate::tx::{DocOp, Transaction};

/// Cause that produced a [`ChangeSet`].
///
/// Consumers synchronize the same way for all variants but may adjust UI behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// A normal forward transaction.
    Do,
    /// An inverse undo application.
    Undo,
    /// A forward redo application.
    Redo,
}

/// Deduplicated net effect of a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    added: Vec<EntityId>,
    removed: Vec<EntityId>,
    modified: Vec<EntityId>,
    layers_changed: Vec<LayerId>,
    doc_changed: bool,
    cause: Cause,
    tx_seq: u64,
}

impl ChangeSet {
    /// Net added entities, sorted by ID.
    #[must_use]
    pub fn added(&self) -> &[EntityId] {
        &self.added
    }

    /// Net removed entities, sorted by ID.
    #[must_use]
    pub fn removed(&self) -> &[EntityId] {
        &self.removed
    }

    /// Net modified entities, sorted by ID.
    #[must_use]
    pub fn modified(&self) -> &[EntityId] {
        &self.modified
    }

    /// Net changed layer-table entries, sorted by ID.
    ///
    /// Consumers query the document to distinguish additions, removals, and
    /// modifications. Changing the current layer sets [`doc_changed`] instead.
    ///
    /// [`doc_changed`]: ChangeSet::doc_changed
    #[must_use]
    pub fn layers_changed(&self) -> &[LayerId] {
        &self.layers_changed
    }

    /// Whether a document property or the group table changed.
    #[must_use]
    pub fn doc_changed(&self) -> bool {
        self.doc_changed
    }

    /// Cause of the change.
    #[must_use]
    pub fn cause(&self) -> Cause {
        self.cause
    }

    /// Sequence number of the originating transaction.
    #[must_use]
    pub fn tx_seq(&self) -> u64 {
        self.tx_seq
    }

    /// Whether there is no net change to synchronize.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.modified.is_empty()
            && self.layers_changed.is_empty()
            && !self.doc_changed
    }

    /// Builds a transaction change set in the direction implied by its cause.
    pub(crate) fn from_transaction(transaction: &Transaction, cause: Cause) -> Self {
        let inverse = matches!(cause, Cause::Undo);
        Self::build(transaction.ops(), transaction.seq(), cause, inverse)
    }

    fn build<'a>(ops: &'a [DocOp], tx_seq: u64, cause: Cause, inverse: bool) -> Self {
        let mut entities: HashMap<EntityId, Delta<'a, EntityRecord>> = HashMap::new();
        let mut layers: HashMap<LayerId, Delta<'a, Layer>> = HashMap::new();
        let mut doc_changed = false;

        // Forward operations stay ordered; inverse operations run in reverse.
        if inverse {
            for op in ops.iter().rev() {
                doc_changed |= fold_op(&mut entities, &mut layers, op, inverse);
            }
        } else {
            for op in ops {
                doc_changed |= fold_op(&mut entities, &mut layers, op, inverse);
            }
        }

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();
        for (id, d) in entities {
            match (d.origin_present, d.end_present) {
                (false, true) => added.push(id),
                (true, false) => removed.push(id),
                // Present at both ends: changed only when the records differ.
                (true, true) => {
                    if d.start_rec != d.end_rec {
                        modified.push(id);
                    }
                }
                // Absent at both ends: created then removed, so omit it.
                (false, false) => {}
            }
        }
        added.sort_unstable_by_key(|id| id.raw().0);
        removed.sort_unstable_by_key(|id| id.raw().0);
        modified.sort_unstable_by_key(|id| id.raw().0);

        // Layer additions, removals, and modifications share one net-change list.
        let mut layers_changed = Vec::new();
        for (id, d) in layers {
            match (d.origin_present, d.end_present) {
                (false, true) | (true, false) => layers_changed.push(id),
                (true, true) => {
                    if d.start_rec != d.end_rec {
                        layers_changed.push(id);
                    }
                }
                (false, false) => {}
            }
        }
        layers_changed.sort_unstable_by_key(|id| id.raw().0);

        ChangeSet {
            added,
            removed,
            modified,
            layers_changed,
            doc_changed,
            cause,
            tx_seq,
        }
    }
}

/// Net state of an entity or layer record while folding a `ChangeSet`.
///
/// Records are borrowed without cloning and only compare initial and final state.
struct Delta<'a, R> {
    /// Present before the first touching operation.
    origin_present: bool,
    /// Present after the last touching operation.
    end_present: bool,
    /// Initial record used for net comparison.
    start_rec: Option<&'a R>,
    /// Final record.
    end_rec: Option<&'a R>,
}

/// Folds an operation into entity and layer deltas for the selected direction.
/// Returns whether it changed a document property.
fn fold_op<'a>(
    entities: &mut HashMap<EntityId, Delta<'a, EntityRecord>>,
    layers: &mut HashMap<LayerId, Delta<'a, Layer>>,
    op: &'a DocOp,
    inverse: bool,
) -> bool {
    match op {
        DocOp::AddEntity { record, .. } => {
            if inverse {
                disappear(entities, record.id, record);
            } else {
                appear(entities, record.id, record);
            }
            false
        }
        DocOp::RemoveEntity { record, .. } => {
            if inverse {
                appear(entities, record.id, record);
            } else {
                disappear(entities, record.id, record);
            }
            false
        }
        DocOp::ModifyEntity { before, after, .. } => {
            let (from, to) = if inverse {
                (after, before)
            } else {
                (before, after)
            };
            alter(entities, from.id, from, to);
            false
        }
        DocOp::AddLayer { layer, .. } => {
            if inverse {
                disappear(layers, layer.id(), layer);
            } else {
                appear(layers, layer.id(), layer);
            }
            false
        }
        DocOp::RemoveLayer { layer, .. } => {
            if inverse {
                appear(layers, layer.id(), layer);
            } else {
                disappear(layers, layer.id(), layer);
            }
            false
        }
        DocOp::ModifyLayer { before, after } => {
            let (from, to) = if inverse {
                (after, before)
            } else {
                (before, after)
            };
            alter(layers, from.id(), from, to);
            false
        }
        // Groups are not rendered, so their table uses the document-change flag.
        DocOp::AddGroup { .. } | DocOp::ModifyGroup { .. } | DocOp::RemoveGroup { .. } => true,
        // Document properties have no ID to fold, so they set `doc_changed`.
        DocOp::SetDocProp(_) => true,
        // Line-type table changes require catalog refresh but no render delta.
        DocOp::AddLineType { .. } | DocOp::RemoveLineType { .. } => true,
    }
}

/// Records that `rec` becomes present.
fn appear<'a, K: Eq + Hash, R>(map: &mut HashMap<K, Delta<'a, R>>, key: K, rec: &'a R) {
    let e = map.entry(key).or_insert(Delta {
        origin_present: false,
        end_present: false,
        start_rec: None,
        end_rec: None,
    });
    e.end_present = true;
    e.end_rec = Some(rec);
}

/// Records that `rec` becomes absent.
fn disappear<'a, K: Eq + Hash, R>(map: &mut HashMap<K, Delta<'a, R>>, key: K, rec: &'a R) {
    let e = map.entry(key).or_insert(Delta {
        origin_present: true,
        end_present: true,
        start_rec: Some(rec),
        end_rec: None,
    });
    e.end_present = false;
    e.end_rec = None;
}

/// Records a change from `from` to `to` for a present record.
fn alter<'a, K: Eq + Hash, R>(map: &mut HashMap<K, Delta<'a, R>>, key: K, from: &'a R, to: &'a R) {
    let e = map.entry(key).or_insert(Delta {
        origin_present: true,
        end_present: true,
        start_rec: Some(from),
        end_rec: None,
    });
    e.end_present = true;
    e.end_rec = Some(to);
}
