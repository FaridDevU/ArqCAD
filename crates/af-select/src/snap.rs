//! Snapping engine that ranks feature points near the cursor.
//!
//! It starts from [`SpatialIndex`] candidates, gathers declared geometry snaps,
//! adds calculated snaps, then filters by radius, mask, and layer visibility.
//!
//! # Ranking
//! `score = dist_px − bonus(kind)`; lower scores win. Ties use kind priority,
//! entity ID, then coordinates for deterministic results.
//!
//! [`SnapOpts::px_per_unit`] converts world distance to pixel-denominated ranking.
//! Grid snap is independent of entities and remains outside this crate.
//!
//! # Calculated snaps
//! Intersections combine candidate pairs. Nearest projects the cursor.
//! Perpendicular and tangent use `last_point`. Extension projects beyond bounded
//! line or arc spans. Geometric center uses closed polylines.

use af_geom::{
    ArcSeg, LineX, arc_arc, circle_arc, circle_circle, line_arc, line_circle, line_line,
    nearest_on_arc, nearest_on_segment, perp_foot_line, polygon_centroid, project_on_circle,
    tangent_points,
};
use af_math::Point2;
use af_math::angle::{angle_in_sweep, angle_of};
use af_model::Document;
use af_model::entity::{
    EntityGeometry, EntityOps, EntityRecord, SegKind, SnapKind, SplineGeo, WipeoutGeo,
};
use af_model::id::EntityId;

use crate::index::SpatialIndex;

/// Ranked snap with exact point, kind, source entity, and world distance.
///
/// Intersections use the lower source entity ID for deterministic ownership.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapHit {
    /// Exact geometry feature point.
    pub point: Point2,
    /// Snap kind.
    pub kind: SnapKind,
    /// Source entity, using the lower ID for intersections.
    pub entity: EntityId,
    /// Cursor-to-point world distance.
    pub dist: f64,
}

/// User-selectable snap-kind mask. Disabled kinds are not calculated.
///
/// A `u16` covers all 12 [`SnapKind`] variants without another dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapMask(u16);

/// Returns the bit representing a kind in [`SnapMask`].
const fn bit(kind: SnapKind) -> u16 {
    match kind {
        SnapKind::Endpoint => 1 << 0,
        SnapKind::Midpoint => 1 << 1,
        SnapKind::Center => 1 << 2,
        SnapKind::Node => 1 << 3,
        SnapKind::Quadrant => 1 << 4,
        SnapKind::Insertion => 1 << 5,
        SnapKind::Intersection => 1 << 6,
        SnapKind::Perpendicular => 1 << 7,
        SnapKind::Nearest => 1 << 8,
        SnapKind::Tangent => 1 << 9,
        SnapKind::Extension => 1 << 10,
        SnapKind::GeometricCenter => 1 << 11,
    }
}

impl SnapMask {
    /// All 12 kinds enabled.
    pub const ALL: SnapMask = SnapMask(0x0FFF);
    /// No kinds enabled.
    pub const NONE: SnapMask = SnapMask(0);

    /// Returns whether `kind` is enabled.
    #[inline]
    #[must_use]
    pub const fn contains(self, kind: SnapKind) -> bool {
        self.0 & bit(kind) != 0
    }

    /// Returns the mask with `kind` enabled.
    #[inline]
    #[must_use]
    pub const fn with(self, kind: SnapKind) -> Self {
        SnapMask(self.0 | bit(kind))
    }

    /// Returns the mask with `kind` disabled.
    #[inline]
    #[must_use]
    pub const fn without(self, kind: SnapKind) -> Self {
        SnapMask(self.0 & !bit(kind))
    }
}

/// Snapping query options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapOpts {
    /// Enabled snap kinds.
    pub kinds: SnapMask,
    /// World-to-pixel scale used by ranking bonuses.
    pub px_per_unit: f64,
    /// Reference point for perpendicular and tangent snaps.
    pub last_point: Option<Point2>,
}

