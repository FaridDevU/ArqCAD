//! Document object identifiers.
//!
//! Every persistent object receives an [`ObjectId`] from one document-wide space
//! managed by [`IdAllocator`]. IDs are monotonic and never recycled. Zero is the
//! invalid [`ObjectId::NIL`] value and is never allocated.
//!
//! Typed wrappers share that numeric space while preventing accidental cross-type use.
//!
//! ```compile_fail
//! use af_model::id::{EntityId, LayerId, ObjectId};
//!
//! fn needs_entity(_: EntityId) {}
//!
//! let layer: LayerId = ObjectId(1).into();
//! needs_entity(layer); // error[E0308]: expected `EntityId`, found `LayerId`
//! ```

use serde::{Deserialize, Serialize};

/// Opaque document object identifier.
///
/// Values are `u64 >= 1`; zero is [`ObjectId::NIL`]. IDs come from one
/// document-wide space and are never recycled, preserving unambiguous references.
///
/// Serializes as a plain JSON number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

impl ObjectId {
    /// Invalid nil value, never returned by [`IdAllocator::alloc`].
    pub const NIL: ObjectId = ObjectId(0);

    /// Whether this ID is nil.
    #[must_use]
    pub fn is_nil(&self) -> bool {
        self.0 == 0
    }
}

/// Defines a typed ID wrapper over [`ObjectId`].
///
/// Each generated wrapper is distinct despite sharing the document ID space.
macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Typed [`ObjectId`] wrapper in the shared, non-recycling document ID space.
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(ObjectId);

        impl $name {
            /// Returns the underlying [`ObjectId`].
            #[must_use]
            pub fn raw(&self) -> ObjectId {
                self.0
            }
        }

        impl From<ObjectId> for $name {
            fn from(id: ObjectId) -> Self {
                $name(id)
            }
        }
    };
}

id_newtype!(
    EntityId,
    "Identificador de una entidad (línea, círculo, polilínea, ...)."
);
id_newtype!(LayerId, "Identificador de una capa.");
id_newtype!(BlockId, "Identificador de una definición de bloque.");
id_newtype!(
    StyleId,
    "Identificador de un estilo (de texto, de cota, ...)."
);
id_newtype!(LayoutId, "Identificador de un layout (paper space).");
id_newtype!(GroupId, "Identificador de un grupo de objetos.");
id_newtype!(ViewportId, "Identificador de un viewport de layout.");
id_newtype!(MaterialId, "Identificador de un material.");

/// Monotonic document [`ObjectId`] allocator.
///
/// Each allocation returns `next` and increments it. Values never decrease or recycle.
///
/// Serializes as `{"next": n}`. `u64::MAX` is terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdAllocator {
    next: u64,
}

/// Persistent ID space is exhausted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdExhausted;

impl core::fmt::Display for IdExhausted {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "persistent object id space exhausted")
    }
}

impl std::error::Error for IdExhausted {}

impl IdAllocator {
    /// Creates an allocator whose first allocation is `ObjectId(1)`.
    #[must_use]
    pub fn new() -> Self {
        IdAllocator { next: 1 }
    }

    /// Allocates a new unique, non-nil ID and advances the counter.
    ///
    /// # Errors
    /// Returns [`IdExhausted`] for a zero or terminal counter without modifying it.
    pub fn alloc(&mut self) -> Result<ObjectId, IdExhausted> {
        if self.next == 0 || self.next == u64::MAX {
            return Err(IdExhausted);
        }
        let id = ObjectId(self.next);
        self.next += 1;
        Ok(id)
    }

    /// Peeks at the next ID without consuming it.
    #[must_use]
    pub fn peek(&self) -> ObjectId {
        ObjectId(self.next)
    }

    /// Raises the cursor above `max_seen` without ever lowering it.
    ///
    /// # Errors
    /// Returns [`IdExhausted`] when no value exists above `max_seen`.
    pub fn ensure_above(&mut self, max_seen: u64) -> Result<(), IdExhausted> {
        if max_seen == u64::MAX {
            self.next = u64::MAX;
            return Err(IdExhausted);
        }
        if self.next <= max_seen {
            self.next = max_seen + 1;
        }
        Ok(())
    }
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::new()
    }
}
