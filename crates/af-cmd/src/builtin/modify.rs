//! Headless geometric-modification commands: TRIM (`TR`), EXTEND (`EX`), FILLET
//! (`F`), and OFFSET (`O`). Successful execution commits exactly one transaction.
//!
//! # Geometry ownership
//!
//! TRIM and EXTEND consume curve parameters from [`af_geom::intersect`], FILLET
//! uses the tangent kernel, and OFFSET delegates to [`af_geom::offset`]. This layer
//! chooses surviving segments and applies the transaction.
//!
//! # Quick mode
//!
//! An empty edge set uses every other model-space entity as a boundary. An explicit
//! set uses only its members.
//!
//! # Supported geometry
//!
//! - TRIM and EXTEND support lines, circles, arcs, and polylines. TRIM rebuilds
//!   polyline vertices and bulges; EXTEND changes only an open endpoint segment.
//! - FILLET supports line, arc, and circle pairs. Candidate tangent centers must
//!   lie on bounded entity ranges and are ranked by proximity to pick points.
//!   Complete circles are not trimmed. Omitting radius requests an exact corner.

use core::f64::consts::{PI, TAU};

use af_geom::bulge::{ArcSeg, bulge_to_arc, seg_angle_fraction, seg_point_at, split_bulge_segment};
use af_geom::intersect::{
    LineX, SegGeom, arc_arc, circle_arc, circle_circle, line_arc, line_circle, line_line,
    resolve_poly_seg,
};
use af_geom::offset::{OffsetError, offset_arc, offset_circle, offset_line, offset_polyline};
use af_geom::tangent::{TangentCurve, tangent_circle_centers, tangent_point_on};
use af_math::angle::{angle_in_sweep, angle_of, normalize_0_2pi, sweep_ccw};
use af_math::{Point2, Tol, Vec2};
use af_model::container::ContainerRef;
use af_model::entity::{
    ArcGeo, CircleGeo, EntityGeometry, EntityRecord, LineGeo, PolyVertex, PolylineGeo, SegKind,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::{Document, TxContext};

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Tolerance for treating line parameters and angular offsets as endpoint-coincident.
const PEPS: f64 = 1e-9;

// ============================================================================
// Registration
// ============================================================================

/// Registers TRIM, EXTEND, FILLET, and OFFSET.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(trim_spec())?;
    registry.register(extend_spec())?;
    registry.register(fillet_spec())?;
    registry.register(offset_spec())?;
    Ok(())
}

/// Returns the TRIM specification with alias `TR`.
#[must_use]
pub fn trim_spec() -> CommandSpec {
    CommandSpec::new("TRIM", "Trim", true, trim_exec)
        .preview(trim_preview)
        .alias("TR")
        .param(ParamSpec::with_default(
            "edges",
            ParamType::EntitySet,
            empty_set(),
        ))
        .param(ParamSpec::required("target", ParamType::EntitySet))
        .param(ParamSpec::required("pick", ParamType::Point))
}

/// Returns the EXTEND specification with alias `EX`.
#[must_use]
pub fn extend_spec() -> CommandSpec {
    CommandSpec::new("EXTEND", "Extend", true, extend_exec)
        .preview(extend_preview)
        .alias("EX")
        .param(ParamSpec::with_default(
            "edges",
            ParamType::EntitySet,
            empty_set(),
        ))
        .param(ParamSpec::required("target", ParamType::EntitySet))
        .param(ParamSpec::required("pick", ParamType::Point))
}

/// Returns the FILLET specification with alias `F`.
#[must_use]
pub fn fillet_spec() -> CommandSpec {
    CommandSpec::new("FILLET", "Fillet", true, fillet_exec)
        .preview(fillet_preview)
        .alias("F")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        // Omitting the positive radius requests an exact corner.
        .param(ParamSpec::optional("radius", ParamType::Distance))
        // Optional picks choose the fillet side; midpoint defaults are used otherwise.
        .param(ParamSpec::optional("pick0", ParamType::Point))
        .param(ParamSpec::optional("pick1", ParamType::Point))
}

/// Returns the OFFSET specification with alias `O`.
#[must_use]
pub fn offset_spec() -> CommandSpec {
    CommandSpec::new("OFFSET", "Offset", true, offset_exec)
        .preview(offset_preview)
        .alias("O")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("distance", ParamType::Distance))
        .param(ParamSpec::required("side", ParamType::Point))
}

/// Returns an empty entity set for quick mode.
fn empty_set() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

// ============================================================================
// Shared execution and preview plan
// ============================================================================

/// A read-only modification plan shared by execution and preview so previewed
/// geometry exactly matches committed geometry.
pub(crate) struct ModifyPlan {
    /// Existing entities and their replacement geometry.
    pub(crate) modify: Vec<(EntityId, EntityGeometry)>,
    /// Source records for inherited properties and new geometry.
    pub(crate) add: Vec<(EntityRecord, EntityGeometry)>,
}

impl ModifyPlan {
    /// Consumes the plan and returns all resulting geometry for preview.
    fn result_geoms(self) -> Vec<EntityGeometry> {
        self.modify
            .into_iter()
            .map(|(_, g)| g)
            .chain(self.add.into_iter().map(|(_, g)| g))
            .collect()
    }

    /// Consumes and applies the plan, returning created IDs in order.
    pub(crate) fn apply(self, tx: &mut TxContext<'_>) -> Result<Vec<EntityId>, CmdError> {
        for (id, geom) in self.modify {
            tx.modify_entity(id, move |r| r.geometry = geom)?;
        }
        let mut created = Vec::with_capacity(self.add.len());
        for (src, geom) in self.add {
            created.push(tx.add_entity(ContainerRef::ModelSpace, record_like(&src, geom))?);
        }
        Ok(created)
    }
}

// ============================================================================
// TRIM
// ============================================================================

/// Plans replacement geometry and an optional second segment for TRIM.
struct TrimPlan {
    new_geom: EntityGeometry,
    extra: Option<EntityGeometry>,
}

fn trim_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let plan = trim_plan(ctx.document(), &args)?;
    let created = ctx.transact("Trim", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::created(created))
}

/// Returns TRIM result geometry without a transaction.
fn trim_preview(doc: &Document, args: ParsedArgs) -> Result<Vec<EntityGeometry>, CmdError> {
    Ok(trim_plan(doc, &args)?.result_geoms())
}

/// Computes surviving TRIM segments without mutation.
fn trim_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let target = single_target(args, "target")?;
    let pick = args
        .point("pick")
        .ok_or_else(|| CmdError::MissingParam("pick".to_string()))?;

    let (src, container) = doc.entity(target).ok_or(CmdError::UnknownEntity(target))?;
    ensure_editable(doc, container, src.layer, "TRIM")?;
    let target_geom = src.geometry.clone();
    let src = src.clone();

    let curves = gather_curves(doc, args.entity_set("edges").unwrap_or(&[]), target, "TRIM")?;

    let plan = match &target_geom {
        EntityGeometry::Line(l) => trim_line(*l, &curves, pick)?,
        EntityGeometry::Circle(c) => trim_circle(*c, &curves, pick)?,
        EntityGeometry::Arc(a) => trim_arc(*a, &curves, pick)?,
        EntityGeometry::Polyline(p) => trim_polyline(p, &curves, pick)?,
        EntityGeometry::Ellipse(_) => {
            return Err(CmdError::Failed(
                "TRIM: trimming an ellipse target is deferred".to_string(),
            ));
        }
        EntityGeometry::Point(_) => {
            return Err(CmdError::Failed("TRIM: cannot trim a point".to_string()));
        }
        EntityGeometry::Xline(_) | EntityGeometry::Ray(_) => {
            return Err(CmdError::Failed(
                "TRIM: trimming an infinite xline/ray target is deferred".to_string(),
            ));
        }
        EntityGeometry::Spline(_) => {
            return Err(CmdError::Failed(
                "TRIM: trimming a spline target is deferred".to_string(),
            ));
        }
        EntityGeometry::Wipeout(_) => {
            return Err(CmdError::Failed(
                "TRIM: trimming a wipeout target is deferred".to_string(),
            ));
        }
    };

    let mut add = Vec::new();
    if let Some(extra) = plan.extra {
        add.push((src, extra));
    }
    Ok(ModifyPlan {
        modify: vec![(target, plan.new_geom)],
        add,
    })
}

