//! ERASE (`E`) removes an entity set in one transaction.
//!
//! Missing IDs, non-model-space entities, and locked layers reject the entire set
//! before deletion.

use af_model::TxContext;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the ERASE specification with alias `E`.
#[must_use]
pub fn erase_spec() -> CommandSpec {
    CommandSpec::new("ERASE", "Erase", true, erase_exec)
        .alias("E")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Registers ERASE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(erase_spec())
}

/// Removes the complete set in one transaction.
fn erase_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    ctx.transact("Erase", |tx| apply_erase(tx, &ids))?;
    Ok(CommandOutcome::new())
}

/// Removes `ids` atomically inside `tx`.
pub(crate) fn apply_erase(tx: &mut TxContext<'_>, ids: &[EntityId]) -> Result<(), CmdError> {
    let records = validate_editable(tx, "ERASE", ids)?;
    for (id, _) in records {
        tx.remove_entity(id)?;
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
                            Point2::new(0.0, 0.0),
                            Point2::new(1.0, 1.0),
                        )),
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    #[test]
    fn apply_erase_changeset_removed_is_exactly_the_set() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);

        let out = session
            .transact("Erase", |tx| apply_erase(tx, &[id]))
            .expect("commits");
        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.removed(), &[id]);
        assert!(cs.added().is_empty() && cs.modified().is_empty());
        assert!(session.document().entity(id).is_none());
    }
}
