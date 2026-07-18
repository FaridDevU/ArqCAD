//! REVERSE atomically reverses polyline traversal order. Other geometry types have
//! no supported traversal direction.
//!
//! It has no standard PGP alias.
//!
//! # Bulge mapping
//!
//! A [`PolyVertex`] stores the bulge of its outgoing segment. Reversing traversal
//! reverses each arc sweep, so its bulge changes sign and moves to the new segment start.
//!
//! Formally, reversed vertex `w[j] = v[n-1-j]` receives
//! `-b[(n-2-j) mod n]`. This covers open and closed polylines; the final bulge of
//! an open polyline remains unused.

use af_model::TxContext;
use af_model::entity::{EntityGeometry, PolyVertex, PolylineGeo};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the REVERSE specification without aliases.
#[must_use]
pub fn reverse_spec() -> CommandSpec {
    CommandSpec::new("REVERSE", "Reverse", true, reverse_exec)
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Registers REVERSE.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(reverse_spec())
}

fn reverse_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();

    ctx.transact("Reverse", |tx| apply_reverse(tx, &ids))?;
    Ok(CommandOutcome::new())
}

/// Reverses validated model-space polylines atomically and rejects mixed geometry.
pub(crate) fn apply_reverse(tx: &mut TxContext<'_>, ids: &[EntityId]) -> Result<(), CmdError> {
    let records = validate_editable(tx, "REVERSE", ids)?;

    let not_polyline: Vec<EntityId> = records
        .iter()
        .filter(|(_, rec)| !matches!(rec.geometry, EntityGeometry::Polyline(_)))
        .map(|(id, _)| *id)
        .collect();
    if !not_polyline.is_empty() {
        return Err(CmdError::Failed(format!(
            "REVERSE: only polylines can be reversed; not a polyline: [{}]",
            not_polyline
                .iter()
                .map(|id| id.raw().0.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    for (id, record) in records {
        let EntityGeometry::Polyline(poly) = &record.geometry else {
            unreachable!("filtrado arriba: todos los ids validados son Polyline");
        };
        let reversed = EntityGeometry::Polyline(reverse_polyline(poly));
        tx.modify_entity(id, move |rec| rec.geometry = reversed)?;
    }
    Ok(())
}

/// Reverses `poly` and relocates each negated bulge to its new segment start.
fn reverse_polyline(poly: &PolylineGeo) -> PolylineGeo {
    let n = poly.vertices.len();
    let vertices = (0..n)
        .map(|j| {
            let pt = poly.vertices[n - 1 - j].pt;
            // Negate the original bulge traversed backward by this result segment.
            let src = (n + n - 2 - j) % n;
            PolyVertex::new(pt, -poly.vertices[src].bulge)
        })
        .collect();
    PolylineGeo::new(vertices, poly.closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::container::ContainerRef;
    use af_model::entity::{Color, EntityRecord, LineGeo, LineTypeRef, Lineweight};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn v(x: f64, y: f64, bulge: f64) -> PolyVertex {
        PolyVertex::new(Point2::new(x, y), bulge)
    }

    fn seed_polyline(session: &mut Session, geo: PolylineGeo) -> EntityId {
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
                        EntityGeometry::Polyline(geo),
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    /// Verifies that reversed segments traverse the same curves in reverse order.
    #[test]
    fn reverse_bulge_mapping_matches_hand_derivation() {
        let original = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.6), v(4.0, 0.0, -0.3), v(4.0, 4.0, 0.0)],
            false,
        );
        let reversed = reverse_polyline(&original);

        assert_eq!(reversed.vertices[0].pt, Point2::new(4.0, 4.0));
        assert_eq!(reversed.vertices[1].pt, Point2::new(4.0, 0.0));
        assert_eq!(reversed.vertices[2].pt, Point2::new(0.0, 0.0));
        assert_eq!(reversed.vertices[1].bulge, -0.6);
        assert_eq!(reversed.vertices[0].bulge, 0.3);
    }

    #[test]
    fn reverse_twice_is_identity() {
        let original = PolylineGeo::new(
            vec![v(0.0, 0.0, 0.6), v(4.0, 0.0, -0.3), v(4.0, 4.0, 0.9)],
            true,
        );
        let twice = reverse_polyline(&reverse_polyline(&original));
        assert_eq!(twice, original);
    }

    #[test]
    fn reverse_preserves_the_segments_traced_open() {
        use af_model::entity::EntityOps;
        let original = PolylineGeo::new(vec![v(0.0, 0.0, 1.0), v(2.0, 0.0, 0.0)], false);
        let reversed = reverse_polyline(&original);
        let on_arc = Point2::new(1.0, -1.0);
        assert!(original.hit(on_arc, 1e-6).unwrap() < 1e-9);
        assert!(reversed.hit(on_arc, 1e-6).unwrap() < 1e-9);
    }

    #[test]
    fn apply_reverse_rejects_non_polyline_without_mutating() {
        let mut session = Session::new(Units::default());
        let line_id = {
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
        };
        let before = serde_json::to_string(session.document()).unwrap();

        let err = session
            .transact("Reverse", |tx| apply_reverse(tx, &[line_id]))
            .unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    #[test]
    fn apply_reverse_changeset_modified_is_exactly_the_set() {
        let mut session = Session::new(Units::default());
        let id = seed_polyline(
            &mut session,
            PolylineGeo::new(vec![v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0)], false),
        );

        let out = session
            .transact("Reverse", |tx| apply_reverse(tx, &[id]))
            .expect("commits");
        let cs = out.change_set.expect("tx no vacía");
        assert_eq!(cs.modified(), &[id]);

        let rec = session.document().entity(id).unwrap().0;
        let EntityGeometry::Polyline(poly) = &rec.geometry else {
            panic!("esperaba polyline");
        };
        assert_eq!(poly.vertices[0].pt, Point2::new(10.0, 0.0));
        assert_eq!(poly.vertices[1].pt, Point2::new(0.0, 0.0));
    }
}