/// Removes the line interval between cuts that contains `pick`.
fn trim_line(line: LineGeo, curves: &[Curve], pick: Point2) -> Result<TrimPlan, CmdError> {
    let mut cuts = Vec::new();
    for c in curves {
        cross_line_curve(line.p1, line.p2, c, &mut cuts);
    }
    cuts.retain(|&t| t > PEPS && t < 1.0 - PEPS);
    if cuts.is_empty() {
        return Err(no_cross("TRIM"));
    }
    sort_dedup(&mut cuts);

    let tp = param_on_line(line.p1, line.p2, pick).clamp(0.0, 1.0);
    let lo = cuts
        .iter()
        .copied()
        .filter(|&c| c < tp)
        .fold(0.0_f64, f64::max);
    let hi = cuts
        .iter()
        .copied()
        .filter(|&c| c > tp)
        .fold(1.0_f64, f64::min);

    let mut pieces = Vec::new();
    if lo > PEPS {
        pieces.push((0.0, lo));
    }
    if hi < 1.0 - PEPS {
        pieces.push((hi, 1.0));
    }
    if pieces.is_empty() {
        return Err(no_cross("TRIM"));
    }
    let mk = |t0: f64, t1: f64| LineGeo::new(line.point_at(t0), line.point_at(t1));
    Ok(TrimPlan {
        new_geom: EntityGeometry::Line(mk(pieces[0].0, pieces[0].1)),
        extra: pieces
            .get(1)
            .map(|&(t0, t1)| EntityGeometry::Line(mk(t0, t1))),
    })
}

/// Removes the picked arc between circle cuts and returns the complementary arc.
fn trim_circle(circle: CircleGeo, curves: &[Curve], pick: Point2) -> Result<TrimPlan, CmdError> {
    let mut angs = Vec::new();
    for c in curves {
        cross_circle_curve(circle.center, circle.radius, c, &mut angs);
    }
    sort_dedup_angles(&mut angs);
    if angs.len() < 2 {
        return Err(CmdError::Failed(
            "TRIM: a circle needs at least two crossing points to trim".to_string(),
        ));
    }
    let phi = normalize_0_2pi(angle_of(pick - circle.center));
    let (lo, hi) = cyclic_gap(&angs, phi);
    // Keep the counterclockwise complement from `hi` to `lo`.
    Ok(TrimPlan {
        new_geom: EntityGeometry::Arc(ArcGeo::new(circle.center, circle.radius, hi, lo)),
        extra: None,
    })
}

/// Removes the arc interval between cuts that contains `pick`.
fn trim_arc(arc: ArcGeo, curves: &[Curve], pick: Point2) -> Result<TrimPlan, CmdError> {
    let sweep = arc.arc_seg().sweep();
    let mut offs = Vec::new();
    for c in curves {
        let mut angs = Vec::new();
        cross_circle_curve(arc.center, arc.radius, c, &mut angs);
        for a in angs {
            let off = normalize_0_2pi(a - arc.start_angle);
            if off > PEPS && off < sweep - PEPS {
                offs.push(off);
            }
        }
    }
    if offs.is_empty() {
        return Err(no_cross("TRIM"));
    }
    sort_dedup(&mut offs);

    let poff = normalize_0_2pi(angle_of(pick - arc.center) - arc.start_angle).clamp(0.0, sweep);
    let lo = offs
        .iter()
        .copied()
        .filter(|&c| c < poff)
        .fold(0.0, f64::max);
    let hi = offs
        .iter()
        .copied()
        .filter(|&c| c > poff)
        .fold(sweep, f64::min);

    let mut pieces = Vec::new();
    if lo > PEPS {
        pieces.push((0.0, lo));
    }
    if hi < sweep - PEPS {
        pieces.push((hi, sweep));
    }
    if pieces.is_empty() {
        return Err(no_cross("TRIM"));
    }
    let mk = |o0: f64, o1: f64| {
        ArcGeo::new(
            arc.center,
            arc.radius,
            normalize_0_2pi(arc.start_angle + o0),
            normalize_0_2pi(arc.start_angle + o1),
        )
    };
    Ok(TrimPlan {
        new_geom: EntityGeometry::Arc(mk(pieces[0].0, pieces[0].1)),
        extra: pieces
            .get(1)
            .map(|&(o0, o1)| EntityGeometry::Arc(mk(o0, o1))),
    })
}

// ============================================================================
// Polyline TRIM
// ============================================================================

/// Returns endpoints and bulge for polyline segment `i`.
fn seg_ab_bulge(poly: &PolylineGeo, i: usize) -> (Point2, Point2, f64) {
    let n = poly.vertices.len();
    (
        poly.vertices[i].pt,
        poly.vertices[(i + 1) % n].pt,
        poly.vertices[i].bulge,
    )
}

/// Returns segment count: `N` when closed, otherwise `N-1`.
fn poly_seg_count(poly: &PolylineGeo) -> usize {
    let n = poly.vertices.len();
    if poly.closed { n } else { n - 1 }
}

/// Removes the picked polyline interval and rebuilds vertices and bulges. Open
/// polylines yield one or two pieces; closed polylines become open.
fn trim_polyline(poly: &PolylineGeo, curves: &[Curve], pick: Point2) -> Result<TrimPlan, CmdError> {
    let n = poly.vertices.len();
    if n < 2 {
        return Err(CmdError::Failed(
            "TRIM: the polyline has too few vertices".to_string(),
        ));
    }
    let count = poly_seg_count(poly);

    // Global cut parameter equals segment index plus local traversal fraction.
    let mut cuts: Vec<f64> = Vec::new();
    for i in 0..count {
        let (a, b, bulge) = seg_ab_bulge(poly, i);
        match resolve_poly_seg(a, b, bulge) {
            SegGeom::Straight { .. } => {
                let mut ts = Vec::new();
                for c in curves {
                    cross_line_curve(a, b, c, &mut ts);
                }
                for t in ts {
                    if t > PEPS && t < 1.0 - PEPS {
                        cuts.push(i as f64 + t);
                    }
                }
            }
            SegGeom::Arc(arc) => {
                let mut angs = Vec::new();
                for c in curves {
                    cross_circle_curve(arc.center, arc.radius, c, &mut angs);
                }
                for theta in angs {
                    if !angle_in_sweep(theta, arc.start_angle, arc.end_angle) {
                        continue;
                    }
                    if let Some(f) = seg_angle_fraction(a, b, bulge, theta)
                        && f > PEPS
                        && f < 1.0 - PEPS
                    {
                        cuts.push(i as f64 + f);
                    }
                }
            }
        }
    }
    if cuts.is_empty() {
        return Err(no_cross("TRIM"));
    }
    sort_dedup(&mut cuts);

    let g_pick = pick_global(poly, count, pick);

    let pieces = if poly.closed {
        trim_poly_closed(poly, count, &cuts, g_pick)?
    } else {
        trim_poly_open(poly, count, &cuts, g_pick)?
    };

    let mut it = pieces.into_iter();
    let first = it.next().ok_or_else(|| no_cross("TRIM"))?;
    Ok(TrimPlan {
        new_geom: EntityGeometry::Polyline(first),
        extra: it.next().map(EntityGeometry::Polyline),
    })
}

