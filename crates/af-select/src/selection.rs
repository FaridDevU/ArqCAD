//! [`SelectionState`], an ordered runtime set of selected IDs.
//!
//! Selection order is stable. State is neither serialized nor transactional, and
//! its callback runs only when the set changes.

use af_model::Document;
use af_model::id::EntityId;
use indexmap::IndexSet;

/// Callback receiving the new selection after an effective change.
type SelectionCallback = Box<dyn FnMut(&[EntityId])>;

/// Ordered selection state with an optional callback.
#[derive(Default)]
pub struct SelectionState {
    items: IndexSet<EntityId>,
    /// Archived selection used by the `Previous` option; it does not emit callbacks.
    previous: Vec<EntityId>,
    /// One callback is sufficient for the runtime selection consumer.
    on_change: Option<SelectionCallback>,
}

impl SelectionState {
    /// Creates empty selection without a callback.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers or replaces the change callback.
    pub fn on_change(&mut self, cb: impl FnMut(&[EntityId]) + 'static) {
        self.on_change = Some(Box::new(cb));
    }

    /// Returns selected IDs in stable order.
    ///
    /// Returns a copy because `IndexSet` has no contiguous borrowed slice.
    #[must_use]
    pub fn items(&self) -> Vec<EntityId> {
        self.items.iter().copied().collect()
    }

    /// Number of selected IDs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether selection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns whether `id` is selected.
    #[must_use]
    pub fn contains(&self, id: EntityId) -> bool {
        self.items.contains(&id)
    }

    /// Replaces selection, deduplicating while preserving arrival order.
    pub fn set(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        let new: IndexSet<EntityId> = ids.into_iter().collect();
        let changed = self.items.iter().ne(new.iter());
        self.items = new;
        if changed {
            self.notify();
        }
    }

    /// Appends `id` when absent and notifies on change.
    pub fn add(&mut self, id: EntityId) {
        if self.items.insert(id) {
            self.notify();
        }
    }

    /// Toggles `id` and always notifies.
    pub fn toggle(&mut self, id: EntityId) {
        if !self.items.shift_remove(&id) {
            self.items.insert(id);
        }
        self.notify();
    }

    /// Removes `id` while preserving order and notifies when present.
    pub fn remove(&mut self, id: EntityId) {
        if self.items.shift_remove(&id) {
            self.notify();
        }
    }

    /// Clears selection and notifies when it was nonempty.
    pub fn clear(&mut self) {
        if !self.items.is_empty() {
            self.items.clear();
            self.notify();
        }
    }

    /// Archives deduplicated IDs as `Previous` without notifying.
    pub fn set_previous(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        let set: IndexSet<EntityId> = ids.into_iter().collect();
        self.previous = set.into_iter().collect();
    }

    /// Returns a copy of archived `Previous` selection.
    #[must_use]
    pub fn previous(&self) -> Vec<EntityId> {
        self.previous.clone()
    }

    /// Removes IDs absent from the document from live and archived selection.
    pub fn retain_existing(&mut self, doc: &Document) {
        let before = self.items.len();
        self.items.retain(|id| doc.entity(*id).is_some());
        self.previous.retain(|id| doc.entity(*id).is_some());
        if self.items.len() != before {
            self.notify();
        }
    }

    /// Invokes the callback with current selection when registered.
    fn notify(&mut self) {
        if let Some(cb) = self.on_change.as_mut() {
            let snapshot: Vec<EntityId> = self.items.iter().copied().collect();
            cb(&snapshot);
        }
    }
}
