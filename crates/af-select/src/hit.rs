//! Point and window/crossing hit testing.
//!
//! Both start from [`SpatialIndex`] candidates, then evaluate exact geometry and
//! layer visibility without scanning the entire document.

use std::cmp::Ordering;

use af_math::{BBox, Point2};
use af_model::entity::{EntityGeometry, EntityOps, EntityRecord};
use af_model::id::EntityId;
use af_model::{Document, EntityContainer};

use crate::index::SpatialIndex;

/// Result of [`pick`], including exact distance and layer lock state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// Hit entity.
    pub id: EntityId,
    /// Exact point-to-geometry distance used for ranking.
    pub dist: f64,
    /// Whether the entity layer is locked.
    pub locked: bool,
}

/// Rectangle selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    /// Select only fully contained entities.
    Window,
    /// Select contained or intersecting entities.
    Crossing,
}

/// Samples per rectangle side for [`crosses_boundary`].
const CROSS_SAMPLES: usize = 32;

/// Returns entities hit near `pt` within world tolerance `tol`.
///
/// Candidates pass visibility and exact geometry checks, then sort by ascending
/// distance and descending draw order.
///
/// Off, frozen, and hidden entities are excluded; locked entities remain selectable.
#[must_use]
pub fn pick(doc: &Document, index: &SpatialIndex, pt: Point2, tol: f64) -> Vec<Hit> {
    let container = doc.container(index.container());
    // Keep draw order with each hit to avoid recomputing tie-break positions.
    let mut scored: Vec<(Hit, usize)> = Vec::new();

    for id in index.candidates_near(pt, tol) {
        let Some((rec, _)) = doc.entity(id) else {
            continue;
        };
        if !hit_visible(doc, &rec) {
            continue;
        }
        let Some(dist) = rec.geometry.hit(pt, tol) else {
            continue;
        };
        let locked = doc.layer(rec.layer).is_some_and(af_model::Layer::is_locked);
        let draw_order = container.and_then(|c| c.index_of(id)).unwrap_or(0);
        scored.push((Hit { id, dist, locked }, draw_order));
    }

    scored.sort_by(|(a, ao), (b, bo)| {
        a.dist
            .partial_cmp(&b.dist)
            .unwrap_or(Ordering::Equal)
            .then_with(|| bo.cmp(ao)) // Descending draw order: topmost first.
    });
    scored.into_iter().map(|(h, _)| h).collect()
}

/// Returns all IDs under `pt` in deterministic selection-cycling order.
///
/// This projects [`pick`] to IDs so ranking has one source of truth.
#[must_use]
pub fn pick_all(doc: &Document, index: &SpatialIndex, pt: Point2, tol: f64) -> Vec<EntityId> {
    pick(doc, index, pt, tol)
        .into_iter()
        .map(|h| h.id)
        .collect()
}

/// Selects entities against world rectangle `rect` using the requested mode.
///
/// - **Window** requires the entity bounds to be fully contained.
/// - **Crossing** accepts contained geometry or boundary intersections
///   ([`crosses_boundary`]).
///
/// Results follow ascending draw order and include locked entities.
#[must_use]
pub fn select_window(
    doc: &Document,
    index: &SpatialIndex,
    rect: BBox,
    mode: WindowMode,
) -> Vec<EntityId> {
    let container = doc.container(index.container());
    let mut candidates = index.query_rect(rect);
    sort_by_draw_order(container, &mut candidates);

    let mut out = Vec::new();
    for id in candidates {
        let Some((rec, _)) = doc.entity(id) else {
            continue;
        };
        if !hit_visible(doc, &rec) {
            continue;
        }
        let bb = rec.geometry.bbox();
        let selected = match mode {
            WindowMode::Window => rect.contains_bbox(bb),
            WindowMode::Crossing => {
                rect.contains_bbox(bb) || crosses_boundary(&rec.geometry, bb, rect)
            }
        };
        if selected {
            out.push(id);
        }
    }
    out
}

/// Sorts IDs by ascending draw order with raw ID as a stable fallback.
pub(crate) fn sort_by_draw_order(container: Option<&EntityContainer>, ids: &mut [EntityId]) {
    ids.sort_by_key(|id| {
        (
            container
                .and_then(|c| c.index_of(*id))
                .unwrap_or(usize::MAX),
            id.raw().0,
        )
    });
}

/// Returns whether an entity is visible and its layer is neither off nor frozen.
/// Unknown layers are treated as visible to preserve recoverable content.
pub(crate) fn hit_visible(doc: &Document, rec: &EntityRecord) -> bool {
    if !rec.visible {
        return false;
    }
    match doc.layer(rec.layer) {
        Some(layer) => !(layer.is_off() || layer.is_frozen()),
        None => true,
    }
}

/// Returns whether non-contained geometry crosses the rectangle boundary.
///
/// Rectangle sides are sampled across the bounds overlap and checked with
/// [`EntityOps::hit`]. The sampling tolerance prevents missed crossings at the cost
/// of a narrow false-positive band outside the rectangle.
fn crosses_boundary(geom: &EntityGeometry, bb: BBox, rect: BBox) -> bool {
    // Boundary crossings can occur only within the bounds intersection.
    let lox = bb.min.x.max(rect.min.x);
    let hix = bb.max.x.min(rect.max.x);
    let loy = bb.min.y.max(rect.min.y);
    let hiy = bb.max.y.min(rect.max.y);
    if hix < lox || hiy < loy {
        return false; // Defensive: candidates normally always overlap.
    }

    let ext = (hix - lox).max(hiy - loy);
    if ext <= 0.0 {
        // Use a rectangle-relative tolerance for point overlap at one corner.
        let corner = Point2::new(lox, loy);
        let t = rect.width().max(rect.height()).max(1.0) * 1e-9;
        return geom.hit(corner, t).is_some();
    }
    let tol = ext / CROSS_SAMPLES as f64;

    // Test only sides reachable within the entity bounds.
    let horiz = |y: f64| bb.min.y <= y && y <= bb.max.y && edge_hit(geom, true, y, lox, hix, tol);
    let vert = |x: f64| bb.min.x <= x && x <= bb.max.x && edge_hit(geom, false, x, loy, hiy, tol);

    horiz(rect.min.y) || horiz(rect.max.y) || vert(rect.min.x) || vert(rect.max.x)
}

/// Samples one axis-aligned side with spacing at most `tol`. Degenerate spans use
/// one point.
fn edge_hit(
    geom: &EntityGeometry,
    horizontal: bool,
    fixed: f64,
    lo: f64,
    hi: f64,
    tol: f64,
) -> bool {
    let span = hi - lo;
    // `n` intervals yield `n + 1` samples with spacing no greater than `tol`.
    let n = if span <= tol {
        1
    } else {
        (span / tol).ceil() as usize
    };
    for i in 0..=n {
        let t = lo + span * (i as f64 / n as f64);
        let p = if horizontal {
            Point2::new(t, fixed)
        } else {
            Point2::new(fixed, t)
        };
        if geom.hit(p, tol).is_some() {
            return true;
        }
    }
    false
}