/// Returns the pick's global polyline parameter from its nearest segment projection.
fn pick_global(poly: &PolylineGeo, count: usize, pick: Point2) -> f64 {
    let mut best_d = f64::INFINITY;
    let mut best_g = 0.0;
    for i in 0..count {
        let (a, b, bulge) = seg_ab_bulge(poly, i);
        let (d, f) = match bulge_to_arc(a, b, bulge) {
            Ok(arc) => {
                let theta = angle_of(pick - arc.center);
                let f = seg_angle_fraction(a, b, bulge, theta)
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                (arc.distance_to(pick), f)
            }
            Err(_) => (
                dist_point_segment(pick, a, b),
                param_on_line(a, b, pick).clamp(0.0, 1.0),
            ),
        };
        if d < best_d {
            best_d = d;
            best_g = i as f64 + f;
        }
    }
    best_g
}

/// Removes `(lo, hi)` from an open polyline and returns surviving endpoint pieces.
fn trim_poly_open(
    poly: &PolylineGeo,
    count: usize,
    cuts: &[f64],
    g_pick: f64,
) -> Result<Vec<PolylineGeo>, CmdError> {
    let lo = cuts
        .iter()
        .copied()
        .filter(|&c| c < g_pick - PEPS)
        .fold(f64::NEG_INFINITY, f64::max);
    let hi = cuts
        .iter()
        .copied()
        .filter(|&c| c > g_pick + PEPS)
        .fold(f64::INFINITY, f64::min);

    let mut pieces = Vec::new();
    if lo.is_finite() {
        pieces.push(reconstruct_poly(poly, count, 0.0, lo));
    }
    if hi.is_finite() {
        pieces.push(reconstruct_poly(poly, count, hi, count as f64));
    }
    if pieces.is_empty() {
        return Err(no_cross("TRIM"));
    }
    Ok(pieces)
}

/// Removes the picked interval from a closed polyline and returns one open remainder.
fn trim_poly_closed(
    poly: &PolylineGeo,
    count: usize,
    cuts: &[f64],
    g_pick: f64,
) -> Result<Vec<PolylineGeo>, CmdError> {
    let k = cuts.len();
    if k < 2 {
        return Err(CmdError::Failed(
            "TRIM: a closed polyline needs at least two crossing points to trim".to_string(),
        ));
    }
    let span = count as f64;
    // Find the cyclic cut interval containing the pick.
    let mut lo = cuts[k - 1];
    let mut hi = cuts[0];
    for w in 0..k {
        let c0 = cuts[w];
        let c1 = if w + 1 < k {
            cuts[w + 1]
        } else {
            cuts[0] + span
        };
        let p = if g_pick >= c0 { g_pick } else { g_pick + span };
        if p > c0 + PEPS && p < c1 - PEPS {
            lo = c0;
            hi = if w + 1 < k { cuts[w + 1] } else { cuts[0] };
            break;
        }
    }
    // Keep the wraparound path from `hi` to `lo` as one open piece.
    let end = if lo > hi { lo } else { lo + span };
    Ok(vec![reconstruct_poly(poly, count, hi, end)])
}

/// Rebuilds an open sub-polyline over global interval `[g_start, g_end]`, splitting
/// segments at fractional cuts and recalculating sub-arc bulges.
fn reconstruct_poly(poly: &PolylineGeo, count: usize, g_start: f64, g_end: f64) -> PolylineGeo {
    // Break at the endpoints and every strictly internal integer parameter.
    let mut bps = vec![g_start];
    let mut x = g_start.floor() + 1.0;
    while x < g_end - PEPS {
        bps.push(x);
        x += 1.0;
    }
    bps.push(g_end);

    let mut verts: Vec<PolyVertex> = Vec::with_capacity(bps.len());
    for w in 0..bps.len() - 1 {
        let g0 = bps[w];
        let g1 = bps[w + 1];
        let j = g0.floor() as i64;
        let jm = j.rem_euclid(count as i64) as usize;
        let (a, b, bulge) = seg_ab_bulge(poly, jm);
        let f0 = g0 - j as f64;
        let f1 = g1 - j as f64;
        let (_, _, sub_bulge) = split_bulge_segment(a, b, bulge, f0, f1);
        verts.push(PolyVertex::new(point_at_global(poly, count, g0), sub_bulge));
    }
    verts.push(PolyVertex::new(point_at_global(poly, count, g_end), 0.0));
    PolylineGeo::new(verts, false)
}

/// Returns the polyline point at global parameter `g`, preserving exact vertices
/// at integer parameters.
fn point_at_global(poly: &PolylineGeo, count: usize, g: f64) -> Point2 {
    let n = poly.vertices.len();
    let gr = g.round();
    if (g - gr).abs() < PEPS {
        let m = gr as i64;
        let idx = if poly.closed {
            m.rem_euclid(n as i64) as usize
        } else {
            (m.clamp(0, (n - 1) as i64)) as usize
        };
        poly.vertices[idx].pt
    } else {
        let j = g.floor() as i64;
        let f = g - j as f64;
        let jm = j.rem_euclid(count as i64) as usize;
        let (a, b, bulge) = seg_ab_bulge(poly, jm);
        seg_point_at(a, b, bulge, f)
    }
}

// ============================================================================
// EXTEND
// ============================================================================

fn extend_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let plan = extend_plan(ctx.document(), &args)?;
    ctx.transact("Extend", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::new())
}

/// Returns EXTEND result geometry without a transaction.
fn extend_preview(doc: &Document, args: ParsedArgs) -> Result<Vec<EntityGeometry>, CmdError> {
    Ok(extend_plan(doc, &args)?.result_geoms())
}

/// Computes EXTEND geometry without mutation.
fn extend_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let target = single_target(args, "target")?;
    let pick = args
        .point("pick")
        .ok_or_else(|| CmdError::MissingParam("pick".to_string()))?;

    let (src, container) = doc.entity(target).ok_or(CmdError::UnknownEntity(target))?;
    ensure_editable(doc, container, src.layer, "EXTEND")?;
    let target_geom = src.geometry.clone();

    let curves = gather_curves(
        doc,
        args.entity_set("edges").unwrap_or(&[]),
        target,
        "EXTEND",
    )?;

    let new_geom = match &target_geom {
        EntityGeometry::Line(l) => EntityGeometry::Line(extend_line(*l, &curves, pick)?),
        EntityGeometry::Arc(a) => EntityGeometry::Arc(extend_arc(*a, &curves, pick)?),
        EntityGeometry::Circle(_) => {
            return Err(CmdError::Failed(
                "EXTEND: a full circle has no open end to extend".to_string(),
            ));
        }
        EntityGeometry::Polyline(p) => EntityGeometry::Polyline(extend_polyline(p, &curves, pick)?),
        EntityGeometry::Ellipse(_) => {
            return Err(CmdError::Failed(
                "EXTEND: extending an ellipse target is deferred".to_string(),
            ));
        }
        EntityGeometry::Spline(_) => {
            return Err(CmdError::Failed(
                "EXTEND: extending a spline target is deferred".to_string(),
            ));
        }
        EntityGeometry::Point(_) => {
            return Err(CmdError::Failed(
                "EXTEND: cannot extend a point".to_string(),
            ));
        }
        EntityGeometry::Xline(_) | EntityGeometry::Ray(_) => {
            return Err(CmdError::Failed(
                "EXTEND: an infinite xline/ray has no open end to extend".to_string(),
            ));
        }
        EntityGeometry::Wipeout(_) => {
            return Err(CmdError::Failed(
                "EXTEND: a closed wipeout has no open end to extend".to_string(),
            ));
        }
    };

    Ok(ModifyPlan {
        modify: vec![(target, new_geom)],
        add: Vec::new(),
    })
}

