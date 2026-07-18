//! JOIN (`J`) merges compatible entities of the same type in one transaction. The
//! first entity keeps its identity; the others are removed.
//!
//! Supported sets are collinear lines, cocircular arcs, and endpoint-connected
//! polylines. Lines and arcs span gaps; a full arc sweep becomes a circle.
//!
//! Mixed or disconnected types fail before mutation.
//!
//! ponytail: homogeneous joining only; add mixed line/arc/polyline conversion when
//! users need a single resulting polyline.

use core::f64::consts::TAU;

use af_geom::intersect::{LineX, line_line};
use af_math::Tol;
use af_math::angle::normalize_0_2pi;
use af_model::Document;
use af_model::entity::{ArcGeo, CircleGeo, EntityGeometry, LineGeo, PolyVertex, PolylineGeo};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::modify::{ensure_editable, param_on_line};
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the JOIN specification with alias `J`.
#[must_use]
pub fn join_spec() -> CommandSpec {
    CommandSpec::new("JOIN", "Join", true, join_exec)
        .alias("J")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Registers JOIN.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(join_spec())
}

fn join_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let (keep, merged, remove) = join_plan(ctx.document(), &args)?;
    ctx.transact("Join", |tx| {
        tx.modify_entity(keep, move |r| r.geometry = merged)?;
        for id in &remove {
            tx.remove_entity(*id)?;
        }
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}

/// Plans the retained ID, merged geometry, and removed IDs without mutation.
#[allow(clippy::type_complexity)]
fn join_plan(
    doc: &Document,
    args: &ParsedArgs,
) -> Result<(EntityId, EntityGeometry, Vec<EntityId>), CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    if ids.len() < 2 {
        return Err(CmdError::Failed(
            "JOIN needs at least two entities".to_string(),
        ));
    }

    // Validate editability while collecting geometry.
    let mut geoms: Vec<EntityGeometry> = Vec::with_capacity(ids.len());
    for &id in ids {
        let (src, container) = doc.entity(id).ok_or(CmdError::UnknownEntity(id))?;
        ensure_editable(doc, container, src.layer, "JOIN")?;
        geoms.push(src.geometry.clone());
    }

    let merged = match &geoms[0] {
        EntityGeometry::Line(_) => join_lines(&geoms)?,
        EntityGeometry::Arc(_) => join_arcs(&geoms)?,
        EntityGeometry::Polyline(_) => join_polylines(&geoms)?,
        other => {
            return Err(CmdError::Failed(format!(
                "JOIN: unsupported source type {other:?} (join lines, arcs or polylines)"
            )));
        }
    };

    let keep = ids[0];
    let remove = ids[1..].to_vec();
    Ok((keep, merged, remove))
}

// ============================================================================
// Collinear lines
// ============================================================================

