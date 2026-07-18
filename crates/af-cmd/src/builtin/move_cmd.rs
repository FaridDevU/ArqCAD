//! MOVE translates an entity set by `to - from` in one transaction.
//!
//! It validates the entire set, builds [`Transform2::translate`], and applies it
//! through [`modify_entity`](af_model::TxContext::modify_entity).
//!
//! # Atomicity
//!
//! Missing IDs, non-model-space entities, and locked layers reject the complete
//! set before mutation.
//!
//! # Identity and draw order
//!
//! `modify_entity` preserves IDs and draw order. Only model-space entities are
//! currently supported.

use af_math::Transform2;
use af_model::entity::{EntityGeometry, EntityOps};
use af_model::id::EntityId;
use af_model::{ContainerRef, Layer, TxContext};

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the MOVE specification with alias `M`.
///
/// A successful MOVE produces exactly one transaction.
#[must_use]
pub fn move_spec() -> CommandSpec {
    CommandSpec::new("MOVE", "Move", true, move_exec)
        .alias("M")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("from", ParamType::Point))
        .param(ParamSpec::required("to", ParamType::Point))
}

/// Registers MOVE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(move_spec())
}

/// Applies translation `to - from` to the set in one transaction.
///
/// `to == from` produces an empty transaction that the registry reports as a
/// contract violation.
fn move_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Keep defensive missing-argument errors after registry validation.
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let from = args
        .point("from")
        .ok_or_else(|| CmdError::MissingParam("from".to_string()))?;
    let to = args
        .point("to")
        .ok_or_else(|| CmdError::MissingParam("to".to_string()))?;
    let t = Transform2::translate(to - from);

    ctx.transact("Move", |tx| apply_move(tx, &ids, &t))?;
    Ok(CommandOutcome::new())
}

/// Applies translation `t` to every ID atomically.
///
/// IDs are revalidated inside the transaction before any write. `modify_entity`
/// preserves identity and draw order.
pub(crate) fn apply_move(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    t: &Transform2,
) -> Result<(), CmdError> {
    // Validate and precompute every translated geometry before mutation.
    let mut locked: Vec<EntityId> = Vec::new();
    let mut foreign: Vec<EntityId> = Vec::new();
    let mut planned: Vec<(EntityId, EntityGeometry)> = Vec::with_capacity(ids.len());

    for &id in ids {
        let (record, container) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        if container != ContainerRef::ModelSpace {
            foreign.push(id);
            continue;
        }
        if tx.doc().layer(record.layer).is_some_and(Layer::is_locked) {
            locked.push(id);
            continue;
        }
        // Translation should remain representable; retain error mapping defensively.
        let geometry = record.geometry.transform(t).map_err(|e| {
            CmdError::Failed(format!(
                "MOVE: entity {} cannot be translated: {e}",
                id.raw().0
            ))
        })?;
        planned.push((id, geometry));
    }

    if !locked.is_empty() {
        return Err(CmdError::Failed(format!(
            "MOVE: entities on locked layers cannot be moved: [{}]",
            join_ids(&locked)
        )));
    }
    if !foreign.is_empty() {
        return Err(CmdError::Failed(format!(
            "MOVE: only model-space entities can be moved; not in model space: [{}]",
            join_ids(&foreign)
        )));
    }

    // Apply in place to preserve IDs and draw order.
    for (id, geometry) in planned {
        tx.modify_entity(id, move |record| record.geometry = geometry)?;
    }
    Ok(())
}

/// Formats IDs for diagnostics.
fn join_ids(ids: &[EntityId]) -> String {
    ids.iter()
        .map(|id| id.raw().0.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::{Point2, Vec2};
    use af_model::container::ContainerRef;
    use af_model::entity::{
        CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    };
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    use crate::args::{ArgValue, ParsedArgs};

    /// Seeds a model-space line and circle for tests.
    fn seed_line_and_circle(session: &mut Session) -> Vec<EntityId> {
        let layer = session.document().current_layer();
        let rec = |g| {
            EntityRecord::new(
                ObjectId::NIL.into(),
                layer,
                Color::ByLayer,
                LineTypeRef::ByLayer,
                Lineweight::ByLayer,
                g,
            )
        };
        session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![
                    tx.add_entity(
                        ContainerRef::ModelSpace,
                        rec(EntityGeometry::Line(LineGeo::new(
                            Point2::new(0.0, 0.0),
                            Point2::new(2.0, 1.0),
                        ))),
                    )?,
                    tx.add_entity(
                        ContainerRef::ModelSpace,
                        rec(EntityGeometry::Circle(CircleGeo::new(
                            Point2::new(4.0, 4.0),
                            1.5,
                        ))),
                    )?,
                ])
            })
            .expect("seed commits")
            .value
    }

    /// Verifies that MOVE reports only modified entities.
    #[test]
    fn apply_move_changeset_modified_is_exactly_the_set() {
        let mut session = Session::new(Units::default());
        let ids = seed_line_and_circle(&mut session);
        let t = Transform2::translate(Vec2::new(3.0, -4.0));

        let out = session
            .transact("Move", |tx| apply_move(tx, &ids, &t))
            .expect("commits");
        let cs = out.change_set.expect("tx no vacía");

        let mut expected = ids.clone();
        expected.sort_by_key(|id| id.raw().0);
        assert_eq!(cs.modified(), expected.as_slice());
        assert!(cs.added().is_empty() && cs.removed().is_empty());
    }

    /// Verifies that a zero delta leaves the document intact and commits no transaction.
    #[test]
    fn from_equals_to_is_a_trivial_success_without_a_transaction() {
        let mut session = Session::new(Units::default());
        let ids = seed_line_and_circle(&mut session);
        let before = serde_json::to_string(session.document()).unwrap();

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert("entities".to_string(), ArgValue::EntitySet(ids));
        args.insert("from".to_string(), ArgValue::Point(Point2::new(4.0, 4.0)));
        args.insert("to".to_string(), ArgValue::Point(Point2::new(4.0, 4.0)));

        let out = move_exec(&mut ctx, args).expect("to == from es éxito trivial");
        assert_eq!(ctx.tx_count(), 0, "una tx vacía no cuenta");
        assert!(out.tx_seq.is_none());
        assert_eq!(before, serde_json::to_string(ctx.document()).unwrap());
    }
}
