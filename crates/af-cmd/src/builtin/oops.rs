//! OOPS restores entities removed by the latest ERASE without undoing later commands.
//!
//! It has no standard PGP alias.
//!
//! # History lookup
//!
//! OOPS reads the latest undo-stack transaction labeled `Erase` without consuming
//! or modifying history, avoiding command-specific mutable session state.
//!
//! [`TxContext::add_entity`] assigns new IDs and appends restored entities to draw
//! order. IDs are never recycled.
//!
//! Repeating OOPS without another ERASE recreates the same set with new IDs.

use af_model::entity::EntityRecord;
use af_model::id::EntityId;
use af_model::{ContainerRef, DocOp, TxContext};

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec};

/// Returns the parameterless OOPS specification without aliases.
#[must_use]
pub fn oops_spec() -> CommandSpec {
    CommandSpec::new("OOPS", "Oops", true, oops_exec)
}

/// Registers OOPS.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(oops_spec())
}

/// Recreates entities from the latest `Erase` history entry in one transaction.
fn oops_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let removed = last_erased_records(ctx)?;
    let created = ctx.transact("Oops", |tx| apply_oops(tx, &removed))?;
    Ok(CommandOutcome::created(created))
}

/// Collects removed records from the newest `Erase` transaction.
///
/// # Errors
/// Returns [`CmdError::Failed`] when history contains no `Erase` transaction.
fn last_erased_records(
    ctx: &CommandCtx<'_>,
) -> Result<Vec<(ContainerRef, EntityRecord)>, CmdError> {
    ctx.undo_transactions()
        .find(|tx| tx.label() == "Erase")
        .map(|tx| {
            tx.ops()
                .iter()
                .filter_map(|op| match op {
                    DocOp::RemoveEntity {
                        container, record, ..
                    } => Some((*container, record.clone())),
                    _ => None,
                })
                .collect()
        })
        .ok_or_else(|| CmdError::Failed("OOPS: no hay ningún ERASE que restaurar".to_string()))
}

/// Recreates each record in `tx` and returns new IDs in the same order.
pub(crate) fn apply_oops(
    tx: &mut TxContext<'_>,
    removed: &[(ContainerRef, EntityRecord)],
) -> Result<Vec<EntityId>, CmdError> {
    let mut ids = Vec::with_capacity(removed.len());
    for (container, record) in removed {
        ids.push(tx.add_entity(*container, record.clone())?);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::container::ContainerRef as CRef;
    use af_model::entity::{Color, EntityGeometry, LineGeo, LineTypeRef, Lineweight};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    use crate::builtin::erase::apply_erase;

    fn seed_lines(session: &mut Session, n: u32) -> Vec<EntityId> {
        let layer = session.document().current_layer();
        session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                (0..n)
                    .map(|i| {
                        tx.add_entity(
                            CRef::ModelSpace,
                            EntityRecord::new(
                                ObjectId::NIL.into(),
                                layer,
                                Color::ByLayer,
                                LineTypeRef::ByLayer,
                                Lineweight::ByLayer,
                                EntityGeometry::Line(LineGeo::new(
                                    Point2::new(f64::from(i), 0.0),
                                    Point2::new(f64::from(i) + 1.0, 1.0),
                                )),
                            ),
                        )
                    })
                    .collect()
            })
            .expect("seed commits")
            .value
    }

    #[test]
    fn oops_restaura_lo_borrado_por_erase_con_ids_nuevos() {
        let mut session = Session::new(Units::default());
        let ids = seed_lines(&mut session, 2);

        session
            .transact("Erase", |tx| apply_erase(tx, &ids))
            .expect("erase commits");
        assert!(session.document().entity(ids[0]).is_none());
        assert!(session.document().entity(ids[1]).is_none());

        let removed = last_erased_records(&CommandCtx::new(&mut session)).expect("hay un Erase");
        assert_eq!(removed.len(), 2);

        let out = session
            .transact("Oops", |tx| apply_oops(tx, &removed))
            .expect("oops commits");
        let new_ids = out.value;
        assert_eq!(new_ids.len(), 2);
        for new_id in &new_ids {
            assert!(!ids.contains(new_id));
        }
        assert!(session.document().entity(new_ids[0]).is_some());
        assert!(session.document().entity(new_ids[1]).is_some());
    }

    #[test]
    fn oops_dos_veces_seguidas_re_crea_el_mismo_conjunto() {
        let mut session = Session::new(Units::default());
        let ids = seed_lines(&mut session, 1);
        session
            .transact("Erase", |tx| apply_erase(tx, &ids))
            .expect("erase commits");

        let removed = last_erased_records(&CommandCtx::new(&mut session)).unwrap();
        let first = session
            .transact("Oops", |tx| apply_oops(tx, &removed))
            .unwrap()
            .value;
        let removed_again = last_erased_records(&CommandCtx::new(&mut session)).unwrap();
        let second = session
            .transact("Oops", |tx| apply_oops(tx, &removed_again))
            .unwrap()
            .value;

        assert_ne!(first[0], second[0], "cada invocación reparte ids nuevos");
        assert!(session.document().entity(first[0]).is_some());
        assert!(session.document().entity(second[0]).is_some());
    }

    #[test]
    fn oops_ignora_comandos_posteriores_al_erase() {
        let mut session = Session::new(Units::default());
        let ids = seed_lines(&mut session, 1);

        session
            .transact("Erase", |tx| apply_erase(tx, &ids[..1]))
            .expect("erase commits");

        let extra = seed_lines(&mut session, 1);

        let removed = last_erased_records(&CommandCtx::new(&mut session))
            .expect("Erase sigue siendo encontrable pese al comando posterior");
        assert_eq!(removed.len(), 1);

        let out = session
            .transact("Oops", |tx| apply_oops(tx, &removed))
            .expect("oops commits");
        assert!(session.document().entity(extra[0]).is_some());
        assert!(session.document().entity(out.value[0]).is_some());
    }

    #[test]
    fn oops_sin_ningun_erase_previo_falla_sin_transaccion() {
        let mut session = Session::new(Units::default());
        seed_lines(&mut session, 1); // Only one seed and no Erase transaction.
        let before = serde_json::to_string(session.document()).unwrap();

        let err = last_erased_records(&CommandCtx::new(&mut session))
            .expect_err("no hay Erase que restaurar");
        assert!(matches!(err, CmdError::Failed(_)));
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }
}
