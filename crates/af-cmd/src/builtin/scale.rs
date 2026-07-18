//! SCALE (`SC`) uniformly scales an entity set from a base point in one transaction.
//!
//! Equal scale factors avoid unsupported nonuniform geometry transforms. `factor`
//! reuses positive finite `ParamType::Distance` validation.
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

/// Returns the SCALE specification with alias `SC`.
#[must_use]
pub fn scale_spec() -> CommandSpec {
    CommandSpec::new("SCALE", "Scale", true, scale_exec)
        .alias("SC")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("base", ParamType::Point))
        .param(ParamSpec::required("factor", ParamType::Distance))
}

/// Registers SCALE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(scale_spec())
}

/// Uniformly scales the set by `factor` from `base`.
fn scale_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let base = args
        .point("base")
        .ok_or_else(|| CmdError::MissingParam("base".to_string()))?;
    // `ParamType::Distance` guarantees a positive finite factor.
    let factor = args
        .distance("factor")
        .ok_or_else(|| CmdError::MissingParam("factor".to_string()))?;
    let t = Transform2::scale_about(factor, factor, base);

    ctx.transact("Scale", |tx| apply_scale(tx, &ids, &t))?;
    Ok(CommandOutcome::new())
}

/// Applies scale `t` atomically after validating the entire set.
pub(crate) fn apply_scale(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    t: &Transform2,
) -> Result<(), CmdError> {
    let records = validate_editable(tx, "SCALE", ids)?;
    let mut planned = Vec::with_capacity(records.len());
    for (id, record) in records {
        let geometry = record.geometry.transform(t).map_err(|e| {
            CmdError::Failed(format!(
                "SCALE: entity {} cannot be scaled: {e}",
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
    use af_model::entity::{
        CircleGeo, Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight,
    };
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn seed_circle(session: &mut Session) -> EntityId {
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
                        EntityGeometry::Circle(CircleGeo::new(Point2::new(2.0, 2.0), 1.0)),
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    #[test]
    fn apply_scale_doubles_radius_from_base() {
        let mut session = Session::new(Units::default());
        let id = seed_circle(&mut session);
        let t = Transform2::scale_about(2.0, 2.0, Point2::ORIGIN);

        let out = session
            .transact("Scale", |tx| apply_scale(tx, &[id], &t))
            .expect("commits");
        assert_eq!(out.change_set.expect("tx no vacía").modified(), &[id]);

        let (rec, _) = session.document().entity(id).unwrap();
        match &rec.geometry {
            EntityGeometry::Circle(g) => {
                assert_eq!(g.center, Point2::new(4.0, 4.0));
                assert_eq!(g.radius, 2.0);
            }
            other => panic!("esperaba círculo, fue {other:?}"),
        }
    }
}