impl Default for SnapOpts {
    fn default() -> Self {
        Self {
            kinds: SnapMask::ALL,
            px_per_unit: 1.0,
            last_point: None,
        }
    }
}

/// Pixel bonus for each kind in `score = dist_px − bonus`.
#[inline]
fn bonus_px(kind: SnapKind) -> f64 {
    match kind {
        SnapKind::Endpoint => 3.0,
        SnapKind::Midpoint
        | SnapKind::Center
        | SnapKind::Node
        | SnapKind::Quadrant
        | SnapKind::Insertion
        | SnapKind::Intersection
        | SnapKind::GeometricCenter => 2.0,
        SnapKind::Perpendicular | SnapKind::Tangent | SnapKind::Extension | SnapKind::Nearest => {
            1.0
        }
    }
}

/// Kind tie-break priority; lower values win.
#[inline]
fn priority(kind: SnapKind) -> u8 {
    match kind {
        SnapKind::Endpoint => 0,
        SnapKind::Midpoint => 1,
        SnapKind::Center => 2,
        SnapKind::Node => 3,
        SnapKind::Quadrant => 4,
        SnapKind::Intersection => 5,
        SnapKind::GeometricCenter => 6,
        SnapKind::Insertion => 7,
        SnapKind::Perpendicular => 8,
        SnapKind::Tangent => 9,
        SnapKind::Extension => 10,
        SnapKind::Nearest => 11,
    }
}

/// Computes `score = world_distance · px_per_unit − bonus`.
#[inline]
fn score(hit: &SnapHit, px_per_unit: f64) -> f64 {
    hit.dist * px_per_unit - bonus_px(hit.kind)
}

/// Total deterministic ordering by score, kind priority, entity ID, then coordinates.
fn rank(a: &SnapHit, b: &SnapHit, px_per_unit: f64) -> std::cmp::Ordering {
    score(a, px_per_unit)
        .total_cmp(&score(b, px_per_unit))
        .then_with(|| priority(a.kind).cmp(&priority(b.kind)))
        .then_with(|| a.entity.raw().0.cmp(&b.entity.raw().0))
        .then_with(|| a.point.x.total_cmp(&b.point.x))
        .then_with(|| a.point.y.total_cmp(&b.point.y))
}

/// Pushes a snap when it lies within the circular aperture.
#[inline]
fn push_near(
    hits: &mut Vec<SnapHit>,
    point: Point2,
    kind: SnapKind,
    entity: EntityId,
    cursor: Point2,
    r: f64,
) {
    let dist = cursor.dist(point);
    if dist <= r {
        hits.push(SnapHit {
            point,
            kind,
            entity,
            dist,
        });
    }
}

/// Returns ranked snap candidates within world-space `radius` of `cursor`.
///
/// Off, frozen, and hidden entities are excluded. Locked entities remain eligible.
///
/// Ranking is independent of candidate iteration order.
#[must_use]
pub fn snap(
    doc: &Document,
    index: &SpatialIndex,
    cursor: Point2,
    radius: f64,
    opts: SnapOpts,
) -> Vec<SnapHit> {
    let r = radius.max(0.0);

    // Materialize visible candidates once for both declared and calculated snaps.
    let owned: Vec<(EntityId, EntityRecord)> = index
        .candidates_near(cursor, r)
        .into_iter()
        .filter_map(|id| doc.entity(id).map(|(rec, _)| (id, rec)))
        .filter(|(_, rec)| snap_visible(doc, rec))
        .collect();
    let cands: Vec<(EntityId, &EntityRecord)> = owned.iter().map(|(id, rec)| (*id, rec)).collect();

    let mut hits: Vec<SnapHit> = Vec::new();

    // Geometry-declared snaps.
    for (id, rec) in &cands {
        for sp in rec.geometry.snap_points() {
            if opts.kinds.contains(sp.kind) {
                push_near(&mut hits, sp.point, sp.kind, *id, cursor, r);
            }
        }
    }

    // Calculated snaps.
    push_calculated(&mut hits, &cands, cursor, r, opts);

    hits.sort_by(|a, b| rank(a, b, opts.px_per_unit));
    hits
}