/// Extends the line endpoint nearest `pick` to the first boundary in that direction.
fn extend_line(line: LineGeo, curves: &[Curve], pick: Point2) -> Result<LineGeo, CmdError> {
    let mut cuts = Vec::new();
    for c in curves {
        cross_line_curve(line.p1, line.p2, c, &mut cuts);
    }
    let extend_p2 = pick.dist(line.p2) <= pick.dist(line.p1);
    if extend_p2 {
        let t = cuts
            .iter()
            .copied()
            .filter(|&t| t > 1.0 + PEPS)
            .fold(f64::INFINITY, f64::min);
        if !t.is_finite() {
            return Err(no_boundary("EXTEND"));
        }
        Ok(LineGeo::new(line.p1, line.point_at(t)))
    } else {
        let t = cuts
            .iter()
            .copied()
            .filter(|&t| t < -PEPS)
            .fold(f64::NEG_INFINITY, f64::max);
        if !t.is_finite() {
            return Err(no_boundary("EXTEND"));
        }
        Ok(LineGeo::new(line.point_at(t), line.p2))
    }
}

/// Extends the arc endpoint nearest `pick` to the first boundary outside its sweep.
fn extend_arc(arc: ArcGeo, curves: &[Curve], pick: Point2) -> Result<ArcGeo, CmdError> {
    let seg = arc.arc_seg();
    let complement = TAU - seg.sweep();
    let mut angs = Vec::new();
    for c in curves {
        cross_circle_curve(arc.center, arc.radius, c, &mut angs);
    }
    let extend_end = pick.dist(seg.end_point()) <= pick.dist(seg.start_point());
    let mut best = f64::INFINITY;
    for a in angs {
        let off = if extend_end {
            normalize_0_2pi(a - arc.end_angle)
        } else {
            normalize_0_2pi(arc.start_angle - a)
        };
        if off > PEPS && off < complement - PEPS && off < best {
            best = off;
        }
    }
    if !best.is_finite() {
        return Err(no_boundary("EXTEND"));
    }
    Ok(if extend_end {
        ArcGeo::new(
            arc.center,
            arc.radius,
            arc.start_angle,
            normalize_0_2pi(arc.end_angle + best),
        )
    } else {
        ArcGeo::new(
            arc.center,
            arc.radius,
            normalize_0_2pi(arc.start_angle - best),
            arc.end_angle,
        )
    })
}

// ============================================================================
// Polyline EXTEND
// ============================================================================

/// Extends an open polyline's endpoint segment to the first boundary.
///
/// The nearest first or last segment selects which endpoint moves. Interior
/// segments and closed polylines are rejected. Arc segments update their bulge.
fn extend_polyline(
    poly: &PolylineGeo,
    curves: &[Curve],
    pick: Point2,
) -> Result<PolylineGeo, CmdError> {
    let n = poly.vertices.len();
    if n < 2 {
        return Err(CmdError::Failed(
            "EXTEND: the polyline has too few vertices".to_string(),
        ));
    }
    if poly.closed {
        return Err(CmdError::Failed(
            "EXTEND: a closed polyline has no open end to extend".to_string(),
        ));
    }
    let count = n - 1; // Segment count for an open polyline.
    let seg = nearest_poly_segment(poly, count, pick);
    let extend_last = if count == 1 {
        // For one segment, choose the endpoint nearest the pick.
        pick.dist(poly.vertices[n - 1].pt) <= pick.dist(poly.vertices[0].pt)
    } else if seg == 0 {
        false
    } else if seg == count - 1 {
        true
    } else {
        return Err(CmdError::Failed(
            "EXTEND: only the first or last polyline segment can be extended".to_string(),
        ));
    };

    let mut verts = poly.vertices.clone();
    if extend_last {
        // Extend the final segment beyond `b`.
        let (a, b, bulge) = seg_ab_bulge(poly, count - 1);
        match resolve_poly_seg(a, b, bulge) {
            SegGeom::Straight { .. } => {
                verts[n - 1].pt = extend_seg_straight(a, b, curves, true)?;
            }
            SegGeom::Arc(arc) => {
                let (p, sub) = extend_seg_arc(&arc, bulge, curves, true)?;
                verts[n - 1].pt = p;
                verts[n - 2].bulge = sub;
            }
        }
    } else {
        // Extend the first segment before `a`.
        let (a, b, bulge) = seg_ab_bulge(poly, 0);
        match resolve_poly_seg(a, b, bulge) {
            SegGeom::Straight { .. } => {
                verts[0].pt = extend_seg_straight(a, b, curves, false)?;
            }
            SegGeom::Arc(arc) => {
                let (p, sub) = extend_seg_arc(&arc, bulge, curves, false)?;
                verts[0].pt = p;
                verts[0].bulge = sub;
            }
        }
    }
    Ok(PolylineGeo::new(verts, false))
}

