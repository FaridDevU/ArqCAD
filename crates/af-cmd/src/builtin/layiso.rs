//! LAYISO turns off every layer except those referenced by selected entities;
//! LAYUNISO restores the previous `off` state.
//!
//! # Backup lifetime
//!
//! The backup belongs to [`Session`](af_model::Session), not the document. It is
//! neither serialized nor part of undo/redo, and narrow [`CommandCtx`] methods
//! expose it without granting commands mutable session access.
//!
//! Each LAYISO replaces the prior backup; LAYUNISO consumes it.
//!
//! ponytail: isolation always turns other layers off; add lock/fade modes only if
//! a `LAYISOMODE` setting is introduced.

use std::collections::BTreeSet;

use af_model::TxContext;
use af_model::id::{EntityId, LayerId};
use af_model::layers_ops::{self, LayerPatch};

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Registers LAYISO and LAYUNISO.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(layiso_spec())?;
    registry.register(layuniso_spec())?;
    Ok(())
}

/// Returns the LAYISO specification without aliases.
#[must_use]
pub fn layiso_spec() -> CommandSpec {
    CommandSpec::new("LAYISO", "Layiso", true, layiso_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

fn layiso_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    let backup = ctx.transact("Layiso", |tx| apply_layiso(tx, &ids))?;
    // Session-only backup state intentionally lives outside the transaction.
    ctx.set_layer_iso_backup(backup);
    Ok(CommandOutcome::new())
}

/// Turns off unreferenced layers and returns their previous `off` states.
fn apply_layiso(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
) -> Result<Vec<(LayerId, bool)>, CmdError> {
    let mut keep: BTreeSet<LayerId> = BTreeSet::new();
    for &id in ids {
        let (record, _container) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        keep.insert(record.layer);
    }

    let others: Vec<(LayerId, bool)> = tx
        .doc()
        .layers()
        .filter(|l| !keep.contains(&l.id()))
        .map(|l| (l.id(), l.is_off()))
        .collect();

    let mut backup = Vec::with_capacity(others.len());
    for (layer, was_off) in others {
        backup.push((layer, was_off));
        // Already-off layers require no document operation.
        layers_ops::set_layer_props(
            tx,
            layer,
            LayerPatch {
                off: Some(true),
                ..LayerPatch::default()
            },
        )?;
    }
    Ok(backup)
}

/// Returns the parameterless LAYUNISO specification.
#[must_use]
pub fn layuniso_spec() -> CommandSpec {
    CommandSpec::new("LAYUNISO", "Layuniso", true, layuniso_exec)
}

fn layuniso_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let backup = ctx.take_layer_iso_backup().ok_or_else(|| {
        CmdError::Failed("LAYUNISO: nothing to restore (no LAYISO pending)".to_string())
    })?;

    ctx.transact("Layuniso", |tx| apply_layuniso(tx, &backup))?;
    Ok(CommandOutcome::new())
}

