//! [`SpatialIndex`], an AABB R*-tree for one document container.
//!
//! # Lifecycle
//! - [`build`](SpatialIndex::build) bulk-loads a document.
//! - [`apply_changeset`](SpatialIndex::apply_changeset) removes and reinserts only
//!   affected IDs without rebuilding the tree.
//!
//! The derived index is not serialized. It stores IDs and cached AABBs for removal.
//! Layer visibility is filtered by queries so layer-state changes need no reindex.
//!
//! Infinite geometry must stay outside this tree because infinite AABBs degrade it.

use std::collections::HashMap;

use af_math::{BBox, Point2};
use af_model::entity::EntityOps;
use af_model::id::EntityId;
use af_model::{ChangeSet, ContainerRef, Document};
use rstar::{AABB, RTree, RTreeObject};

/// R-tree envelope using `[f64; 2]` points.
type Aabb = AABB<[f64; 2]>;

#[inline]
fn to_aabb(bb: BBox) -> Aabb {
    AABB::from_corners([bb.min.x, bb.min.y], [bb.max.x, bb.max.y])
}

/// R-tree item containing an ID and its AABB.
///
/// Equality uses only ID so removal works when entities share an AABB.
#[derive(Clone, Debug)]
struct IndexEntry {
    id: EntityId,
    env: Aabb,
}

impl PartialEq for IndexEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl RTreeObject for IndexEntry {
    type Envelope = Aabb;
    fn envelope(&self) -> Aabb {
        self.env
    }
}

/// R*-tree spatial index for one container.
pub struct SpatialIndex {
    container: ContainerRef,
    tree: RTree<IndexEntry>,
    /// Latest indexed `id → AABB`, used for exact removal and constant-time lookup.
    aabbs: HashMap<EntityId, Aabb>,
}

impl SpatialIndex {
    /// Bulk-loads the index from document `container`.
    ///
    /// A missing container produces an empty index.
    #[must_use]
    pub fn build(doc: &Document, container: ContainerRef) -> Self {
        let mut aabbs = HashMap::new();
        let mut entries = Vec::new();
        if let Some(c) = doc.container(container) {
            entries.reserve(c.len());
            aabbs.reserve(c.len());
            // Read the maintained pool bounds column without materializing records.
            c.visit_bboxes(|id, _common, bb| {
                let env = to_aabb(*bb);
                aabbs.insert(id, env);
                entries.push(IndexEntry { id, env });
            });
        }
        Self {
            container,
            tree: RTree::bulk_load(entries),
            aabbs,
        }
    }

    /// Container indexed by this tree.
    #[must_use]
    pub fn container(&self) -> ContainerRef {
        self.container
    }

    /// Applies a [`ChangeSet`] incrementally against the post-change document.
    ///
    /// Only changed IDs in this container are removed or inserted.
    pub fn apply_changeset(&mut self, cs: &ChangeSet, doc: &Document) {
        for &id in cs.added() {
            self.reinsert(id, doc);
        }
        for &id in cs.removed() {
            self.remove_id(id);
        }
        for &id in cs.modified() {
            self.reinsert(id, doc);
        }
    }

    /// Returns unordered IDs whose AABBs intersect `rect`.
    #[must_use]
    pub fn query_rect(&self, rect: BBox) -> Vec<EntityId> {
        let env = to_aabb(rect);
        self.tree
            .locate_in_envelope_intersecting(env)
            .map(|e| e.id)
            .collect()
    }

    /// Returns IDs whose AABBs intersect the `pt ± radius` query box.
    #[must_use]
    pub fn candidates_near(&self, pt: Point2, radius: f64) -> Vec<EntityId> {
        let r = radius.max(0.0);
        let rect = BBox::new(
            Point2::new(pt.x - r, pt.y - r),
            Point2::new(pt.x + r, pt.y + r),
        );
        self.query_rect(rect)
    }

    /// Number of indexed entities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.aabbs.len()
    }

    /// Returns whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.aabbs.is_empty()
    }

    /// Returns whether `id` is indexed.
    #[must_use]
    pub fn contains(&self, id: EntityId) -> bool {
        self.aabbs.contains_key(&id)
    }

    /// Returns every indexed ID in deterministic ascending order.
    #[must_use]
    pub fn ids(&self) -> Vec<EntityId> {
        let mut v: Vec<EntityId> = self.aabbs.keys().copied().collect();
        v.sort_unstable_by_key(|id| id.raw().0);
        v
    }

    /// Removes `id` using its cached AABB; unknown IDs are ignored.
    fn remove_id(&mut self, id: EntityId) {
        if let Some(env) = self.aabbs.remove(&id) {
            self.tree.remove(&IndexEntry { id, env });
        }
    }

    /// Reinserts `id` with current bounds, ignoring missing or foreign-container IDs.
    fn reinsert(&mut self, id: EntityId, doc: &Document) {
        self.remove_id(id);
        if let Some((rec, c)) = doc.entity(id)
            && c == self.container
        {
            let env = to_aabb(rec.geometry.bbox());
            self.aabbs.insert(id, env);
            self.tree.insert(IndexEntry { id, env });
        }
    }
}
