#![forbid(unsafe_code)]
//! Produces a render model by converting visible document entities into
//! primitives with fully resolved styles. It also produces incremental deltas
//! from [`af_model::ChangeSet`]. This crate does not mutate the document, access
//! the GPU, or work in pixels; chord error arrives in world units through
//! [`RenderOpts::chord_err`].
//!
//! # Behavior
//!
//! - [`build_full`] visits visible entities, resolves color and width, flattens
//!   curves by chord error, and groups primitives into [`RenderBatch`] values.
//! - [`apply_changeset`] rebuilds only affected batches and returns a
//!   [`RenderDelta`].
//!
//! # Style resolution
//!
//! `ByLayer` and `ByBlock` are resolved exclusively in this crate. The rest of
//! the pipeline receives concrete styles.
//!
//! # Current scope
//!
//! - `build_full` and `apply_changeset` currently render model space only.
//! - `ByBlock` without an insertion context falls back to white.
//! - Resolved line width travels in [`WidthClass`], although the current desktop
//!   renderer still uses hairlines.
//! - Text primitives remain deferred until the model supports text entities.

mod build;
mod palette;
pub mod shx;

pub use build::{apply_changeset, build_full};

use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::id::{EntityId, LayerId, StyleId};

/// Resolved RGBA color with eight bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel. Current rendering always resolves it to 255.
    pub a: u8,
}

impl Rgba {
    /// Creates an [`Rgba`] value.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Resolved line width in millimeters.
///
/// `0.0` means the default hairline. The concrete value is preserved for
/// renderers that support physical line weights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidthClass(pub f32);

/// Marker class for [`PrimGeom::Marker`].
///
/// The current model needs one node style for `Point` entities.
// ponytail: Additional point display modes can wait until they are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// Node for a `Point` entity.
    Node,
}

/// Geometry of a render primitive.
///
/// Each current entity produces exactly one primitive.
#[derive(Debug, Clone, PartialEq)]
pub enum PrimGeom {
    /// Straight-segment strip. Curves arrive flattened by chord error; closed
    /// geometry repeats its first point at the end.
    PolylineStrip {
        /// Ordered vertices in world units.
        points: Vec<Point2>,
        /// Resolved lineweight in millimeters.
        width_class: WidthClass,
        /// Geometric polyline width in world units. Unlike `width_class`, it
        /// scales with zoom. Zero means a thin strip.
        poly_width: f32,
        /// Exact mathematical length when available; `None` keeps the visual
        /// fallback explicit.
        analytic_length: Option<f64>,
    },
    /// Point marker.
    Marker {
        /// Marker position in world units.
        at: Point2,
        /// Marker class.
        kind: MarkerKind,
    },
    /// Closed masking polygon for a `Wipeout` entity.
    ///
    /// The renderer fills it with the drawing background color while preserving
    /// drawing order. The polygon closes implicitly without repeating its first
    /// vertex and has no stroke width.
    MaskPolygon {
        /// Ordered polygon vertices in world units with implicit closure.
        points: Vec<Point2>,
    },
    // Text quads remain deferred until the model has a text entity.
}

/// Render geometry paired with its source entity.
///
/// The entity ID supports precise deltas and selection highlighting.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPrim {
    /// Entity that produced this primitive.
    pub entity: EntityId,
    /// Resolved primitive geometry.
    pub geom: PrimGeom,
}

/// Grouping key for a [`RenderBatch`]: container, layer, resolved color, and
/// resolved line type.
///
/// Entities share a batch when their resolved RGBA colors match, even if their
/// source color modes differ.
///
/// Line types are likewise resolved to a concrete model ID. Global LTSCALE lives
/// in [`RenderModel::ltscale`] instead of this hashable key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BatchKey {
    /// Source container, currently [`ContainerRef::ModelSpace`].
    pub container: ContainerRef,
    /// Layer shared by the batch entities.
    pub layer: LayerId,
    /// Resolved color shared by every primitive in the batch.
    pub color: Rgba,
    /// Resolved line-type ID shared by the batch.
    pub linetype: StyleId,
}

/// Primitives that share a [`BatchKey`], in drawing order.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderBatch {
    /// Grouping key.
    pub key: BatchKey,
    /// Batch primitives from back to front.
    pub prims: Vec<RenderPrim>,
}

/// Complete render model with batches in drawing order.
///
/// Batches use first-seen drawing order. Grouping may cross entities from other
/// batches, while primitive order remains stable within each batch.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderModel {
    /// Batches in drawing order.
    pub batches: Vec<RenderBatch>,
    /// Global line-type pattern scale copied from the document. Empty/default
    /// models use 1.0.
    pub ltscale: f64,
}

impl Default for RenderModel {
    fn default() -> Self {
        Self {
            batches: Vec::new(),
            ltscale: 1.0,
        }
    }
}

impl RenderModel {
    /// Finds the batch for a key.
    #[must_use]
    pub fn batch(&self, key: &BatchKey) -> Option<&RenderBatch> {
        self.batches.iter().find(|b| &b.key == key)
    }
}

/// Render-model build options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOpts {
    /// Target curve-flattening chord error in world units.
    pub chord_err: f64,
}

impl RenderOpts {
    /// Creates options with the given chord error.
    #[must_use]
    pub fn new(chord_err: f64) -> Self {
        Self { chord_err }
    }
}

/// Update to one batch inside a [`RenderDelta`].
#[derive(Debug, Clone, PartialEq)]
pub enum BatchUpdate {
    /// Replaces or creates the batch for this key.
    Upsert(RenderBatch),
    /// Removes a batch that became empty.
    Remove(BatchKey),
}

/// Incremental render-model delta listing changed batches.
///
/// Applying every [`BatchUpdate`] to the pre-transaction [`RenderModel`] produces
/// the post-transaction model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderDelta {
    /// Inserted, replaced, or removed batches.
    pub batch_updates: Vec<BatchUpdate>,
}
