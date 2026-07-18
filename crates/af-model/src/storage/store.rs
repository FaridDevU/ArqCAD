//! [`TypedStore<G>`] keeps geometry, common properties, and bounding boxes aligned.
//!
//! Geometry lives in a contiguous [`Pool`], while common properties use parallel
//! vectors at the same slot indices. Every mutation goes through `TypedStore`.

use af_math::BBox;

use super::pool::{Handle, Pool, SlotFill};
use crate::container::CompactError;
use crate::entity::{Color, LineTypeRef, Lineweight};
use crate::id::{EntityId, LayerId};

/// Copyable common-property row for an entity.
///
/// This mirrors the non-geometry fields of
/// [`EntityRecord`](crate::entity::EntityRecord). It omits `Eq` because color and
/// lineweight values contain `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CommonRow {
    /// Persistent entity identity.
    pub(crate) id: EntityId,
    /// Owning layer.
    pub(crate) layer: LayerId,
    /// Color, which may be inherited.
    pub(crate) color: Color,
    /// Line type, which may be inherited.
    pub(crate) line_type: LineTypeRef,
    /// Lineweight, which may be inherited.
    pub(crate) lineweight: Lineweight,
    /// Per-entity visibility.
    pub(crate) visible: bool,
}

/// Structure-of-arrays common columns indexed by pool slot.
///
/// Private mutation through [`TypedStore`] keeps these vectors aligned with the
/// geometry pool. Released rows remain inaccessible until overwritten on reuse.
#[derive(Debug, Default, Clone)]
pub(crate) struct CommonColumns {
    id: Vec<EntityId>,
    layer: Vec<LayerId>,
    color: Vec<Color>,
    line_type: Vec<LineTypeRef>,
    lineweight: Vec<Lineweight>,
    visible: Vec<bool>,
}

impl CommonColumns {
    fn new() -> Self {
        Self::default()
    }

    /// Returns the shared column length.
    fn len(&self) -> usize {
        self.id.len()
    }

    /// Places `row` at a new or recycled pool slot while keeping columns aligned.
    fn place(&mut self, index: usize, row: CommonRow) {
        if index == self.len() {
            self.id.push(row.id);
            self.layer.push(row.layer);
            self.color.push(row.color);
            self.line_type.push(row.line_type);
            self.lineweight.push(row.lineweight);
            self.visible.push(row.visible);
        } else {
            self.id[index] = row.id;
            self.layer[index] = row.layer;
            self.color[index] = row.color;
            self.line_type[index] = row.line_type;
            self.lineweight[index] = row.lineweight;
            self.visible[index] = row.visible;
        }
    }

    /// Reads a row after the caller has validated its pool handle.
    fn row(&self, index: usize) -> CommonRow {
        CommonRow {
            id: self.id[index],
            layer: self.layer[index],
            color: self.color[index],
            line_type: self.line_type[index],
            lineweight: self.lineweight[index],
            visible: self.visible[index],
        }
    }
}

/// Zero-copy reference to the common columns of a validated live slot.
///
/// Accessors return copyable values without exposing or mutating the columns.
///
/// [`EntityContainer::visit`]: crate::container::EntityContainer::visit
#[derive(Clone, Copy, Debug)]
pub struct CommonRef<'a> {
    cols: &'a CommonColumns,
    index: usize,
}

impl CommonRef<'_> {
    /// Persistent entity identity.
    #[must_use]
    pub fn id(&self) -> EntityId {
        self.cols.id[self.index]
    }

    /// Owning layer.
    #[must_use]
    pub fn layer(&self) -> LayerId {
        self.cols.layer[self.index]
    }

    /// Color, which may be inherited from a layer or block.
    #[must_use]
    pub fn color(&self) -> Color {
        self.cols.color[self.index]
    }

    /// Line type, which may be inherited.
    #[must_use]
    pub fn line_type(&self) -> LineTypeRef {
        self.cols.line_type[self.index]
    }

    /// Lineweight, which may be inherited.
    #[must_use]
    pub fn lineweight(&self) -> Lineweight {
        self.cols.lineweight[self.index]
    }

    /// Per-entity visibility.
    #[must_use]
    pub fn visible(&self) -> bool {
        self.cols.visible[self.index]
    }
}

/// Typed geometry pool with aligned common-property and bounding-box columns.
///
/// Structural cloning preserves handles. Geometry writes update the supplied
/// bounding box in the same operation so the parallel columns stay synchronized.
///
/// [`insert`]: TypedStore::insert
/// [`set_geometry`]: TypedStore::set_geometry
#[derive(Debug, Clone)]
pub(crate) struct TypedStore<G> {
    geo: Pool<G>,
    common: CommonColumns,
    /// Bounding box for each pool slot.
    bbox: Vec<BBox>,
}