fn join_lines(geoms: &[EntityGeometry]) -> Result<EntityGeometry, CmdError> {
    let mut lines = Vec::with_capacity(geoms.len());
    for g in geoms {
        match g {
            EntityGeometry::Line(l) => lines.push(*l),
            _ => return Err(mixed_types()),
        }
    }
    let base = lines[0];
    // Every line must share the first line's infinite support.
    for l in &lines[1..] {
        if !matches!(line_line(base.p1, base.p2, l.p1, l.p2), LineX::Collinear) {
            return Err(CmdError::Failed(
                "JOIN: the lines are not collinear".to_string(),
            ));
        }
    }
    // Span the minimum and maximum endpoint parameters, including gaps.
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for l in &lines {
        for p in [l.p1, l.p2] {
            let t = param_on_line(base.p1, base.p2, p);
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    Ok(EntityGeometry::Line(LineGeo::new(
        base.point_at(lo),
        base.point_at(hi),
    )))
}

// ============================================================================
// Cocircular arcs
// ============================================================================

fn join_arcs(geoms: &[EntityGeometry]) -> Result<EntityGeometry, CmdError> {
    let mut arcs = Vec::with_capacity(geoms.len());
    for g in geoms {
        match g {
            EntityGeometry::Arc(a) => arcs.push(*a),
            _ => return Err(mixed_types()),
        }
    }
    let tol = Tol::default();
    let base = arcs[0];
    // Require a shared center and radius.
    for a in &arcs[1..] {
        if !tol.points_coincide(a.center, base.center)
            || (a.radius - base.radius).abs() > tol.linear
        {
            return Err(CmdError::Failed(
                "JOIN: the arcs are not on the same circle".to_string(),
            ));
        }
    }
    // Sweep counterclockwise from the first arc through the farthest end offset.
    let start = base.start_angle;
    let mut max_end = 0.0_f64;
    for a in &arcs {
        let so = normalize_0_2pi(a.start_angle - start);
        let eo = so + a.sweep();
        max_end = max_end.max(eo);
    }
    if max_end >= TAU - tol.angle {
        return Ok(EntityGeometry::Circle(CircleGeo::new(
            base.center,
            base.radius,
        )));
    }
    Ok(EntityGeometry::Arc(ArcGeo::new(
        base.center,
        base.radius,
        start,
        normalize_0_2pi(start + max_end),
    )))
}

// ============================================================================
// Connected polylines
// ============================================================================

fn join_polylines(geoms: &[EntityGeometry]) -> Result<EntityGeometry, CmdError> {
    let mut polys = Vec::with_capacity(geoms.len());
    for g in geoms {
        match g {
            EntityGeometry::Polyline(p) if !p.closed && p.vertices.len() >= 2 => {
                polys.push(p.clone())
            }
            EntityGeometry::Polyline(_) => {
                return Err(CmdError::Failed(
                    "JOIN: closed or degenerate polylines cannot be joined".to_string(),
                ));
            }
            _ => return Err(mixed_types()),
        }
    }
    let tol = Tol::default();

    // Chain endpoints, reversing pieces when necessary.
    let mut chain: Vec<PolyVertex> = polys[0].vertices.clone();
    // An open polyline's last vertex starts no segment.
    if let Some(last) = chain.last_mut() {
        last.bulge = 0.0;
    }
    let mut used = vec![false; polys.len()];
    used[0] = true;

    let mut remaining = polys.len() - 1;
    while remaining > 0 {
        let mut progressed = false;
        for i in 0..polys.len() {
            if used[i] {
                continue;
            }
            let cs = chain[0].pt;
            let ce = chain[chain.len() - 1].pt;
            let fwd = &polys[i].vertices;
            let ps = fwd[0].pt;
            let pe = fwd[fwd.len() - 1].pt;

            if tol.points_coincide(ce, ps) {
                append(&mut chain, fwd.clone());
            } else if tol.points_coincide(ce, pe) {
                append(&mut chain, reversed(fwd));
            } else if tol.points_coincide(cs, pe) {
                prepend(&mut chain, fwd.clone());
            } else if tol.points_coincide(cs, ps) {
                prepend(&mut chain, reversed(fwd));
            } else {
                continue;
            }
            used[i] = true;
            remaining -= 1;
            progressed = true;
            break;
        }
        if !progressed {
            return Err(CmdError::Failed(
                "JOIN: the polylines are not contiguous (a free end does not meet any other)"
                    .to_string(),
            ));
        }
    }

    Ok(EntityGeometry::Polyline(PolylineGeo::new(chain, false)))
}

/// Reverses an open polyline and transfers each negated bulge to its new start vertex.
fn reversed(verts: &[PolyVertex]) -> Vec<PolyVertex> {
    let n = verts.len();
    (0..n)
        .map(|j| {
            let pt = verts[n - 1 - j].pt;
            // Reverse each source segment and negate its bulge.
            let bulge = if j < n - 1 {
                -verts[n - 2 - j].bulge
            } else {
                0.0
            };
            PolyVertex::new(pt, bulge)
        })
        .collect()
}

/// Appends `piece` when its first vertex meets the end of `chain`.
fn append(chain: &mut Vec<PolyVertex>, piece: Vec<PolyVertex>) {
    // The shared vertex inherits the first segment's bulge from `piece`.
    let last = chain.len() - 1;
    chain[last].bulge = piece[0].bulge;
    chain.extend_from_slice(&piece[1..]);
}

/// Prepends `piece` when its last vertex meets the start of `chain`.
fn prepend(chain: &mut Vec<PolyVertex>, piece: Vec<PolyVertex>) {
    // Prepend all but the shared last vertex while preserving bulges.
    let mut new_chain = piece;
    new_chain.pop(); // `chain[0]` provides the shared vertex.
    new_chain.extend_from_slice(chain);
    *chain = new_chain;
}

fn mixed_types() -> CmdError {
    CmdError::Failed("JOIN: all entities must be of the same type".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }
    fn line(a: [f64; 2], b: [f64; 2]) -> EntityGeometry {
        EntityGeometry::Line(LineGeo::new(
            Point2::new(a[0], a[1]),
            Point2::new(b[0], b[1]),
        ))
    }
    fn v(x: f64, y: f64, bulge: f64) -> PolyVertex {
        PolyVertex::new(Point2::new(x, y), bulge)
    }

    #[test]
    fn join_lines_colineales_abarca_todo_con_hueco() {
        let g = join_lines(&[line([0.0, 0.0], [3.0, 0.0]), line([5.0, 0.0], [10.0, 0.0])]).unwrap();
        let EntityGeometry::Line(l) = g else {
            panic!("línea");
        };
        assert!(close_pt(l.p1, Point2::new(0.0, 0.0)) && close_pt(l.p2, Point2::new(10.0, 0.0)));
    }

    #[test]
    fn join_lines_no_colineales_es_error() {
        let e = join_lines(&[line([0.0, 0.0], [3.0, 0.0]), line([0.0, 1.0], [3.0, 1.0])]);
        assert!(matches!(e, Err(CmdError::Failed(_))));
    }

    #[test]
    fn join_arcs_contiguos_forman_un_arco() {
        use core::f64::consts::{FRAC_PI_2, PI};
        let a = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 1.0, 0.0, FRAC_PI_2));
        let b = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 1.0, FRAC_PI_2, PI));
        let g = join_arcs(&[a, b]).unwrap();
        let EntityGeometry::Arc(arc) = g else {
            panic!("arco");
        };
        assert!(close(arc.start_angle, 0.0));
        assert!(close(arc.sweep(), PI));
    }

    #[test]
    fn join_arcs_cerrando_360_da_circulo() {
        use core::f64::consts::PI;
        let a = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 2.0, 0.0, PI));
        let b = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 2.0, PI, TAU));
        let g = join_arcs(&[a, b]).unwrap();
        assert!(matches!(g, EntityGeometry::Circle(_)));
    }

    #[test]
    fn join_arcs_distinto_radio_es_error() {
        use core::f64::consts::FRAC_PI_2;
        let a = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 1.0, 0.0, FRAC_PI_2));
        let b = EntityGeometry::Arc(ArcGeo::new(
            Point2::ORIGIN,
            2.0,
            FRAC_PI_2,
            core::f64::consts::PI,
        ));
        assert!(matches!(join_arcs(&[a, b]), Err(CmdError::Failed(_))));
    }

    #[test]
    fn join_polylines_contiguas_encadena() {
        let a = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0)],
            false,
        ));
        let b = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(2.0, 0.0, 0.0), v(2.0, 1.0, 0.0), v(2.0, 2.0, 0.0)],
            false,
        ));
        let g = join_polylines(&[a, b]).unwrap();
        let EntityGeometry::Polyline(p) = g else {
            panic!("polilínea");
        };
        assert_eq!(p.vertices.len(), 5);
        assert!(close_pt(p.vertices[0].pt, Point2::new(0.0, 0.0)));
        assert!(close_pt(p.vertices[4].pt, Point2::new(2.0, 2.0)));
    }

    #[test]
    fn join_polylines_con_reversa() {
        let a = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0)],
            false,
        ));
        let b = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(4.0, 0.0, 0.0), v(2.0, 0.0, 0.0)],
            false,
        ));
        let g = join_polylines(&[a, b]).unwrap();
        let EntityGeometry::Polyline(p) = g else {
            panic!("polilínea");
        };
        assert_eq!(p.vertices.len(), 3);
        assert!(close_pt(p.vertices[0].pt, Point2::new(0.0, 0.0)));
        assert!(close_pt(p.vertices[2].pt, Point2::new(4.0, 0.0)));
    }

    /// Reversing an arc segment preserves its curve by negating and moving the bulge.
    #[test]
    fn reversed_preserva_el_arco() {
        use af_geom::bulge::bulge_to_arc;
        let orig = vec![v(0.0, 0.0, 0.6), v(4.0, 0.0, 0.0)];
        let rev = reversed(&orig);
        assert!(close_pt(rev[0].pt, Point2::new(4.0, 0.0)));
        assert!(close(rev[0].bulge, -0.6));
        let arc_o = bulge_to_arc(orig[0].pt, orig[1].pt, orig[0].bulge).unwrap();
        let arc_r = bulge_to_arc(rev[0].pt, rev[1].pt, rev[0].bulge).unwrap();
        assert!(close_pt(arc_o.center, arc_r.center));
        assert!(close(arc_o.radius, arc_r.radius));
    }

    #[test]
    fn join_polylines_no_contiguas_es_error() {
        let a = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)],
            false,
        ));
        let b = EntityGeometry::Polyline(PolylineGeo::new(
            vec![v(5.0, 5.0, 0.0), v(6.0, 5.0, 0.0)],
            false,
        ));
        assert!(matches!(join_polylines(&[a, b]), Err(CmdError::Failed(_))));
    }
}
