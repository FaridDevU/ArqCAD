//! Bulk layer operations: LAYMCH matches a source entity's layer, LAYMRG merges
//! layers, LAYDEL removes a layer and its entities, and COPYTOLAYER copies entities
//! unchanged to another layer. Each uses one transaction.

use af_model::TxContext;
use af_model::container::ContainerRef;
use af_model::id::{EntityId, LayerId};
use af_model::layers_ops::{self, DeletePolicy};

use crate::args::ParsedArgs;
use crate::builtin::edit_common::{join_ids, validate_editable};
use crate::builtin::line::uneditable_reason;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Registers LAYMCH, LAYMRG, LAYDEL, and COPYTOLAYER.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(laymch_spec())?;
    registry.register(laymrg_spec())?;
    registry.register(laydel_spec())?;
    registry.register(copytolayer_spec())?;
    Ok(())
}

// ---- LAYMCH ------------------------------------------------------------------

/// Returns the LAYMCH specification without aliases.
///
/// `source` must contain exactly one entity ID.
#[must_use]
pub fn laymch_spec() -> CommandSpec {
    CommandSpec::new("LAYMCH", "Laymch", true, laymch_exec)
        .param(ParamSpec::required("source", ParamType::EntitySet))
        .param(ParamSpec::required("targets", ParamType::EntitySet))
}

fn laymch_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let source = args
        .entity_set("source")
        .ok_or_else(|| CmdError::MissingParam("source".to_string()))?;
    let &[src] = source else {
        return Err(CmdError::Failed(format!(
            "LAYMCH: 'source' must contain exactly 1 entity ({} given)",
            source.len()
        )));
    };
    let targets: Vec<EntityId> = args
        .entity_set("targets")
        .ok_or_else(|| CmdError::MissingParam("targets".to_string()))?
        .to_vec();

    ctx.transact("Laymch", |tx| apply_laymch(tx, src, &targets))?;
    Ok(CommandOutcome::new())
}

/// Atomically moves validated `targets` to `src`'s layer without editing `src`.
fn apply_laymch(
    tx: &mut TxContext<'_>,
    src: EntityId,
    targets: &[EntityId],
) -> Result<(), CmdError> {
    let (src_rec, _) = tx.doc().entity(src).ok_or(CmdError::UnknownEntity(src))?;
    let layer = src_rec.layer;
    let records = validate_editable(tx, "LAYMCH", targets)?;
    for (id, _) in records {
        tx.modify_entity(id, |rec| rec.layer = layer)?;
    }
    Ok(())
}

// ---- LAYMRG ------------------------------------------------------------------

/// Returns the LAYMRG specification without aliases.
#[must_use]
pub fn laymrg_spec() -> CommandSpec {
    CommandSpec::new("LAYMRG", "Laymrg", true, laymrg_exec)
        .param(ParamSpec::required("from", ParamType::LayerRef))
        .param(ParamSpec::required("to", ParamType::LayerRef))
}