impl<G> TypedStore<G> {
    /// Creates an empty store.
    pub(crate) fn new() -> Self {
        Self {
            geo: Pool::new(),
            common: CommonColumns::new(),
            bbox: Vec::new(),
        }
    }

    /// Places a bounding box at a new or recycled pool slot.
    fn place_bbox(&mut self, index: usize, bb: BBox) {
        if index == self.bbox.len() {
            self.bbox.push(bb);
        } else {
            self.bbox[index] = bb;
        }
    }

    /// Inserts geometry, common properties, and bounding box at one slot.
    pub(crate) fn insert(&mut self, geometry: G, row: CommonRow, bbox: BBox) -> Handle {
        let handle = self.geo.insert(geometry);
        // New slots append; recycled slots overwrite every parallel column.
        self.common.place(handle.index as usize, row);
        self.place_bbox(handle.index as usize, bbox);
        handle
    }

    /// Removes and returns geometry plus common properties for `handle`.
    ///
    /// The dense pool leaves a [`SlotFill`] value after moving geometry out.
    pub(crate) fn remove(&mut self, handle: Handle) -> Option<(G, CommonRow)>
    where
        G: SlotFill,
    {
        let geometry = self.geo.remove(handle)?;
        let row = self.common.row(handle.index as usize);
        Some((geometry, row))
    }

    /// Reads geometry and copied common properties from one validated slot.
    pub(crate) fn get(&self, handle: Handle) -> Option<(&G, CommonRow)> {
        let geometry = self.geo.get(handle)?;
        let row = self.common.row(handle.index as usize);
        Some((geometry, row))
    }

    /// Returns zero-copy geometry and common-column views for a validated handle.
    pub(crate) fn view(&self, handle: Handle) -> Option<(&G, CommonRef<'_>)> {
        let geometry = self.geo.get(handle)?;
        let common = CommonRef {
            cols: &self.common,
            index: handle.index as usize,
        };
        Some((geometry, common))
    }

    /// Returns common properties and cached bounding box without reading geometry.
    pub(crate) fn view_bbox(&self, handle: Handle) -> Option<(CommonRef<'_>, &BBox)> {
        if self.geo.contains(handle) {
            let index = handle.index as usize;
            let common = CommonRef {
                cols: &self.common,
                index,
            };
            Some((common, &self.bbox[index]))
        } else {
            None
        }
    }

    /// Returns the cached bounding box for `handle`.
    pub(crate) fn bbox(&self, handle: Handle) -> Option<BBox> {
        if self.geo.contains(handle) {
            Some(self.bbox[handle.index as usize])
        } else {
            None
        }
    }

    /// Replaces geometry and its bounding box in place.
    ///
    /// Returns `false` if the handle does not resolve. Identity and order remain
    /// unchanged.
    pub(crate) fn set_geometry(&mut self, handle: Handle, geometry: G, bbox: BBox) -> bool {
        if let Some(slot) = self.geo.get_mut(handle) {
            *slot = geometry;
            self.bbox[handle.index as usize] = bbox;
            true
        } else {
            false
        }
    }

    /// Returns only the geometry for `handle`.
    pub(crate) fn geometry(&self, handle: Handle) -> Option<&G> {
        self.geo.get(handle)
    }

    /// Returns mutable geometry for in-place editing.
    pub(crate) fn geometry_mut(&mut self, handle: Handle) -> Option<&mut G> {
        self.geo.get_mut(handle)
    }

    /// Returns only the copied common-property row for `handle`.
    pub(crate) fn common(&self, handle: Handle) -> Option<CommonRow> {
        if self.geo.contains(handle) {
            Some(self.common.row(handle.index as usize))
        } else {
            None
        }
    }

    /// Replaces common properties without changing geometry.
    pub(crate) fn set_common(&mut self, handle: Handle, row: CommonRow) -> bool {
        if self.geo.contains(handle) {
            self.common.place(handle.index as usize, row);
            true
        } else {
            false
        }
    }

    /// Returns whether `handle` resolves.
    pub(crate) fn contains(&self, handle: Handle) -> bool {
        self.geo.contains(handle)
    }

    /// Returns the number of live entities.
    pub(crate) fn len(&self) -> usize {
        self.geo.len()
    }

