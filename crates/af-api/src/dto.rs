//! Serializable facade DTOs: types that cross the JSON boundary.
//!
//! af-api does **not** serialize internal `af-*` crate types. It maps them to
//! stable DTOs versioned with the API. All fields serialize as `camelCase`, and
//! IDs cross the boundary as plain `u64` values.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Result of a successful `execute` call.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteResult {
    /// Committed transaction sequence, or `None` when no new transaction exists.
    pub tx_seq: Option<u64>,
    /// IDs of entities created by the command.
    pub created: Vec<u64>,
    /// Optional console or UI message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Absolute point resolved by [`parse_input`](crate::ApiSession::parse_input).
///
/// Coordinate input (`x,y`, `@Δx,Δy`, `@d<a`) is resolved against the base point,
/// so consumers always receive an absolute world point.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedPoint {
    /// Absolute point `[x, y]`.
    pub point: [f64; 2],
}

/// Stable facade for `af_model::entity::Color`.
///
/// Externally tagged `camelCase` representation: `"byLayer"`, `"byBlock"`,
/// `{ "aci": 7 }`, or `{ "rgb": [r, g, b] }`.
///
/// Used in both directions by `layers()` and `set_entity_props`. Model conversion
/// rejects ACI value `0` with a typed error.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ColorView {
    /// Inherits the layer color.
    ByLayer,
    /// Inherits the block-reference color.
    ByBlock,
    /// ACI index `1..=255`.
    Aci(u8),
    /// True color `[r, g, b]`.
    Rgb([u8; 3]),
}

/// Stable facade for `af_model::entity::Lineweight`.
///
/// `camelCase` representation: `"byLayer"`, `"byBlock"`, or `{ "mm": 0.25 }`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LineweightView {
    /// Inherits the layer lineweight.
    ByLayer,
    /// Inherits the block-reference lineweight.
    ByBlock,
    /// Explicit lineweight in millimeters.
    Mm(f32),
}

/// Stable facade for `af_model::entity::LineTypeRef`.
///
/// `camelCase` representation: `"byLayer"`, `"byBlock"`, or `{ "style": 3 }`.
/// Transactions reject unknown document line-type IDs with a typed error.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LineTypeRefView {
    /// Inherits the layer line type.
    ByLayer,
    /// Inherits the block-reference line type.
    ByBlock,
    /// Explicit document line-type ID.
    Style(u64),
}

/// Stable facade for `af_model::SysvarValue`.
///
/// Externally tagged `camelCase` representation. Values cross in both directions;
/// angles such as `POLARANG` use radians throughout the core.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SysvarValueView {
    /// Integer value for toggles, bitcodes, indices, sizes, and percentages.
    Int(i64),
    /// Non-negative real value, such as `POLARANG` in radians.
    Real(f64),
    /// Real pair `(X, Y)`, used by `SNAPUNIT` and `GRIDUNIT`.
    Real2([f64; 2]),
}

/// Entity properties changed in bulk by `set_entity_props`.
///
/// Missing fields remain unchanged. All changes share one atomic transaction;
/// an unknown entity, layer, or style aborts the entire update.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntityProps {
    /// New layer ID. It must exist or the transaction is rolled back.
    #[serde(default)]
    pub layer: Option<u64>,
    /// New color.
    #[serde(default)]
    pub color: Option<ColorView>,
    /// New line type.
    #[serde(default)]
    pub line_type: Option<LineTypeRefView>,
    /// New lineweight.
    #[serde(default)]
    pub lineweight: Option<LineweightView>,
    /// New per-entity visibility.
    #[serde(default)]
    pub visible: Option<bool>,
}

/// Layer catalog entry returned by `ApiSession::layers`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    /// Stable layer ID.
    pub id: u64,
    /// Name, unique in the document ignoring case.
    pub name: String,
    /// Default layer color.
    pub color: ColorView,
    /// Default document line-type ID.
    pub line_type: u64,
    /// Default layer lineweight.
    pub lineweight: LineweightView,
    /// Off layers are not drawn.
    pub off: bool,
    /// Frozen layers are excluded from drawing, snapping, hits, and extents.
    pub frozen: bool,
    /// Locked layers remain visible and selectable but cannot be edited.
    pub locked: bool,
    /// Plot eligibility; does not affect the viewport.
    pub plot: bool,
    /// Whether this is the document's current layer.
    pub current: bool,
}

