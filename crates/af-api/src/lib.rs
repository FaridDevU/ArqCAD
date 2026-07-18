#![forbid(unsafe_code)]
//! ArcCAD core facade and WASM bindings.
//!
//! [`ApiSession`] is the single UI boundary. It owns the model session, spatial
//! index, selection, cached render model, and built-in command registry. Internal
//! state is never exposed through mutable references.
//!
//! # Delegation
//! Geometry, commands, persistence, and DXF remain in their respective `af-*`
//! crates. Every document mutation goes through a transaction.
//!
//! # Boundary invariants
//! - All input is validated; unknown IDs produce typed errors.
//! - Failures cross as serializable [`ApiError`] values, never panics.
//! - After each [`ApiSession::execute`], the spatial index, selection, and render
//!   model are synchronized with the same post-transaction document.
//!
//! # Render buffers
//! Control data crosses as structured JSON, while geometry uses packed `f32`
//! arrays. WASM currently copies those arrays into `Float32Array` values.

mod dto;
mod error;
mod session;

pub use dto::{
    ApiEvent, BatchKeyView, BatchView, ColorView, CommandInfo, DocInfo, DxfReport, EntityProps,
    ExecuteResult, GroupInfo, HitView, LayerInfo, LineTypeRefView, LineweightView, MarkerView,
    ParamInfo, ParsedPoint, PreviewView, RenderDeltaView, RenderView, SelectionFilterView,
    SnapView, StripView, SysvarValueView,
};
pub use error::ApiError;
pub use session::ApiSession;

/// Semantic version of the plugin and scripting API.
///
/// This is the API version, not the application version. `0.x` is unstable.
pub const API_VERSION: &str = "0.1.0";

#[cfg(feature = "wasm")]
mod wasm;