    /// Returns whether the store is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.geo.is_empty()
    }

    /// Iterates live handles in physical slot order.
    pub(crate) fn iter_handles(&self) -> impl Iterator<Item = Handle> + '_ {
        self.geo.iter_handles()
    }

    /// Visits aligned entity-ID and geometry slices for each maximal live run.
    ///
    /// Runs use physical pool order, and borrowed slices are valid only during the
    /// callback.
    pub(crate) fn visit_slab<F>(&self, mut f: F)
    where
        F: FnMut(&[EntityId], &[G]),
    {
        self.geo.visit_runs(|start, geos| {
            let ids = &self.common.id[start..start + geos.len()];
            f(ids, geos);
        });
    }

    /// Visits each live entity ID and geometry reference in physical pool order.
    pub(crate) fn visit_each<F>(&self, mut f: F)
    where
        F: FnMut(EntityId, &G),
    {
        self.geo.visit_runs(|start, geos| {
            for (k, g) in geos.iter().enumerate() {
                f(self.common.id[start + k], g);
            }
        });
    }

    /// Compacts the pool and parallel columns with the same permutation.
    ///
    /// Returns old-to-new handle mappings without changing identity or survivor
    /// order.
    pub(crate) fn preflight_compact(&self) -> Result<(), CompactError> {
        self.geo.next_compact_generation().map(|_| ())
    }

    pub(crate) fn compact(&mut self) -> Result<Vec<(Handle, Handle)>, CompactError> {
        let remap = self.geo.compact()?;
        // Rebuild parallel columns from old positions in ascending new-slot order.
        let mut common = CommonColumns::new();
        let mut bbox = Vec::with_capacity(remap.len());
        for (old, new) in &remap {
            common.place(new.index as usize, self.common.row(old.index as usize));
            bbox.push(self.bbox[old.index as usize]);
        }
        self.common = common;
        self.bbox = bbox;
        Ok(remap)
    }

    #[cfg(test)]
    pub(crate) fn force_generation_exhaustion(&mut self) {
        self.geo.force_generation_exhaustion();
    }
}

