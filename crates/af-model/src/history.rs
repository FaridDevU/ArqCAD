//! [`History`] stores a session's committed [`Transaction`] undo/redo stacks.
//!
//! History stores transactions and stack policy. [`Session`](crate::session::Session)
//! applies them through [`apply_inverse`](crate::tx::apply_inverse) and
//! [`apply_forward`](crate::tx::apply_forward).
//!
//! # Stack model
//!
//! `undo` stores oldest-to-newest transactions; `redo` is LIFO. Recording a new
//! non-empty transaction clears redo and evicts the oldest undo entry over limit.
//!
//! A zero limit disables history and clears both stacks. History belongs to the
//! session and is never serialized with the document.

use std::collections::VecDeque;

use crate::tx::Transaction;

/// Default undo-stack limit in transactions.
pub const DEFAULT_UNDO_LIMIT: usize = 100;

/// Session undo/redo stacks with a configurable limit.
///
/// [`Session`](crate::session::Session) updates it through transaction operations.
#[derive(Debug)]
pub struct History {
    /// Committed transactions from oldest to newest.
    undo: VecDeque<Transaction>,
    /// Undone transactions in LIFO order.
    redo: Vec<Transaction>,
    /// Maximum retained undo transactions; zero disables history.
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    /// Creates empty history with [`DEFAULT_UNDO_LIMIT`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_UNDO_LIMIT)
    }

    /// Creates empty history with an explicit limit.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit,
        }
    }

    // Queries.

    /// Whether a transaction can be undone.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether a transaction can be redone.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Label of the next transaction to undo.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(Transaction::label)
    }

    /// Label of the next transaction to redo.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(Transaction::label)
    }

    /// Undo labels from oldest to newest.
    #[must_use]
    pub fn undo_labels(&self) -> Vec<&str> {
        self.undo.iter().map(Transaction::label).collect()
    }

    /// Borrows committed undo transactions from newest to oldest without popping.
    ///
    /// Supports finding a labeled transaction without undoing newer entries.
    #[must_use]
    pub fn undo_transactions(&self) -> impl DoubleEndedIterator<Item = &Transaction> + '_ {
        self.undo.iter().rev()
    }

    /// Number of undo transactions.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Number of redo transactions.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Current undo limit.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }

    // Limit mutation.

    /// Sets the undo-stack limit.
    ///
    /// Lower limits discard oldest undo entries. Zero clears and disables both stacks.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        if limit == 0 {
            self.undo.clear();
            self.redo.clear();
            return;
        }
        while self.undo.len() > limit {
            self.undo.pop_front();
        }
    }

    // Crate-private stack updates orchestrated by `Session`.

    /// Records a non-empty committed transaction, clears redo, and enforces limit.
    ///
    /// A zero limit stores nothing.
    pub(crate) fn record(&mut self, transaction: Transaction) {
        self.redo.clear();
        if self.limit == 0 {
            return;
        }
        self.undo.push_back(transaction);
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
    }

    /// Pops the newest undo transaction.
    pub(crate) fn take_undo(&mut self) -> Option<Transaction> {
        self.undo.pop_back()
    }

    /// Pops the newest redo transaction.
    pub(crate) fn take_redo(&mut self) -> Option<Transaction> {
        self.redo.pop()
    }

    /// Pushes a newly undone transaction onto redo.
    pub(crate) fn push_undone(&mut self, transaction: Transaction) {
        self.redo.push(transaction);
    }

    /// Returns a newly redone transaction to undo.
    ///
    /// Preserves redo and enforces any reduced undo limit.
    pub(crate) fn push_redone(&mut self, transaction: Transaction) {
        self.undo.push_back(transaction);
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;

    use crate::container::ContainerRef;
    use crate::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
    use crate::id::EntityId;
    use crate::session::Session;
    use crate::tx::{TxError, apply_forward, apply_inverse};
    use crate::units::Units;

    fn point_rec(session: &Session, x: f64) -> EntityRecord {
        EntityRecord::new(
            crate::id::ObjectId::NIL.into(),
            session.document().current_layer(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(x, 0.0))),
        )
    }

    fn add(session: &mut Session, x: f64) -> EntityId {
        let rec = point_rec(session, x);
        session
            .transact("add", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .expect("commit")
            .value
    }

    #[test]
    fn record_limpia_redo_y_respeta_limite() {
        let mut h = History::with_limit(2);
        // Record three transactions produced by a real session.
        let mut session = Session::new(Units::default());
        let rec = point_rec(&session, 0.0);
        let t0 = session
            .transact("a", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap()
            .transaction
            .unwrap();
        let rec = point_rec(&session, 1.0);
        let t1 = session
            .transact("b", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap()
            .transaction
            .unwrap();
        let rec = point_rec(&session, 2.0);
        let t2 = session
            .transact("c", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap()
            .transaction
            .unwrap();

        h.record(t0);
        h.record(t1);
        assert_eq!(h.undo_labels(), vec!["a", "b"]);
        h.record(t2); // Exceeds limit 2 and evicts "a".
        assert_eq!(h.undo_labels(), vec!["b", "c"]);
        assert_eq!(h.undo_depth(), 2);
    }

    #[test]
    fn set_limit_cero_vacia_ambas_pilas() {
        let mut session = Session::new(Units::default());
        add(&mut session, 0.0);
        add(&mut session, 1.0);
        session.undo().unwrap(); // Leaves one redo entry.
        assert!(session.can_undo());
        assert!(session.can_redo());

        session.set_undo_limit(0);
        assert!(!session.can_undo());
        assert!(!session.can_redo());

        // A zero limit records no new commit.
        add(&mut session, 2.0);
        assert!(!session.can_undo());
        assert!(!session.can_redo());
    }

    /// Undo and redo apply snapshots without revalidating command rules.
    ///
    /// The test injects a serde-produced locked-layer state while retaining history.
    #[test]
    fn undo_redo_no_revalidan_reglas_de_comando_capa_locked() {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let id = add(&mut session, 3.0);
        assert!(session.can_undo());

        // Inject the same document with layer 0 locked.
        let locked = lock_all_layers(session.document());
        assert!(locked.layer(l0).unwrap().is_locked());
        *session.document_mut() = locked;

        // Undo still succeeds without revalidation.
        session.undo().expect("undo ignora el lock posterior");
        assert!(session.document().entity(id).is_none());
        assert!(session.document().layer(l0).unwrap().is_locked());

        // Redo behaves the same way.
        session.redo().expect("redo ignora el lock posterior");
        assert!(session.document().entity(id).is_some());
    }

    #[test]
    fn apply_inverse_forward_ignoran_lock_de_capa() {
        // Verify the same invariant in the free functions used by `Session`.
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let rec = point_rec(&session, 7.0);
        let out = session
            .transact("add", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap();
        let id = out.value;
        let tx = out.transaction.unwrap();

        let mut locked = lock_all_layers(session.document());
        assert!(locked.layer(l0).unwrap().is_locked());
        assert!(locked.entity(id).is_some());

        apply_inverse(&mut locked, &tx).expect("inverse ignora lock");
        assert!(locked.entity(id).is_none());
        apply_forward(&mut locked, &tx).expect("forward ignora lock");
        assert!(locked.entity(id).is_some());
    }

    /// Marks all layers as locked through the public serialized document form.
    fn lock_all_layers(doc: &crate::doc::Document) -> crate::doc::Document {
        let mut v = serde_json::to_value(doc).expect("doc serializa");
        let layers = v
            .get_mut("layers")
            .and_then(serde_json::Value::as_object_mut)
            .expect("layers es un objeto");
        for (_id, layer) in layers.iter_mut() {
            let obj = layer.as_object_mut().expect("cada capa es un objeto");
            obj.insert("locked".to_string(), serde_json::Value::Bool(true));
        }
        serde_json::from_value(v).expect("doc con capa bloqueada deserializa")
    }
}
