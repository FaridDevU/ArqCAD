//! LENGTHEN (`LEN`) changes a line or arc at the endpoint nearest `pick` and
//! commits exactly one transaction.
//!
//! `total` sets an absolute length. `delta` adds a positive distance, or subtracts
//! it when `shrink` is true.
//!
//! The opposite endpoint remains fixed. Arc sweeps cannot exceed a full circle.
//! Other geometry types are unsupported.

use core::f64::consts::TAU;

use af_math::Point2;
use af_math::angle::normalize_0_2pi;
use af_model::Document;
use af_model::entity::{ArcGeo, EntityGeometry, LineGeo};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::modify::{ensure_editable, single_target};
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Minimum usable result length.
const MIN_LEN: f64 = 1e-9;

/// Returns the LENGTHEN specification with alias `LEN`.
#[must_use]
pub fn lengthen_spec() -> CommandSpec {
    CommandSpec::new("LENGTHEN", "Lengthen", true, lengthen_exec)
        .alias("LEN")
        .param(ParamSpec::required("target", ParamType::EntitySet))
        .param(ParamSpec::required("pick", ParamType::Point))
        .param(ParamSpec::optional("total", ParamType::Distance))
        .param(ParamSpec::optional("delta", ParamType::Distance))
        .param(ParamSpec::optional("shrink", ParamType::Flag))
}

/// Registers LENGTHEN.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(lengthen_spec())
}

fn lengthen_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let (target, new_geom) = lengthen_plan(ctx.document(), &args)?;
    ctx.transact("Lengthen", |tx| {
        tx.modify_entity(target, move |r| r.geometry = new_geom)
            .map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

/// Plans the target and replacement geometry without mutation.
fn lengthen_plan(
    doc: &Document,
    args: &ParsedArgs,
) -> Result<(EntityId, EntityGeometry), CmdError> {
    let target = single_target(args, "target")?;
    let pick = args
        .point("pick")
        .ok_or_else(|| CmdError::MissingParam("pick".to_string()))?;
    let shrink = args.flag("shrink");
    let mode = match (args.distance("total"), args.distance("delta")) {
        (Some(t), None) => LenMode::Total(t),
        (None, Some(d)) => LenMode::Delta(if shrink { -d } else { d }),
        _ => {
            return Err(CmdError::Failed(
                "LENGTHEN: provide exactly one of 'total' or 'delta'".to_string(),
            ));
        }
    };

    let (src, container) = doc.entity(target).ok_or(CmdError::UnknownEntity(target))?;
    ensure_editable(doc, container, src.layer, "LENGTHEN")?;

    let new_geom = match &src.geometry {
        EntityGeometry::Line(l) => EntityGeometry::Line(lengthen_line(*l, pick, mode)?),
        EntityGeometry::Arc(a) => EntityGeometry::Arc(lengthen_arc(*a, pick, mode)?),
        EntityGeometry::Circle(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: a full circle has no endpoint to lengthen".to_string(),
            ));
        }
        EntityGeometry::Polyline(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: lengthening a polyline is deferred".to_string(),
            ));
        }
        EntityGeometry::Ellipse(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: lengthening an ellipse is deferred".to_string(),
            ));
        }
        EntityGeometry::Point(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: cannot lengthen a point".to_string(),
            ));
        }
        EntityGeometry::Xline(_) | EntityGeometry::Ray(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: cannot lengthen an infinite xline/ray".to_string(),
            ));
        }
        EntityGeometry::Spline(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: lengthening a spline is deferred".to_string(),
            ));
        }
        EntityGeometry::Wipeout(_) => {
            return Err(CmdError::Failed(
                "LENGTHEN: cannot lengthen a closed wipeout".to_string(),
            ));
        }
    };
    Ok((target, new_geom))
}

/// A resolved length-change mode.
#[derive(Clone, Copy)]
enum LenMode {
    /// Absolute result length.
    Total(f64),
    /// Signed change from current length.
    Delta(f64),
}

impl LenMode {
    /// Returns the result length from `current`.
    fn resolve(self, current: f64) -> f64 {
        match self {
            LenMode::Total(t) => t,
            LenMode::Delta(d) => current + d,
        }
    }
}