/// Document and session state snapshot for panels and status bars.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    /// Global document UUID as a string.
    pub id: String,
    /// Linear unit (`"mm"`, `"cm"`, `"m"`, `"in"`, `"ft"`, or `"unitless"`).
    pub units: String,
    /// Number of model-space entities.
    pub entity_count: usize,
    /// Number of catalog layers.
    pub layer_count: usize,
    /// Current layer ID.
    pub current_layer: u64,
    /// Whether undo is available.
    pub can_undo: bool,
    /// Whether redo is available.
    pub can_redo: bool,
    /// Next undo label, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_label: Option<String>,
    /// Next redo label, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo_label: Option<String>,
    /// Visible-entity bounds `[minX, minY, maxX, maxY]`, or `None` when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extents: Option<[f64; 4]>,
}

/// Entity hit by [`pick`](crate::ApiSession::pick).
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HitView {
    /// Hit entity.
    pub id: u64,
    /// Exact point-to-geometry distance.
    pub dist: f64,
    /// Whether the entity's layer is locked.
    pub locked: bool,
}

/// Snap point ranked by [`snap`](crate::ApiSession::snap).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapView {
    /// Exact feature coordinate `[x, y]`.
    pub point: [f64; 2],
    /// Snap kind (`"endpoint"`, `"midpoint"`, `"center"`, `"node"`,
    /// `"quadrant"`, `"insertion"`).
    pub kind: String,
    /// Entity that supplied the snap.
    pub entity: u64,
    /// Cursor-to-point world distance.
    pub dist: f64,
}

/// Polyline strip within a [`BatchView`], indexing packed `f32` geometry.
///
/// `offset` and `count` are measured in points. Strip vertices occupy
/// `vertices[offset*2 .. (offset + count)*2]` as `(x, y)` pairs.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StripView {
    /// Source entity.
    pub entity: u64,
    /// First point index in the vertex array.
    pub offset: u32,
    /// Number of points in the strip.
    pub count: u32,
    /// Resolved lineweight in millimeters.
    pub width: f32,
    /// Geometric polyline width in world units; `0` means hairline.
    pub poly_width: f32,
    /// Native mathematical length, or `null` for visual-only strips.
    pub analytic_length: Option<f64>,
}

/// Point-entity marker within a [`BatchView`].
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarkerView {
    /// Source entity.
    pub entity: u64,
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
}

/// Render batch keyed by `(layer, resolved color, line type)`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchView {
    /// Layer shared by batch entities.
    pub layer: u64,
    /// Shared resolved RGBA color `[r, g, b, a]`.
    pub color: [u8; 4],
    /// Resolved document line-type ID shared by the batch. The UI scales its
    /// pattern with [`RenderView::ltscale`] or [`RenderDeltaView::ltscale`].
    pub linetype: u64,
    /// Polyline strips for flattened lines, circles, and polylines.
    pub strips: Vec<StripView>,
    /// Point markers.
    pub markers: Vec<MarkerView>,
}

/// Complete render model: control batches plus packed `f32` geometry.
///
/// The small control plane crosses as JSON; geometry uses a flat `vertices`
/// array indexed by `StripView::offset` and `count`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderView {
    /// Batches in draw order.
    pub batches: Vec<BatchView>,
    /// Concatenated `(x, y)` vertices for every strip.
    pub vertices: Vec<f32>,
    /// Global document line-type pattern scale. Empty models use `1.0`.
    pub ltscale: f64,
}

impl Default for RenderView {
    fn default() -> Self {
        Self {
            batches: Vec::new(),
            vertices: Vec::new(),
            ltscale: 1.0,
        }
    }
}

/// Render delta containing batch upserts and removal keys.
///
/// Applying `upserts` and `removes` to the previous [`RenderView`] produces the
/// current view. Upsert geometry uses the same `offset`/`count` scheme.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderDeltaView {
    /// Inserted or replaced batches.
    pub upserts: Vec<BatchView>,
    /// Keys of removed batches.
    pub removes: Vec<BatchKeyView>,
    /// Packed `f32` geometry for upserts.
    pub vertices: Vec<f32>,
    /// Current global LTSCALE, always reported even when no batch changes.
    pub ltscale: f64,
}

