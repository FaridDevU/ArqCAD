//! Object-driven layer tools act on layers referenced by selected entities, while
//! global counterparts act on every layer. Each invocation uses one transaction.
//!
//! LAYCUR moves entities to the current layer; LAYMCUR sets the current layer from
//! one entity. LAYFRZ, LAYOFF, LAYLCK, and LAYULK change selected entities' layers.
//! LAYTHW and LAYON thaw or enable every layer.
//!
//! These operations follow LAYER policy and may affect the current layer.

use af_model::Layer;
use af_model::TxContext;
use af_model::id::{EntityId, LayerId};
use af_model::layers_ops::{self, LayerPatch};

use crate::args::ParsedArgs;
use crate::builtin::edit_common::{distinct_layers, validate_editable};
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Registers all eight object-layer tools.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(laycur_spec())?;
    registry.register(laymcur_spec())?;
    registry.register(layfrz_spec())?;
    registry.register(laythw_spec())?;
    registry.register(layoff_spec())?;
    registry.register(layon_spec())?;
    registry.register(laylck_spec())?;
    registry.register(layulk_spec())?;
    Ok(())
}

// ---- LAYCUR -----------------------------------------------------------------

/// Returns the LAYCUR specification without aliases.
#[must_use]
pub fn laycur_spec() -> CommandSpec {
    CommandSpec::new("LAYCUR", "Laycur", true, laycur_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

fn laycur_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    ctx.transact("Laycur", |tx| apply_laycur(tx, &ids))?;
    Ok(CommandOutcome::new())
}

/// Atomically moves `ids` to the current layer.
fn apply_laycur(tx: &mut TxContext<'_>, ids: &[EntityId]) -> Result<(), CmdError> {
    let target = tx.doc().current_layer();
    let records = validate_editable(tx, "LAYCUR", ids)?;
    for (id, _) in records {
        tx.modify_entity(id, |rec| rec.layer = target)?;
    }
    Ok(())
}

// ---- LAYMCUR -----------------------------------------------------------------

/// Returns the LAYMCUR specification without aliases.
///
/// `entity` must contain exactly one ID.
#[must_use]
pub fn laymcur_spec() -> CommandSpec {
    CommandSpec::new("LAYMCUR", "Laymcur", true, laymcur_exec)
        .param(ParamSpec::required("entity", ParamType::EntitySet))
}

fn laymcur_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let entity = args
        .entity_set("entity")
        .ok_or_else(|| CmdError::MissingParam("entity".to_string()))?;
    let &[id] = entity else {
        return Err(CmdError::Failed(format!(
            "LAYMCUR: 'entity' must contain exactly 1 entity ({} given)",
            entity.len()
        )));
    };

    ctx.transact("Laymcur", |tx| {
        let (record, _) = tx.doc().entity(id).ok_or(CmdError::UnknownEntity(id))?;
        let layer = record.layer;
        layers_ops::set_current_layer(tx, layer).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

// ---- LAYFRZ / LAYOFF / LAYLCK / LAYULK (by object) ---------------------------

/// Returns the LAYFRZ specification without aliases.
#[must_use]
pub fn layfrz_spec() -> CommandSpec {
    CommandSpec::new("LAYFRZ", "Layfrz", true, layfrz_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

fn layfrz_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = entities_arg(&args)?;
    ctx.transact("Layfrz", |tx| {
        apply_by_object(tx, &ids, |p| p.frozen = Some(true))
    })?;
    Ok(CommandOutcome::new())
}

/// Returns the LAYOFF specification without aliases.
#[must_use]
pub fn layoff_spec() -> CommandSpec {
    CommandSpec::new("LAYOFF", "Layoff", true, layoff_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

fn layoff_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = entities_arg(&args)?;
    ctx.transact("Layoff", |tx| {
        apply_by_object(tx, &ids, |p| p.off = Some(true))
    })?;
    Ok(CommandOutcome::new())
}

/// Returns the LAYLCK specification without aliases.
#[must_use]
pub fn laylck_spec() -> CommandSpec {
    CommandSpec::new("LAYLCK", "Laylck", true, laylck_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

fn laylck_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = entities_arg(&args)?;
    ctx.transact("Laylck", |tx| {
        apply_by_object(tx, &ids, |p| p.locked = Some(true))
    })?;
    Ok(CommandOutcome::new())
}

/// Returns the LAYULK specification without aliases.
#[must_use]
pub fn layulk_spec() -> CommandSpec {
    CommandSpec::new("LAYULK", "Layulk", true, layulk_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

fn layulk_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = entities_arg(&args)?;
    ctx.transact("Layulk", |tx| {
        apply_by_object(tx, &ids, |p| p.locked = Some(false))
    })?;
    Ok(CommandOutcome::new())
}

fn entities_arg(args: &ParsedArgs) -> Result<Vec<EntityId>, CmdError> {
    Ok(args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec())
}

/// Applies `patch_fn` to distinct layers referenced by `ids` without editing entities.
fn apply_by_object(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    patch_fn: impl Fn(&mut LayerPatch),
) -> Result<(), CmdError> {
    for layer in distinct_layers(tx, ids)? {
        let mut patch = LayerPatch::default();
        patch_fn(&mut patch);
        layers_ops::set_layer_props(tx, layer, patch)?;
    }
    Ok(())
}

// ---- LAYTHW / LAYON (global) -------------------------------------------------

/// Returns the parameterless LAYTHW specification.
#[must_use]
pub fn laythw_spec() -> CommandSpec {
    CommandSpec::new("LAYTHW", "Laythw", true, laythw_exec)
}

fn laythw_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    ctx.transact("Laythw", |tx| apply_to_all(tx, |p| p.frozen = Some(false)))?;
    Ok(CommandOutcome::new())
}

/// Returns the parameterless LAYON specification.
#[must_use]
pub fn layon_spec() -> CommandSpec {
    CommandSpec::new("LAYON", "Layon", true, layon_exec)
}

fn layon_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    ctx.transact("Layon", |tx| apply_to_all(tx, |p| p.off = Some(false)))?;
    Ok(CommandOutcome::new())
}

/// Applies `patch_fn` to every layer, producing operations only for actual changes.
fn apply_to_all(
    tx: &mut TxContext<'_>,
    patch_fn: impl Fn(&mut LayerPatch),
) -> Result<(), CmdError> {
    let layers: Vec<LayerId> = tx.doc().layers().map(Layer::id).collect();
    for layer in layers {
        let mut patch = LayerPatch::default();
        patch_fn(&mut patch);
        layers_ops::set_layer_props(tx, layer, patch)?;
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

    /// Creates test layer `A` and returns its ID.
    fn make_layer_a(session: &mut Session) -> LayerId {
        session
            .transact("mk A", |tx| {
                let continuous = tx.doc().line_types().next().unwrap().id();
                layers_ops::create_layer(
                    tx,
                    layers_ops::LayerProps::new(
                        "A",
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

    #[test]
    fn laycur_moves_entities_to_current_layer() {
        let mut session = Session::new(Units::default());
        let a = make_layer_a(&mut session);
        let zero = session.document().current_layer();
        let id = seed_line_on(&mut session, zero);

        session
            .transact("set current", |tx| {
                layers_ops::set_current_layer(tx, a).map_err(CmdError::from)
            })
            .expect("commits");

        let mut ctx = CommandCtx::new(&mut session);
        laycur_exec(&mut ctx, {
            let mut args = ParsedArgs::new();
            args.insert(
                "entities".to_string(),
                crate::args::ArgValue::EntitySet(vec![id]),
            );
            args
        })
        .expect("laycur executes");
        assert_eq!(session.document().entity(id).unwrap().0.layer, a);
    }

    #[test]
    fn laymcur_sets_current_layer_from_entity() {
        let mut session = Session::new(Units::default());
        let a = make_layer_a(&mut session);
        let id = seed_line_on(&mut session, a);

        let mut ctx = CommandCtx::new(&mut session);
        laymcur_exec(&mut ctx, {
            let mut args = ParsedArgs::new();
            args.insert(
                "entity".to_string(),
                crate::args::ArgValue::EntitySet(vec![id]),
            );
            args
        })
        .expect("laymcur executes");
        assert_eq!(session.document().current_layer(), a);
    }

    #[test]
    fn layfrz_layoff_laylck_toggle_the_entity_layer_not_the_entity() {
        let mut session = Session::new(Units::default());
        let zero = session.document().current_layer();
        let id = seed_line_on(&mut session, zero);

        let mut ctx = CommandCtx::new(&mut session);
        let entities = || {
            let mut args = ParsedArgs::new();
            args.insert(
                "entities".to_string(),
                crate::args::ArgValue::EntitySet(vec![id]),
            );
            args
        };
        layfrz_exec(&mut ctx, entities()).expect("layfrz executes");
        layoff_exec(&mut ctx, entities()).expect("layoff executes");
        laylck_exec(&mut ctx, entities()).expect("laylck executes");

        let layer = session.document().layer(zero).unwrap();
        assert!(layer.is_frozen() && layer.is_off() && layer.is_locked());
        assert_eq!(session.document().entity(id).unwrap().0.layer, zero);
    }

    #[test]
    fn laythw_and_layon_are_global_no_selection_needed() {
        let mut session = Session::new(Units::default());
        let zero = session.document().current_layer();
        session
            .transact("freeze+off zero", |tx| {
                layers_ops::set_layer_props(
                    tx,
                    zero,
                    LayerPatch {
                        frozen: Some(true),
                        ..Default::default()
                    },
                )
                .map_err(CmdError::from)
            })
            .expect("commits");
        session
            .transact("off zero", |tx| {
                layers_ops::set_layer_props(
                    tx,
                    zero,
                    LayerPatch {
                        frozen: Some(false),
                        off: Some(true),
                        ..Default::default()
                    },
                )
                .map_err(CmdError::from)
            })
            .expect("commits");

        let mut ctx = CommandCtx::new(&mut session);
        layon_exec(&mut ctx, ParsedArgs::new()).expect("layon executes");
        assert!(!session.document().layer(zero).unwrap().is_off());

        session
            .transact("freeze zero", |tx| {
                layers_ops::set_layer_props(
                    tx,
                    zero,
                    LayerPatch {
                        frozen: Some(true),
                        ..Default::default()
                    },
                )
                .map_err(CmdError::from)
            })
            .expect("commits");
        let mut ctx = CommandCtx::new(&mut session);
        laythw_exec(&mut ctx, ParsedArgs::new()).expect("laythw executes");
        assert!(!session.document().layer(zero).unwrap().is_frozen());
    }
}
