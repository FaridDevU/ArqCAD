//! [`EntityContainer`] stores entities in draw order across typed DOD pools.
//!
//! The container consists of:
//!
//! - `draw_order`, the sole owner of observable draw order;
//! - `by_id`, an O(1) ID-to-cell lookup;
//! - one [`TypedStore`] per [`EntityGeometry`] variant.
//!
//! An [`EntityKey`] selects a typed pool and generational cell. Keys and handles
//! are derived state; persistent identity is the [`EntityId`]. Serialization is
//! a draw-ordered `Vec<EntityRecord>` and reconstructs pools on load.
//!
//! # Materialized records
//!
//! `get` and `iter_records` materialize owned records from pool geometry and
//! common columns. Visitor APIs provide zero-copy access for hot paths.

use std::collections::HashMap;

use af_math::BBox;
use serde::{Deserialize, Serialize};

use crate::entity::{
    ArcGeo, CircleGeo, EllipseGeo, EntityGeometry, EntityOps, EntityRecord, LineGeo, PointGeo,
    PolylineGeo, RayGeo, SplineGeo, WipeoutGeo, XlineGeo,
};
use crate::id::{BlockId, EntityId, LayoutId};
use crate::storage::key::{EntityKey, GeoKind};
use crate::storage::pool::Handle;
use crate::storage::store::{CommonRow, TypedStore};

pub use crate::storage::store::CommonRef;

/// A live handle must always resolve; failure means internal index/pool desynchronization.
const DESYNC: &str = "EntityContainer: handle vivo no resuelve (by_id/pools desincronizados)";

/// Compaction cannot issue a new generation without reusing an old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactError {
    /// A pool reached `u32::MAX`.
    GenerationExhausted,
}

impl core::fmt::Display for CompactError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CompactError::GenerationExhausted => write!(f, "handle generation space exhausted"),
        }
    }
}

impl std::error::Error for CompactError {}

/// Locates an [`EntityContainer`] within a document.
///
/// A document has one model space, one paper space per
/// [`Layout`](crate::layouts::Layout), and one entity list per
/// [`BlockDefinition`](crate::doc::BlockDefinition).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContainerRef {
    /// Main drawing geometry in world units.
    ModelSpace,
    /// Paper space for a layout.
    Layout(LayoutId),
    /// Entities in a block definition.
    Block(BlockId),
}