impl<G> Default for TypedStore<G> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::collection::vec;
    use proptest::prelude::*;

    use af_math::Point2;

    use crate::entity::{AciColor, Color, LineTypeRef, Lineweight};
    use crate::id::ObjectId;

    /// Returns a deterministic synthetic bounding box for alignment checks.
    fn bb(seed: u64) -> BBox {
        let s = seed as f64;
        BBox::new(Point2::new(s, s + 1.0), Point2::new(s + 2.0, s + 3.0))
    }

    /// Returns a deterministic common row with distinct values in each column.
    fn row_from(seed: u64) -> CommonRow {
        let n = seed.max(1);
        CommonRow {
            id: ObjectId(n).into(),
            layer: ObjectId(n + 1).into(),
            color: Color::Aci(AciColor::new((seed % 255) as u8 + 1).unwrap()),
            line_type: if seed.is_multiple_of(2) {
                LineTypeRef::ByLayer
            } else {
                LineTypeRef::ByBlock
            },
            lineweight: Lineweight::Mm(seed as f32 * 0.5),
            visible: !seed.is_multiple_of(3),
        }
    }

    #[test]
    fn insert_get_remove_roundtrip() {
        let mut s = TypedStore::<u64>::new();
        let h = s.insert(7, row_from(7), bb(7));
        assert_eq!(s.get(h), Some((&7, row_from(7))));
        assert_eq!(s.geometry(h), Some(&7));
        assert_eq!(s.common(h), Some(row_from(7)));
        // The bounding-box column returns the inserted value.
        assert_eq!(s.bbox(h), Some(bb(7)));
        assert_eq!(s.len(), 1);
        assert_eq!(s.remove(h), Some((7, row_from(7))));
        assert_eq!(s.get(h), None);
        // A stale handle cannot access a bounding box.
        assert_eq!(s.bbox(h), None);
        assert!(s.is_empty());
    }

    #[test]
    fn view_matches_get_zero_copy() {
        let mut s = TypedStore::<u64>::new();
        let h = s.insert(9, row_from(9), bb(9));
        let (g, common) = s.view(h).unwrap();
        // Geometry is borrowed from the same pool cell rather than copied.
        assert!(std::ptr::eq(g, s.geometry(h).unwrap()));
        // Common-column accessors match the inserted row.
        let row = row_from(9);
        assert_eq!(common.id(), row.id);
        assert_eq!(common.layer(), row.layer);
        assert_eq!(common.color(), row.color);
        assert_eq!(common.line_type(), row.line_type);
        assert_eq!(common.lineweight(), row.lineweight);
        assert_eq!(common.visible(), row.visible);
        // The bounding-box view borrows the same slot and common row.
        let (vcommon, vbb) = s.view_bbox(h).unwrap();
        assert_eq!(vcommon.id(), row.id);
        assert_eq!(*vbb, bb(9));
        // A stale handle provides neither geometry nor bounding-box views.
        assert_eq!(s.remove(h), Some((9, row_from(9))));
        assert!(s.view(h).is_none());
        assert!(s.view_bbox(h).is_none());
    }

    #[test]
    fn set_common_edits_only_properties() {
        let mut s = TypedStore::<u64>::new();
        let h = s.insert(1, row_from(1), bb(1));
        assert!(s.set_common(h, row_from(50)));
        // Geometry and bounding box remain intact while common properties change.
        assert_eq!(s.geometry(h), Some(&1));
        assert_eq!(s.bbox(h), Some(bb(1)));
        assert_eq!(s.common(h), Some(row_from(50)));
    }

    #[test]
    fn set_geometry_rewrites_geo_and_bbox() {
        let mut s = TypedStore::<u64>::new();
        let h = s.insert(1, row_from(1), bb(1));
        // Geometry and bounding box change together; common properties remain.
        assert!(s.set_geometry(h, 2, bb(99)));
        assert_eq!(s.geometry(h), Some(&2));
        assert_eq!(s.bbox(h), Some(bb(99)));
        assert_eq!(s.common(h), Some(row_from(1)));
        // A stale handle cannot be rewritten.
        s.remove(h);
        assert!(!s.set_geometry(h, 3, bb(0)));
    }

    #[test]
    fn recycled_slot_rewrites_common_row() {
        let mut s = TypedStore::<u64>::new();
        let h1 = s.insert(10, row_from(10), bb(10));
        assert_eq!(s.remove(h1), Some((10, row_from(10))));
        let h2 = s.insert(20, row_from(20), bb(20));
        // Reuse keeps the slot but replaces all values and invalidates the old handle.
        assert_eq!(h1.index, h2.index);
        assert_eq!(s.get(h1), None);
        assert_eq!(s.get(h2), Some((&20, row_from(20))));
        assert_eq!(s.bbox(h1), None);
        assert_eq!(s.bbox(h2), Some(bb(20)));
    }

    // ---------- Property test: geometry and common columns stay aligned ----------

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u64),
        Remove(usize),
        Get(usize),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            any::<u64>().prop_map(Op::Insert),
            any::<usize>().prop_map(Op::Remove),
            any::<usize>().prop_map(Op::Get),
        ]
    }

    proptest! {
        #[test]
        fn geo_and_common_stay_synced(ops in vec(op_strategy(), 0..200)) {
            let mut store = TypedStore::<u64>::new();
            // Model each live handle with the seed used for geometry and properties.
            let mut live: Vec<(Handle, u64)> = Vec::new();
            let mut freed: Vec<Handle> = Vec::new();

            for op in ops {
                match op {
                    Op::Insert(seed) => {
                        let h = store.insert(seed, row_from(seed), bb(seed));
                        prop_assert!(!freed.contains(&h));
                        live.push((h, seed));
                    }
                    Op::Remove(i) => {
                        if live.is_empty() {
                            continue;
                        }
                        let idx = i % live.len();
                        let (h, seed) = live.remove(idx);
                        prop_assert_eq!(store.remove(h), Some((seed, row_from(seed))));
                        prop_assert_eq!(store.remove(h), None);
                        freed.push(h);
                    }
                    Op::Get(i) => {
                        if live.is_empty() {
                            continue;
                        }
                        let idx = i % live.len();
                        let (h, seed) = live[idx];
                        prop_assert_eq!(store.get(h), Some((&seed, row_from(seed))));
                    }
                }

                prop_assert_eq!(store.len(), live.len());
                prop_assert_eq!(store.is_empty(), live.is_empty());

                // Released handles cannot resolve geometry, properties, views, or boxes.
                for h in &freed {
                    prop_assert!(store.get(*h).is_none());
                    prop_assert!(store.common(*h).is_none());
                    prop_assert!(store.view(*h).is_none());
                    prop_assert!(store.bbox(*h).is_none());
                    prop_assert!(store.view_bbox(*h).is_none());
                    prop_assert!(!store.contains(*h));
                }

                // Live geometry, common properties, and boxes match their seed.
                for (h, seed) in &live {
                    prop_assert_eq!(store.get(*h), Some((seed, row_from(*seed))));
                    prop_assert_eq!(store.geometry(*h), Some(seed));
                    prop_assert_eq!(store.common(*h), Some(row_from(*seed)));
                    prop_assert_eq!(store.bbox(*h), Some(bb(*seed)));
                    // The zero-copy view observes the same geometry and properties.
                    let (g, common) = store.view(*h).unwrap();
                    prop_assert_eq!(g, seed);
                    let row = row_from(*seed);
                    prop_assert_eq!(common.id(), row.id);
                    prop_assert_eq!(common.layer(), row.layer);
                    prop_assert_eq!(common.visible(), row.visible);
                    // The bounding-box view exposes the same box and entity ID.
                    let (bcommon, bbb) = store.view_bbox(*h).unwrap();
                    prop_assert_eq!(bcommon.id(), row.id);
                    prop_assert_eq!(*bbb, bb(*seed));
                }
            }
        }
    }
}