/// Returns whether an entity is visible and its layer is neither off nor frozen.
/// Unknown layers are treated as visible to preserve recoverable content.
fn snap_visible(doc: &Document, rec: &EntityRecord) -> bool {
    if !rec.visible {
        return false;
    }
    match doc.layer(rec.layer) {
        Some(layer) => !(layer.is_off() || layer.is_frozen()),
        None => true,
    }
}

// ===========================================================================
// Calculated snaps
// ===========================================================================

/// Affine-parameter tolerance for accepting a point on a segment.
const EPS_T: f64 = 1e-9;

/// Returns whether `t` lies within the segment, allowing tolerance.
#[inline]
fn on_seg(t: f64) -> bool {
    (-EPS_T..=1.0 + EPS_T).contains(&t)
}

/// Returns whether `theta` lies in the arc's counterclockwise sweep.
#[inline]
fn in_sweep(theta: f64, arc: &ArcSeg) -> bool {
    angle_in_sweep(theta, arc.start_angle, arc.end_angle)
}

/// Bounded geometry primitive used by calculated snaps.
#[derive(Debug, Clone, Copy)]
enum Prim {
    /// Line segment `a → b`.
    Seg { a: Point2, b: Point2 },
    /// Full circle.
    Circle { c: Point2, r: f64 },
    /// Counterclockwise arc filtered by `in_sweep`.
    Arc(ArcSeg),
}

/// Returns bounded primitives for intersection and projection.
///
/// Points need none because their coordinate is already a declared node snap.
fn prims_of(geom: &EntityGeometry) -> Vec<Prim> {
    match geom {
        EntityGeometry::Line(g) => vec![Prim::Seg { a: g.p1, b: g.p2 }],
        EntityGeometry::Circle(g) => vec![Prim::Circle {
            c: g.center,
            r: g.radius,
        }],
        EntityGeometry::Arc(g) => vec![Prim::Arc(g.arc_seg())],
        // ponytail: ellipses currently contribute declared snaps only; add an
        // elliptical primitive when full intersection and projection math exists.
        EntityGeometry::Ellipse(_) => Vec::new(),
        EntityGeometry::Polyline(g) => g
            .segments()
            .map(|s| match s {
                SegKind::Line { a, b } => Prim::Seg { a, b },
                SegKind::Arc(arc) => Prim::Arc(arc),
            })
            .collect(),
        // Approximate splines with a fine polyline for calculated snaps.
        EntityGeometry::Spline(g) => spline_prims(g),
        EntityGeometry::Point(_) => Vec::new(),
        // Materialize infinite geometry as a large segment for projection and intersection.
        EntityGeometry::Xline(g) => {
            let (a, b) = g.endpoints();
            vec![Prim::Seg { a, b }]
        }
        EntityGeometry::Ray(g) => {
            let (a, b) = g.endpoints();
            vec![Prim::Seg { a, b }]
        }
        // Wipeouts contribute their closed polygon edges as segments.
        EntityGeometry::Wipeout(g) => wipeout_prims(g),
    }
}

/// Flattens a spline into segments with chord tolerance scaled to curve size.
fn spline_prims(g: &SplineGeo) -> Vec<Prim> {
    let points = match g.fit_spline() {
        Some(sp) => {
            let bb = g.bbox();
            let span = bb.width().max(bb.height());
            let chord = (span * 1.0e-3).max(1.0e-9);
            sp.flatten(chord)
        }
        None => g.fit_points.clone(),
    };
    points
        .windows(2)
        .map(|w| Prim::Seg { a: w[0], b: w[1] })
        .collect()
}

