#![forbid(unsafe_code)]
//! Selection and spatial indexing with read-only document access.
//!
//! - [`SpatialIndex`] bulk-loads and incrementally updates derived AABBs.
//! - Hit testing provides point, window, polygon, fence, and property queries.
//! - [`SelectionState`] stores ordered runtime IDs and change callbacks.
//! - [`snap`](snap()) ranks nearby declared and calculated snap points.
//!
//! Off, frozen, and hidden entities are excluded. Locked entities remain selectable
//! and are marked through [`Hit::locked`].

mod filter;
mod hit;
mod index;
mod poly;
mod selection;
mod snap;

pub use filter::{EntityKind, SelectionFilter, apply_filter, select_similar};
pub use hit::{Hit, WindowMode, pick, pick_all, select_window};
pub use index::SpatialIndex;
pub use poly::{select_fence, select_polygon};
pub use selection::SelectionState;
pub use snap::{SnapHit, SnapMask, SnapOpts, snap};
