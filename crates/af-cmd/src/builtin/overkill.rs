//! OVERKILL removes exact duplicates from an entity set in one transaction. Exact
//! means matching properties and tolerance-equal geometry.
//!
//! It has no standard PGP alias.
//!
//! # Scope
//!
//! Duplicate geometry must have the same type and parameters in the same order
//! within [`Tol::default`]. Overlap merging is intentionally not inferred.
//!
//! # Retention
//!
//! The earliest entity in draw order survives each duplicate group.

use af_math::Tol;
use af_model::TxContext;
use af_model::entity::{EntityGeometry, EntityRecord};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the OVERKILL specification without aliases.
#[must_use]
pub fn overkill_spec() -> CommandSpec {
    CommandSpec::new("OVERKILL", "Overkill", true, overkill_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Registers OVERKILL.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(overkill_spec())
}

fn overkill_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    let removed_count = ctx.transact("Overkill", |tx| apply_overkill(tx, &ids))?;
    let mut outcome = CommandOutcome::new();
    outcome.message = Some(format!(
        "OVERKILL: {removed_count} duplicate entit{} removed",
        if removed_count == 1 { "y" } else { "ies" }
    ));
    Ok(outcome)
}

/// Removes exact duplicates from `ids`, retaining the earliest in draw order, and
/// returns the removal count.
pub(crate) fn apply_overkill(tx: &mut TxContext<'_>, ids: &[EntityId]) -> Result<usize, CmdError> {
    let mut records = validate_editable(tx, "OVERKILL", ids)?;
    // Stable draw order makes the first member of each duplicate group authoritative.
    records.sort_by_key(|(id, _)| tx.doc().model_space().index_of(*id).unwrap_or(usize::MAX));

    let tol = Tol::default();
    let mut kept: Vec<&EntityRecord> = Vec::with_capacity(records.len());
    let mut to_erase: Vec<EntityId> = Vec::new();
    for (id, record) in &records {
        if kept.iter().any(|k| is_duplicate(k, record, &tol)) {
            to_erase.push(*id);
        } else {
            kept.push(record);
        }
    }

    let count = to_erase.len();
    for id in to_erase {
        tx.remove_entity(id)?;
    }
    Ok(count)
}

/// Returns whether all properties and geometry match within `tol`.
fn is_duplicate(a: &EntityRecord, b: &EntityRecord, tol: &Tol) -> bool {
    a.layer == b.layer
        && a.color == b.color
        && a.line_type == b.line_type
        && a.lineweight == b.lineweight
        && a.visible == b.visible
        && geometry_matches(&a.geometry, &b.geometry, tol)
}

/// Returns whether geometry type and ordered parameters match within `tol`.
fn geometry_matches(a: &EntityGeometry, b: &EntityGeometry, tol: &Tol) -> bool {
    match (a, b) {
        (EntityGeometry::Point(g1), EntityGeometry::Point(g2)) => {
            tol.points_coincide(g1.position, g2.position)
        }
        (EntityGeometry::Line(g1), EntityGeometry::Line(g2)) => {
            tol.points_coincide(g1.p1, g2.p1) && tol.points_coincide(g1.p2, g2.p2)
        }
        (EntityGeometry::Circle(g1), EntityGeometry::Circle(g2)) => {
            tol.points_coincide(g1.center, g2.center) && tol.approx_eq(g1.radius, g2.radius)
        }
        (EntityGeometry::Arc(g1), EntityGeometry::Arc(g2)) => {
            tol.points_coincide(g1.center, g2.center)
                && tol.approx_eq(g1.radius, g2.radius)
                && tol.angles_eq(g1.start_angle, g2.start_angle)
                && tol.angles_eq(g1.end_angle, g2.end_angle)
        }
        (EntityGeometry::Polyline(g1), EntityGeometry::Polyline(g2)) => {
            g1.closed == g2.closed
                && g1.vertices.len() == g2.vertices.len()
                && g1.vertices.iter().zip(g2.vertices.iter()).all(|(v1, v2)| {
                    tol.points_coincide(v1.pt, v2.pt) && tol.approx_eq(v1.bulge, v2.bulge)
                })
        }
        // Distinct geometry variants are never duplicates.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::container::ContainerRef;
    use af_model::entity::{Color, LineGeo, LineTypeRef, Lineweight, PointGeo};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn line_rec(session: &Session, p1: Point2, p2: Point2) -> EntityRecord {
        EntityRecord::new(
            ObjectId::NIL.into(),
            session.document().current_layer(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Line(LineGeo::new(p1, p2)),
        )
    }

    #[test]
    fn apply_overkill_borra_duplicados_exactos_y_conserva_el_mas_antiguo() {
        let mut session = Session::new(Units::default());
        let a = line_rec(&session, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
        let b = a.clone();
        let c = line_rec(&session, Point2::new(5.0, 5.0), Point2::new(6.0, 6.0));

        let ids = session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![
                    tx.add_entity(ContainerRef::ModelSpace, a)?,
                    tx.add_entity(ContainerRef::ModelSpace, b)?,
                    tx.add_entity(ContainerRef::ModelSpace, c)?,
                ])
            })
            .expect("seed commits")
            .value;

        let removed = session
            .transact("Overkill", |tx| apply_overkill(tx, &ids))
            .expect("commits")
            .value;
        assert_eq!(removed, 1);

        assert!(session.document().entity(ids[0]).is_some());
        assert!(session.document().entity(ids[1]).is_none());
        assert!(session.document().entity(ids[2]).is_some());
    }

    #[test]
    fn apply_overkill_no_confunde_geometrias_dentro_de_tolerancia_con_geometrias_distintas() {
        let mut session = Session::new(Units::default());
        let p1 = EntityRecord::new(
            ObjectId::NIL.into(),
            session.document().current_layer(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(0.0, 0.0))),
        );
        let mut p2 = p1.clone();
        if let EntityGeometry::Point(g) = &mut p2.geometry {
            g.position = Point2::new(1e-9, 0.0);
        }
        let p3 = EntityRecord::new(
            ObjectId::NIL.into(),
            session.document().current_layer(),
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(1.0, 0.0))),
        );

        let ids = session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![
                    tx.add_entity(ContainerRef::ModelSpace, p1)?,
                    tx.add_entity(ContainerRef::ModelSpace, p2)?,
                    tx.add_entity(ContainerRef::ModelSpace, p3)?,
                ])
            })
            .expect("seed commits")
            .value;

        let removed = session
            .transact("Overkill", |tx| apply_overkill(tx, &ids))
            .expect("commits")
            .value;
        assert_eq!(removed, 1);
        assert!(session.document().entity(ids[0]).is_some());
        assert!(session.document().entity(ids[1]).is_none());
        assert!(session.document().entity(ids[2]).is_some());
    }

    #[test]
    fn apply_overkill_sin_duplicados_no_borra_nada() {
        let mut session = Session::new(Units::default());
        let a = line_rec(&session, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
        let b = line_rec(&session, Point2::new(2.0, 2.0), Point2::new(3.0, 3.0));

        let ids = session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![
                    tx.add_entity(ContainerRef::ModelSpace, a)?,
                    tx.add_entity(ContainerRef::ModelSpace, b)?,
                ])
            })
            .expect("seed commits")
            .value;

        let removed = session
            .transact("Overkill", |tx| apply_overkill(tx, &ids))
            .expect("commits")
            .value;
        assert_eq!(removed, 0);
        assert!(session.document().entity(ids[0]).is_some());
        assert!(session.document().entity(ids[1]).is_some());
    }
}