/// Returns wipeout polygon edges, including the closing segment.
fn wipeout_prims(g: &WipeoutGeo) -> Vec<Prim> {
    let n = g.points.len();
    if n < 2 {
        return Vec::new();
    }
    (0..n)
        .map(|i| Prim::Seg {
            a: g.points[i],
            b: g.points[(i + 1) % n],
        })
        .collect()
}

/// Appends intersections that lie on both bounded primitives.
fn cross_points(p: &Prim, q: &Prim, out: &mut Vec<Point2>) {
    match (p, q) {
        (Prim::Seg { a: a1, b: b1 }, Prim::Seg { a: a2, b: b2 }) => {
            if let LineX::Point(h) = line_line(*a1, *b1, *a2, *b2)
                && on_seg(h.t1)
                && on_seg(h.t2)
            {
                out.push(h.point);
            }
        }
        (Prim::Seg { a, b }, Prim::Circle { c, r })
        | (Prim::Circle { c, r }, Prim::Seg { a, b }) => {
            for h in line_circle(*a, *b, *c, *r) {
                if on_seg(h.t1) {
                    out.push(h.point);
                }
            }
        }
        (Prim::Seg { a, b }, Prim::Arc(arc)) | (Prim::Arc(arc), Prim::Seg { a, b }) => {
            for h in line_arc(*a, *b, arc) {
                if on_seg(h.t1) && in_sweep(h.t2, arc) {
                    out.push(h.point);
                }
            }
        }
        (Prim::Circle { c: c1, r: r1 }, Prim::Circle { c: c2, r: r2 }) => {
            for h in circle_circle(*c1, *r1, *c2, *r2) {
                out.push(h.point);
            }
        }
        (Prim::Circle { c, r }, Prim::Arc(arc)) | (Prim::Arc(arc), Prim::Circle { c, r }) => {
            // For circle-arc intersections, `t2` is the arc angle.
            for h in circle_arc(*c, *r, arc) {
                if in_sweep(h.t2, arc) {
                    out.push(h.point);
                }
            }
        }
        (Prim::Arc(a1), Prim::Arc(a2)) => {
            for h in arc_arc(a1, a2) {
                if in_sweep(h.t1, a1) && in_sweep(h.t2, a2) {
                    out.push(h.point);
                }
            }
        }
    }
}

/// Returns the closest point on a primitive, or `None` for a circle centered on the cursor.
fn nearest_on_prim(prim: &Prim, cursor: Point2) -> Option<Point2> {
    match prim {
        Prim::Seg { a, b } => Some(nearest_on_segment(cursor, *a, *b)),
        Prim::Circle { c, r } => project_on_circle(cursor, *c, *r),
        Prim::Arc(arc) => Some(nearest_on_arc(cursor, arc)),
    }
}

/// Returns perpendicular feet from `lp`, constrained to bounded primitives.
fn perp_on_prim(prim: &Prim, lp: Point2) -> Vec<Point2> {
    match prim {
        Prim::Seg { a, b } => match perp_foot_line(lp, *a, *b) {
            Some((foot, t)) if on_seg(t) => vec![foot],
            _ => Vec::new(),
        },
        Prim::Circle { c, r } => project_on_circle(lp, *c, *r).into_iter().collect(),
        Prim::Arc(arc) => match project_on_circle(lp, arc.center, arc.radius) {
            Some(f) if in_sweep(angle_of(f - arc.center), arc) => vec![f],
            _ => Vec::new(),
        },
    }
}

/// Returns tangent points from `lp` to circles and arcs.
fn tan_on_prim(prim: &Prim, lp: Point2) -> Vec<Point2> {
    match prim {
        Prim::Seg { .. } => Vec::new(),
        Prim::Circle { c, r } => tangent_points(lp, *c, *r),
        Prim::Arc(arc) => tangent_points(lp, arc.center, arc.radius)
            .into_iter()
            .filter(|p| in_sweep(angle_of(*p - arc.center), arc))
            .collect(),
    }
}

