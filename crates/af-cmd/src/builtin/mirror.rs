//! MIRROR (`MI`) reflects an entity set across the line through `p1` and `p2` in
//! one transaction.
//!
//! Reflected copies receive new IDs. `erase_source`, false by default, removes the
//! originals in the same transaction. Orientation reversal and bulge handling are
//! delegated to `EntityOps::transform`.
//!
//! Invalid sets and degenerate axes fail before mutation.

use af_math::Transform2;
use af_model::entity::EntityOps;
use af_model::id::EntityId;
use af_model::{ContainerRef, TxContext};

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the MIRROR specification with alias `MI`.
#[must_use]
pub fn mirror_spec() -> CommandSpec {
    CommandSpec::new("MIRROR", "Mirror", true, mirror_exec)
        .alias("MI")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("p1", ParamType::Point))
        .param(ParamSpec::required("p2", ParamType::Point))
        .param(ParamSpec::optional("erase_source", ParamType::Flag))
}

/// Registers MIRROR.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(mirror_spec())
}

/// Reflects the set across axis `p1`-`p2`.
fn mirror_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let p1 = args
        .point("p1")
        .ok_or_else(|| CmdError::MissingParam("p1".to_string()))?;
    let p2 = args
        .point("p2")
        .ok_or_else(|| CmdError::MissingParam("p2".to_string()))?;
    // An omitted flag keeps sources by default.
    let erase_source = args.flag("erase_source");

    // Reject a directionless axis before touching the session.
    let t = Transform2::reflect_about_line(p1, p2).map_err(|_| {
        CmdError::Failed("MIRROR: axis points must be distinct (p1 == p2)".to_string())
    })?;

    let created = ctx.transact("Mirror", |tx| apply_mirror(tx, &ids, &t, erase_source))?;
    Ok(CommandOutcome::created(created))
}

/// Applies reflection `t` atomically, optionally removing sources after creating copies.
pub(crate) fn apply_mirror(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    t: &Transform2,
    erase_source: bool,
) -> Result<Vec<EntityId>, CmdError> {
    let records = validate_editable(tx, "MIRROR", ids)?;

    let mut planned = Vec::with_capacity(records.len());
    for (id, record) in records {
        let mut mirrored = record.clone();
        mirrored.geometry = record.geometry.transform(t).map_err(|e| {
            CmdError::Failed(format!(
                "MIRROR: entity {} cannot be mirrored: {e}",
                id.raw().0
            ))
        })?;
        planned.push((id, mirrored));
    }

    let mut new_ids = Vec::with_capacity(planned.len());
    for (_, mirrored) in &planned {
        new_ids.push(tx.add_entity(ContainerRef::ModelSpace, mirrored.clone())?);
    }
    if erase_source {
        for (id, _) in &planned {
            tx.remove_entity(*id)?;
        }
    }
    Ok(new_ids)
}

/// Builds a reflection transform for an already-validated axis.
#[cfg(test)]
fn axis(p1: af_math::Point2, p2: af_math::Point2) -> Transform2 {
    Transform2::reflect_about_line(p1, p2).expect("eje no degenerado")
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
                            Point2::new(1.0, 0.0),
                            Point2::new(3.0, 0.0),
                        )),
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    #[test]
    fn apply_mirror_default_keeps_source_and_adds_reflection() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);
        let t = axis(Point2::new(0.0, 0.0), Point2::new(0.0, 1.0));

        let out = session
            .transact("Mirror", |tx| apply_mirror(tx, &[id], &t, false))
            .expect("commits");
        let new_ids = out.value;
        assert_eq!(new_ids.len(), 1);
        assert_ne!(new_ids[0], id);

        assert!(session.document().entity(id).is_some());
        let (rec, _) = session.document().entity(new_ids[0]).unwrap();
        match &rec.geometry {
            EntityGeometry::Line(g) => {
                assert_eq!(g.p1, Point2::new(-1.0, 0.0));
                assert_eq!(g.p2, Point2::new(-3.0, 0.0));
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }

        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.added(), &[new_ids[0]]);
        assert!(cs.removed().is_empty());
    }

    #[test]
    fn apply_mirror_with_erase_source_removes_the_original() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);
        let t = axis(Point2::new(0.0, 0.0), Point2::new(0.0, 1.0));

        let out = session
            .transact("Mirror", |tx| apply_mirror(tx, &[id], &t, true))
            .expect("commits");
        let new_ids = out.value;

        assert!(session.document().entity(id).is_none(), "fuente borrada");
        assert!(session.document().entity(new_ids[0]).is_some());

        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.added(), &[new_ids[0]]);
        assert_eq!(cs.removed(), &[id]);
    }
}