/// Zero-copy reference to the concrete geometry of a live entity.
///
/// Each variant borrows geometry from its typed pool without cloning. Variants
/// map one-to-one to [`EntityGeometry`].
#[derive(Clone, Copy, Debug)]
pub enum GeoRef<'a> {
    /// Line segment.
    Line(&'a LineGeo),
    /// Point node.
    Point(&'a PointGeo),
    /// Circle.
    Circle(&'a CircleGeo),
    /// Circular arc.
    Arc(&'a ArcGeo),
    /// Ellipse or elliptical arc.
    Ellipse(&'a EllipseGeo),
    /// Polyline with line and arc segments.
    Polyline(&'a PolylineGeo),
    /// Infinite construction line.
    Xline(&'a XlineGeo),
    /// Infinite ray.
    Ray(&'a RayGeo),
    /// Cubic spline through fit points.
    Spline(&'a SplineGeo),
    /// Wipeout mask polygon.
    Wipeout(&'a WipeoutGeo),
}

impl<'a> GeoRef<'a> {
    /// Borrows the concrete variant of an [`EntityGeometry`].
    ///
    /// Lets materialized-geometry consumers reuse visitor logic without cloning.
    #[must_use]
    pub fn of(geometry: &'a EntityGeometry) -> Self {
        match geometry {
            EntityGeometry::Line(g) => GeoRef::Line(g),
            EntityGeometry::Point(g) => GeoRef::Point(g),
            EntityGeometry::Circle(g) => GeoRef::Circle(g),
            EntityGeometry::Arc(g) => GeoRef::Arc(g),
            EntityGeometry::Ellipse(g) => GeoRef::Ellipse(g),
            EntityGeometry::Polyline(g) => GeoRef::Polyline(g),
            EntityGeometry::Xline(g) => GeoRef::Xline(g),
            EntityGeometry::Ray(g) => GeoRef::Ray(g),
            EntityGeometry::Spline(g) => GeoRef::Spline(g),
            EntityGeometry::Wipeout(g) => GeoRef::Wipeout(g),
        }
    }

    /// Bounding box of the borrowed geometry without materializing it.
    #[must_use]
    pub fn bbox(&self) -> BBox {
        match self {
            GeoRef::Line(g) => g.bbox(),
            GeoRef::Point(g) => g.bbox(),
            GeoRef::Circle(g) => g.bbox(),
            GeoRef::Arc(g) => g.bbox(),
            GeoRef::Ellipse(g) => g.bbox(),
            GeoRef::Polyline(g) => g.bbox(),
            GeoRef::Xline(g) => g.bbox(),
            GeoRef::Ray(g) => g.bbox(),
            GeoRef::Spline(g) => g.bbox(),
            GeoRef::Wipeout(g) => g.bbox(),
        }
    }

    /// Clones this reference into an owned [`EntityGeometry`].
    #[must_use]
    pub fn to_geometry(&self) -> EntityGeometry {
        match self {
            GeoRef::Line(g) => EntityGeometry::Line(**g),
            GeoRef::Point(g) => EntityGeometry::Point(**g),
            GeoRef::Circle(g) => EntityGeometry::Circle(**g),
            GeoRef::Arc(g) => EntityGeometry::Arc(**g),
            GeoRef::Ellipse(g) => EntityGeometry::Ellipse(**g),
            GeoRef::Polyline(g) => EntityGeometry::Polyline((*g).clone()),
            GeoRef::Xline(g) => EntityGeometry::Xline(**g),
            GeoRef::Ray(g) => EntityGeometry::Ray(**g),
            GeoRef::Spline(g) => EntityGeometry::Spline((*g).clone()),
            GeoRef::Wipeout(g) => EntityGeometry::Wipeout((*g).clone()),
        }
    }
}

/// Draw-ordered entity container backed by typed DOD pools.
///
/// Serialization contains only draw-ordered records. Equality compares that
/// semantic sequence and ignores derived physical pool layout.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(from = "Vec<EntityRecord>", into = "Vec<EntityRecord>")]
pub struct EntityContainer {
    /// Stable draw order; index 0 is drawn first.
    draw_order: Vec<EntityKey>,
    /// Derived ID-to-cell lookup; never serialized.
    by_id: HashMap<EntityId, EntityKey>,
    // One typed arena per `EntityGeometry` variant.
    line: TypedStore<LineGeo>,
    point: TypedStore<PointGeo>,
    circle: TypedStore<CircleGeo>,
    arc: TypedStore<ArcGeo>,
    ellipse: TypedStore<EllipseGeo>,
    polyline: TypedStore<PolylineGeo>,
    xline: TypedStore<XlineGeo>,
    ray: TypedStore<RayGeo>,
    spline: TypedStore<SplineGeo>,
    wipeout: TypedStore<WipeoutGeo>,
}

/// Common record columns used when inserting into a typed pool.
fn row_of(record: &EntityRecord) -> CommonRow {
    CommonRow {
        id: record.id,
        layer: record.layer,
        color: record.color,
        line_type: record.line_type,
        lineweight: record.lineweight,
        visible: record.visible,
    }
}

/// Converts a compaction remap into an old-to-new handle map.
fn remap_map(remap: Vec<(Handle, Handle)>) -> HashMap<Handle, Handle> {
    remap.into_iter().collect()
}

/// Reconstructs an [`EntityRecord`] from geometry and common columns.
fn record_of(geometry: EntityGeometry, row: CommonRow) -> EntityRecord {
    EntityRecord {
        id: row.id,
        layer: row.layer,
        color: row.color,
        line_type: row.line_type,
        lineweight: row.lineweight,
        visible: row.visible,
        geometry,
    }
}

impl EntityContainer {
    /// Creates an empty container.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a container from draw-ordered records.
    ///
    /// For corrupt duplicate IDs, `by_id` points to the last occurrence while
    /// `draw_order` retains all entries for `validate_full` to report.
    #[must_use]
    pub fn from_records(records: Vec<EntityRecord>) -> Self {
        let mut c = Self::new();
        for record in records {
            c.push(record);
        }
        c
    }

    // Public reads.

    /// Number of entities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.draw_order.len()
    }

    /// Whether the container is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.draw_order.is_empty()
    }

    /// Iterates materialized records in draw order.
    ///
    /// Each item is reconstructed by value from geometry and common columns.
    pub fn iter_records(&self) -> impl Iterator<Item = EntityRecord> + '_ {
        self.draw_order
            .iter()
            .map(move |&key| self.materialize(key))
    }

    /// Compatibility alias for [`iter_records`](Self::iter_records).
    ///
    /// New code should use `iter_records`.
    pub fn iter(&self) -> impl Iterator<Item = EntityRecord> + '_ {
        self.iter_records()
    }

    /// Visits each entity in draw order without materializing it.
    ///
    /// The callback receives its [`EntityId`], borrowed [`CommonRef`], and
    /// borrowed [`GeoRef`].
    pub fn visit<F>(&self, mut f: F)
    where
        F: FnMut(EntityId, CommonRef<'_>, GeoRef<'_>),
    {
        for &key in &self.draw_order {
            let (common, geo) = self.view_key(key);
            f(common.id(), common, geo);
        }
    }

    /// Fallible [`visit`](Self::visit) variant that stops on the first error.
    ///
    /// # Errors
    /// Returns the first error from `f`.
    pub fn try_visit<F, E>(&self, mut f: F) -> Result<(), E>
    where
        F: FnMut(EntityId, CommonRef<'_>, GeoRef<'_>) -> Result<(), E>,
    {
        for &key in &self.draw_order {
            let (common, geo) = self.view_key(key);
            f(common.id(), common, geo)?;
        }
        Ok(())
    }

    // Typed slabs use physical pool order, not draw order.
    //
    // Each callback receives aligned ID and geometry slices for a maximal live run.
    //
    // Free-list holes may produce multiple callbacks; `compact` guarantees one.
    // Borrowed slices are valid only for the callback duration.

    /// Visits [`LineGeo`] slabs.
    pub fn visit_line_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[LineGeo]),
    {
        self.line.visit_slab(f);
    }

    /// Visits [`PointGeo`] slabs.
    pub fn visit_point_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[PointGeo]),
    {
        self.point.visit_slab(f);
    }

    /// Visits [`CircleGeo`] slabs.
    pub fn visit_circle_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[CircleGeo]),
    {
        self.circle.visit_slab(f);
    }

    /// Visits [`ArcGeo`] slabs.
    pub fn visit_arc_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[ArcGeo]),
    {
        self.arc.visit_slab(f);
    }

    /// Visits [`EllipseGeo`] slabs.
    pub fn visit_ellipse_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[EllipseGeo]),
    {
        self.ellipse.visit_slab(f);
    }

    /// Visits [`XlineGeo`] slabs.
    pub fn visit_xline_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[XlineGeo]),
    {
        self.xline.visit_slab(f);
    }

    /// Visits [`RayGeo`] slabs.
    pub fn visit_ray_slab<F>(&self, f: F)
    where
        F: FnMut(&[EntityId], &[RayGeo]),
    {
        self.ray.visit_slab(f);
    }

    // Geometries with internal vectors are visited one item at a time.
    //
    // These callbacks also use physical pool order and callback-scoped borrows.

    /// Visits each live [`PolylineGeo`] as `(id, &geo)`.
    pub fn visit_polyline_each<F>(&self, f: F)
    where
        F: FnMut(EntityId, &PolylineGeo),
    {
        self.polyline.visit_each(f);
    }

    /// Visits each live [`SplineGeo`] as `(id, &geo)`.
    pub fn visit_spline_each<F>(&self, f: F)
    where
        F: FnMut(EntityId, &SplineGeo),
    {
        self.spline.visit_each(f);
    }

    /// Visits each live [`WipeoutGeo`] as `(id, &geo)`.
    pub fn visit_wipeout_each<F>(&self, f: F)
    where
        F: FnMut(EntityId, &WipeoutGeo),
    {
        self.wipeout.visit_each(f);
    }

    /// Visits each entity in draw order with its ID, common properties, and
    /// cached [`BBox`].
    ///
    /// The box is read from the pool column without materializing the record.
    pub fn visit_bboxes<F>(&self, mut f: F)
    where
        F: FnMut(EntityId, CommonRef<'_>, &BBox),
    {
        for &key in &self.draw_order {
            let (common, bb) = self.view_bbox_key(key);
            f(common.id(), common, bb);
        }
    }

    /// Cached bounding box by entity ID, or `None` if absent.
    #[must_use]
    pub fn bbox(&self, id: EntityId) -> Option<BBox> {
        let key = *self.by_id.get(&id)?;
        let (_common, bb) = self.view_bbox_key(key);
        Some(*bb)
    }

    /// Borrows common properties and the cached box for a live key.
    fn view_bbox_key(&self, key: EntityKey) -> (CommonRef<'_>, &BBox) {
        match key.kind {
            GeoKind::Line => self.line.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Point => self.point.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Circle => self.circle.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Arc => self.arc.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Ellipse => self.ellipse.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Polyline => self.polyline.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Xline => self.xline.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Ray => self.ray.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Spline => self.spline.view_bbox(key.handle).expect(DESYNC),
            GeoKind::Wipeout => self.wipeout.view_bbox(key.handle).expect(DESYNC),
        }
    }

    /// Borrows common properties and geometry for a live key.
    fn view_key(&self, key: EntityKey) -> (CommonRef<'_>, GeoRef<'_>) {
        match key.kind {
            GeoKind::Line => {
                let (g, c) = self.line.view(key.handle).expect(DESYNC);
                (c, GeoRef::Line(g))
            }
            GeoKind::Point => {
                let (g, c) = self.point.view(key.handle).expect(DESYNC);
                (c, GeoRef::Point(g))
            }
            GeoKind::Circle => {
                let (g, c) = self.circle.view(key.handle).expect(DESYNC);
                (c, GeoRef::Circle(g))
            }
            GeoKind::Arc => {
                let (g, c) = self.arc.view(key.handle).expect(DESYNC);
                (c, GeoRef::Arc(g))
            }
            GeoKind::Ellipse => {
                let (g, c) = self.ellipse.view(key.handle).expect(DESYNC);
                (c, GeoRef::Ellipse(g))
            }
            GeoKind::Polyline => {
                let (g, c) = self.polyline.view(key.handle).expect(DESYNC);
                (c, GeoRef::Polyline(g))
            }
            GeoKind::Xline => {
                let (g, c) = self.xline.view(key.handle).expect(DESYNC);
                (c, GeoRef::Xline(g))
            }
            GeoKind::Ray => {
                let (g, c) = self.ray.view(key.handle).expect(DESYNC);
                (c, GeoRef::Ray(g))
            }
            GeoKind::Spline => {
                let (g, c) = self.spline.view(key.handle).expect(DESYNC);
                (c, GeoRef::Spline(g))
            }
            GeoKind::Wipeout => {
                let (g, c) = self.wipeout.view(key.handle).expect(DESYNC);
                (c, GeoRef::Wipeout(g))
            }
        }
    }

    /// Returns the [`EntityId`] for a live key without materializing geometry.
    fn id_of(&self, key: EntityKey) -> EntityId {
        let row = match key.kind {
            GeoKind::Line => self.line.common(key.handle),
            GeoKind::Point => self.point.common(key.handle),
            GeoKind::Circle => self.circle.common(key.handle),
            GeoKind::Arc => self.arc.common(key.handle),
            GeoKind::Ellipse => self.ellipse.common(key.handle),
            GeoKind::Polyline => self.polyline.common(key.handle),
            GeoKind::Xline => self.xline.common(key.handle),
            GeoKind::Ray => self.ray.common(key.handle),
            GeoKind::Spline => self.spline.common(key.handle),
            GeoKind::Wipeout => self.wipeout.common(key.handle),
        };
        row.expect(DESYNC).id
    }

    /// Finds and materializes an owned entity by ID in O(1).
    #[must_use]
    pub fn get(&self, id: EntityId) -> Option<EntityRecord> {
        let key = *self.by_id.get(&id)?;
        Some(self.materialize(key))
    }

    /// Draw position for an entity ID, or `None`.
    #[must_use]
    pub fn index_of(&self, id: EntityId) -> Option<usize> {
        let key = *self.by_id.get(&id)?;
        self.draw_order.iter().position(|k| *k == key)
    }

    /// Whether the container contains an entity ID.
    #[must_use]
    pub fn contains(&self, id: EntityId) -> bool {
        self.by_id.contains_key(&id)
    }

    // Internal transaction mutations.

    /// Appends an entity and returns its draw position.
    pub(crate) fn push(&mut self, record: EntityRecord) -> usize {
        let (id, key) = self.store_record(record);
        let idx = self.draw_order.len();
        self.draw_order.push(key);
        self.by_id.insert(id, key);
        idx
    }

    /// Removes an entity and returns its materialized record and former position.
    pub(crate) fn remove_by_id(&mut self, id: EntityId) -> Option<(EntityRecord, usize)> {
        let key = self.by_id.remove(&id)?;
        let pos = self
            .draw_order
            .iter()
            .position(|k| *k == key)
            .expect(DESYNC);
        self.draw_order.remove(pos);
        let record = self.take_from_store(key);
        Some((record, pos))
    }

    /// Inserts a record at a draw position, clamped to the current length.
    pub(crate) fn insert_at(&mut self, index: usize, record: EntityRecord) {
        let (id, key) = self.store_record(record);
        let i = index.min(self.draw_order.len());
        self.draw_order.insert(i, key);
        self.by_id.insert(id, key);
    }

    /// Replaces an entity while preserving its draw position.
    ///
    /// Same-kind geometry is updated in place. A kind change moves pools and
    /// updates the key at the same draw position.
    pub(crate) fn replace(&mut self, id: EntityId, record: EntityRecord) -> bool {
        let Some(old_key) = self.by_id.get(&id).copied() else {
            return false;
        };
        let new_kind = GeoKind::of(&record.geometry);
        if new_kind == old_key.kind {
            let row = row_of(&record);
            self.overwrite_in_place(old_key, record.geometry, row);
        } else {
            // Move to the new variant pool while preserving draw position.
            let _ = self.take_from_store(old_key);
            let (rid, new_key) = self.store_record(record);
            let slot = self
                .draw_order
                .iter_mut()
                .find(|k| **k == old_key)
                .expect(DESYNC);
            *slot = new_key;
            self.by_id.insert(rid, new_key);
        }
        true
    }

    /// Compacts all typed pools and atomically remaps indexes to new handles.
    ///
    /// Entity IDs, draw order, geometry, properties, serialization, and equality
    /// remain unchanged. Each pool has one contiguous live run afterward.
    ///
    /// # Transaction safety
    ///
    /// Call only while no transaction is active because compaction rewrites handles.
    ///
    /// # Errors
    /// Returns [`CompactError::GenerationExhausted`] before mutation if any pool
    /// cannot issue a strictly newer generation.
    pub fn compact(&mut self) -> Result<(), CompactError> {
        self.line.preflight_compact()?;
        self.point.preflight_compact()?;
        self.circle.preflight_compact()?;
        self.arc.preflight_compact()?;
        self.ellipse.preflight_compact()?;
        self.polyline.preflight_compact()?;
        self.xline.preflight_compact()?;
        self.ray.preflight_compact()?;
        self.spline.preflight_compact()?;
        self.wipeout.preflight_compact()?;

        // Compact each pool and build old-to-new handle maps.
        let line = remap_map(self.line.compact()?);
        let point = remap_map(self.point.compact()?);
        let circle = remap_map(self.circle.compact()?);
        let arc = remap_map(self.arc.compact()?);
        let ellipse = remap_map(self.ellipse.compact()?);
        let polyline = remap_map(self.polyline.compact()?);
        let xline = remap_map(self.xline.compact()?);
        let ray = remap_map(self.ray.compact()?);
        let spline = remap_map(self.spline.compact()?);
        let wipeout = remap_map(self.wipeout.compact()?);

        // Remap draw-order keys without changing their entity sequence.
        for key in &mut self.draw_order {
            let new_handle = match key.kind {
                GeoKind::Line => line[&key.handle],
                GeoKind::Point => point[&key.handle],
                GeoKind::Circle => circle[&key.handle],
                GeoKind::Arc => arc[&key.handle],
                GeoKind::Ellipse => ellipse[&key.handle],
                GeoKind::Polyline => polyline[&key.handle],
                GeoKind::Xline => xline[&key.handle],
                GeoKind::Ray => ray[&key.handle],
                GeoKind::Spline => spline[&key.handle],
                GeoKind::Wipeout => wipeout[&key.handle],
            };
            key.handle = new_handle;
        }

        // Rebuild `by_id`; clone keys to avoid retaining the `self` borrow.
        let keys: Vec<EntityKey> = self.draw_order.clone();
        self.by_id.clear();
        for key in keys {
            let id = self.id_of(key);
            self.by_id.insert(id, key);
        }
        Ok(())
    }

    // Internal dispatch between records and typed pools.

    /// Inserts geometry into its typed pool and returns `(id, key)`.
    fn store_record(&mut self, record: EntityRecord) -> (EntityId, EntityKey) {
        let id = record.id;
        let row = row_of(&record);
        // Compute the cached box before consuming geometry in the variant match.
        let bbox = record.geometry.bbox();
        let (kind, handle) = match record.geometry {
            EntityGeometry::Line(g) => (GeoKind::Line, self.line.insert(g, row, bbox)),
            EntityGeometry::Point(g) => (GeoKind::Point, self.point.insert(g, row, bbox)),
            EntityGeometry::Circle(g) => (GeoKind::Circle, self.circle.insert(g, row, bbox)),
            EntityGeometry::Arc(g) => (GeoKind::Arc, self.arc.insert(g, row, bbox)),
            EntityGeometry::Ellipse(g) => (GeoKind::Ellipse, self.ellipse.insert(g, row, bbox)),
            EntityGeometry::Polyline(g) => (GeoKind::Polyline, self.polyline.insert(g, row, bbox)),
            EntityGeometry::Xline(g) => (GeoKind::Xline, self.xline.insert(g, row, bbox)),
            EntityGeometry::Ray(g) => (GeoKind::Ray, self.ray.insert(g, row, bbox)),
            EntityGeometry::Spline(g) => (GeoKind::Spline, self.spline.insert(g, row, bbox)),
            EntityGeometry::Wipeout(g) => (GeoKind::Wipeout, self.wipeout.insert(g, row, bbox)),
        };
        (id, EntityKey::new(kind, handle))
    }

    /// Materializes a record for a live key without consuming its cell.
    fn materialize(&self, key: EntityKey) -> EntityRecord {
        match key.kind {
            // Copy fixed-size variants directly.
            GeoKind::Line => {
                let (g, row) = self.line.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Line(*g), row)
            }
            GeoKind::Point => {
                let (g, row) = self.point.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Point(*g), row)
            }
            GeoKind::Circle => {
                let (g, row) = self.circle.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Circle(*g), row)
            }
            GeoKind::Arc => {
                let (g, row) = self.arc.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Arc(*g), row)
            }
            GeoKind::Ellipse => {
                let (g, row) = self.ellipse.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Ellipse(*g), row)
            }
            // Clone variants with internal vectors.
            GeoKind::Polyline => {
                let (g, row) = self.polyline.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Polyline(g.clone()), row)
            }
            GeoKind::Xline => {
                let (g, row) = self.xline.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Xline(*g), row)
            }
            GeoKind::Ray => {
                let (g, row) = self.ray.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Ray(*g), row)
            }
            GeoKind::Spline => {
                let (g, row) = self.spline.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Spline(g.clone()), row)
            }
            GeoKind::Wipeout => {
                let (g, row) = self.wipeout.get(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Wipeout(g.clone()), row)
            }
        }
    }

    /// Removes a live key and moves its geometry into a materialized record.
    fn take_from_store(&mut self, key: EntityKey) -> EntityRecord {
        match key.kind {
            GeoKind::Line => {
                let (g, row) = self.line.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Line(g), row)
            }
            GeoKind::Point => {
                let (g, row) = self.point.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Point(g), row)
            }
            GeoKind::Circle => {
                let (g, row) = self.circle.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Circle(g), row)
            }
            GeoKind::Arc => {
                let (g, row) = self.arc.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Arc(g), row)
            }
            GeoKind::Ellipse => {
                let (g, row) = self.ellipse.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Ellipse(g), row)
            }
            GeoKind::Polyline => {
                let (g, row) = self.polyline.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Polyline(g), row)
            }
            GeoKind::Xline => {
                let (g, row) = self.xline.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Xline(g), row)
            }
            GeoKind::Ray => {
                let (g, row) = self.ray.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Ray(g), row)
            }
            GeoKind::Spline => {
                let (g, row) = self.spline.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Spline(g), row)
            }
            GeoKind::Wipeout => {
                let (g, row) = self.wipeout.remove(key.handle).expect(DESYNC);
                record_of(EntityGeometry::Wipeout(g), row)
            }
        }
    }

    /// Replaces geometry and common columns in place for a same-kind live key.
    ///
    /// [`TypedStore::set_geometry`] also updates the recalculated cached box.
    fn overwrite_in_place(&mut self, key: EntityKey, geometry: EntityGeometry, row: CommonRow) {
        // Recalculate the box before consuming the new geometry.
        let bbox = geometry.bbox();
        match geometry {
            EntityGeometry::Line(g) => {
                self.line.set_geometry(key.handle, g, bbox);
                self.line.set_common(key.handle, row);
            }
            EntityGeometry::Point(g) => {
                self.point.set_geometry(key.handle, g, bbox);
                self.point.set_common(key.handle, row);
            }
            EntityGeometry::Circle(g) => {
                self.circle.set_geometry(key.handle, g, bbox);
                self.circle.set_common(key.handle, row);
            }
            EntityGeometry::Arc(g) => {
                self.arc.set_geometry(key.handle, g, bbox);
                self.arc.set_common(key.handle, row);
            }
            EntityGeometry::Ellipse(g) => {
                self.ellipse.set_geometry(key.handle, g, bbox);
                self.ellipse.set_common(key.handle, row);
            }
            EntityGeometry::Polyline(g) => {
                self.polyline.set_geometry(key.handle, g, bbox);
                self.polyline.set_common(key.handle, row);
            }
            EntityGeometry::Xline(g) => {
                self.xline.set_geometry(key.handle, g, bbox);
                self.xline.set_common(key.handle, row);
            }
            EntityGeometry::Ray(g) => {
                self.ray.set_geometry(key.handle, g, bbox);
                self.ray.set_common(key.handle, row);
            }
            EntityGeometry::Spline(g) => {
                self.spline.set_geometry(key.handle, g, bbox);
                self.spline.set_common(key.handle, row);
            }
            EntityGeometry::Wipeout(g) => {
                self.wipeout.set_geometry(key.handle, g, bbox);
                self.wipeout.set_common(key.handle, row);
            }
        }
    }
}

