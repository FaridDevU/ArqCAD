//! BREAK (`BR`) and BREAKATPOINT.
//!
//! BREAK removes the section between two points. Lines and arcs split around a
//! gap; circles become the remaining arc. BREAKATPOINT splits a line or arc at one
//! point without a gap.
//!
//! Both plan before mutation and commit exactly one transaction.

use af_math::Point2;
use af_math::angle::{angle_of, normalize_0_2pi};
use af_model::Document;
use af_model::entity::{ArcGeo, EntityGeometry, EntityRecord, LineGeo};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::modify::{ModifyPlan, ensure_editable, param_on_line, single_target};
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Tolerance for treating a cut as coincident with an endpoint.
const PEPS: f64 = 1e-9;

/// Returns the BREAK specification with alias `BR`.
#[must_use]
pub fn break_spec() -> CommandSpec {
    CommandSpec::new("BREAK", "Break", true, break_exec)
        .alias("BR")
        .param(ParamSpec::required("target", ParamType::EntitySet))
        .param(ParamSpec::required("p1", ParamType::Point))
        .param(ParamSpec::required("p2", ParamType::Point))
}

/// Returns the BREAKATPOINT specification without a standard alias.
#[must_use]
pub fn break_at_point_spec() -> CommandSpec {
    CommandSpec::new("BREAKATPOINT", "Break at Point", true, break_at_point_exec)
        .param(ParamSpec::required("target", ParamType::EntitySet))
        .param(ParamSpec::required("point", ParamType::Point))
}

/// Registers BREAK and BREAKATPOINT.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(break_spec())?;
    registry.register(break_at_point_spec())?;
    Ok(())
}

// ============================================================================
// BREAK
// ============================================================================

fn break_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let plan = break_plan(ctx.document(), &args)?;
    let created = ctx.transact("Break", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::created(created))
}

fn break_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let target = single_target(args, "target")?;
    let p1 = args
        .point("p1")
        .ok_or_else(|| CmdError::MissingParam("p1".to_string()))?;
    let p2 = args
        .point("p2")
        .ok_or_else(|| CmdError::MissingParam("p2".to_string()))?;

    let (src, container) = doc.entity(target).ok_or(CmdError::UnknownEntity(target))?;
    ensure_editable(doc, container, src.layer, "BREAK")?;
    let src = src.clone();

    let (modify, extra) = match &src.geometry {
        EntityGeometry::Line(l) => break_line(*l, p1, p2)?,
        EntityGeometry::Arc(a) => break_arc(*a, p1, p2)?,
        EntityGeometry::Circle(c) => {
            // Remove the counterclockwise `p1` to `p2` arc from the circle.
            let a1 = angle_of(p1 - c.center);
            let a2 = angle_of(p2 - c.center);
            if (normalize_0_2pi(a2 - a1)).abs() <= PEPS {
                return Err(CmdError::Failed(
                    "BREAK: the two points map to the same angle on the circle".to_string(),
                ));
            }
            (
                EntityGeometry::Arc(ArcGeo::new(c.center, c.radius, a2, a1)),
                None,
            )
        }
        EntityGeometry::Polyline(_) => {
            return Err(CmdError::Failed(
                "BREAK: breaking a polyline is deferred".to_string(),
            ));
        }
        EntityGeometry::Ellipse(_) => {
            return Err(CmdError::Failed(
                "BREAK: breaking an ellipse is deferred".to_string(),
            ));
        }
        EntityGeometry::Point(_) => {
            return Err(CmdError::Failed("BREAK: cannot break a point".to_string()));
        }
        EntityGeometry::Xline(_) | EntityGeometry::Ray(_) => {
            return Err(CmdError::Failed(
                "BREAK: breaking an infinite xline/ray is not supported".to_string(),
            ));
        }
        EntityGeometry::Spline(_) => {
            return Err(CmdError::Failed(
                "BREAK: breaking a spline is deferred".to_string(),
            ));
        }
        EntityGeometry::Wipeout(_) => {
            return Err(CmdError::Failed(
                "BREAK: breaking a wipeout is deferred".to_string(),
            ));
        }
    };

    Ok(plan_modify_add(target, &src, modify, extra))
}