/// Changes a line at the endpoint nearest `pick` while fixing the other endpoint.
fn lengthen_line(line: LineGeo, pick: Point2, mode: LenMode) -> Result<LineGeo, CmdError> {
    let new_len = mode.resolve(line.length());
    if new_len <= MIN_LEN {
        return Err(CmdError::Failed(
            "LENGTHEN: the resulting length would be non-positive".to_string(),
        ));
    }
    // Move the endpoint nearest the pick.
    let move_p2 = pick.dist(line.p2) <= pick.dist(line.p1);
    let (fixed, moving) = if move_p2 {
        (line.p1, line.p2)
    } else {
        (line.p2, line.p1)
    };
    let dir = (moving - fixed)
        .normalize()
        .map_err(|_| CmdError::Failed("LENGTHEN: the line is degenerate".to_string()))?;
    let new_moving = fixed + dir * new_len;
    Ok(if move_p2 {
        LineGeo::new(fixed, new_moving)
    } else {
        LineGeo::new(new_moving, fixed)
    })
}

/// Changes an arc at the endpoint nearest `pick` without exceeding a full circle.
fn lengthen_arc(arc: ArcGeo, pick: Point2, mode: LenMode) -> Result<ArcGeo, CmdError> {
    let seg = arc.arc_seg();
    let new_len = mode.resolve(arc.length());
    let new_sweep = new_len / arc.radius;
    if new_sweep <= MIN_LEN || new_sweep >= TAU {
        return Err(CmdError::Failed(
            "LENGTHEN: the arc sweep would be non-positive or exceed a full circle".to_string(),
        ));
    }
    // Move the endpoint nearest the pick.
    let move_end = pick.dist(seg.end_point()) <= pick.dist(seg.start_point());
    Ok(if move_end {
        ArcGeo::new(
            arc.center,
            arc.radius,
            arc.start_angle,
            normalize_0_2pi(arc.start_angle + new_sweep),
        )
    } else {
        ArcGeo::new(
            arc.center,
            arc.radius,
            normalize_0_2pi(arc.end_angle - new_sweep),
            arc.end_angle,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, PI};

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn lengthen_line_total_por_el_extremo_cercano() {
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let m = lengthen_line(l, Point2::new(9.0, 0.0), LenMode::Total(15.0)).unwrap();
        assert!(close_pt(m.p1, Point2::new(0.0, 0.0)));
        assert!(close_pt(m.p2, Point2::new(15.0, 0.0)));
    }

    #[test]
    fn lengthen_line_delta_negativo_acorta() {
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let m = lengthen_line(l, Point2::new(1.0, 0.0), LenMode::Delta(-4.0)).unwrap();
        assert!(close_pt(m.p2, Point2::new(10.0, 0.0)));
        assert!(close(m.length(), 6.0));
        assert!(close_pt(m.p1, Point2::new(4.0, 0.0)));
    }

    #[test]
    fn lengthen_line_a_cero_es_error() {
        let l = LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        assert!(lengthen_line(l, Point2::new(9.0, 0.0), LenMode::Total(0.0)).is_err());
        assert!(lengthen_line(l, Point2::new(9.0, 0.0), LenMode::Delta(-10.0)).is_err());
    }

    #[test]
    fn lengthen_arc_cambia_el_barrido_por_el_extremo_cercano() {
        let arc = ArcGeo::new(Point2::ORIGIN, 2.0, 0.0, FRAC_PI_2);
        let target_len = 2.0 * PI; // A π sweep.
        let m = lengthen_arc(arc, Point2::new(0.0, 2.0), LenMode::Total(target_len)).unwrap();
        assert!(close(m.start_angle, 0.0));
        assert!(close(m.sweep(), PI));
    }

    #[test]
    fn lengthen_arc_por_el_inicio() {
        let arc = ArcGeo::new(Point2::ORIGIN, 2.0, 0.0, FRAC_PI_2);
        let m = lengthen_arc(arc, Point2::new(2.0, 0.0), LenMode::Delta(PI)).unwrap();
        assert!(close(m.end_angle, FRAC_PI_2));
        assert!(close(m.sweep(), PI));
    }

    #[test]
    fn lengthen_arc_mayor_que_circulo_es_error() {
        let arc = ArcGeo::new(Point2::ORIGIN, 1.0, 0.0, FRAC_PI_2);
        assert!(lengthen_arc(arc, Point2::new(0.0, 1.0), LenMode::Total(10.0)).is_err());
    }
}
