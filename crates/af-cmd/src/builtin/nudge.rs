//! NUDGE translates an entity set by fixed `(dx, dy)` in one transaction. It
//! delegates directly to [`apply_move`] to share MOVE validation and atomicity.
//!
//! It has no PGP alias.
//!
//! `delta` uses `ParamType::Point` so both components may be signed finite values.

use af_math::Transform2;
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::move_cmd::apply_move;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the NUDGE specification without aliases.
#[must_use]
pub fn nudge_spec() -> CommandSpec {
    CommandSpec::new("NUDGE", "Nudge", true, nudge_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("delta", ParamType::Point))
}

/// Registers NUDGE.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(nudge_spec())
}

/// Translates the set by `delta` through [`apply_move`].
fn nudge_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let delta = args
        .point("delta")
        .ok_or_else(|| CmdError::MissingParam("delta".to_string()))?;
    let t = Transform2::translate(delta.to_vec());

    ctx.transact("Nudge", |tx| apply_move(tx, &ids, &t))?;
    Ok(CommandOutcome::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::{Point2, Vec2};
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
    fn nudge_translates_by_delta_in_one_tx() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);
        let t = Transform2::translate(Vec2::new(1.0, -2.0));

        let out = session
            .transact("Nudge", |tx| apply_move(tx, &[id], &t))
            .expect("commits");
        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.modified(), &[id]);

        let rec = session.document().entity(id).unwrap().0;
        match &rec.geometry {
            EntityGeometry::Line(g) => {
                assert_eq!(g.p1, Point2::new(1.0, -2.0));
                assert_eq!(g.p2, Point2::new(2.0, -1.0));
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }
    }

    /// A zero delta produces the same no-op behavior as MOVE with equal endpoints.
    #[test]
    fn nudge_zero_delta_is_a_trivial_success_without_a_transaction() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);
        let before = serde_json::to_string(session.document()).unwrap();

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "entities".to_string(),
            crate::args::ArgValue::EntitySet(vec![id]),
        );
        args.insert(
            "delta".to_string(),
            crate::args::ArgValue::Point(Point2::new(0.0, 0.0)),
        );

        let out = nudge_exec(&mut ctx, args).expect("delta nulo es éxito trivial");
        assert_eq!(ctx.tx_count(), 0);
        assert!(out.tx_seq.is_none());
        assert_eq!(before, serde_json::to_string(ctx.document()).unwrap());
    }
}