/// Removes the line interval between projected `p1` and `p2`.
fn break_line(
    line: LineGeo,
    p1: Point2,
    p2: Point2,
) -> Result<(EntityGeometry, Option<EntityGeometry>), CmdError> {
    let t1 = param_on_line(line.p1, line.p2, p1).clamp(0.0, 1.0);
    let t2 = param_on_line(line.p1, line.p2, p2).clamp(0.0, 1.0);
    let (lo, hi) = (t1.min(t2), t1.max(t2));

    let mut pieces = Vec::new();
    if lo > PEPS {
        pieces.push((0.0, lo));
    }
    if hi < 1.0 - PEPS {
        pieces.push((hi, 1.0));
    }
    if pieces.is_empty() {
        return Err(CmdError::Failed(
            "BREAK: the two points span the whole line; nothing would remain".to_string(),
        ));
    }
    let mk =
        |a: f64, b: f64| EntityGeometry::Line(LineGeo::new(line.point_at(a), line.point_at(b)));
    let first = mk(pieces[0].0, pieces[0].1);
    let extra = pieces.get(1).map(|&(a, b)| mk(a, b));
    Ok((first, extra))
}

/// Removes the arc interval between the angular projections of `p1` and `p2`.
fn break_arc(
    arc: ArcGeo,
    p1: Point2,
    p2: Point2,
) -> Result<(EntityGeometry, Option<EntityGeometry>), CmdError> {
    let sweep = arc.sweep();
    let off = |p: Point2| normalize_0_2pi(angle_of(p - arc.center) - arc.start_angle).min(sweep);
    let (o1, o2) = (off(p1), off(p2));
    let (lo, hi) = (o1.min(o2), o1.max(o2));

    let mut pieces = Vec::new();
    if lo > PEPS {
        pieces.push((0.0, lo));
    }
    if hi < sweep - PEPS {
        pieces.push((hi, sweep));
    }
    if pieces.is_empty() {
        return Err(CmdError::Failed(
            "BREAK: the two points span the whole arc; nothing would remain".to_string(),
        ));
    }
    let mk = |a: f64, b: f64| {
        EntityGeometry::Arc(ArcGeo::new(
            arc.center,
            arc.radius,
            normalize_0_2pi(arc.start_angle + a),
            normalize_0_2pi(arc.start_angle + b),
        ))
    };
    let first = mk(pieces[0].0, pieces[0].1);
    let extra = pieces.get(1).map(|&(a, b)| mk(a, b));
    Ok((first, extra))
}

// ============================================================================
// BREAKATPOINT
// ============================================================================

fn break_at_point_exec(
    ctx: &mut CommandCtx<'_>,
    args: ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let plan = break_at_point_plan(ctx.document(), &args)?;
    let created = ctx.transact("Break at Point", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::created(created))
}

fn break_at_point_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let target = single_target(args, "target")?;
    let point = args
        .point("point")
        .ok_or_else(|| CmdError::MissingParam("point".to_string()))?;

    let (src, container) = doc.entity(target).ok_or(CmdError::UnknownEntity(target))?;
    ensure_editable(doc, container, src.layer, "BREAKATPOINT")?;
    let src = src.clone();

    let (modify, extra) = match &src.geometry {
        EntityGeometry::Line(l) => {
            let t = param_on_line(l.p1, l.p2, point).clamp(0.0, 1.0);
            if !(PEPS..=1.0 - PEPS).contains(&t) {
                return Err(CmdError::Failed(
                    "BREAKATPOINT: the point is at (or beyond) an endpoint; nothing to split"
                        .to_string(),
                ));
            }
            let split = l.point_at(t);
            (
                EntityGeometry::Line(LineGeo::new(l.p1, split)),
                Some(EntityGeometry::Line(LineGeo::new(split, l.p2))),
            )
        }
        EntityGeometry::Arc(a) => {
            let sweep = a.sweep();
            let off = normalize_0_2pi(angle_of(point - a.center) - a.start_angle).min(sweep);
            if !(PEPS..=sweep - PEPS).contains(&off) {
                return Err(CmdError::Failed(
                    "BREAKATPOINT: the point is at (or beyond) an arc endpoint; nothing to split"
                        .to_string(),
                ));
            }
            let mid = normalize_0_2pi(a.start_angle + off);
            (
                EntityGeometry::Arc(ArcGeo::new(a.center, a.radius, a.start_angle, mid)),
                Some(EntityGeometry::Arc(ArcGeo::new(
                    a.center,
                    a.radius,
                    mid,
                    a.end_angle,
                ))),
            )
        }
        EntityGeometry::Circle(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: cannot split a full circle at a single point".to_string(),
            ));
        }
        EntityGeometry::Polyline(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: splitting a polyline is deferred".to_string(),
            ));
        }
        EntityGeometry::Ellipse(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: splitting an ellipse is deferred".to_string(),
            ));
        }
        EntityGeometry::Point(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: cannot split a point".to_string(),
            ));
        }
        EntityGeometry::Xline(_) | EntityGeometry::Ray(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: splitting an infinite xline/ray is not supported".to_string(),
            ));
        }
        EntityGeometry::Spline(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: splitting a spline is deferred".to_string(),
            ));
        }
        EntityGeometry::Wipeout(_) => {
            return Err(CmdError::Failed(
                "BREAKATPOINT: splitting a wipeout is deferred".to_string(),
            ));
        }
    };

    Ok(plan_modify_add(target, &src, modify, extra))
}

