//! CHAMFER (`CHA`) trims two lines to bevel points at distances `d1` and `d2`
//! from their intersection and inserts the connecting segment. It plans before
//! mutation and commits exactly one transaction.
//!
//! Omitting both distances creates an exact corner without a bevel segment.
//! Supplying both creates a chamfer; supplying only one is an error.
//!
//! Only line-line chamfers are supported.

use af_geom::intersect::{LineX, line_line};
use af_math::Point2;
use af_model::Document;
use af_model::entity::{EntityGeometry, EntityRecord, LineGeo};

use crate::args::ParsedArgs;
use crate::builtin::modify::{ModifyPlan, ensure_editable, far_endpoint, set_near};
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the CHAMFER specification with alias `CHA`.
#[must_use]
pub fn chamfer_spec() -> CommandSpec {
    CommandSpec::new("CHAMFER", "Chamfer", true, chamfer_exec)
        .alias("CHA")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        // Omitting both positive distances requests an exact corner.
        .param(ParamSpec::optional("d1", ParamType::Distance))
        .param(ParamSpec::optional("d2", ParamType::Distance))
}

/// Registers CHAMFER.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(chamfer_spec())
}

fn chamfer_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let plan = chamfer_plan(ctx.document(), &args)?;
    let created = ctx.transact("Chamfer", |tx| plan.apply(tx))?;
    Ok(CommandOutcome::created(created))
}

/// Computes trimmed lines and the chamfer segment without mutation.
fn chamfer_plan(doc: &Document, args: &ParsedArgs) -> Result<ModifyPlan, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    if ids.len() != 2 {
        return Err(CmdError::Failed(
            "CHAMFER requires exactly two line entities".to_string(),
        ));
    }
    // Distances must be supplied together because one-sided input is ambiguous.
    let (d1, d2) = match (args.distance("d1"), args.distance("d2")) {
        (None, None) => (0.0, 0.0),
        (Some(a), Some(b)) => (a, b),
        _ => {
            return Err(CmdError::Failed(
                "CHAMFER: provide both distances (d1 and d2) or neither (corner)".to_string(),
            ));
        }
    };
    let (id0, id1) = (ids[0], ids[1]);

    let (src0, c0) = doc.entity(id0).ok_or(CmdError::UnknownEntity(id0))?;
    let (src1, c1) = doc.entity(id1).ok_or(CmdError::UnknownEntity(id1))?;
    ensure_editable(doc, c0, src0.layer, "CHAMFER")?;
    ensure_editable(doc, c1, src1.layer, "CHAMFER")?;
    let l0 = as_line(&src0.geometry)?;
    let l1 = as_line(&src1.geometry)?;
    let src0 = src0.clone();

    let (new_l0, new_l1, seg) = chamfer_lines(l0, l1, d1, d2)?;

    let mut add: Vec<(EntityRecord, EntityGeometry)> = Vec::new();
    if let Some(seg) = seg {
        add.push((src0, EntityGeometry::Line(seg)));
    }
    Ok(ModifyPlan {
        modify: vec![
            (id0, EntityGeometry::Line(new_l0)),
            (id1, EntityGeometry::Line(new_l1)),
        ],
        add,
    })
}