impl Default for RenderDeltaView {
    fn default() -> Self {
        Self {
            upserts: Vec::new(),
            removes: Vec::new(),
            vertices: Vec::new(),
            ltscale: 1.0,
        }
    }
}

/// `(layer, color, line type)` key removed by a [`RenderDeltaView`].
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchKeyView {
    /// Batch layer.
    pub layer: u64,
    /// Resolved color `[r, g, b, a]`.
    pub color: [u8; 4],
    /// Resolved line-type ID, part of the batch identity.
    pub linetype: u64,
}

/// Dry-run preview for a modifying command such as TRIM, EXTEND, FILLET, or
/// OFFSET. It creates no transaction and does not change the document.
///
/// Each polyline traces resulting geometry, already flattened with the session's
/// chord tolerance and ready for a transient overlay.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    /// Resulting polylines as ordered `[x, y]` points.
    pub polylines: Vec<Vec<[f32; 2]>>,
}

/// Command parameter description returned by `list_commands`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParamInfo {
    /// Parameter name.
    pub name: String,
    /// Type name such as `"Point"` or `"Distance"`.
    pub ty: String,
    /// Whether the parameter may be omitted.
    pub optional: bool,
}

/// Registered command description returned by `list_commands`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    /// Canonical name.
    pub name: String,
    /// Aliases.
    pub aliases: Vec<String>,
    /// Human-readable label.
    pub label: String,
    /// Whether success mutates the document and creates one transaction.
    pub affects_document: bool,
    /// Ordered parameter schema.
    pub params: Vec<ParamInfo>,
}

/// DXF import or export report with per-type counts and warnings.
///
/// `counts` records processed entities; `skipped` records omitted entities.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DxfReport {
    /// Successfully processed entities by DXF type.
    pub counts: BTreeMap<String, usize>,
    /// Omitted entities by type.
    pub skipped: BTreeMap<String, usize>,
    /// Human-readable warnings.
    pub warnings: Vec<String>,
}

/// Stable output view of a document group returned by `ApiSession::groups`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    /// Stable group ID in the document ID space.
    pub id: u64,
    /// Group name, unique ignoring case.
    pub name: String,
    /// Member entity IDs in insertion order.
    pub members: Vec<u64>,
    /// Whether selecting one member selects the entire group (`PICKSTYLE`).
    pub selectable: bool,
}

/// Property criteria for `ApiSession::select_filter` (`QSELECT`).
///
/// `None` means no restriction. Values within a field use OR semantics, while
/// populated fields combine with AND semantics.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectionFilterView {
    /// Accepted entity types, compared without case; unknown values are ignored.
    #[serde(default)]
    pub kinds: Option<Vec<String>>,
    /// Accepted layer IDs.
    #[serde(default)]
    pub layers: Option<Vec<u64>>,
    /// Accepted exact colors. ACI value `0` produces a typed error.
    #[serde(default)]
    pub colors: Option<Vec<ColorView>>,
}

/// Drainable event from [`ApiSession::poll_events`].
///
/// Subscription uses a drainable queue instead of callbacks across FFI. This
/// avoids lifetime, reentrancy, and callback-panic hazards at the wasm boundary.
///
/// `CommandExecuted` carries the known transaction sequence and created IDs;
/// consumers refresh render and index state after receiving it.
///
/// Serde renames variants to `camelCase`, while variant fields remain `snake_case`.
///
/// This type is not `Eq` because `SysvarChanged` may contain `f64` values.
///
/// [`ApiSession`]: crate::ApiSession
/// [`ApiSession::poll_events`]: crate::ApiSession::poll_events
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ApiEvent {
    /// A command completed successfully, including UNDO and REDO.
    CommandExecuted {
        /// Executed command name.
        name: String,
        /// Committed transaction sequence, or `null`.
        tx_seq: Option<u64>,
        /// Created entities.
        created: Vec<u64>,
    },
    /// Selection changed, with IDs in stable order.
    SelectionChanged {
        /// Current selection.
        ids: Vec<u64>,
    },
    /// A system variable changed after a successful `set_sysvar` call.
    SysvarChanged {
        /// Canonical uppercase system-variable name.
        name: String,
        /// New value.
        value: SysvarValueView,
    },
}
