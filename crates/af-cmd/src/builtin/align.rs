//! ALIGN (`AL`) aligns entities using one or two source-to-destination point pairs
//! in one transaction.
//!
//! - One pair applies translation only.
//! - Two pairs add rotation; `scale` also applies the uniform ratio between the
//!   destination and source pair lengths.
//!
//! Geometry uses [`Transform2`] and [`EntityOps::transform`], with editability
//! validated before atomic mutation.

use af_math::Transform2;
use af_math::angle::angle_of;
use af_model::TxContext;
use af_model::entity::EntityOps;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Minimum direction-pair length for rotation and scale.
const MIN_SPAN: f64 = 1e-9;

/// Returns the ALIGN command specification with alias `AL`.
#[must_use]
pub fn align_spec() -> CommandSpec {
    CommandSpec::new("ALIGN", "Align", true, align_exec)
        .alias("AL")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("src1", ParamType::Point))
        .param(ParamSpec::required("dst1", ParamType::Point))
        .param(ParamSpec::optional("src2", ParamType::Point))
        .param(ParamSpec::optional("dst2", ParamType::Point))
        .param(ParamSpec::optional("scale", ParamType::Flag))
}

/// Registers ALIGN.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(align_spec())
}

fn align_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let t = align_transform(&args)?;
    ctx.transact("Align", |tx| apply_align(tx, &ids, &t))?;
    Ok(CommandOutcome::new())
}

/// Composes the ALIGN affine transform from validated arguments.
fn align_transform(args: &ParsedArgs) -> Result<Transform2, CmdError> {
    let src1 = args
        .point("src1")
        .ok_or_else(|| CmdError::MissingParam("src1".to_string()))?;
    let dst1 = args
        .point("dst1")
        .ok_or_else(|| CmdError::MissingParam("dst1".to_string()))?;

    match (args.point("src2"), args.point("dst2")) {
        // One pair applies translation only.
        (None, None) => Ok(Transform2::translate(dst1 - src1)),
        // Two pairs add rotation and optional uniform scale.
        (Some(src2), Some(dst2)) => {
            let v_src = src2 - src1;
            let v_dst = dst2 - dst1;
            let src_len = v_src.norm();
            let dst_len = v_dst.norm();
            if src_len <= MIN_SPAN {
                return Err(CmdError::Failed(
                    "ALIGN: the two source points coincide (no direction)".to_string(),
                ));
            }
            let angle = angle_of(v_dst) - angle_of(v_src);
            let scale = if args.flag("scale") {
                if dst_len <= MIN_SPAN {
                    return Err(CmdError::Failed(
                        "ALIGN: the two destination points coincide; cannot scale".to_string(),
                    ));
                }
                dst_len / src_len
            } else {
                1.0
            };
            // Translate to `dst1`, then rotate and scale around it.
            Ok(Transform2::translate(dst1 - src1)
                .then(Transform2::rotate_about(angle, dst1))
                .then(Transform2::scale_about(scale, scale, dst1)))
        }
        // A partial second pair is ambiguous.
        _ => Err(CmdError::Failed(
            "ALIGN: the second pair needs both 'src2' and 'dst2' (or neither)".to_string(),
        )),
    }
}

/// Applies `t` to `ids` atomically after validating the entire set.
fn apply_align(tx: &mut TxContext<'_>, ids: &[EntityId], t: &Transform2) -> Result<(), CmdError> {
    let records = validate_editable(tx, "ALIGN", ids)?;
    let mut planned = Vec::with_capacity(records.len());
    for (id, record) in records {
        let geometry = record.geometry.transform(t).map_err(|e| {
            CmdError::Failed(format!(
                "ALIGN: entity {} cannot be aligned: {e}",
                id.raw().0
            ))
        })?;
        planned.push((id, geometry));
    }
    for (id, geometry) in planned {
        tx.modify_entity(id, move |record| record.geometry = geometry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use serde_json::json;

    fn args(v: serde_json::Value) -> ParsedArgs {
        // ponytail: integration tests cover registry validation; this test builds
        // crate-internal ArgValue values to exercise only transform math.
        use crate::args::ArgValue;
        let mut a = ParsedArgs::new();
        for (k, val) in v.as_object().unwrap() {
            let av = if k == "scale" {
                ArgValue::Flag(val.as_bool().unwrap())
            } else {
                let arr = val.as_array().unwrap();
                ArgValue::Point(Point2::new(
                    arr[0].as_f64().unwrap(),
                    arr[1].as_f64().unwrap(),
                ))
            };
            a.insert(k.clone(), av);
        }
        a
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close_pt(a: Point2, b: Point2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn un_par_es_traslacion() {
        let t = align_transform(&args(json!({ "src1": [1.0, 1.0], "dst1": [4.0, 5.0] }))).unwrap();
        assert!(close_pt(
            t.apply(Point2::new(1.0, 1.0)),
            Point2::new(4.0, 5.0)
        ));
        assert!(close_pt(
            t.apply(Point2::new(2.0, 1.0)),
            Point2::new(5.0, 5.0)
        ));
    }

    #[test]
    fn dos_pares_sin_escala_rota_y_conserva_longitud() {
        let t = align_transform(&args(json!({
            "src1": [0.0, 0.0], "dst1": [0.0, 0.0],
            "src2": [1.0, 0.0], "dst2": [0.0, 2.0],
        })))
        .unwrap();
        assert!(close_pt(
            t.apply(Point2::new(0.0, 0.0)),
            Point2::new(0.0, 0.0)
        ));
        assert!(close_pt(
            t.apply(Point2::new(1.0, 0.0)),
            Point2::new(0.0, 1.0)
        ));
    }

    #[test]
    fn dos_pares_con_escala_mapea_src2_a_dst2() {
        let t = align_transform(&args(json!({
            "src1": [0.0, 0.0], "dst1": [0.0, 0.0],
            "src2": [1.0, 0.0], "dst2": [0.0, 2.0],
            "scale": true,
        })))
        .unwrap();
        assert!(close_pt(
            t.apply(Point2::new(1.0, 0.0)),
            Point2::new(0.0, 2.0)
        ));
    }

    #[test]
    fn src_coincidentes_es_error() {
        let e = align_transform(&args(json!({
            "src1": [0.0, 0.0], "dst1": [0.0, 0.0],
            "src2": [0.0, 0.0], "dst2": [1.0, 1.0],
        })));
        assert!(matches!(e, Err(CmdError::Failed(_))));
    }

    #[test]
    fn segundo_par_incompleto_es_error() {
        let e = align_transform(&args(json!({
            "src1": [0.0, 0.0], "dst1": [1.0, 0.0],
            "src2": [2.0, 0.0],
        })));
        assert!(matches!(e, Err(CmdError::Failed(_))));
    }
}
