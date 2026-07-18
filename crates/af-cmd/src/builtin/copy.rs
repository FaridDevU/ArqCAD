//! COPY (`CO`, `CP`) duplicates an entity set by displacement `to - from` in one
//! transaction.
//!
//! Each copy receives a new ID and inherits all properties; only translated
//! geometry changes. Sources remain unchanged.
//!
//! Atomic validation requires model-space sources on unlocked layers because each
//! copy inherits its source layer.

use af_math::Transform2;
use af_model::entity::EntityOps;
use af_model::id::EntityId;
use af_model::{ContainerRef, TxContext};

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the COPY specification with aliases `CO` and `CP`.
#[must_use]
pub fn copy_spec() -> CommandSpec {
    CommandSpec::new("COPY", "Copy", true, copy_exec)
        .alias("CO")
        .alias("CP")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("from", ParamType::Point))
        .param(ParamSpec::required("to", ParamType::Point))
}

/// Registers COPY.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(copy_spec())
}

/// Duplicates the set by `to - from` and reports the new IDs.
fn copy_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
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

    let created = ctx.transact("Copy", |tx| apply_copy(tx, &ids, &t))?;
    Ok(CommandOutcome::created(created))
}

/// Copies `ids` with transform `t` after validating the entire set.
pub(crate) fn apply_copy(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    t: &Transform2,
) -> Result<Vec<EntityId>, CmdError> {
    let records = validate_editable(tx, "COPY", ids)?;

    let mut new_ids = Vec::with_capacity(records.len());
    for (id, mut record) in records {
        record.geometry = record.geometry.transform(t).map_err(|e| {
            CmdError::Failed(format!("COPY: entity {} cannot be copied: {e}", id.raw().0))
        })?;
        new_ids.push(tx.add_entity(ContainerRef::ModelSpace, record)?);
    }
    Ok(new_ids)
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
    fn apply_copy_creates_new_id_and_leaves_source_untouched() {
        let mut session = Session::new(Units::default());
        let id = seed_line(&mut session);
        let source_before = session.document().entity(id).unwrap().0.clone();
        let t = Transform2::translate(Vec2::new(2.0, 3.0));

        let out = session
            .transact("Copy", |tx| apply_copy(tx, &[id], &t))
            .expect("commits");
        let new_ids = out.value;
        assert_eq!(new_ids.len(), 1);
        assert_ne!(new_ids[0], id, "la copia recibe un id nuevo");

        assert_eq!(&session.document().entity(id).unwrap().0, &source_before);
        let copy_rec = session.document().entity(new_ids[0]).unwrap().0;
        assert_eq!(copy_rec.layer, source_before.layer);
        match &copy_rec.geometry {
            EntityGeometry::Line(g) => {
                assert_eq!(g.p1, Point2::new(2.0, 3.0));
                assert_eq!(g.p2, Point2::new(3.0, 4.0));
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }

        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.added(), &[new_ids[0]]);
        assert!(cs.modified().is_empty() && cs.removed().is_empty());
    }
}
