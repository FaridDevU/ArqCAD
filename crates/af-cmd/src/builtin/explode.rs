//! EXPLODE (`X`) replaces polylines with individual line and arc segments in one
//! transaction.
//!
//! [`PolylineGeo::segments`] resolves bulges to exact arcs without flattening.
//! Pieces inherit the source polyline's properties.
//!
//! Only polylines are supported. Any unsupported entity rejects the entire set.

use af_model::entity::{ArcGeo, EntityGeometry, EntityRecord, LineGeo, PolylineGeo, SegKind};
use af_model::id::EntityId;
use af_model::{ContainerRef, TxContext};

use crate::args::ParsedArgs;
use crate::builtin::edit_common::{join_ids, validate_editable};
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the EXPLODE specification with alias `X`.
#[must_use]
pub fn explode_spec() -> CommandSpec {
    CommandSpec::new("EXPLODE", "Explode", true, explode_exec)
        .alias("X")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Registers EXPLODE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(explode_spec())
}

/// Explodes the set in one transaction and reports new piece IDs.
fn explode_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    let created = ctx.transact("Explode", |tx| apply_explode(tx, &ids))?;
    Ok(CommandOutcome::created(created))
}

/// Plans every piece before atomically replacing the source polylines.
pub(crate) fn apply_explode(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
) -> Result<Vec<EntityId>, CmdError> {
    let records = validate_editable(tx, "EXPLODE", ids)?;

    // Plan all polyline pieces before mutation.
    let mut offenders: Vec<EntityId> = Vec::new();
    let mut planned: Vec<(EntityId, EntityRecord, Vec<EntityGeometry>)> = Vec::new();
    for (id, record) in records {
        // Keep the match exhaustive so new geometry requires an explicit decision.
        match &record.geometry {
            EntityGeometry::Polyline(poly) => {
                let pieces = explode_polyline(poly);
                if pieces.is_empty() {
                    return Err(CmdError::Failed(format!(
                        "EXPLODE: la polilínea {} no tiene tramos que explotar",
                        id.raw().0
                    )));
                }
                planned.push((id, record.clone(), pieces));
            }
            EntityGeometry::Line(_)
            | EntityGeometry::Circle(_)
            | EntityGeometry::Arc(_)
            | EntityGeometry::Ellipse(_)
            | EntityGeometry::Point(_)
            | EntityGeometry::Xline(_)
            | EntityGeometry::Ray(_)
            | EntityGeometry::Spline(_)
            | EntityGeometry::Wipeout(_) => offenders.push(id),
        }
    }
    if !offenders.is_empty() {
        return Err(CmdError::Failed(format!(
            "EXPLODE: solo se soporta Polyline por ahora; no explotables: [{}]",
            join_ids(&offenders)
        )));
    }

    // Replace each polyline with pieces that inherit its style.
    let mut created = Vec::new();
    for (id, record, pieces) in planned {
        tx.remove_entity(id)?;
        for geometry in pieces {
            let mut piece = record.clone();
            piece.geometry = geometry;
            created.push(tx.add_entity(ContainerRef::ModelSpace, piece)?);
        }
    }
    Ok(created)
}

/// Returns exact line or arc geometry for each polyline segment.
fn explode_polyline(poly: &PolylineGeo) -> Vec<EntityGeometry> {
    poly.segments()
        .map(|seg| match seg {
            SegKind::Line { a, b } => EntityGeometry::Line(LineGeo::new(a, b)),
            SegKind::Arc(arc) => EntityGeometry::Arc(ArcGeo::new(
                arc.center,
                arc.radius,
                arc.start_angle,
                arc.end_angle,
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::entity::{
        Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PolyVertex,
    };
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn seed_geo(session: &mut Session, g: EntityGeometry) -> EntityId {
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
                        g,
                    ),
                )
            })
            .expect("seed")
            .value
    }

    #[test]
    fn explodes_polyline_into_lines_and_arcs() {
        let mut session = Session::new(Units::default());
        let poly = PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.0), // Straight segment to (10,0).
                PolyVertex::new(Point2::new(10.0, 0.0), 1.0), // Arc to (10,10).
                PolyVertex::new(Point2::new(10.0, 10.0), 0.0),
            ],
            false,
        );
        let id = seed_geo(&mut session, EntityGeometry::Polyline(poly));

        let created = session
            .transact("Explode", |tx| apply_explode(tx, &[id]))
            .expect("commits")
            .value;
        assert_eq!(created.len(), 2);
        let n_lines = created
            .iter()
            .filter(|&&id| {
                matches!(
                    session.document().entity(id).unwrap().0.geometry,
                    EntityGeometry::Line(_)
                )
            })
            .count();
        let n_arcs = created
            .iter()
            .filter(|&&id| {
                matches!(
                    session.document().entity(id).unwrap().0.geometry,
                    EntityGeometry::Arc(_)
                )
            })
            .count();
        assert_eq!((n_lines, n_arcs), (1, 1));
        assert!(session.document().entity(id).is_none());
    }

    #[test]
    fn non_polyline_is_rejected_atomically() {
        let mut session = Session::new(Units::default());
        let line = seed_geo(
            &mut session,
            EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(1.0, 1.0))),
        );
        let before = serde_json::to_string(session.document()).unwrap();
        let err = session
            .transact("Explode", |tx| apply_explode(tx, &[line]))
            .unwrap_err();
        match err {
            CmdError::Failed(msg) => {
                assert!(msg.contains("Polyline"), "mensaje: {msg}");
                assert!(
                    msg.contains(&line.raw().0.to_string()),
                    "lista el id: {msg}"
                );
            }
            other => panic!("esperaba Failed, fue {other:?}"),
        }
        assert_eq!(
            before,
            serde_json::to_string(session.document()).unwrap(),
            "rollback atómico: la línea sigue intacta"
        );
    }
}
