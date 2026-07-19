//! [`Session`] owns the [`Document`], gates mutation through
//! [`transact`](Session::transact), and manages undo/redo history.
//!
//! Confirmed non-empty transactions enter [`History`]. Undo and redo apply stored
//! snapshots and emit [`ChangeSet`] events through the same channel as transactions.
//!
//! Undo/redo do not rerun commands or revalidate command rules.
//!
//! No public mutable document access exists; external mutation requires a [`TxContext`].

use crate::changeset::{Cause, ChangeSet};
use crate::doc::Document;
use crate::history::History;
use crate::id::LayerId;
use crate::sysvar::{SysvarDef, SysvarError, SysvarTable, SysvarValue};
use crate::tx::{self, Transaction, TxContext, TxError};
use crate::units::Units;

/// Result of [`Session::transact`].
///
/// Empty and semantic no-op transactions return `None` for both transaction
/// and change set.
#[derive(Debug)]
pub struct TxOutcome<T> {
    /// Value returned by the closure.
    pub value: T,
    /// Built transaction, or `None` when empty.
    pub transaction: Option<Transaction>,
    /// Change event, or `None` when empty.
    pub change_set: Option<ChangeSet>,
}

/// [`Session::undo`] error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoError {
    /// No transaction can be undone.
    NothingToUndo,
}

impl core::fmt::Display for UndoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UndoError::NothingToUndo => write!(f, "nothing to undo"),
        }
    }
}

impl std::error::Error for UndoError {}

/// [`Session::redo`] error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedoError {
    /// No transaction can be redone.
    NothingToRedo,
}

impl core::fmt::Display for RedoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RedoError::NothingToRedo => write!(f, "nothing to redo"),
        }
    }
}

impl std::error::Error for RedoError {}

/// Editing state for a document and its transaction history.
#[derive(Clone)]
pub struct Session {
    doc: Document,
    /// Next confirmed-transaction sequence number.
    next_seq: u64,
    /// Session-only undo/redo history.
    history: History,
    /// Previous off states from the most recent LAYISO, consumed by LAYUNISO.
    layer_iso_backup: Option<Vec<(LayerId, bool)>>,
    /// Session-only system variables, excluded from serialization and undo/redo.
    sysvars: SysvarTable,
}

impl Session {
    /// Creates a session with a new document in the requested units.
    #[must_use]
    pub fn new(units: Units) -> Self {
        Self {
            doc: Document::new(units),
            next_seq: 0,
            history: History::new(),
            layer_iso_backup: None,
            sysvars: SysvarTable::default(),
        }
    }

    /// Takes ownership of an existing document with empty history.
    #[must_use]
    pub fn from_document(doc: Document) -> Self {
        Self {
            doc,
            next_seq: 0,
            history: History::new(),
            layer_iso_backup: None,
            sysvars: SysvarTable::default(),
        }
    }