/// Returns the segment index nearest `pick`.
fn nearest_poly_segment(poly: &PolylineGeo, count: usize, pick: Point2) -> usize {
    let mut best_d = f64::INFINITY;
    let mut best = 0;
    for i in 0..count {
        let (a, b, bulge) = seg_ab_bulge(poly, i);
        let d = match bulge_to_arc(a, b, bulge) {
            Ok(arc) => arc.distance_to(pick),
            Err(_) => dist_point_segment(pick, a, b),
        };
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

/// Returns a line segment's new endpoint at the first boundary beyond `b` or before `a`.
fn extend_seg_straight(
    a: Point2,
    b: Point2,
    curves: &[Curve],
    beyond_b: bool,
) -> Result<Point2, CmdError> {
    let mut cuts = Vec::new();
    for c in curves {
        cross_line_curve(a, b, c, &mut cuts);
    }
    let t = if beyond_b {
        cuts.iter()
            .copied()
            .filter(|&t| t > 1.0 + PEPS)
            .fold(f64::INFINITY, f64::min)
    } else {
        cuts.iter()
            .copied()
            .filter(|&t| t < -PEPS)
            .fold(f64::NEG_INFINITY, f64::max)
    };
    if !t.is_finite() {
        return Err(no_boundary("EXTEND"));
    }
    Ok(a + (b - a) * t)
}

/// Returns an arc segment's extended endpoint and bulge on its supporting circle,
/// preserving traversal direction and avoiding the original sweep.
fn extend_seg_arc(
    arc: &ArcSeg,
    bulge: f64,
    curves: &[Curve],
    beyond_b: bool,
) -> Result<(Point2, f64), CmdError> {
    let sweep = arc.sweep();
    let complement = TAU - sweep;
    let mut angs = Vec::new();
    for c in curves {
        cross_circle_curve(arc.center, arc.radius, c, &mut angs);
    }
    let mut best = f64::INFINITY;
    for theta in angs {
        let delta = arc_extend_delta(arc, bulge, beyond_b, theta);
        if delta > PEPS && delta < complement - PEPS && delta < best {
            best = delta;
        }
    }
    if !best.is_finite() {
        return Err(no_boundary("EXTEND"));
    }
    let new_bulge = ((sweep + best) * 0.25).tan().copysign(bulge);
    let new_angle = arc_extend_angle(arc, bulge, beyond_b, best);
    Ok((arc.point_at(new_angle), new_bulge))
}

/// Returns angular advance from the extended endpoint to `theta` in traversal direction.
fn arc_extend_delta(arc: &ArcSeg, bulge: f64, beyond_b: bool, theta: f64) -> f64 {
    // Continue from `b` in the bulge's traversal direction.
    match (beyond_b, bulge >= 0.0) {
        (true, true) => normalize_0_2pi(theta - arc.end_angle),
        (true, false) => normalize_0_2pi(arc.start_angle - theta),
        (false, true) => normalize_0_2pi(arc.start_angle - theta),
        (false, false) => normalize_0_2pi(theta - arc.end_angle),
    }
}

/// Returns the new vertex angle after extending by `delta` radians.
fn arc_extend_angle(arc: &ArcSeg, bulge: f64, beyond_b: bool, delta: f64) -> f64 {
    match (beyond_b, bulge >= 0.0) {
        (true, true) => normalize_0_2pi(arc.end_angle + delta),
        (true, false) => normalize_0_2pi(arc.start_angle - delta),
        (false, true) => normalize_0_2pi(arc.start_angle - delta),
        (false, false) => normalize_0_2pi(arc.end_angle + delta),
    }
}

// ============================================================================
// FILLET
// ============================================================================

fn fillet_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let plan = fillet_plan(ctx.document(), &args)?;
    let created = ctx.transact("Fillet", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::created(created))
}

/// Returns trimmed entities and the FILLET arc without a transaction.
fn fillet_preview(doc: &Document, args: ParsedArgs) -> Result<Vec<EntityGeometry>, CmdError> {
    Ok(fillet_plan(doc, &args)?.result_geoms())
}

/// Computes trimmed entities and the FILLET arc without mutation.
fn fillet_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    if ids.len() != 2 {
        return Err(CmdError::Failed(
            "FILLET requires exactly two entities".to_string(),
        ));
    }
    let (id0, id1) = (ids[0], ids[1]);
    let radius = args.distance("radius"); // None means an exact corner (R = 0).
    let pick0 = args.point("pick0");
    let pick1 = args.point("pick1");

    let (src0, c0) = doc.entity(id0).ok_or(CmdError::UnknownEntity(id0))?;
    let (src1, c1) = doc.entity(id1).ok_or(CmdError::UnknownEntity(id1))?;
    ensure_editable(doc, c0, src0.layer, "FILLET")?;
    ensure_editable(doc, c1, src1.layer, "FILLET")?;
    let k0 = fillet_kind(&src0.geometry)?;
    let k1 = fillet_kind(&src1.geometry)?;
    let src0 = src0.clone();

    // Line-line uses its dedicated bisector path; other pairs use tangent geometry.
    let (modify, arc) = match (&k0, &k1) {
        (FilletKind::Line(l0), FilletKind::Line(l1)) => {
            let (new_l0, new_l1, arc) = fillet_lines(*l0, *l1, radius)?;
            (
                vec![
                    (id0, EntityGeometry::Line(new_l0)),
                    (id1, EntityGeometry::Line(new_l1)),
                ],
                arc.map(EntityGeometry::Arc),
            )
        }
        _ => fillet_general(&k0, &k1, id0, id1, radius, pick0, pick1)?,
    };

    let mut add = Vec::new();
    if let Some(arc) = arc {
        add.push((src0, arc));
    }
    Ok(ModifyPlan { modify, add })
}

/// Builds a radius-`R` tangent fillet between two lines. No radius returns the exact corner.
#[allow(clippy::type_complexity)]
fn fillet_lines(
    l0: LineGeo,
    l1: LineGeo,
    radius: Option<f64>,
) -> Result<(LineGeo, LineGeo, Option<ArcGeo>), CmdError> {
    let p = match line_line(l0.p1, l0.p2, l1.p1, l1.p2) {
        LineX::Point(h) => h.point,
        LineX::Parallel | LineX::Collinear => {
            return Err(CmdError::Failed(
                "FILLET: the two lines are parallel and cannot meet".to_string(),
            ));
        }
    };

    let r = radius.unwrap_or(0.0);
    if r <= 0.0 {
        // Exact corner: move both lines to the intersection without an arc.
        return Ok((set_near(l0, p, p), set_near(l1, p, p), None));
    }

    let far0 = far_endpoint(l0, p);
    let far1 = far_endpoint(l1, p);
    let dir0 = (far0 - p)
        .normalize()
        .map_err(|_| CmdError::Failed("FILLET: a line is degenerate".to_string()))?;
    let dir1 = (far1 - p)
        .normalize()
        .map_err(|_| CmdError::Failed("FILLET: a line is degenerate".to_string()))?;
    let theta = dir0.dot(dir1).clamp(-1.0, 1.0).acos();
    if theta <= 1e-6 || theta >= PI - 1e-6 {
        return Err(CmdError::Failed(
            "FILLET: the two lines are (nearly) collinear".to_string(),
        ));
    }

    let half = theta * 0.5;
    let tlen = r / half.tan(); // Distance from the vertex to the tangent point.
    let t0 = p + dir0 * tlen;
    let t1 = p + dir1 * tlen;
    let bis = (dir0 + dir1)
        .normalize()
        .map_err(|_| CmdError::Failed("FILLET: cannot bisect the corner".to_string()))?;
    let center = p + bis * (r / half.sin());

    let sa = angle_of(t0 - center);
    let ea = angle_of(t1 - center);
    // Use the minor arc between tangent points.
    let (start, end) = if sweep_ccw(sa, ea) <= PI {
        (sa, ea)
    } else {
        (ea, sa)
    };
    Ok((
        set_near(l0, p, t0),
        set_near(l1, p, t1),
        Some(ArcGeo::new(center, r, start, end)),
    ))
}

/// Returns the endpoint of `l` farthest from corner `p`.
pub(crate) fn far_endpoint(l: LineGeo, p: Point2) -> Point2 {
    if l.p1.dist(p) >= l.p2.dist(p) {
        l.p1
    } else {
        l.p2
    }
}

/// Replaces the endpoint of `l` nearest `p`, preserving the far endpoint.
pub(crate) fn set_near(l: LineGeo, p: Point2, np: Point2) -> LineGeo {
    if l.p1.dist(p) >= l.p2.dist(p) {
        LineGeo::new(l.p1, np)
    } else {
        LineGeo::new(np, l.p2)
    }
}

// ============================================================================
// General FILLET for lines, arcs, and circles
// ============================================================================

/// A FILLET participant represented by its supporting curve and trim behavior.
enum FilletKind {
    /// A line trimmed by moving one endpoint to tangency.
    Line(LineGeo),
    /// An arc trimmed at a tangent point within its sweep.
    Arc(ArcGeo),
    /// A complete circle, which has no endpoint to trim.
    Circle(CircleGeo),
}

/// Classifies line, arc, and circle geometry for FILLET.
///
/// # Errors
/// Returns [`CmdError::Failed`] for unsupported geometry.
fn fillet_kind(g: &EntityGeometry) -> Result<FilletKind, CmdError> {
    match g {
        EntityGeometry::Line(l) => Ok(FilletKind::Line(*l)),
        EntityGeometry::Arc(a) => Ok(FilletKind::Arc(*a)),
        EntityGeometry::Circle(c) => Ok(FilletKind::Circle(*c)),
        _ => Err(CmdError::Failed(
            "FILLET: each entity must be a line, arc, or circle (other geometries are deferred)"
                .to_string(),
        )),
    }
}

impl FilletKind {
    /// Returns the supporting tangent curve.
    fn tangent_curve(&self) -> TangentCurve {
        match self {
            FilletKind::Line(l) => TangentCurve::Line { a: l.p1, b: l.p2 },
            FilletKind::Arc(a) => TangentCurve::Circle {
                center: a.center,
                radius: a.radius,
            },
            FilletKind::Circle(c) => TangentCurve::Circle {
                center: c.center,
                radius: c.radius,
            },
        }
    }