// ============================================================================
// Shared utility
// ============================================================================

/// Builds a plan that replaces `target` and optionally creates the second segment
/// with inherited properties.
fn plan_modify_add(
    target: EntityId,
    src: &EntityRecord,
    modify: EntityGeometry,
    extra: Option<EntityGeometry>,
) -> ModifyPlan {
    let mut add: Vec<(EntityRecord, EntityGeometry)> = Vec::new();
    if let Some(g) = extra {
        add.push((src.clone(), g));
    }
    ModifyPlan {
        modify: vec![(target, modify)],
        add,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_model::entity::CircleGeo;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }
    fn as_line(g: &EntityGeometry) -> LineGeo {
        match g {
            EntityGeometry::Line(l) => *l,
            o => panic!("esperaba línea, fue {o:?}"),
        }
    }

    #[test]
    fn break_line_deja_dos_tramos_con_hueco() {
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let (first, extra) = break_line(l, Point2::new(3.0, 0.0), Point2::new(7.0, 0.0)).unwrap();
        let f = as_line(&first);
        assert!(close_pt(f.p1, Point2::new(0.0, 0.0)) && close_pt(f.p2, Point2::new(3.0, 0.0)));
        let e = as_line(&extra.expect("segundo tramo"));
        assert!(close_pt(e.p1, Point2::new(7.0, 0.0)) && close_pt(e.p2, Point2::new(10.0, 0.0)));
    }

    #[test]
    fn break_line_un_punto_en_extremo_recorta_sin_hueco() {
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let (first, extra) = break_line(l, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)).unwrap();
        assert!(extra.is_none(), "un solo tramo");
        let f = as_line(&first);
        assert!(close_pt(f.p1, Point2::new(4.0, 0.0)) && close_pt(f.p2, Point2::new(10.0, 0.0)));
    }

    #[test]
    fn break_line_cubriendo_todo_es_error() {
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let e = break_line(l, Point2::new(-5.0, 0.0), Point2::new(15.0, 0.0));
        assert!(matches!(e, Err(CmdError::Failed(_))));
    }

    #[test]
    fn break_circle_becomes_arc_kept_ccw_from_p2_to_p1() {
        let c = CircleGeo::new(Point2::ORIGIN, 1.0);
        let a1 = angle_of(Point2::new(1.0, 0.0) - c.center);
        let a2 = angle_of(Point2::new(0.0, 1.0) - c.center);
        let arc = ArcGeo::new(c.center, c.radius, a2, a1);
        assert!(close(arc.sweep(), 1.5 * std::f64::consts::PI));
    }

    #[test]
    fn break_at_point_line_da_dos_lineas_contiguas() {
        let doc_line = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let t = param_on_line(doc_line.p1, doc_line.p2, Point2::new(4.0, 0.0));
        let split = doc_line.point_at(t);
        assert!(close_pt(split, Point2::new(4.0, 0.0)));
    }
}
