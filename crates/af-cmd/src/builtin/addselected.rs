//! ADDSELECTED creates a new entity with the type and properties of a reference
//! entity and geometry defined by command points. Creation is atomic.
//!
//! It has no standard PGP alias.
//!
//! # Type-specific points
//!
//! The reference's [`EntityGeometry`] variant determines how many `points` are
//! required:
//!
//! - `Point`: one point.
//! - `Line`: two endpoints.
//! - `Circle`: center and a point on the circumference.
//! - `Arc`: start, on-arc, and end points, using [`arc_from_three_points`].
//! - `Ellipse`: center and major-axis endpoint; `ratio` is inherited.
//! - `Polyline`: at least two points with optional bulges; `closed` is inherited.
//!
//! A mismatched point count fails without creating a transaction.

use af_math::Point2;
use af_math::angle::angle_of;
use af_model::entity::{
    CircleGeo, EllipseGeo, EntityGeometry, LineGeo, PointGeo, PolyVertex, PolylineGeo, RayGeo,
    SplineGeo, WipeoutGeo, XlineGeo,
};
use af_model::id::EntityId;
use af_model::{ContainerRef, TxContext};

use crate::args::ParsedArgs;
use crate::builtin::arc::arc_from_three_points;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the ADDSELECTED command specification without aliases.
#[must_use]
pub fn addselected_spec() -> CommandSpec {
    CommandSpec::new("ADDSELECTED", "Addselected", true, addselected_exec)
        .param(ParamSpec::required("reference", ParamType::EntitySet))
        .param(ParamSpec::required("points", ParamType::Path))
}

/// Registers ADDSELECTED.
///
/// # Errors
/// Returns [`RegisterError`] when its name collides with a registered command.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(addselected_spec())
}

fn addselected_exec(
    ctx: &mut CommandCtx<'_>,
    args: ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let reference_ids = args
        .entity_set("reference")
        .ok_or_else(|| CmdError::MissingParam("reference".to_string()))?;
    let &[reference] = reference_ids else {
        return Err(CmdError::Failed(format!(
            "ADDSELECTED: 'reference' must be exactly one entity, got {}",
            reference_ids.len()
        )));
    };
    let points: Vec<(Point2, f64)> = args
        .path("points")
        .ok_or_else(|| CmdError::MissingParam("points".to_string()))?
        .to_vec();

    let id = ctx.transact("Addselected", |tx| {
        apply_addselected(tx, reference, &points)
    })?;
    Ok(CommandOutcome::created(vec![id]))
}

/// Validates `reference`, builds matching geometry from `points`, and atomically
/// creates the new entity in `tx`.
pub(crate) fn apply_addselected(
    tx: &mut TxContext<'_>,
    reference: EntityId,
    points: &[(Point2, f64)],
) -> Result<EntityId, CmdError> {
    let mut records = validate_editable(tx, "ADDSELECTED", &[reference])?;
    let (_, mut record) = records.remove(0);
    record.geometry = geometry_from_points(&record.geometry, points)?;
    Ok(tx.add_entity(ContainerRef::ModelSpace, record)?)
}

/// Rebuilds geometry matching `reference` from type-specific `points`.
fn geometry_from_points(
    reference: &EntityGeometry,
    points: &[(Point2, f64)],
) -> Result<EntityGeometry, CmdError> {
    let pts: Vec<Point2> = points.iter().map(|&(p, _)| p).collect();
    match reference {
        EntityGeometry::Point(_) => match pts[..] {
            [p] => Ok(EntityGeometry::Point(PointGeo::new(p))),
            _ => Err(wrong_point_count("Point", 1, pts.len())),
        },
        EntityGeometry::Line(_) => match pts[..] {
            [p1, p2] => Ok(EntityGeometry::Line(LineGeo::new(p1, p2))),
            _ => Err(wrong_point_count("Line", 2, pts.len())),
        },
        EntityGeometry::Circle(_) => match pts[..] {
            [center, on_circle] => Ok(EntityGeometry::Circle(CircleGeo::new(
                center,
                center.dist(on_circle),
            ))),
            _ => Err(wrong_point_count("Circle", 2, pts.len())),
        },
        EntityGeometry::Arc(_) => match pts[..] {
            [p1, p2, p3] => Ok(EntityGeometry::Arc(arc_from_three_points(p1, p2, p3)?)),
            _ => Err(wrong_point_count("Arc", 3, pts.len())),
        },
        // Preserve the ellipse ratio while replacing its center and major axis.
        EntityGeometry::Ellipse(e) => match pts[..] {
            [center, axis_end] => {
                let major = axis_end - center;
                Ok(EntityGeometry::Ellipse(EllipseGeo::new(
                    center,
                    major.norm(),
                    e.ratio,
                    angle_of(major),
                    0.0,
                    core::f64::consts::TAU,
                )))
            }
            _ => Err(wrong_point_count("Ellipse", 2, pts.len())),
        },
        EntityGeometry::Polyline(poly) => {
            if pts.len() < 2 {
                return Err(CmdError::Failed(format!(
                    "ADDSELECTED: Polyline needs >= 2 points, got {}",
                    pts.len()
                )));
            }
            let vertices = points
                .iter()
                .map(|&(pt, bulge)| PolyVertex::new(pt, bulge))
                .collect();
            Ok(EntityGeometry::Polyline(PolylineGeo::new(
                vertices,
                poly.closed,
            )))
        }
        // Infinite curves use a base point and a through point.
        EntityGeometry::Xline(_) => match pts[..] {
            [p1, p2] => Ok(EntityGeometry::Xline(XlineGeo::through(p1, p2))),
            _ => Err(wrong_point_count("Xline", 2, pts.len())),
        },
        EntityGeometry::Ray(_) => match pts[..] {
            [p1, p2] => Ok(EntityGeometry::Ray(RayGeo::through(p1, p2))),
            _ => Err(wrong_point_count("Ray", 2, pts.len())),
        },
        EntityGeometry::Spline(sp) => {
            let min = if sp.closed { 3 } else { 2 };
            if pts.len() < min {
                return Err(CmdError::Failed(format!(
                    "ADDSELECTED: Spline needs >= {min} points, got {}",
                    pts.len()
                )));
            }
            Ok(EntityGeometry::Spline(SplineGeo::new(pts, sp.closed)))
        }
        EntityGeometry::Wipeout(_) => {
            if pts.len() < 3 {
                return Err(CmdError::Failed(format!(
                    "ADDSELECTED: Wipeout needs >= 3 points, got {}",
                    pts.len()
                )));
            }
            Ok(EntityGeometry::Wipeout(WipeoutGeo::new(pts)))
        }
    }
}