/// Trims or extends two lines to their bevel points and connects them. Zero
/// distances produce the exact intersection without a segment.
#[allow(clippy::type_complexity)]
fn chamfer_lines(
    l0: LineGeo,
    l1: LineGeo,
    d1: f64,
    d2: f64,
) -> Result<(LineGeo, LineGeo, Option<LineGeo>), CmdError> {
    let p = match line_line(l0.p1, l0.p2, l1.p1, l1.p2) {
        LineX::Point(h) => h.point,
        LineX::Parallel | LineX::Collinear => {
            return Err(CmdError::Failed(
                "CHAMFER: the two lines are parallel and cannot meet".to_string(),
            ));
        }
    };

    if d1 <= 0.0 && d2 <= 0.0 {
        // Exact corner: both lines meet at the intersection without a segment.
        return Ok((set_near(l0, p, p), set_near(l1, p, p), None));
    }

    let dir0 = (far_endpoint(l0, p) - p)
        .normalize()
        .map_err(|_| CmdError::Failed("CHAMFER: a line is degenerate".to_string()))?;
    let dir1 = (far_endpoint(l1, p) - p)
        .normalize()
        .map_err(|_| CmdError::Failed("CHAMFER: a line is degenerate".to_string()))?;
    let c0: Point2 = p + dir0 * d1;
    let c1: Point2 = p + dir1 * d2;
    Ok((
        set_near(l0, p, c0),
        set_near(l1, p, c1),
        Some(LineGeo::new(c0, c1)),
    ))
}

/// Returns line geometry or rejects unsupported entity types.
fn as_line(g: &EntityGeometry) -> Result<LineGeo, CmdError> {
    match g {
        EntityGeometry::Line(l) => Ok(*l),
        _ => Err(CmdError::Failed(
            "CHAMFER: both entities must be lines".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(a: [f64; 2], b: [f64; 2]) -> LineGeo {
        LineGeo::new(Point2::new(a[0], a[1]), Point2::new(b[0], b[1]))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn chamfer_recorta_ambas_y_crea_el_segmento() {
        let (n0, n1, seg) = chamfer_lines(
            l([0.0, 0.0], [10.0, 0.0]),
            l([0.0, 0.0], [0.0, 10.0]),
            3.0,
            4.0,
        )
        .unwrap();
        let e0 = [n0.p1, n0.p2];
        assert!(e0.iter().any(|p| close_pt(*p, Point2::new(10.0, 0.0))));
        assert!(e0.iter().any(|p| close_pt(*p, Point2::new(3.0, 0.0))));
        let e1 = [n1.p1, n1.p2];
        assert!(e1.iter().any(|p| close_pt(*p, Point2::new(0.0, 10.0))));
        assert!(e1.iter().any(|p| close_pt(*p, Point2::new(0.0, 4.0))));
        let seg = seg.expect("chaflán con segmento");
        let es = [seg.p1, seg.p2];
        assert!(es.iter().any(|p| close_pt(*p, Point2::new(3.0, 0.0))));
        assert!(es.iter().any(|p| close_pt(*p, Point2::new(0.0, 4.0))));
        assert!(close(seg.length(), 5.0));
    }

    #[test]
    fn chamfer_esquina_sin_distancias_no_crea_segmento() {
        let (n0, n1, seg) = chamfer_lines(
            l([2.0, 0.0], [10.0, 0.0]),
            l([0.0, 2.0], [0.0, 10.0]),
            0.0,
            0.0,
        )
        .unwrap();
        assert!(seg.is_none(), "la esquina no inserta segmento");
        assert!(close_pt(n0.p1, Point2::ORIGIN) || close_pt(n0.p2, Point2::ORIGIN));
        assert!(close_pt(n1.p1, Point2::ORIGIN) || close_pt(n1.p2, Point2::ORIGIN));
    }

    #[test]
    fn chamfer_paralelas_es_error() {
        let e = chamfer_lines(
            l([0.0, 0.0], [10.0, 0.0]),
            l([0.0, 5.0], [10.0, 5.0]),
            1.0,
            1.0,
        );
        assert!(matches!(e, Err(CmdError::Failed(_))));
    }

    /// A symmetric bevel on a right angle is 45 degrees.
    #[test]
    fn chamfer_simetrico_angulo_recto() {
        let (_, _, seg) = chamfer_lines(
            l([0.0, 0.0], [10.0, 0.0]),
            l([0.0, 0.0], [0.0, 10.0]),
            2.0,
            2.0,
        )
        .unwrap();
        let seg = seg.unwrap();
        let d = seg.p2 - seg.p1;
        assert!(close(d.x.abs(), d.y.abs()));
    }
}