    /// Returns a deterministic midpoint-style default pick.
    fn default_pick(&self) -> Point2 {
        match self {
            FilletKind::Line(l) => l.midpoint(),
            FilletKind::Arc(a) => a.arc_seg().midpoint(),
            FilletKind::Circle(c) => Point2::new(c.center.x + c.radius, c.center.y),
        }
    }

    /// Returns whether tangent point `t` lies within the bounded entity range.
    fn contains_tangent(&self, t: Point2) -> bool {
        match self {
            FilletKind::Line(_) | FilletKind::Circle(_) => true,
            FilletKind::Arc(a) => {
                angle_in_sweep(angle_of(t - a.center), a.start_angle, a.end_angle)
            }
        }
    }

    /// Returns geometry trimmed to `target`, or `None` for a complete circle.
    fn trim_to(&self, target: Point2, pick: Point2) -> Option<EntityGeometry> {
        match self {
            FilletKind::Line(l) => Some(EntityGeometry::Line(trim_line_to(*l, target, pick))),
            FilletKind::Arc(a) => Some(EntityGeometry::Arc(trim_arc_to(*a, target, pick))),
            FilletKind::Circle(_) => None,
        }
    }

    /// Returns whether this is a complete circle without trimmable endpoints.
    fn is_circle(&self) -> bool {
        matches!(self, FilletKind::Circle(_))
    }
}

/// Plans a general fillet and returns modified entities plus an optional fillet arc.
#[allow(clippy::type_complexity)]
fn fillet_general(
    k0: &FilletKind,
    k1: &FilletKind,
    id0: EntityId,
    id1: EntityId,
    radius: Option<f64>,
    pick0: Option<Point2>,
    pick1: Option<Point2>,
) -> Result<(Vec<(EntityId, EntityGeometry)>, Option<EntityGeometry>), CmdError> {
    let pk0 = pick0.unwrap_or_else(|| k0.default_pick());
    let pk1 = pick1.unwrap_or_else(|| k1.default_pick());
    let r = radius.unwrap_or(0.0);

    if r <= 0.0 {
        return fillet_corner(k0, k1, id0, id1, pk0, pk1);
    }

    let tc0 = k0.tangent_curve();
    let tc1 = k1.tangent_curve();

    // Require tangent points within entity ranges and rank them by pick proximity.
    let mut best: Option<(f64, Point2, Point2, Point2)> = None;
    for c in tangent_circle_centers(&tc0, &tc1, r) {
        let t0 = tangent_point_on(&tc0, c);
        let t1 = tangent_point_on(&tc1, c);
        if !k0.contains_tangent(t0) || !k1.contains_tangent(t1) {
            continue;
        }
        let score = t0.dist(pk0) + t1.dist(pk1);
        if best.as_ref().is_none_or(|b| score < b.0) {
            best = Some((score, c, t0, t1));
        }
    }
    let (_, center, t0, t1) = best.ok_or_else(|| {
        CmdError::Failed(
            "FILLET: no arc of that radius is tangent to both entities within their range"
                .to_string(),
        )
    })?;

    // Orient the fillet toward the nearest support intersection or pick midpoint.
    let ref_pt = mid_point(pk0, pk1);
    let corner = nearest_point(&support_intersections(k0, k1), ref_pt).unwrap_or(ref_pt);
    let arc = build_fillet_arc(center, r, t0, t1, corner);

    let mut modify = Vec::new();
    if let Some(g) = k0.trim_to(t0, pk0) {
        modify.push((id0, g));
    }
    if let Some(g) = k1.trim_to(t1, pk1) {
        modify.push((id1, g));
    }
    Ok((modify, Some(EntityGeometry::Arc(arc))))
}

/// Joins trimmable endpoints at the nearest real support intersection without an arc.
#[allow(clippy::type_complexity)]
fn fillet_corner(
    k0: &FilletKind,
    k1: &FilletKind,
    id0: EntityId,
    id1: EntityId,
    pk0: Point2,
    pk1: Point2,
) -> Result<(Vec<(EntityId, EntityGeometry)>, Option<EntityGeometry>), CmdError> {
    if k0.is_circle() || k1.is_circle() {
        return Err(CmdError::Failed(
            "FILLET: R=0 joins two endpoints, but a full circle has none".to_string(),
        ));
    }
    let pts = support_intersections(k0, k1);
    let corner = nearest_point(&pts, mid_point(pk0, pk1)).ok_or_else(|| {
        CmdError::Failed(
            "FILLET: R=0 needs the two entities to intersect, but they do not".to_string(),
        )
    })?;

    let mut modify = Vec::new();
    if let Some(g) = k0.trim_to(corner, pk0) {
        modify.push((id0, g));
    }
    if let Some(g) = k1.trim_to(corner, pk1) {
        modify.push((id1, g));
    }
    Ok((modify, None))
}

/// Trims or extends the line to `target` at the endpoint nearest `pick`.
fn trim_line_to(l: LineGeo, target: Point2, pick: Point2) -> LineGeo {
    if l.p1.dist(pick) <= l.p2.dist(pick) {
        LineGeo::new(target, l.p2)
    } else {
        LineGeo::new(l.p1, target)
    }
}

/// Trims the arc to in-sweep `target` at the endpoint nearest `pick`.
fn trim_arc_to(a: ArcGeo, target: Point2, pick: Point2) -> ArcGeo {
    let ta = angle_of(target - a.center);
    let seg = a.arc_seg();
    if seg.start_point().dist(pick) <= seg.end_point().dist(pick) {
        // Move the start when it is nearest the pick.
        ArcGeo::new(a.center, a.radius, ta, a.end_angle)
    } else {
        ArcGeo::new(a.center, a.radius, a.start_angle, ta)
    }
}

/// Chooses the radius-`r` arc from `t0` to `t1` whose midpoint faces `corner`.
fn build_fillet_arc(center: Point2, r: f64, t0: Point2, t1: Point2, corner: Point2) -> ArcGeo {
    let sa = angle_of(t0 - center);
    let ea = angle_of(t1 - center);
    let mid_ab = point_on_circle(center, r, sa + sweep_ccw(sa, ea) * 0.5);
    let mid_ba = point_on_circle(center, r, ea + sweep_ccw(ea, sa) * 0.5);
    if mid_ab.dist(corner) <= mid_ba.dist(corner) {
        ArcGeo::new(center, r, sa, ea)
    } else {
        ArcGeo::new(center, r, ea, sa)
    }
}

/// Returns real support intersections, filtering arc points to their sweeps.
fn support_intersections(k0: &FilletKind, k1: &FilletKind) -> Vec<Point2> {
    match (k0, k1) {
        (FilletKind::Line(l0), FilletKind::Line(l1)) => {
            match line_line(l0.p1, l0.p2, l1.p1, l1.p2) {
                LineX::Point(h) => vec![h.point],
                LineX::Parallel | LineX::Collinear => Vec::new(),
            }
        }
        (FilletKind::Line(l), FilletKind::Arc(a)) | (FilletKind::Arc(a), FilletKind::Line(l)) => {
            line_arc(l.p1, l.p2, &a.arc_seg())
                .into_iter()
                .filter(|h| angle_in_sweep(h.t2, a.start_angle, a.end_angle))
                .map(|h| h.point)
                .collect()
        }
        (FilletKind::Line(l), FilletKind::Circle(c))
        | (FilletKind::Circle(c), FilletKind::Line(l)) => {
            line_circle(l.p1, l.p2, c.center, c.radius)
                .into_iter()
                .map(|h| h.point)
                .collect()
        }
        (FilletKind::Arc(a0), FilletKind::Arc(a1)) => arc_arc(&a0.arc_seg(), &a1.arc_seg())
            .into_iter()
            .filter(|h| {
                angle_in_sweep(h.t1, a0.start_angle, a0.end_angle)
                    && angle_in_sweep(h.t2, a1.start_angle, a1.end_angle)
            })
            .map(|h| h.point)
            .collect(),
        (FilletKind::Arc(a), FilletKind::Circle(c))
        | (FilletKind::Circle(c), FilletKind::Arc(a)) => {
            circle_arc(c.center, c.radius, &a.arc_seg())
                .into_iter()
                .filter(|h| angle_in_sweep(h.t2, a.start_angle, a.end_angle))
                .map(|h| h.point)
                .collect()
        }
        (FilletKind::Circle(c0), FilletKind::Circle(c1)) => {
            circle_circle(c0.center, c0.radius, c1.center, c1.radius)
                .into_iter()
                .map(|h| h.point)
                .collect()
        }
    }
}

