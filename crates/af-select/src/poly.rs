//! Arbitrary polygon and fence selection for non-rectangular regions.
//!
//! # Lasso
//! A lasso is a [`select_polygon`] call with a dense cursor trace. Polygons close
//! implicitly; freehand fences use [`select_fence`].
//!
//! Boundary intersections sample each path segment and call [`EntityOps::hit`].
//! This avoids missed crossings at the cost of a narrow tolerance-sized
//! false-positive band. Window containment uses ray-casting on feature points.

use af_math::{BBox, Point2};
use af_model::Document;
use af_model::entity::{EntityGeometry, EntityOps};
use af_model::id::EntityId;

use crate::WindowMode;
use crate::hit::{hit_visible, sort_by_draw_order};
use crate::index::SpatialIndex;

/// Samples per side; more samples narrow the false-positive band.
const EDGE_SAMPLES: usize = 32;

/// Selects entities by an implicitly closed, possibly concave polygon.
///
/// - [`WindowMode::Window`] requires full containment.
/// - [`WindowMode::Crossing`] accepts containment or boundary contact.
///
/// Results follow draw order. Polygons with fewer than three vertices return empty.
#[must_use]
pub fn select_polygon(
    doc: &Document,
    index: &SpatialIndex,
    polygon: &[Point2],
    mode: WindowMode,
) -> Vec<EntityId> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let Some(region) = BBox::from_points(polygon.iter().copied()) else {
        return Vec::new();
    };
    let container = doc.container(index.container());
    let mut candidates = index.query_rect(region);
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
        let crosses = crosses_path(&rec.geometry, bb, polygon, true, region);
        let selected = match mode {
            // Accept contained geometry or boundary contact.
            WindowMode::Crossing => crosses || any_inside(&rec.geometry, bb, polygon),
            // Full containment requires no contact and every feature point inside.
            WindowMode::Window => !crosses && all_inside(&rec.geometry, bb, polygon),
        };
        if selected {
            out.push(id);
        }
    }
    out
}

/// Selects entities touching any segment of open polyline `fence`.
///
/// Results follow draw order. Fewer than two points return empty.
#[must_use]
pub fn select_fence(doc: &Document, index: &SpatialIndex, fence: &[Point2]) -> Vec<EntityId> {
    if fence.len() < 2 {
        return Vec::new();
    }
    let Some(region) = BBox::from_points(fence.iter().copied()) else {
        return Vec::new();
    };
    let container = doc.container(index.container());
    let mut candidates = index.query_rect(region);
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
        if crosses_path(&rec.geometry, bb, fence, false, region) {
            out.push(id);
        }
    }
    out
}

/// Returns whether geometry touches any segment of `path`.
///
/// Sampling tolerance derives from entity size, with a region-relative minimum
/// for degenerate bounds.
///
/// Each side is clipped to inflated entity bounds so long paths cannot create an
/// unbounded number of samples.
fn crosses_path(
    geom: &EntityGeometry,
    bb: BBox,
    path: &[Point2],
    closed: bool,
    region_bb: BBox,
) -> bool {
    let diag = (bb.width() * bb.width() + bb.height() * bb.height()).sqrt();
    let tol = if diag > 0.0 {
        diag / EDGE_SAMPLES as f64
    } else {
        // Degenerate bounds use a region-relative nonzero tolerance.
        region_bb.width().max(region_bb.height()).max(1.0) * 1e-9
    };
    let min = Point2::new(bb.min.x - tol, bb.min.y - tol);
    let max = Point2::new(bb.max.x + tol, bb.max.y + tol);

    let n = path.len();
    let seg_count = if closed { n } else { n - 1 };
    (0..seg_count).any(|k| sample_segment(geom, path[k], path[(k + 1) % n], min, max, tol))
}

/// Clips segment `a→b` to `[min,max]` and samples it with spacing at most `tol`.
fn sample_segment(
    geom: &EntityGeometry,
    a: Point2,
    b: Point2,
    min: Point2,
    max: Point2,
    tol: f64,
) -> bool {
    let Some((t0, t1)) = clip_segment(a, b, min, max) else {
        return false;
    };
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let p0 = Point2::new(a.x + dx * t0, a.y + dy * t0);
    let (sx, sy) = (dx * (t1 - t0), dy * (t1 - t0));
    let len = (sx * sx + sy * sy).sqrt();
    // `n` intervals yield `n + 1` samples; clipping bounds their count.
    let n = if len <= tol {
        1
    } else {
        (len / tol).ceil() as usize
    };
    (0..=n).any(|i| {
        let f = i as f64 / n as f64;
        let p = Point2::new(p0.x + sx * f, p0.y + sy * f);
        geom.hit(p, tol).is_some()
    })
}

/// Liang-Barsky clips segment `a→b` to AABB `[min,max]` and returns its parameter range.
fn clip_segment(a: Point2, b: Point2, min: Point2, max: Point2) -> Option<(f64, f64)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    // Per edge, inside means `p·t ≤ q`; `p = 0` means a parallel segment.
    for (p, q) in [
        (-dx, a.x - min.x),
        (dx, max.x - a.x),
        (-dy, a.y - min.y),
        (dy, max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None; // Parallel and outside the edge half-plane.
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                t0 = t0.max(r);
            } else {
                if r < t0 {
                    return None;
                }
                t1 = t1.min(r);
            }
        }
    }
    Some((t0, t1))
}

/// Returns geometry feature points, or the bounds center when none are declared.
fn representatives(geom: &EntityGeometry, bb: BBox) -> Vec<Point2> {
    let pts: Vec<Point2> = geom.snap_points().iter().map(|sp| sp.point).collect();
    if pts.is_empty() {
        vec![Point2::new(
            (bb.min.x + bb.max.x) * 0.5,
            (bb.min.y + bb.max.y) * 0.5,
        )]
    } else {
        pts
    }
}

/// Returns whether any representative point lies inside the polygon.
fn any_inside(geom: &EntityGeometry, bb: BBox, polygon: &[Point2]) -> bool {
    representatives(geom, bb)
        .into_iter()
        .any(|p| point_in_polygon(p, polygon))
}

/// Returns whether every representative point lies inside the polygon.
fn all_inside(geom: &EntityGeometry, bb: BBox, polygon: &[Point2]) -> bool {
    representatives(geom, bb)
        .into_iter()
        .all(|p| point_in_polygon(p, polygon))
}

/// Ray-casting point-in-polygon test supporting concave, implicitly closed polygons.
pub(crate) fn point_in_polygon(p: Point2, poly: &[Point2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        // Does edge `pj→pi` cross the horizontal ray through `p`?
        if (pi.y > p.y) != (pj.y > p.y) {
            let x_int = pi.x + (p.y - pi.y) / (pj.y - pi.y) * (pj.x - pi.x);
            if p.x < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
