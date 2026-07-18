//! [`Group`] is a named set of entities operated on as a unit.
//!
//! The document enforces a case-insensitively unique name. Members retain order;
//! `selectable` controls whether the group is selected as one unit.
//!
//! Getters expose private fields. Builder-style `with_*` methods create inert
//! values; changing a stored group still requires a transaction.

use serde::{Deserialize, Serialize};

use crate::id::{EntityId, GroupId};

/// A document entity group.
///
/// Members preserve serialized insertion order. Group IDs share the document ID
/// space and are assigned by transactions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    id: GroupId,
    name: String,
    /// Member entity IDs in insertion order.
    members: Vec<EntityId>,
    /// Whether the group is selected as one unit.
    selectable: bool,
}

impl Group {
    /// Creates an empty selectable group. Transactions assign its persistent ID.
    #[must_use]
    pub fn new(id: GroupId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            members: Vec::new(),
            selectable: true,
        }
    }

    /// Returns a copy with a replacement ID.
    #[must_use]
    pub(crate) fn with_id(mut self, id: GroupId) -> Self {
        self.id = id;
        self
    }

    /// Returns a copy with a replacement name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Returns a copy with replacement ordered members.
    #[must_use]
    pub fn with_members(mut self, members: Vec<EntityId>) -> Self {
        self.members = members;
        self
    }

    /// Returns a copy with a replacement selectable flag.
    #[must_use]
    pub fn with_selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Stable group ID.
    #[must_use]
    pub fn id(&self) -> GroupId {
        self.id
    }

    /// Case-insensitively unique group name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Member entity IDs in insertion order.
    #[must_use]
    pub fn members(&self) -> &[EntityId] {
        &self.members
    }

    /// Whether `id` is a member.
    #[must_use]
    pub fn contains(&self, id: EntityId) -> bool {
        self.members.contains(&id)
    }

    /// Whether the group is selected as one unit.
    #[must_use]
    pub fn is_selectable(&self) -> bool {
        self.selectable
    }
}
