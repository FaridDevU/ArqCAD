//! SETBYLAYER sets color, line type, and lineweight on an entity set to `ByLayer`
//! in one atomic transaction.
//!
//! It has no standard PGP alias.
//!
//! Unlike CHPROP, it changes all three overrides together and needs no value parser.

use af_model::TxContext;
use af_model::entity::{Color, LineTypeRef, Lineweight};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the SETBYLAYER specification without aliases.
#[must_use]
pub fn setbylayer_spec() -> CommandSpec {
    CommandSpec::new("SETBYLAYER", "Setbylayer", true, setbylayer_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Registers SETBYLAYER.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(setbylayer_spec())
}

fn setbylayer_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    ctx.transact("Setbylayer", |tx| apply_setbylayer(tx, &ids))?;
    Ok(CommandOutcome::new())
}

/// Applies SETBYLAYER atomically to validated model-space entities.
pub(crate) fn apply_setbylayer(tx: &mut TxContext<'_>, ids: &[EntityId]) -> Result<(), CmdError> {
    let records = validate_editable(tx, "SETBYLAYER", ids)?;
    for (id, _) in records {
        tx.modify_entity(id, |rec| {
            rec.color = Color::ByLayer;
            rec.line_type = LineTypeRef::ByLayer;
            rec.lineweight = Lineweight::ByLayer;
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::container::ContainerRef;
    use af_model::entity::{AciColor, EntityGeometry, EntityRecord, LineGeo};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn seed_override(session: &mut Session) -> EntityId {
        let layer = session.document().current_layer();
        session
            .transact("seed", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        layer,
                        Color::Aci(AciColor::new(3).unwrap()),
                        LineTypeRef::ByLayer,
                        Lineweight::Mm(0.5),
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
    fn apply_setbylayer_forces_the_three_overrides_in_one_tx() {
        let mut session = Session::new(Units::default());
        let id = seed_override(&mut session);

        let out = session
            .transact("Setbylayer", |tx| apply_setbylayer(tx, &[id]))
            .expect("commits");
        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.modified(), &[id]);

        let rec = session.document().entity(id).unwrap().0;
        assert_eq!(rec.color, Color::ByLayer);
        assert_eq!(rec.line_type, LineTypeRef::ByLayer);
        assert_eq!(rec.lineweight, Lineweight::ByLayer);
    }

    #[test]
    fn apply_setbylayer_already_bylayer_is_a_noop_transaction() {
        let mut session = Session::new(Units::default());
        let layer = session.document().current_layer();
        let id = session
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
            .value;

        let out = session
            .transact("Setbylayer", |tx| apply_setbylayer(tx, &[id]))
            .expect("commits");
        // An already-ByLayer entity records no modification.
        assert!(out.transaction.is_none());
    }
}