/// Moves every entity from `from` to `to`, then removes `from`, through
/// [`DeletePolicy::MoveEntitiesTo`]. Protected-layer rules remain centralized.
fn laymrg_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let from = args
        .layer("from")
        .ok_or_else(|| CmdError::MissingParam("from".to_string()))?;
    let to = args
        .layer("to")
        .ok_or_else(|| CmdError::MissingParam("to".to_string()))?;

    ctx.transact("Laymrg", |tx| {
        layers_ops::delete_layer(tx, from, DeletePolicy::MoveEntitiesTo(to)).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

// ---- LAYDEL ------------------------------------------------------------------

/// Returns the LAYDEL specification without aliases.
///
/// Omitted `force` is `false`.
#[must_use]
pub fn laydel_spec() -> CommandSpec {
    CommandSpec::new("LAYDEL", "Laydel", true, laydel_exec)
        .param(ParamSpec::required("layer", ParamType::LayerRef))
        .param(ParamSpec::optional("force", ParamType::Flag))
}

fn laydel_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = args
        .layer("layer")
        .ok_or_else(|| CmdError::MissingParam("layer".to_string()))?;
    let force = args.flag("force");

    ctx.transact("Laydel", |tx| apply_laydel(tx, layer, force))?;
    Ok(CommandOutcome::new())
}

/// Removes `layer` and all its entities with [`DeletePolicy::DeleteEntities`].
/// Nonempty layers require explicit `force` confirmation.
///
/// Layer `0` and the current layer remain protected.
fn apply_laydel(tx: &mut TxContext<'_>, layer: LayerId, force: bool) -> Result<(), CmdError> {
    let count = layers_ops::layer_entity_count(tx.doc(), layer);
    if count > 0 && !force {
        return Err(CmdError::Failed(format!(
            "LAYDEL: layer id {} has {count} entit{} and would be deleted along with the layer; pass force=true to confirm",
            layer.raw().0,
            if count == 1 { "y" } else { "ies" }
        )));
    }
    layers_ops::delete_layer(tx, layer, DeletePolicy::DeleteEntities).map_err(CmdError::from)
}

// ---- COPYTOLAYER --------------------------------------------------------------

/// Returns the COPYTOLAYER specification without aliases.
#[must_use]
pub fn copytolayer_spec() -> CommandSpec {
    CommandSpec::new("COPYTOLAYER", "Copytolayer", true, copytolayer_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("layer", ParamType::LayerRef))
}

fn copytolayer_exec(
    ctx: &mut CommandCtx<'_>,
    args: ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let target = args
        .layer("layer")
        .ok_or_else(|| CmdError::MissingParam("layer".to_string()))?;

    let created = ctx.transact("Copytolayer", |tx| apply_copytolayer(tx, &ids, target))?;
    Ok(CommandOutcome::created(created))
}

/// Copies unchanged `ids` to `target` with new IDs and leaves sources untouched.
///
/// All sources must be in model space, and `target` must be unlocked, thawed, and
/// on. Validation completes before any entity is created.
fn apply_copytolayer(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    target: LayerId,
) -> Result<Vec<EntityId>, CmdError> {
    {
        let layer = tx
            .doc()
            .layer(target)
            .ok_or_else(|| CmdError::UnknownLayer(target.raw().0.to_string()))?;
        if let Some(reason) = uneditable_reason(layer) {
            return Err(CmdError::Failed(format!(
                "COPYTOLAYER: cannot copy onto layer '{}': {reason}",
                layer.name()
            )));
        }
    }

    let mut foreign: Vec<EntityId> = Vec::new();
    let mut records = Vec::with_capacity(ids.len());
    for &id in ids {
        let (record, container) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        if container != ContainerRef::ModelSpace {
            foreign.push(id);
            continue;
        }
        records.push(record.clone());
    }
    if !foreign.is_empty() {
        return Err(CmdError::Failed(format!(
            "COPYTOLAYER: only model-space entities are supported; not in model space: [{}]",
            join_ids(&foreign)
        )));
    }

    let mut created = Vec::with_capacity(records.len());
    for mut record in records {
        record.layer = target;
        created.push(tx.add_entity(ContainerRef::ModelSpace, record)?);
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
    use af_model::id::ObjectId;
    use af_model::layers_ops::{LayerPatch, LayerProps};
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn make_layer(session: &mut Session, name: &str) -> LayerId {
        session
            .transact("mk layer", |tx| {
                let continuous = tx.doc().line_types().next().unwrap().id();
                layers_ops::create_layer(
                    tx,
                    LayerProps::new(
                        name,
                        Color::aci(1).unwrap(),
                        continuous,
                        Lineweight::ByLayer,
                    ),
                )
                .map_err(CmdError::from)
            })
            .expect("commits")
            .value
    }

    fn seed_line_on(session: &mut Session, layer: LayerId) -> EntityId {
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
    fn laymch_moves_targets_to_the_source_layer() {
        let mut session = Session::new(Units::default());
        let zero = session.document().current_layer();
        let a = make_layer(&mut session, "A");
        let src = seed_line_on(&mut session, a);
        let t1 = seed_line_on(&mut session, zero);
        let t2 = seed_line_on(&mut session, zero);

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "source".to_string(),
            crate::args::ArgValue::EntitySet(vec![src]),
        );
        args.insert(
            "targets".to_string(),
            crate::args::ArgValue::EntitySet(vec![t1, t2]),
        );
        laymch_exec(&mut ctx, args).expect("laymch executes");

        assert_eq!(session.document().entity(t1).unwrap().0.layer, a);
        assert_eq!(session.document().entity(t2).unwrap().0.layer, a);
        assert_eq!(session.document().entity(src).unwrap().0.layer, a);
    }

    #[test]
    fn laymrg_moves_entities_and_deletes_the_source_layer() {
        let mut session = Session::new(Units::default());
        let a = make_layer(&mut session, "A");
        let b = make_layer(&mut session, "B");
        let id = seed_line_on(&mut session, a);

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert("from".to_string(), crate::args::ArgValue::LayerRef(a));
        args.insert("to".to_string(), crate::args::ArgValue::LayerRef(b));
        laymrg_exec(&mut ctx, args).expect("laymrg executes");

        assert!(session.document().layer(a).is_none());
        assert_eq!(session.document().entity(id).unwrap().0.layer, b);
    }

    #[test]
    fn laydel_without_force_rejects_a_layer_in_use() {
        let mut session = Session::new(Units::default());
        let a = make_layer(&mut session, "A");
        let id = seed_line_on(&mut session, a);

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert("layer".to_string(), crate::args::ArgValue::LayerRef(a));
        let err = laydel_exec(&mut ctx, args).unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));
        assert!(session.document().layer(a).is_some());
        assert!(session.document().entity(id).is_some());
    }

    #[test]
    fn laydel_with_force_deletes_layer_and_its_entities() {
        let mut session = Session::new(Units::default());
        let a = make_layer(&mut session, "A");
        let id = seed_line_on(&mut session, a);

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert("layer".to_string(), crate::args::ArgValue::LayerRef(a));
        args.insert("force".to_string(), crate::args::ArgValue::Flag(true));
        laydel_exec(&mut ctx, args).expect("laydel executes with force");

        assert!(session.document().layer(a).is_none());
        assert!(session.document().entity(id).is_none());
    }

    #[test]
    fn copytolayer_creates_a_new_id_on_the_target_layer_and_keeps_geometry() {
        let mut session = Session::new(Units::default());
        let zero = session.document().current_layer();
        let a = make_layer(&mut session, "A");
        let id = seed_line_on(&mut session, zero);
        let source_before = session.document().entity(id).unwrap().0.clone();

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "entities".to_string(),
            crate::args::ArgValue::EntitySet(vec![id]),
        );
        args.insert("layer".to_string(), crate::args::ArgValue::LayerRef(a));
        let out = copytolayer_exec(&mut ctx, args).expect("copytolayer executes");

        assert_eq!(out.created.len(), 1);
        let new_id = out.created[0];
        assert_ne!(new_id, id);
        assert_eq!(&session.document().entity(id).unwrap().0, &source_before);
        let copy_rec = session.document().entity(new_id).unwrap().0;
        assert_eq!(copy_rec.layer, a);
        assert_eq!(copy_rec.geometry, source_before.geometry);
    }

    #[test]
    fn copytolayer_rejects_a_locked_target_layer() {
        let mut session = Session::new(Units::default());
        let zero = session.document().current_layer();
        let a = make_layer(&mut session, "A");
        session
            .transact("lock A", |tx| {
                layers_ops::set_layer_props(
                    tx,
                    a,
                    LayerPatch {
                        locked: Some(true),
                        ..Default::default()
                    },
                )
                .map_err(CmdError::from)
            })
            .expect("commits");
        let id = seed_line_on(&mut session, zero);

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "entities".to_string(),
            crate::args::ArgValue::EntitySet(vec![id]),
        );
        args.insert("layer".to_string(), crate::args::ArgValue::LayerRef(a));
        let err = copytolayer_exec(&mut ctx, args).unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));
    }
}
