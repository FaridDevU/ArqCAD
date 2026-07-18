//! Data-oriented storage with typed pools and generational handles.
//!
//! Each geometry variant has a [`TypedStore`] containing a contiguous [`Pool`]
//! plus parallel common-property columns. A handle resolves only while its slot
//! is occupied and its generation matches, which prevents stale-handle ABA bugs.
//! Retired generations are never reused.
//!
//! Pools expose physical iteration order only. The entity container owns drawing
//! order through [`EntityKey`] values that pair a [`GeoKind`] with a [`Handle`].
//! Persistent identity belongs to [`EntityId`](crate::id::EntityId); handles are
//! process-local storage locations and are never serialized.

#![allow(dead_code)] // Some typed storage helpers are exercised only by tests.

pub(crate) mod fill;
pub(crate) mod key;
pub(crate) mod pool;
pub(crate) mod store;