/// Returns the point nearest `ref_pt`, or `None` for an empty slice.
fn nearest_point(pts: &[Point2], ref_pt: Point2) -> Option<Point2> {
    pts.iter().copied().min_by(|p, q| {
        p.dist(ref_pt)
            .partial_cmp(&q.dist(ref_pt))
            .unwrap_or(core::cmp::Ordering::Equal)
    })
}

/// Returns the midpoint of two points.
fn mid_point(a: Point2, b: Point2) -> Point2 {
    Point2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

/// Returns a point on `(center, r)` at counterclockwise angle `ang`.
fn point_on_circle(center: Point2, r: f64, ang: f64) -> Point2 {
    let (s, c) = ang.sin_cos();
    Point2::new(center.x + r * c, center.y + r * s)
}

// ============================================================================
// OFFSET
// ============================================================================

fn offset_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let plan = offset_plan(ctx.document(), &args)?;
    let created = ctx.transact("Offset", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::created(created))
}

/// Returns OFFSET result geometry without a transaction.
fn offset_preview(doc: &Document, args: ParsedArgs) -> Result<Vec<EntityGeometry>, CmdError> {
    Ok(offset_plan(doc, &args)?.result_geoms())
}

/// Computes every offset geometry without mutation; any failure rejects the plan.
fn offset_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    let distance = args
        .distance("distance")
        .ok_or_else(|| CmdError::MissingParam("distance".to_string()))?;
    let side = args
        .point("side")
        .ok_or_else(|| CmdError::MissingParam("side".to_string()))?;
    if ids.is_empty() {
        return Err(CmdError::Failed(
            "OFFSET: no source entities to offset".to_string(),
        ));
    }

    let mut add: Vec<(EntityRecord, EntityGeometry)> = Vec::with_capacity(ids.len());
    for &id in ids {
        let (src, container) = doc.entity(id).ok_or(CmdError::UnknownEntity(id))?;
        ensure_editable(doc, container, src.layer, "OFFSET")?;
        let g = offset_geom(&src.geometry, distance, side)?;
        add.push((src.clone(), g));
    }

    Ok(ModifyPlan {
        modify: Vec::new(),
        add,
    })
}

/// Offsets `g` by `distance` toward `side`.
fn offset_geom(
    g: &EntityGeometry,
    distance: f64,
    side: Point2,
) -> Result<EntityGeometry, CmdError> {
    match g {
        EntityGeometry::Line(l) => {
            // Positive distance is left of `p1` to `p2`; `side` selects the sign.
            let d = if (l.p2 - l.p1).cross(side - l.p1) >= 0.0 {
                distance
            } else {
                -distance
            };
            let (a, b) = offset_line(l.p1, l.p2, d).map_err(offset_err)?;
            Ok(EntityGeometry::Line(LineGeo::new(a, b)))
        }
        EntityGeometry::Circle(c) => {
            let d = radial_sign(side, c.center, c.radius) * distance;
            let r = offset_circle(c.radius, d).map_err(offset_err)?;
            Ok(EntityGeometry::Circle(CircleGeo::new(c.center, r)))
        }
        EntityGeometry::Arc(a) => {
            let d = radial_sign(side, a.center, a.radius) * distance;
            let off = offset_arc(&a.arc_seg(), d).map_err(offset_err)?;
            Ok(EntityGeometry::Arc(ArcGeo::new(
                off.center,
                off.radius,
                off.start_angle,
                off.end_angle,
            )))
        }
        EntityGeometry::Polyline(p) => {
            let d = distance * polyline_side_sign(p, side);
            let verts: Vec<(Point2, f64)> = p.vertices.iter().map(|v| (v.pt, v.bulge)).collect();
            let out = offset_polyline(&verts, p.closed, d).map_err(offset_err)?;
            let vs = out
                .into_iter()
                .map(|(pt, b)| PolyVertex::new(pt, b))
                .collect();
            Ok(EntityGeometry::Polyline(PolylineGeo::new(vs, p.closed)))
        }
        EntityGeometry::Ellipse(_) => Err(CmdError::Failed(
            "OFFSET: offsetting an ellipse is deferred".to_string(),
        )),
        EntityGeometry::Point(_) => Err(CmdError::Failed(
            "OFFSET: cannot offset a point".to_string(),
        )),
        // Infinite-line offsets are not exposed; reject them explicitly.
        EntityGeometry::Xline(_) | EntityGeometry::Ray(_) => Err(CmdError::Failed(
            "OFFSET: offsetting an infinite xline/ray is deferred".to_string(),
        )),
        EntityGeometry::Spline(_) => Err(CmdError::Failed(
            "OFFSET: offsetting a spline is deferred".to_string(),
        )),
        EntityGeometry::Wipeout(_) => Err(CmdError::Failed(
            "OFFSET: offsetting a wipeout is deferred".to_string(),
        )),
    }
}

/// Returns `+1` outside a circle and `-1` inside it.
fn radial_sign(side: Point2, center: Point2, radius: f64) -> f64 {
    if side.dist(center) >= radius {
        1.0
    } else {
        -1.0
    }
}

/// Returns the polyline offset sign that moves its nearest segment toward `side`.
fn polyline_side_sign(p: &PolylineGeo, side: Point2) -> f64 {
    let n = p.vertices.len();
    let seg_count = if p.closed { n } else { n.saturating_sub(1) };
    let tol = Tol::default();
    let mut best_d = f64::INFINITY;
    let mut best_sign = 1.0;
    for i in 0..seg_count {
        let a = p.vertices[i].pt;
        let b = p.vertices[(i + 1) % n].pt;
        let bulge = p.vertices[i].bulge;
        let (d, sign) = match bulge_to_arc(a, b, bulge) {
            Ok(arc) if bulge.abs() > tol.linear => {
                let inside = side.dist(arc.center) < arc.radius;
                // The traversal's left side is interior for CCW arcs and exterior for CW.
                let left = if bulge > 0.0 { inside } else { !inside };
                (arc.distance_to(side), if left { 1.0 } else { -1.0 })
            }
            _ => {
                let left = (b - a).cross(side - a) >= 0.0;
                (
                    dist_point_segment(side, a, b),
                    if left { 1.0 } else { -1.0 },
                )
            }
        };
        if d < best_d {
            best_d = d;
            best_sign = sign;
        }
    }
    best_sign
}

// ============================================================================
// Boundary curves
// ============================================================================

/// A finite line, complete circle, or arc used as a boundary curve.
enum Curve {
    Seg { a: Point2, b: Point2 },
    Circle { center: Point2, radius: f64 },
    Arc(ArcSeg),
}