/// Restores backed-up `off` states and skips layers removed since isolation.
fn apply_layuniso(tx: &mut TxContext<'_>, backup: &[(LayerId, bool)]) -> Result<(), CmdError> {
    for &(layer, was_off) in backup {
        if tx.doc().layer(layer).is_none() {
            continue;
        }
        layers_ops::set_layer_props(
            tx,
            layer,
            LayerPatch {
                off: Some(was_off),
                ..LayerPatch::default()
            },
        )?;
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

    /// Creates a document with layers used by isolation tests.
    fn seeded() -> (Session, LayerId, LayerId, LayerId, EntityId) {
        let mut session = Session::new(Units::default());
        let zero = session.document().current_layer();
        let (a, b) = session
            .transact("mk A/B", |tx| -> Result<(LayerId, LayerId), CmdError> {
                let continuous = tx.doc().line_types().next().unwrap().id();
                let a = layers_ops::create_layer(
                    tx,
                    layers_ops::LayerProps::new(
                        "A",
                        Color::aci(1).unwrap(),
                        continuous,
                        Lineweight::ByLayer,
                    ),
                )
                .map_err(CmdError::from)?;
                let b = layers_ops::create_layer(
                    tx,
                    layers_ops::LayerProps::new(
                        "B",
                        Color::aci(2).unwrap(),
                        continuous,
                        Lineweight::ByLayer,
                    ),
                )
                .map_err(CmdError::from)?;
                Ok((a, b))
            })
            .expect("commits")
            .value;
        session
            .transact("off B", |tx| {
                layers_ops::set_layer_props(
                    tx,
                    b,
                    LayerPatch {
                        off: Some(true),
                        ..Default::default()
                    },
                )
                .map_err(CmdError::from)
            })
            .expect("commits");
        let id = session
            .transact("seed", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        zero,
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
            .expect("commits")
            .value;
        (session, zero, a, b, id)
    }

    #[test]
    fn layiso_turns_off_every_other_layer_and_layuniso_restores_exactly() {
        let (mut session, zero, a, b, id) = seeded();
        assert!(session.document().layer(b).unwrap().is_off());

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "entities".to_string(),
            crate::args::ArgValue::EntitySet(vec![id]),
        );
        layiso_exec(&mut ctx, args).expect("layiso executes");

        assert!(!session.document().layer(zero).unwrap().is_off());
        assert!(session.document().layer(a).unwrap().is_off());
        assert!(session.document().layer(b).unwrap().is_off());

        let mut ctx = CommandCtx::new(&mut session);
        layuniso_exec(&mut ctx, ParsedArgs::new()).expect("layuniso executes");

        assert!(!session.document().layer(a).unwrap().is_off());
        assert!(session.document().layer(b).unwrap().is_off());
    }

    #[test]
    fn layuniso_without_a_prior_layiso_fails() {
        let (mut session, _zero, _a, _b, _id) = seeded();
        let mut ctx = CommandCtx::new(&mut session);
        let err = layuniso_exec(&mut ctx, ParsedArgs::new()).unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));
    }

    #[test]
    fn a_second_layiso_replaces_the_backup_of_the_first() {
        let (mut session, zero, a, _b, id) = seeded();

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "entities".to_string(),
            crate::args::ArgValue::EntitySet(vec![id]),
        );
        layiso_exec(&mut ctx, args).expect("layiso executes");
        assert!(session.document().layer(a).unwrap().is_off());

        let (c, id_on_c) = session
            .transact(
                "mk C + seed",
                |tx| -> Result<(LayerId, EntityId), CmdError> {
                    let continuous = tx.doc().line_types().next().unwrap().id();
                    let c = layers_ops::create_layer(
                        tx,
                        layers_ops::LayerProps::new(
                            "C",
                            Color::aci(3).unwrap(),
                            continuous,
                            Lineweight::ByLayer,
                        ),
                    )
                    .map_err(CmdError::from)?;
                    let id = tx
                        .add_entity(
                            ContainerRef::ModelSpace,
                            EntityRecord::new(
                                ObjectId::NIL.into(),
                                c,
                                Color::ByLayer,
                                LineTypeRef::ByLayer,
                                Lineweight::ByLayer,
                                EntityGeometry::Line(LineGeo::new(
                                    Point2::new(0.0, 0.0),
                                    Point2::new(1.0, 1.0),
                                )),
                            ),
                        )
                        .map_err(CmdError::from)?;
                    Ok((c, id))
                },
            )
            .expect("commits")
            .value;

        let mut ctx = CommandCtx::new(&mut session);
        let mut args2 = ParsedArgs::new();
        args2.insert(
            "entities".to_string(),
            crate::args::ArgValue::EntitySet(vec![id_on_c]),
        );
        layiso_exec(&mut ctx, args2).expect("layiso executes");
        assert!(session.document().layer(zero).unwrap().is_off());
        assert!(!session.document().layer(c).unwrap().is_off());

        let mut ctx = CommandCtx::new(&mut session);
        layuniso_exec(&mut ctx, ParsedArgs::new()).expect("layuniso executes");
        assert!(!session.document().layer(zero).unwrap().is_off());
        assert!(session.document().layer(a).unwrap().is_off());
    }
}