impl PartialEq for EntityContainer {
    /// Semantic equality compares materialized records in draw order.
    fn eq(&self, other: &Self) -> bool {
        self.draw_order.len() == other.draw_order.len()
            && self.iter_records().eq(other.iter_records())
    }
}

impl From<Vec<EntityRecord>> for EntityContainer {
    fn from(records: Vec<EntityRecord>) -> Self {
        Self::from_records(records)
    }
}

impl From<EntityContainer> for Vec<EntityRecord> {
    fn from(c: EntityContainer) -> Self {
        c.iter_records().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;

    use crate::entity::{
        CircleGeo, Color, EntityGeometry, EntityOps, LineTypeRef, Lineweight, PointGeo, PolyVertex,
        PolylineGeo,
    };
    use crate::id::{EntityId, ObjectId};

    fn rec(id: u64, x: f64) -> EntityRecord {
        let eid: EntityId = ObjectId(id).into();
        let layer = ObjectId(1).into();
        EntityRecord::new(
            eid,
            layer,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(x, 0.0))),
        )
    }

    fn ids_in_order(c: &EntityContainer) -> Vec<u64> {
        c.iter_records().map(|r| r.id.raw().0).collect()
    }

    #[test]
    fn push_conserva_orden_y_indice() {
        let mut c = EntityContainer::new();
        assert!(c.is_empty());
        assert_eq!(c.push(rec(10, 0.0)), 0);
        assert_eq!(c.push(rec(20, 1.0)), 1);
        assert_eq!(c.push(rec(30, 2.0)), 2);

        // Draw order matches insertion order.
        assert_eq!(ids_in_order(&c), vec![10, 20, 30]);

        // Index positions remain consistent.
        assert_eq!(c.index_of(ObjectId(10).into()), Some(0));
        assert_eq!(c.index_of(ObjectId(30).into()), Some(2));
        assert!(c.get(ObjectId(20).into()).is_some());
        assert!(c.contains(ObjectId(20).into()));
        assert!(!c.contains(ObjectId(99).into()));
    }

    #[test]
    fn remove_compacta_y_reindexa() {
        let mut c = EntityContainer::new();
        c.push(rec(10, 0.0));
        c.push(rec(20, 1.0));
        c.push(rec(30, 2.0));

        let (removed, idx) = c.remove_by_id(ObjectId(20).into()).unwrap();
        assert_eq!(removed.id.raw().0, 20);
        assert_eq!(idx, 1);

        // The remaining entries are [10, 30] with a coherent compacted index.
        assert_eq!(ids_in_order(&c), vec![10, 30]);
        assert_eq!(c.index_of(ObjectId(10).into()), Some(0));
        assert_eq!(c.index_of(ObjectId(30).into()), Some(1));
        assert_eq!(c.index_of(ObjectId(20).into()), None);
        assert!(c.get(ObjectId(20).into()).is_none());

        // Removing an absent ID changes nothing.
        assert!(c.remove_by_id(ObjectId(20).into()).is_none());
    }

    #[test]
    fn insert_at_restaura_posicion_de_dibujo() {
        let mut c = EntityContainer::new();
        c.push(rec(10, 0.0));
        c.push(rec(20, 1.0));
        c.push(rec(30, 2.0));

        // Restore a removed record at its exact former position.
        let (removed, idx) = c.remove_by_id(ObjectId(20).into()).unwrap();
        c.insert_at(idx, removed);

        assert_eq!(ids_in_order(&c), vec![10, 20, 30]);
        assert_eq!(c.index_of(ObjectId(20).into()), Some(1));
        assert_eq!(c.index_of(ObjectId(30).into()), Some(2));
    }

    #[test]
    fn insert_at_recorta_indice_fuera_de_rango() {
        let mut c = EntityContainer::new();
        c.push(rec(10, 0.0));
        c.insert_at(999, rec(20, 1.0)); // Clamped to len, which is 1.
        assert_eq!(ids_in_order(&c), vec![10, 20]);
    }

    #[test]
    fn serde_es_array_de_records_y_reconstruye_indice() {
        let mut c = EntityContainer::new();
        c.push(rec(10, 0.0));
        c.push(rec(20, 1.0));

        let json = serde_json::to_string(&c).unwrap();
        // Serialization is an array, not a physical pool-layout object.
        assert!(json.starts_with('['), "esperaba array JSON, fue: {json}");

        let back: EntityContainer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        // The rebuilt index remains coherent.
        assert_eq!(back.index_of(ObjectId(20).into()), Some(1));
    }

    // Materialization preserves inserted data, including vector-backed geometry.
    #[test]
    fn get_materializa_identico_incluida_polyline_con_bulges() {
        let mut c = EntityContainer::new();
        let eid: EntityId = ObjectId(77).into();
        let poly = PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.5),
                PolyVertex::new(Point2::new(2.0, 1.0), -0.25),
                PolyVertex::new(Point2::new(4.0, 0.0), 0.0),
            ],
            true,
        );
        let original = EntityRecord::new(
            eid,
            ObjectId(1).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Polyline(poly),
        );
        c.push(original.clone());

        // `get` returns exactly what was inserted.
        assert_eq!(c.get(eid).unwrap(), original);
        // Draw-order iteration does too.
        assert_eq!(c.iter_records().next().unwrap(), original);
    }

    // Mixed insertions and removals preserve draw order.
    #[test]
    fn orden_de_dibujo_estable_tras_mutaciones_mezcladas() {
        let mut c = EntityContainer::new();
        for id in [10, 20, 30, 40] {
            c.push(rec(id, id as f64));
        }
        // Remove and restore the middle item at its original position.
        let (mid, pos) = c.remove_by_id(ObjectId(30).into()).unwrap();
        assert_eq!(ids_in_order(&c), vec![10, 20, 40]);
        c.insert_at(pos, mid);
        assert_eq!(ids_in_order(&c), vec![10, 20, 30, 40]);

        // Same-variant replacement preserves order.
        let mut moved = c.get(ObjectId(20).into()).unwrap();
        moved.geometry = EntityGeometry::Point(PointGeo::new(Point2::new(99.0, 99.0)));
        assert!(c.replace(ObjectId(20).into(), moved));
        assert_eq!(ids_in_order(&c), vec![10, 20, 30, 40]);
        assert_eq!(c.index_of(ObjectId(20).into()), Some(1));
        if let EntityGeometry::Point(p) = c.get(ObjectId(20).into()).unwrap().geometry {
            assert_eq!(p.position, Point2::new(99.0, 99.0));
        } else {
            panic!("esperaba punto");
        }
    }

    // Zero-copy visitation matches the materialized draw-order sequence.
    #[test]
    fn visit_equivale_a_iter_records_en_orden_y_valor() {
        let mut c = EntityContainer::new();
        // Mix geometry variants, including vector-backed polyline data.
        c.push(rec(10, 0.0)); // Point
        let poly = PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.5),
                PolyVertex::new(Point2::new(2.0, 1.0), -0.25),
                PolyVertex::new(Point2::new(4.0, 0.0), 0.0),
            ],
            true,
        );
        c.push(EntityRecord::new(
            ObjectId(20).into(),
            ObjectId(1).into(),
            Color::Rgb(1, 2, 3),
            LineTypeRef::ByBlock,
            Lineweight::Mm(0.5),
            EntityGeometry::Polyline(poly),
        ));
        c.push(EntityRecord::new(
            ObjectId(30).into(),
            ObjectId(2).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))),
        ));

        let materialized: Vec<EntityRecord> = c.iter_records().collect();

        // Reconstruct records from borrowed common and geometry views.
        let mut visited: Vec<EntityRecord> = Vec::new();
        c.visit(|id, common, geo| {
            assert_eq!(id, common.id());
            visited.push(EntityRecord {
                id: common.id(),
                layer: common.layer(),
                color: common.color(),
                line_type: common.line_type(),
                lineweight: common.lineweight(),
                visible: common.visible(),
                geometry: geo.to_geometry(),
            });
        });

        assert_eq!(visited, materialized);

        // `try_visit` traverses the same sequence when successful.
        let mut ids = Vec::new();
        let r: Result<(), ()> = c.try_visit(|id, _c, _g| {
            ids.push(id.raw().0);
            Ok(())
        });
        assert_eq!(r, Ok(()));
        assert_eq!(ids, vec![10, 20, 30]);
    }

    // `try_visit` stops at the first error.
    #[test]
    fn try_visit_corta_en_el_primer_error() {
        let mut c = EntityContainer::new();
        c.push(rec(10, 0.0));
        c.push(rec(20, 1.0));
        c.push(rec(30, 2.0));

        let mut seen = Vec::new();
        let r: Result<(), u64> = c.try_visit(|id, _c, _g| {
            let raw = id.raw().0;
            seen.push(raw);
            if raw == 20 { Err(raw) } else { Ok(()) }
        });
        assert_eq!(r, Err(20));
        assert_eq!(seen, vec![10, 20]); // 30 is not visited.
    }

    // Cross-variant replacement preserves ID and draw position.
    #[test]
    fn replace_cambia_variante_conservando_posicion() {
        let mut c = EntityContainer::new();
        c.push(rec(10, 0.0));
        c.push(rec(20, 1.0)); // Point
        c.push(rec(30, 2.0));

        let mut as_line = c.get(ObjectId(20).into()).unwrap();
        as_line.geometry =
            EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)));
        assert!(c.replace(ObjectId(20).into(), as_line.clone()));

        // Same ID and position, new geometry.
        assert_eq!(ids_in_order(&c), vec![10, 20, 30]);
        assert_eq!(c.index_of(ObjectId(20).into()), Some(1));
        assert_eq!(c.get(ObjectId(20).into()).unwrap(), as_line);

        // Replacing an absent ID returns false without mutation.
        assert!(!c.replace(ObjectId(999).into(), rec(999, 0.0)));
        assert_eq!(ids_in_order(&c), vec![10, 20, 30]);
    }

    // Cached boxes match materialized geometry through every mutation path.
    #[test]
    fn columna_bbox_coincide_con_geo_bbox_tras_mutaciones() {
        // Compare cached and materialized boxes by ID.
        fn assert_column_matches(c: &EntityContainer) {
            // ID lookup path.
            for rec in c.iter_records() {
                assert_eq!(
                    c.bbox(rec.id),
                    Some(rec.geometry.bbox()),
                    "columna bbox != geo.bbox() para id {:?}",
                    rec.id
                );
            }
            // Draw-order box visitor path.
            let mut visited: Vec<(u64, BBox)> = Vec::new();
            c.visit_bboxes(|id, _common, bb| visited.push((id.raw().0, *bb)));
            let expected: Vec<(u64, BBox)> = c
                .iter_records()
                .map(|r| (r.id.raw().0, r.geometry.bbox()))
                .collect();
            assert_eq!(visited, expected);
        }

        let mut c = EntityContainer::new();
        // Insert several variants.
        c.push(rec(10, 0.0)); // Point
        c.push(EntityRecord::new(
            ObjectId(20).into(),
            ObjectId(1).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Circle(CircleGeo::new(Point2::new(5.0, 5.0), 3.0)),
        ));
        let poly = PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.5),
                PolyVertex::new(Point2::new(2.0, 1.0), -0.25),
                PolyVertex::new(Point2::new(4.0, 0.0), 0.0),
            ],
            true,
        );
        c.push(EntityRecord::new(
            ObjectId(30).into(),
            ObjectId(1).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Polyline(poly),
        ));
        assert_column_matches(&c);

        // Same-variant edits recalculate the cached box.
        let mut moved = c.get(ObjectId(20).into()).unwrap();
        moved.geometry = EntityGeometry::Circle(CircleGeo::new(Point2::new(-40.0, 12.0), 7.0));
        assert!(c.replace(ObjectId(20).into(), moved));
        assert_column_matches(&c);

        // Cross-variant edits move the cached box to the new pool.
        let mut as_line = c.get(ObjectId(10).into()).unwrap();
        as_line.geometry =
            EntityGeometry::Line(LineGeo::new(Point2::new(-1.0, -2.0), Point2::new(9.0, 8.0)));
        assert!(c.replace(ObjectId(10).into(), as_line));
        assert_column_matches(&c);

        // Remove and restore preserves the cached box.
        let (removed, pos) = c.remove_by_id(ObjectId(30).into()).unwrap();
        assert_column_matches(&c);
        c.insert_at(pos, removed);
        assert_column_matches(&c);
    }

    // Typed slab tests.

    fn line_rec(id: u64, x: f64) -> EntityRecord {
        EntityRecord::new(
            ObjectId(id).into(),
            ObjectId(1).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Line(LineGeo::new(Point2::new(x, 0.0), Point2::new(x + 1.0, 1.0))),
        )
    }

    fn circle_rec(id: u64, x: f64) -> EntityRecord {
        EntityRecord::new(
            ObjectId(id).into(),
            ObjectId(1).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Circle(CircleGeo::new(Point2::new(x, x), 2.0)),
        )
    }

    fn poly_rec(id: u64, x: f64) -> EntityRecord {
        EntityRecord::new(
            ObjectId(id).into(),
            ObjectId(1).into(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Polyline(PolylineGeo::new(
                vec![
                    PolyVertex::new(Point2::new(x, 0.0), 0.0),
                    PolyVertex::new(Point2::new(x + 1.0, 1.0), 0.5),
                ],
                false,
            )),
        )
    }

    // Slabs visit each live entity once with aligned ID and geometry slices.
    #[test]
    fn slab_visita_exactamente_las_vivas_y_suma_len() {
        let mut c = EntityContainer::new();
        // Mix points, a line, and a circle in draw order.
        c.push(rec(10, 0.0)); // Point x=0
        c.push(line_rec(20, 7.0));
        c.push(rec(30, 1.0)); // Point x=1
        c.push(rec(40, 2.0)); // Point x=2
        c.push(circle_rec(50, 5.0));
        c.push(rec(60, 3.0)); // Point x=3

        // Remove a middle point to leave a pool hole.
        assert!(c.remove_by_id(ObjectId(30).into()).is_some());

        // The point slabs contain exactly the three remaining points.
        let mut got: Vec<(u64, f64)> = Vec::new();
        let mut total = 0usize;
        c.visit_point_slab(|ids, geos| {
            assert_eq!(ids.len(), geos.len(), "ids y geos deben ir alineados");
            total += ids.len();
            for (id, g) in ids.iter().zip(geos.iter()) {
                got.push((id.raw().0, g.position.x));
            }
        });
        got.sort_by_key(|(id, _)| *id);
        assert_eq!(got, vec![(10, 0.0), (40, 2.0), (60, 3.0)]);
        assert_eq!(total, 3, "la suma de chunks == número de points vivos");
        assert!(
            !got.iter().any(|(id, _)| *id == 30),
            "el removido no aparece"
        );

        // Line and circle slabs contain their sole entities.
        let mut line_ids = Vec::new();
        c.visit_line_slab(|ids, geos| {
            assert_eq!(ids.len(), geos.len());
            line_ids.extend(ids.iter().map(|id| id.raw().0));
        });
        assert_eq!(line_ids, vec![20]);

        let mut circle_ids = Vec::new();
        c.visit_circle_slab(|ids, geos| {
            assert_eq!(ids.len(), geos.len());
            circle_ids.extend(ids.iter().map(|id| id.raw().0));
        });
        assert_eq!(circle_ids, vec![50]);

        // An empty type never invokes the callback.
        let mut arc_calls = 0;
        c.visit_arc_slab(|_ids, _geos| arc_calls += 1);
        assert_eq!(arc_calls, 0);
    }

    // Holes split slabs; compaction restores one contiguous slab.
    #[test]
    fn slab_multiples_chunks_y_un_solo_chunk_tras_compact() {
        let mut c = EntityContainer::new();
        for id in [10, 20, 30, 40, 50] {
            c.push(rec(id, id as f64));
        }
        // Remove alternating items to force holes.
        c.remove_by_id(ObjectId(20).into());
        c.remove_by_id(ObjectId(40).into());

        let mut chunks = 0;
        let mut total = 0;
        c.visit_point_slab(|ids, _geos| {
            chunks += 1;
            total += ids.len();
        });
        assert_eq!(total, 3);
        assert!(
            chunks >= 2,
            "con huecos debe haber varios chunks, hubo {chunks}"
        );

        c.compact().unwrap();

        let mut chunks_after = 0;
        let mut ids_after = Vec::new();
        c.visit_point_slab(|ids, _geos| {
            chunks_after += 1;
            ids_after.extend(ids.iter().map(|id| id.raw().0));
        });
        assert_eq!(chunks_after, 1, "tras compact debe haber un solo chunk");
        assert_eq!(ids_after, vec![10, 30, 50]);
    }

    #[test]
    fn entity_container_compact_rejects_generation_exhaustion_atomically() {
        let mut c = EntityContainer::new();
        c.push(line_rec(10, 0.0));
        let stale_line = c.by_id[&ObjectId(10).into()];
        c.push(line_rec(20, 1.0));
        let live_line = c.by_id[&ObjectId(20).into()];
        c.push(rec(30, 2.0));
        let stale_point = c.by_id[&ObjectId(30).into()];
        c.push(rec(40, 3.0));
        let live_point = c.by_id[&ObjectId(40).into()];
        c.remove_by_id(ObjectId(10).into()).unwrap();
        c.remove_by_id(ObjectId(30).into()).unwrap();
        // Preflight catches later pool exhaustion before mutating earlier pools.
        c.point.force_generation_exhaustion();

        let before_json = serde_json::to_string(&c).unwrap();
        let before_draw_order = c.draw_order.clone();
        let before_by_id = c.by_id.clone();
        assert!(c.line.contains(live_line.handle));
        assert!(c.point.contains(live_point.handle));
        assert!(!c.line.contains(stale_line.handle));
        assert!(!c.point.contains(stale_point.handle));

        assert_eq!(c.compact(), Err(CompactError::GenerationExhausted));
        assert_eq!(serde_json::to_string(&c).unwrap(), before_json);
        assert_eq!(c.draw_order, before_draw_order);
        assert_eq!(c.by_id, before_by_id);
        assert!(c.line.contains(live_line.handle));
        assert!(c.point.contains(live_point.handle));
        assert!(!c.line.contains(stale_line.handle));
        assert!(!c.point.contains(stale_point.handle));
        assert!(c.get(ObjectId(20).into()).is_some());
        assert!(c.get(ObjectId(40).into()).is_some());
    }

    // Per-item visitors return each live vector-backed geometry once.
    #[test]
    fn each_visita_exactamente_las_vivas_para_vec_geos() {
        let mut c = EntityContainer::new();
        c.push(poly_rec(10, 0.0));
        c.push(poly_rec(20, 1.0));
        c.push(poly_rec(30, 2.0));
        c.remove_by_id(ObjectId(20).into());

        let mut got: Vec<(u64, usize)> = Vec::new();
        c.visit_polyline_each(|id, g| got.push((id.raw().0, g.vertices.len())));
        got.sort_by_key(|(id, _)| *id);
        assert_eq!(got, vec![(10, 2), (30, 2)]);
        assert!(!got.iter().any(|(id, _)| *id == 20));
    }

    // Compaction preserves serialization, draw order, and ID lookup.
    #[test]
    fn compact_serde_byte_identico_y_draw_order() {
        let mut c = EntityContainer::new();
        // Mix variants and create holes in several pools.
        c.push(rec(10, 0.0)); // Point
        c.push(line_rec(20, 1.0));
        c.push(rec(30, 2.0)); // Point
        c.push(circle_rec(40, 3.0));
        c.push(rec(50, 4.0)); // Point
        c.push(line_rec(60, 5.0));
        c.push(poly_rec(70, 6.0));
        // Remove items of different types.
        c.remove_by_id(ObjectId(30).into());
        c.remove_by_id(ObjectId(20).into());

        let before_json = serde_json::to_string(&c).unwrap();
        let before_order = ids_in_order(&c);
        let before_bboxes: Vec<Option<BBox>> = before_order
            .iter()
            .map(|id| c.bbox(ObjectId(*id).into()))
            .collect();

        c.compact().unwrap();

        // Compaction changes no serialized state.
        let after_json = serde_json::to_string(&c).unwrap();
        assert_eq!(after_json, before_json, "compact alteró el serde");
        // Draw order remains intact.
        assert_eq!(ids_in_order(&c), before_order);
        // New handles resolve by ID and retain matching cached boxes.
        let after_bboxes: Vec<Option<BBox>> = before_order
            .iter()
            .map(|id| {
                assert!(c.get(ObjectId(*id).into()).is_some(), "id {id} no resuelve");
                c.bbox(ObjectId(*id).into())
            })
            .collect();
        assert_eq!(
            after_bboxes, before_bboxes,
            "columna bbox descuadrada tras compact"
        );

        // Repeated compaction is idempotent.
        c.compact().unwrap();
        assert_eq!(serde_json::to_string(&c).unwrap(), before_json);

        // Draw-order visitation reconstructs the same records.
        let via_visit: Vec<u64> = {
            let mut v = Vec::new();
            c.visit(|id, _c, _g| v.push(id.raw().0));
            v
        };
        assert_eq!(via_visit, before_order);
    }
}