/// Returns a cursor-aligned point on the extension beyond a bounded segment or arc.
fn ext_on_prim(prim: &Prim, cursor: Point2) -> Vec<Point2> {
    match prim {
        Prim::Seg { a, b } => match perp_foot_line(cursor, *a, *b) {
            Some((foot, t)) if !on_seg(t) => vec![foot],
            _ => Vec::new(),
        },
        Prim::Circle { .. } => Vec::new(),
        Prim::Arc(arc) => match project_on_circle(cursor, arc.center, arc.radius) {
            Some(f) if !in_sweep(angle_of(f - arc.center), arc) => vec![f],
            _ => Vec::new(),
        },
    }
}

/// Returns the vertex-polygon centroid for a closed polyline.
/// ponytail: bulge arcs are ignored until an arc-aware centroid is needed.
fn poly_centroid_of(geom: &EntityGeometry) -> Option<Point2> {
    match geom {
        EntityGeometry::Polyline(g) if g.is_closed_effective() => {
            let verts: Vec<Point2> = g.vertices.iter().map(|v| v.pt).collect();
            polygon_centroid(&verts)
        }
        _ => None,
    }
}

/// Produces calculated snaps for visible candidates within mask and aperture.
///
/// ponytail: intersection is O(k²); add spatial bucketing only if profiling requires it.
fn push_calculated(
    hits: &mut Vec<SnapHit>,
    cands: &[(EntityId, &EntityRecord)],
    cursor: Point2,
    r: f64,
    opts: SnapOpts,
) {
    let k = opts.kinds;
    let want_int = k.contains(SnapKind::Intersection);
    let want_nea = k.contains(SnapKind::Nearest);
    let want_ext = k.contains(SnapKind::Extension);
    let want_gce = k.contains(SnapKind::GeometricCenter);
    let want_per = k.contains(SnapKind::Perpendicular) && opts.last_point.is_some();
    let want_tan = k.contains(SnapKind::Tangent) && opts.last_point.is_some();

    if !(want_int || want_nea || want_ext || want_gce || want_per || want_tan) {
        return;
    }

    // Resolve primitives once per candidate.
    let prims: Vec<Vec<Prim>> = cands
        .iter()
        .map(|(_, rec)| prims_of(&rec.geometry))
        .collect();

    // Intersect every candidate pair.
    if want_int {
        let mut pts: Vec<Point2> = Vec::new();
        for i in 0..cands.len() {
            for j in (i + 1)..cands.len() {
                let (ia, ib) = (cands[i].0, cands[j].0);
                let ent = if ia.raw().0 <= ib.raw().0 { ia } else { ib };
                for pi in &prims[i] {
                    for pj in &prims[j] {
                        pts.clear();
                        cross_points(pi, pj, &mut pts);
                        for &pt in &pts {
                            push_near(hits, pt, SnapKind::Intersection, ent, cursor, r);
                        }
                    }
                }
            }
        }
    }

    // Per-entity nearest, perpendicular, tangent, extension, and center snaps.
    for (idx, (id, rec)) in cands.iter().enumerate() {
        for prim in &prims[idx] {
            if want_nea && let Some(p) = nearest_on_prim(prim, cursor) {
                push_near(hits, p, SnapKind::Nearest, *id, cursor, r);
            }
            if want_ext {
                for p in ext_on_prim(prim, cursor) {
                    push_near(hits, p, SnapKind::Extension, *id, cursor, r);
                }
            }
            if let Some(lp) = opts.last_point {
                if want_per {
                    for p in perp_on_prim(prim, lp) {
                        push_near(hits, p, SnapKind::Perpendicular, *id, cursor, r);
                    }
                }
                if want_tan {
                    for p in tan_on_prim(prim, lp) {
                        push_near(hits, p, SnapKind::Tangent, *id, cursor, r);
                    }
                }
            }
        }
        if want_gce && let Some(g) = poly_centroid_of(&rec.geometry) {
            push_near(hits, g, SnapKind::GeometricCenter, *id, cursor, r);
        }
    }
}