    /// Immutable document access; mutation requires [`transact`](Session::transact).
    ///
    /// Crate-private `document_mut` prevents external mutable access:
    ///
    /// ```compile_fail
    /// use af_model::{Document, Session};
    /// use af_model::units::Units;
    ///
    /// let mut session = Session::new(Units::default());
    /// // error[E0624]: `document_mut` is private (`pub(crate)`)
    /// let _doc: &mut Document = session.document_mut();
    /// ```
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Crate-private mutable document access.
    #[allow(dead_code)]
    pub(crate) fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// Runs a closure with a [`TxContext`], committing on success or rolling back
    /// atomically on error.
    ///
    /// Successful state-changing closures commit and return a
    /// transaction/change set; empty closures and semantic no-ops return
    /// neither. Errors restore the pre-transaction snapshot.
    ///
    /// Closure errors must support conversion from [`TxError`].
    ///
    /// Only committed non-empty transactions enter history.
    ///
    /// Exclusive borrowing prevents nested transactions:
    ///
    /// ```compile_fail
    /// use af_model::{Session, TxError};
    /// use af_model::units::Units;
    ///
    /// let mut session = Session::new(Units::default());
    /// session
    ///     .transact("outer", |_tx| -> Result<(), TxError> {
    ///         // error[E0499]: the outer transaction already borrows `session`.
    ///         session.transact("inner", |_tx2| -> Result<(), TxError> { Ok(()) })?;
    ///         Ok(())
    ///     })
    ///     .unwrap();
    /// ```
    ///
    /// # Errors
    /// Returns the closure error after rollback, or an internal allocator error.
    pub fn transact<T, E, F>(&mut self, label: impl Into<String>, f: F) -> Result<TxOutcome<T>, E>
    where
        F: FnOnce(&mut TxContext<'_>) -> Result<T, E>,
        E: From<TxError>,
    {
        // ponytail: snapshot rollback is fail-closed; optimize only if profiling requires it.
        let before = self.doc.clone();
        let start_next = self.doc.next_object_id();
        let mut ctx = TxContext::new(&mut self.doc, start_next);
        let result = f(&mut ctx);
        // Consume the context to release its document borrow and recover operations.
        let (ops, id_cursor) = ctx.into_parts();

        match result {
            Ok(value) => {
                if ops.is_empty() {
                    return Ok(TxOutcome {
                        value,
                        transaction: None,
                        change_set: None,
                    });
                }
                if self.doc == before {
                    self.doc = before;
                    return Ok(TxOutcome {
                        value,
                        transaction: None,
                        change_set: None,
                    });
                }
                // Commit the deferred allocator cursor only after successful operations.
                if self.doc.advance_id_cursor_to(id_cursor).is_err() {
                    self.doc = before;
                    return Err(E::from(TxError::Internal(
                        "persistent object id space exhausted",
                    )));
                }
                let seq = self.next_seq;
                self.next_seq += 1;
                let transaction = Transaction::new(seq, label.into(), ops);
                let change_set = ChangeSet::from_transaction(&transaction, Cause::Do);
                // Clone because both history and the caller retain the transaction.
                self.history.record(transaction.clone());
                Ok(TxOutcome {
                    value,
                    transaction: Some(transaction),
                    change_set: Some(change_set),
                })
            }
            Err(e) => {
                self.doc = before;
                Err(e)
            }
        }
    }

    /// Applies the latest transaction's stored inverse, moves it to redo, and
    /// emits a [`Cause::Undo`] change set.
    ///
    /// Undo never decrements `nextObjectId`.
    ///
    /// Returns `Ok(None)` for an empty stack and a typed replay error without
    /// changing the document or history when the stored inverse is invalid.
    pub fn try_undo(&mut self) -> Result<Option<ChangeSet>, TxError> {
        let Some(transaction) = self.history.next_undo().cloned() else {
            return Ok(None);
        };
        let mut doc = self.doc.clone();
        tx::apply_inverse(&mut doc, &transaction)?;
        let change_set = ChangeSet::from_transaction(&transaction, Cause::Undo);
        self.doc = doc;
        let transaction = self.history.take_undo().expect("peeked undo transaction");
        self.history.push_undone(transaction);
        Ok(Some(change_set))
    }

    /// Applies undo through [`Session::try_undo`] while retaining the original API.
    ///
    /// # Errors
    /// Returns [`UndoError::NothingToUndo`] for an empty stack or invalid replay.
    pub fn undo(&mut self) -> Result<ChangeSet, UndoError> {
        match self.try_undo() {
            Ok(Some(change_set)) => Ok(change_set),
            Ok(None) | Err(_) => Err(UndoError::NothingToUndo),
        }
    }

    /// Reapplies the latest redo transaction, returns it to undo, and emits a
    /// [`Cause::Redo`] change set.
    ///
    /// Restores original snapshot IDs without changing `nextObjectId`.
    ///
    /// Returns `Ok(None)` for an empty stack and a typed replay error without
    /// changing the document or history when stored operations are invalid.
    pub fn try_redo(&mut self) -> Result<Option<ChangeSet>, TxError> {
        let Some(transaction) = self.history.next_redo().cloned() else {
            return Ok(None);
        };
        let mut doc = self.doc.clone();
        tx::apply_forward(&mut doc, &transaction)?;
        let change_set = ChangeSet::from_transaction(&transaction, Cause::Redo);
        self.doc = doc;
        let transaction = self.history.take_redo().expect("peeked redo transaction");
        self.history.push_redone(transaction);
        Ok(Some(change_set))
    }

    /// Applies redo through [`Session::try_redo`] while retaining the original API.
    ///
    /// # Errors
    /// Returns [`RedoError::NothingToRedo`] for an empty stack or invalid replay.
    pub fn redo(&mut self) -> Result<ChangeSet, RedoError> {
        match self.try_redo() {
            Ok(Some(change_set)) => Ok(change_set),
            Ok(None) | Err(_) => Err(RedoError::NothingToRedo),
        }
    }

    /// Immutable history access.
    #[must_use]
    pub fn history(&self) -> &History {
        &self.history
    }

    /// Whether anything can be undone.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether anything can be redone.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Label of the next undo transaction.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    /// Label of the next redo transaction.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    /// Undo labels from oldest to newest.
    #[must_use]
    pub fn history_labels(&self) -> Vec<&str> {
        self.history.undo_labels()
    }

    /// Committed undo transactions from newest to oldest.
    #[must_use]
    pub fn undo_transactions(&self) -> impl DoubleEndedIterator<Item = &Transaction> + '_ {
        self.history.undo_transactions()
    }

