//! ROTATE (`RO`) rotates an entity set around a base point in one transaction.
//!
//! `angle` is counterclockwise radians, matching [`Transform2::rotate_about`].
//!
//! The entire set is validated before mutation.

use af_math::Transform2;
use af_model::TxContext;
use af_model::entity::EntityOps;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the ROTATE specification with alias `RO`.
#[must_use]
pub fn rotate_spec() -> CommandSpec {
    CommandSpec::new("ROTATE", "Rotate", true, rotate_exec)
        .alias("RO")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("base", ParamType::Point))
        .param(ParamSpec::required("angle", ParamType::Angle))
}

/// Registers ROTATE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(rotate_spec())
}

/// Rotates the set counterclockwise by `angle` radians around `base`.
fn rotate_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let base = args
        .point("base")
        .ok_or_else(|| CmdError::MissingParam("base".to_string()))?;
    let angle = args
        .angle("angle")
        .ok_or_else(|| CmdError::MissingParam("angle".to_string()))?;
    let t = Transform2::rotate_about(angle, base);

    ctx.transact("Rotate", |tx| apply_rotate(tx, &ids, &t))?;
    Ok(CommandOutcome::new())
}

/// Applies rotation `t` atomically after validating the entire set.
pub(crate) fn apply_rotate(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    t: &Transform2,
) -> Result<(), CmdError> {
    let records = validate_editable(tx, "ROTATE", ids)?;
    let mut planned = Vec::with_capacity(records.len());
    for (id, record) in records {
        let geometry = record.geometry.transform(t).map_err(|e| {
            CmdError::Failed(format!(
                "ROTATE: entity {} cannot be rotated: {e}",
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
    use af_model::container::ContainerRef;
    use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};
    use std::f64::consts::FRAC_PI_2;

    fn seed_line(session: &mut Session) -> EntityId {
        let layer = session.document().current_layer();
        session
            .transact("seed", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        layer,
                        Color::ByLayer,
                        LineTypeRef::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Line(LineGeo::new(
                            Point2::new(1.0, 0.0),
                            Point2::new(2.0, 0.0),
                        )),
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    #[test]
    fn apply_rotate_quarter_turn_about_origin() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);
        let t = Transform2::rotate_about(FRAC_PI_2, Point2::ORIGIN);

        let out = session
            .transact("Rotate", |tx| apply_rotate(tx, &[id], &t))
            .expect("commits");
        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.modified(), &[id]);

        let (rec, _) = session.document().entity(id).unwrap();
        match &rec.geometry {
            EntityGeometry::Line(g) => {
                let tol = 1e-9;
                assert!((g.p1.x - 0.0).abs() < tol && (g.p1.y - 1.0).abs() < tol);
                assert!((g.p2.x - 0.0).abs() < tol && (g.p2.y - 2.0).abs() < tol);
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }
    }
}