/// Builds a consistent point-count error for geometry type `kind`.
fn wrong_point_count(kind: &str, expected: usize, got: usize) -> CmdError {
    CmdError::Failed(format!(
        "ADDSELECTED: {kind} needs exactly {expected} point(s), got {got}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_model::container::ContainerRef as CRef;
    use af_model::entity::{AciColor, Color, EntityRecord, LineTypeRef, Lineweight};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};

    fn seed(session: &mut Session, geometry: EntityGeometry) -> EntityId {
        let layer = session.document().current_layer();
        session
            .transact("seed", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(
                    CRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        layer,
                        Color::Aci(AciColor::new(2).unwrap()),
                        LineTypeRef::ByLayer,
                        Lineweight::ByLayer,
                        geometry,
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    #[test]
    fn addselected_line_creates_new_entity_with_same_props_new_geometry() {
        let mut session = Session::new(Units::default());
        let reference = seed(
            &mut session,
            EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))),
        );
        let points = vec![(Point2::new(5.0, 5.0), 0.0), (Point2::new(6.0, 8.0), 0.0)];

        let out = session
            .transact("Addselected", |tx| {
                apply_addselected(tx, reference, &points)
            })
            .expect("commits");
        let new_id = out.value;
        assert_ne!(new_id, reference);

        let rec = session.document().entity(new_id).unwrap().0;
        assert_eq!(rec.color, Color::Aci(AciColor::new(2).unwrap()));
        match &rec.geometry {
            EntityGeometry::Line(g) => {
                assert_eq!(g.p1, Point2::new(5.0, 5.0));
                assert_eq!(g.p2, Point2::new(6.0, 8.0));
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }
        let source = session.document().entity(reference).unwrap().0;
        match &source.geometry {
            EntityGeometry::Line(g) => {
                assert_eq!(g.p1, Point2::new(0.0, 0.0));
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }
    }

    #[test]
    fn addselected_circle_uses_center_plus_point_on_circumference() {
        let mut session = Session::new(Units::default());
        let reference = seed(
            &mut session,
            EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 1.0)),
        );
        let points = vec![(Point2::new(2.0, 2.0), 0.0), (Point2::new(5.0, 2.0), 0.0)];

        let new_id = session
            .transact("Addselected", |tx| {
                apply_addselected(tx, reference, &points)
            })
            .expect("commits")
            .value;

        match &session.document().entity(new_id).unwrap().0.geometry {
            EntityGeometry::Circle(g) => {
                assert_eq!(g.center, Point2::new(2.0, 2.0));
                assert!((g.radius - 3.0).abs() < 1e-9);
            }
            other => panic!("esperaba círculo, fue {other:?}"),
        }
    }

    #[test]
    fn addselected_polyline_inherits_closed_flag_and_uses_given_bulges() {
        let mut session = Session::new(Units::default());
        let reference = seed(
            &mut session,
            EntityGeometry::Polyline(PolylineGeo::new(
                vec![
                    PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
                    PolyVertex::new(Point2::new(1.0, 0.0), 0.0),
                ],
                true,
            )),
        );
        let points = vec![
            (Point2::new(10.0, 10.0), 0.5),
            (Point2::new(20.0, 10.0), 0.0),
            (Point2::new(20.0, 20.0), 0.0),
        ];

        let new_id = session
            .transact("Addselected", |tx| {
                apply_addselected(tx, reference, &points)
            })
            .expect("commits")
            .value;

        match &session.document().entity(new_id).unwrap().0.geometry {
            EntityGeometry::Polyline(g) => {
                assert!(g.closed, "hereda closed=true de la referencia");
                assert_eq!(g.vertices.len(), 3);
                assert_eq!(g.vertices[0].bulge, 0.5);
            }
            other => panic!("esperaba polyline, fue {other:?}"),
        }
    }

    #[test]
    fn addselected_wrong_point_count_fails_without_a_transaction() {
        let mut session = Session::new(Units::default());
        let reference = seed(
            &mut session,
            EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))),
        );
        let before = serde_json::to_string(session.document()).unwrap();

        let points = vec![(Point2::new(5.0, 5.0), 0.0)];
        let err = session
            .transact("Addselected", |tx| {
                apply_addselected(tx, reference, &points)
            })
            .unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }
}