    /// Sets the undo limit; zero disables history.
    pub fn set_undo_limit(&mut self, limit: usize) {
        self.history.set_limit(limit);
    }

    /// Current undo limit.
    #[must_use]
    pub fn undo_limit(&self) -> usize {
        self.history.limit()
    }

    /// Pending LAYISO state to restore.
    #[must_use]
    pub fn layer_iso_backup(&self) -> Option<&[(LayerId, bool)]> {
        self.layer_iso_backup.as_deref()
    }

    /// Replaces the LAYISO backup with the most recent isolation state.
    pub fn set_layer_iso_backup(&mut self, backup: Vec<(LayerId, bool)>) {
        self.layer_iso_backup = Some(backup);
    }

    /// Takes and clears the pending LAYISO backup.
    pub fn take_layer_iso_backup(&mut self) -> Option<Vec<(LayerId, bool)>> {
        self.layer_iso_backup.take()
    }

    // System variables.

    /// Current case-insensitive system-variable value.
    ///
    /// System variables are session state and do not enter transactions or history.
    #[must_use]
    pub fn sysvar(&self, name: &str) -> Option<SysvarValue> {
        self.sysvars.get(name)
    }

    /// Metadata for a case-insensitive system-variable name.
    #[must_use]
    pub fn sysvar_def(&self, name: &str) -> Option<&'static SysvarDef> {
        self.sysvars.def(name)
    }

    /// Sets a system variable after validating type and range.
    ///
    /// # Errors
    /// Returns [`SysvarError`] without changing the previous value.
    pub fn set_sysvar(&mut self, name: &str, value: SysvarValue) -> Result<(), SysvarError> {
        self.sysvars.set(name, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;

    use crate::container::ContainerRef;
    use crate::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
    use crate::groups::Group;
    use crate::id::{EntityId, ObjectId};
    use crate::layers::Layer;

    fn point_rec(session: &Session) -> EntityRecord {
        EntityRecord::new(
            crate::id::ObjectId::NIL.into(),
            session.document().current_layer(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(1.0, 2.0))),
        )
    }

    fn session_with_next_object_id(next: u64) -> Session {
        let doc = Document::new(Units::default());
        let mut value = serde_json::to_value(doc).unwrap();
        value["nextObjectId"] = serde_json::json!(next);
        Session::from_document(serde_json::from_value(value).unwrap())
    }

    fn assert_id_failure_left_session_unchanged(session: &Session, before: &str, next: u64) {
        assert_eq!(serde_json::to_string(session.document()).unwrap(), before);
        assert_eq!(session.document().next_object_id(), next);
        assert!(!session.can_undo());
        assert!(!session.can_redo());
    }

    #[test]
    fn seq_incrementa_solo_en_transacciones_confirmadas() {
        let mut session = Session::new(Units::default());
        let rec = point_rec(&session);
        let a = session
            .transact("a", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec.clone())
            })
            .unwrap();
        assert_eq!(a.transaction.as_ref().unwrap().seq(), 0);

        // Empty transactions do not consume sequence numbers.
        let empty = session
            .transact("empty", |_tx| -> Result<(), TxError> { Ok(()) })
            .unwrap();
        assert!(empty.transaction.is_none());

        let rec2 = point_rec(&session);
        let b = session
            .transact("b", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec2)
            })
            .unwrap();
        assert_eq!(b.transaction.as_ref().unwrap().seq(), 1); // 0 -> 1, no gap.
    }

    #[test]
    fn id_exhaustion_is_fail_closed_for_entity_linetype_layer_and_group() {
        const EXHAUSTED: &str = "persistent object id space exhausted";

        let mut session = session_with_next_object_id(u64::MAX);
        let before = serde_json::to_string(session.document()).unwrap();
        let rec = point_rec(&session);
        let err = session
            .transact("entity", |tx| tx.add_entity(ContainerRef::ModelSpace, rec))
            .unwrap_err();
        assert_eq!(err, TxError::Internal(EXHAUSTED));
        assert_id_failure_left_session_unchanged(&session, &before, u64::MAX);

        let mut session = session_with_next_object_id(u64::MAX);
        let before = serde_json::to_string(session.document()).unwrap();
        let err = session
            .transact("linetype", |tx| {
                tx.add_line_type_raw("DASHED", "", vec![1.0, -1.0])
            })
            .unwrap_err();
        assert_eq!(err, TxError::Internal(EXHAUSTED));
        assert_id_failure_left_session_unchanged(&session, &before, u64::MAX);

        let mut session = session_with_next_object_id(u64::MAX);
        let before = serde_json::to_string(session.document()).unwrap();
        let continuous = session.document().line_types().next().unwrap().id();
        let layer = Layer::new(
            ObjectId::NIL.into(),
            "X",
            Color::ByLayer,
            continuous,
            Lineweight::ByLayer,
        );
        let err = session
            .transact("layer", |tx| tx.add_layer_raw(layer))
            .unwrap_err();
        assert_eq!(err, TxError::Internal(EXHAUSTED));
        assert_id_failure_left_session_unchanged(&session, &before, u64::MAX);

        let mut session = session_with_next_object_id(u64::MAX);
        let before = serde_json::to_string(session.document()).unwrap();
        let err = session
            .transact("group", |tx| {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G"))
            })
            .unwrap_err();
        assert_eq!(err, TxError::Internal(EXHAUSTED));
        assert_id_failure_left_session_unchanged(&session, &before, u64::MAX);
    }

    #[test]
    fn sysvars_son_estado_de_sesion_no_del_documento() {
        use crate::sysvar::SysvarValue;
        let mut session = Session::new(Units::default());
        // Factory default.
        assert_eq!(session.sysvar("OSMODE"), Some(SysvarValue::Int(4133)));
        // System-variable writes create no transaction.
        session.set_sysvar("OSMODE", SysvarValue::Int(191)).unwrap();
        assert_eq!(session.sysvar("OSMODE"), Some(SysvarValue::Int(191)));
        assert!(
            !session.can_undo(),
            "una sysvar no entra en el stack de undo"
        );
        // Case-insensitive lookup and accessible metadata.
        assert_eq!(session.sysvar("osmode"), Some(SysvarValue::Int(191)));
        assert_eq!(session.sysvar_def("osmode").unwrap().name, "OSMODE");
        // Out-of-range writes preserve the previous value.
        assert!(
            session
                .set_sysvar("APERTURE", SysvarValue::Int(999))
                .is_err()
        );
        assert_eq!(session.sysvar("APERTURE"), Some(SysvarValue::Int(10)));
    }

    #[test]
    fn changeset_de_undo_invierte_la_semantica() {
        // An add viewed through undo becomes a removal.
        let mut session = Session::new(Units::default());
        let rec = point_rec(&session);
        let out = session
            .transact("add", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap();
        let id = out.value;
        let tx = out.transaction.unwrap();

        // Forward application places the ID in `added`.
        let cs_do = out.change_set.unwrap();
        assert_eq!(cs_do.added(), &[id]);
        assert!(cs_do.removed().is_empty());
        assert_eq!(cs_do.cause(), Cause::Do);

        // Undo places the same ID in `removed`.
        let cs_undo = ChangeSet::from_transaction(&tx, Cause::Undo);
        assert!(cs_undo.added().is_empty());
        assert_eq!(cs_undo.removed(), &[id]);
        assert_eq!(cs_undo.cause(), Cause::Undo);
        assert_eq!(cs_undo.tx_seq(), tx.seq());
    }

    #[test]
    fn undo_replay_invalido_no_cambia_documento_ni_stacks() {
        let mut session = Session::new(Units::default());
        let rec = point_rec(&session);
        let transaction = session
            .transact("add", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap()
            .transaction
            .unwrap();

        tx::apply_inverse(session.document_mut(), &transaction).unwrap();
        let before = serde_json::to_string(session.document()).unwrap();
        let depths = (
            session.history().undo_depth(),
            session.history().redo_depth(),
        );

        assert!(session.try_undo().is_err());
        assert_eq!(session.undo(), Err(UndoError::NothingToUndo));
        assert_eq!(serde_json::to_string(session.document()).unwrap(), before);
        assert_eq!(
            (
                session.history().undo_depth(),
                session.history().redo_depth()
            ),
            depths
        );
    }

    #[test]
    fn redo_replay_invalido_no_cambia_documento_ni_stacks() {
        let mut session = Session::new(Units::default());
        let rec = point_rec(&session);
        let id = session
            .transact("add", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, rec)
            })
            .unwrap()
            .value;
        let transaction = session
            .transact("remove", |tx| -> Result<(), TxError> {
                tx.remove_entity(id)
            })
            .unwrap()
            .transaction
            .unwrap();
        session.undo().unwrap();
        tx::apply_forward(session.document_mut(), &transaction).unwrap();
        let before = serde_json::to_string(session.document()).unwrap();
        let depths = (
            session.history().undo_depth(),
            session.history().redo_depth(),
        );

        assert!(session.try_redo().is_err());
        assert_eq!(session.redo(), Err(RedoError::NothingToRedo));
        assert_eq!(serde_json::to_string(session.document()).unwrap(), before);
        assert_eq!(
            (
                session.history().undo_depth(),
                session.history().redo_depth()
            ),
            depths
        );
    }
}