/// Collects explicit boundaries, or all other model-space entities in quick mode.
fn gather_curves(
    doc: &Document,
    edges: &[EntityId],
    target: EntityId,
    cmd: &str,
) -> Result<Vec<Curve>, CmdError> {
    let mut curves = Vec::new();
    if edges.is_empty() {
        for r in doc.model_space().iter_records() {
            if r.id != target {
                geom_to_curves(&r.geometry, &mut curves);
            }
        }
    } else {
        for &e in edges {
            if e == target {
                continue;
            }
            let (er, _) = doc.entity(e).ok_or(CmdError::UnknownEntity(e))?;
            geom_to_curves(&er.geometry, &mut curves);
        }
    }
    if curves.is_empty() {
        return Err(CmdError::Failed(format!(
            "{cmd}: no cutting/boundary edges to work against"
        )));
    }
    Ok(curves)
}

/// Decomposes geometry into primitive boundary curves.
fn geom_to_curves(g: &EntityGeometry, out: &mut Vec<Curve>) {
    match g {
        EntityGeometry::Line(l) => out.push(Curve::Seg { a: l.p1, b: l.p2 }),
        EntityGeometry::Circle(c) => out.push(Curve::Circle {
            center: c.center,
            radius: c.radius,
        }),
        EntityGeometry::Arc(a) => out.push(Curve::Arc(a.arc_seg())),
        EntityGeometry::Polyline(p) => {
            for seg in p.segments() {
                match seg {
                    SegKind::Line { a, b } => out.push(Curve::Seg { a, b }),
                    SegKind::Arc(arc) => out.push(Curve::Arc(arc)),
                }
            }
        }
        // ponytail: ellipses do not act as boundaries; add an ellipse Curve variant
        // only when complete ellipse intersection support exists.
        EntityGeometry::Ellipse(_) => {}
        EntityGeometry::Point(_) => {}
        // Materialized infinite curves are large enough to cover practical crossings.
        EntityGeometry::Xline(x) => {
            let (a, b) = x.endpoints();
            out.push(Curve::Seg { a, b });
        }
        EntityGeometry::Ray(r) => {
            let (a, b) = r.endpoints();
            out.push(Curve::Seg { a, b });
        }
        // Splines are not boundary curves until spline intersections are supported.
        EntityGeometry::Spline(_) => {}
        // Wipeouts are masks, not geometric boundaries.
        EntityGeometry::Wipeout(_) => {}
    }
}

/// Returns affine parameters where infinite target line `a` to `b` crosses bounded `curve`.
fn cross_line_curve(a: Point2, b: Point2, curve: &Curve, out: &mut Vec<f64>) {
    match curve {
        Curve::Seg { a: c, b: d } => {
            if let LineX::Point(h) = line_line(a, b, *c, *d)
                && h.t2 >= -PEPS
                && h.t2 <= 1.0 + PEPS
            {
                out.push(h.t1);
            }
        }
        Curve::Circle { center, radius } => {
            for h in line_circle(a, b, *center, *radius) {
                out.push(h.t1);
            }
        }
        Curve::Arc(arc) => {
            for h in line_circle(a, b, arc.center, arc.radius) {
                if angle_in_sweep(h.t2, arc.start_angle, arc.end_angle) {
                    out.push(h.t1);
                }
            }
        }
    }
}

/// Returns angles where target circle `(center, radius)` crosses bounded `curve`.
fn cross_circle_curve(center: Point2, radius: f64, curve: &Curve, out: &mut Vec<f64>) {
    match curve {
        Curve::Seg { a, b } => {
            for h in line_circle(*a, *b, center, radius) {
                if h.t1 >= -PEPS && h.t1 <= 1.0 + PEPS {
                    out.push(h.t2); // Angle on the target circle.
                }
            }
        }
        Curve::Circle {
            center: c2,
            radius: r2,
        } => {
            for h in circle_circle(center, radius, *c2, *r2) {
                out.push(h.t1);
            }
        }
        Curve::Arc(arc) => {
            for h in circle_circle(center, radius, arc.center, arc.radius) {
                if angle_in_sweep(h.t2, arc.start_angle, arc.end_angle) {
                    out.push(h.t1);
                }
            }
        }
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Returns the sole selected entity ID.
pub(crate) fn single_target(args: &ParsedArgs, name: &str) -> Result<EntityId, CmdError> {
    let set = args
        .entity_set(name)
        .ok_or_else(|| CmdError::MissingParam(name.to_string()))?;
    match set {
        [id] => Ok(*id),
        _ => Err(CmdError::Failed(format!(
            "expected exactly one '{name}' entity, got {}",
            set.len()
        ))),
    }
}

/// Rejects entities outside model space or on uneditable layers.
pub(crate) fn ensure_editable(
    doc: &Document,
    container: ContainerRef,
    layer: LayerId,
    cmd: &str,
) -> Result<(), CmdError> {
    if container != ContainerRef::ModelSpace {
        return Err(CmdError::Failed(format!(
            "{cmd}: only model-space entities are supported"
        )));
    }
    if let Some(reason) = uneditable_reason(doc, layer) {
        return Err(CmdError::Failed(format!("{cmd}: {reason}")));
    }
    Ok(())
}

/// Returns why `layer` is not editable, if applicable.
fn uneditable_reason(doc: &Document, layer: LayerId) -> Option<&'static str> {
    let layer = doc.layer(layer)?;
    if layer.is_locked() {
        Some("layer is locked")
    } else if layer.is_frozen() {
        Some("layer is frozen")
    } else if layer.is_off() {
        Some("layer is off")
    } else {
        None
    }
}

/// Builds an unassigned record with `src` properties and `geom`.
pub(crate) fn record_like(src: &EntityRecord, geom: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        src.layer,
        src.color,
        src.line_type,
        src.lineweight,
        geom,
    )
}

/// Returns the unclamped affine projection parameter of `p` on line `a` to `b`.
pub(crate) fn param_on_line(a: Point2, b: Point2, p: Point2) -> f64 {
    let d = b - a;
    let l2 = d.norm_sq();
    if l2 <= 0.0 { 0.0 } else { (p - a).dot(d) / l2 }
}

/// Returns Euclidean distance from `p` to segment `[a, b]`.
fn dist_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    let ab: Vec2 = b - a;
    let l2 = ab.norm_sq();
    if l2 <= 0.0 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

/// Sorts and deduplicates parameters within `PEPS`.
fn sort_dedup(v: &mut Vec<f64>) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    v.dedup_by(|a, b| (*a - *b).abs() <= PEPS);
}

/// Normalizes angles to `[0, 2π)`, sorts, and deduplicates across the wrap boundary.
fn sort_dedup_angles(v: &mut Vec<f64>) {
    for a in v.iter_mut() {
        *a = normalize_0_2pi(*a);
    }
    sort_dedup(v);
    if v.len() >= 2 {
        let first = v[0];
        let last = v[v.len() - 1];
        if (first + TAU - last).abs() <= PEPS {
            v.pop();
        }
    }
}

/// Returns the counterclockwise cut interval containing `phi`.
fn cyclic_gap(angs: &[f64], phi: f64) -> (f64, f64) {
    let n = angs.len();
    for i in 0..n {
        let lo = angs[i];
        let hi = angs[(i + 1) % n];
        let gap = if i + 1 < n { hi - lo } else { hi + TAU - lo };
        if normalize_0_2pi(phi - lo) <= gap + PEPS {
            return (lo, hi);
        }
    }
    (angs[n - 1], angs[0])
}

fn no_cross(cmd: &str) -> CmdError {
    CmdError::Failed(format!("{cmd}: the target does not cross any cutting edge"))
}

fn no_boundary(cmd: &str) -> CmdError {
    CmdError::Failed(format!(
        "{cmd}: no boundary edge lies in the direction to extend"
    ))
}

fn offset_err(e: OffsetError) -> CmdError {
    CmdError::Failed(format!("OFFSET: {e}"))
}
