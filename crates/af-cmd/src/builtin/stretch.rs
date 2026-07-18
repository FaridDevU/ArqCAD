//! STRETCH (`S`) moves vertices inside a crossing window by `to - base` in one transaction.
//!
//! # Geometry behavior
//!
//! Line endpoints and polyline vertices move independently. Points, circles, and
//! ellipses move as a whole when their defining center/position is inside. Arcs move
//! only when both endpoints are inside; one-endpoint arc fitting is unsupported.
//!
//! The entire set and unsupported cases are validated before mutation.

use af_math::{BBox, Point2, Vec2};
use af_model::TxContext;
use af_model::entity::{
    ArcGeo, CircleGeo, EntityGeometry, LineGeo, PointGeo, PolyVertex, PolylineGeo, RayGeo,
    SplineGeo, WipeoutGeo, XlineGeo,
};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the STRETCH specification with alias `S`.
///
/// The crossing window uses `corner1` and `corner2`; displacement is `to - base`.
#[must_use]
pub fn stretch_spec() -> CommandSpec {
    CommandSpec::new("STRETCH", "Stretch", true, stretch_exec)
        .alias("S")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required("corner1", ParamType::Point))
        .param(ParamSpec::required("corner2", ParamType::Point))
        .param(ParamSpec::required("base", ParamType::Point))
        .param(ParamSpec::required("to", ParamType::Point))
}

/// Registers STRETCH.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(stretch_spec())
}

/// Moves captured vertices by `to - base` in one transaction.
fn stretch_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let c1 = args
        .point("corner1")
        .ok_or_else(|| CmdError::MissingParam("corner1".to_string()))?;
    let c2 = args
        .point("corner2")
        .ok_or_else(|| CmdError::MissingParam("corner2".to_string()))?;
    let base = args
        .point("base")
        .ok_or_else(|| CmdError::MissingParam("base".to_string()))?;
    let to = args
        .point("to")
        .ok_or_else(|| CmdError::MissingParam("to".to_string()))?;
    let win = BBox::new(c1, c2);
    let delta = to - base;

    ctx.transact("Stretch", |tx| apply_stretch(tx, &ids, win, delta))?;
    Ok(CommandOutcome::new())
}

/// Precomputes every stretched geometry before modifying entities in place.
pub(crate) fn apply_stretch(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    win: BBox,
    delta: Vec2,
) -> Result<(), CmdError> {
    let records = validate_editable(tx, "STRETCH", ids)?;
    let mut planned = Vec::with_capacity(records.len());
    for (id, record) in records {
        let geometry = stretch_geometry(&record.geometry, win, delta)?;
        planned.push((id, geometry));
    }
    for (id, geometry) in planned {
        tx.modify_entity(id, move |record| record.geometry = geometry)?;
    }
    Ok(())
}

/// Moves `p` by `delta` when it lies inside `win`.
#[inline]
fn stretch_pt(p: Point2, win: BBox, delta: Vec2) -> Point2 {
    if win.contains_point(p) { p + delta } else { p }
}

/// Returns `geo` stretched within `win` by `delta`.
///
/// # Errors
/// Returns [`CmdError::Failed`] for an arc with exactly one captured endpoint.
fn stretch_geometry(
    geo: &EntityGeometry,
    win: BBox,
    delta: Vec2,
) -> Result<EntityGeometry, CmdError> {
    // Keep this match exhaustive so new geometry requires explicit behavior.
    Ok(match geo {
        EntityGeometry::Line(g) => EntityGeometry::Line(LineGeo::new(
            stretch_pt(g.p1, win, delta),
            stretch_pt(g.p2, win, delta),
        )),
        EntityGeometry::Point(g) => {
            EntityGeometry::Point(PointGeo::new(stretch_pt(g.position, win, delta)))
        }
        EntityGeometry::Circle(g) => {
            if win.contains_point(g.center) {
                EntityGeometry::Circle(CircleGeo::new(g.center + delta, g.radius))
            } else {
                geo.clone()
            }
        }
        // ponytail: ellipses move as a whole like circles; add axis editing only
        // when partial ellipse stretching is required.
        EntityGeometry::Ellipse(g) => {
            if win.contains_point(g.center) {
                let mut moved = *g;
                moved.center = g.center + delta;
                EntityGeometry::Ellipse(moved)
            } else {
                geo.clone()
            }
        }
        EntityGeometry::Polyline(g) => {
            let vertices = g
                .vertices
                .iter()
                .map(|v| PolyVertex::new(stretch_pt(v.pt, win, delta), v.bulge))
                .collect();
            EntityGeometry::Polyline(PolylineGeo::new(vertices, g.closed))
        }
        EntityGeometry::Spline(g) => {
            // Refit the spline after moving captured fit points.
            let fit_points = g
                .fit_points
                .iter()
                .map(|&p| stretch_pt(p, win, delta))
                .collect();
            EntityGeometry::Spline(SplineGeo::new(fit_points, g.closed))
        }
        EntityGeometry::Arc(g) => {
            let seg = g.arc_seg();
            let start_in = win.contains_point(seg.start_point());
            let end_in = win.contains_point(seg.end_point());
            match (start_in, end_in) {
                (false, false) => geo.clone(),
                // Whole-arc translation preserves radius and angles.
                (true, true) => EntityGeometry::Arc(ArcGeo::new(
                    g.center + delta,
                    g.radius,
                    g.start_angle,
                    g.end_angle,
                )),
                _ => {
                    return Err(CmdError::Failed(
                        "STRETCH: un arco con un solo extremo dentro de la ventana no está \
                         soportado; selecciona ambos extremos para moverlo entero, o \
                         ninguno para dejarlo fijo"
                            .to_string(),
                    ));
                }
            }
        }
        // Infinite curves translate as a whole when their defining point is captured.
        EntityGeometry::Xline(g) => {
            if win.contains_point(g.point) {
                EntityGeometry::Xline(XlineGeo::new(g.point + delta, g.direction))
            } else {
                geo.clone()
            }
        }
        EntityGeometry::Ray(g) => {
            if win.contains_point(g.origin) {
                EntityGeometry::Ray(RayGeo::new(g.origin + delta, g.direction))
            } else {
                geo.clone()
            }
        }
        // Move captured mask vertices while keeping the polygon closed.
        EntityGeometry::Wipeout(g) => {
            let points = g
                .points
                .iter()
                .map(|&p| stretch_pt(p, win, delta))
                .collect();
            EntityGeometry::Wipeout(WipeoutGeo::new(points))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Vec2;
    use af_model::entity::{
        ArcGeo, CircleGeo, Color, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
        PolyVertex, PolylineGeo,
    };
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{ContainerRef, Session, TxError};
    use core::f64::consts::FRAC_PI_2;

    /// Returns a test window containing `(10,0)` but not `(0,0)`.
    fn window_around_10_0() -> BBox {
        BBox::new(Point2::new(9.0, -1.0), Point2::new(11.0, 1.0))
    }

    #[test]
    fn line_stretches_only_the_contained_endpoint() {
        let g = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)));
        let out = stretch_geometry(&g, window_around_10_0(), Vec2::new(0.0, 5.0)).unwrap();
        match out {
            EntityGeometry::Line(l) => {
                assert_eq!(l.p1, Point2::new(0.0, 0.0), "extremo fuera queda fijo");
                assert_eq!(l.p2, Point2::new(10.0, 5.0), "extremo dentro se desplaza");
            }
            other => panic!("esperaba línea, fue {other:?}"),
        }
    }

    #[test]
    fn point_moves_iff_inside() {
        let inside = EntityGeometry::Point(PointGeo::new(Point2::new(10.0, 0.0)));
        let outside = EntityGeometry::Point(PointGeo::new(Point2::new(0.0, 0.0)));
        let d = Vec2::new(1.0, 2.0);
        assert_eq!(
            stretch_geometry(&inside, window_around_10_0(), d).unwrap(),
            EntityGeometry::Point(PointGeo::new(Point2::new(11.0, 2.0)))
        );
        assert_eq!(
            stretch_geometry(&outside, window_around_10_0(), d).unwrap(),
            outside
        );
    }

    #[test]
    fn circle_moves_whole_when_center_inside_else_fixed() {
        let inside = EntityGeometry::Circle(CircleGeo::new(Point2::new(10.0, 0.0), 3.0));
        let outside = EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 3.0));
        let d = Vec2::new(0.0, 5.0);
        match stretch_geometry(&inside, window_around_10_0(), d).unwrap() {
            EntityGeometry::Circle(c) => {
                assert_eq!(c.center, Point2::new(10.0, 5.0));
                assert_eq!(c.radius, 3.0, "el radio no cambia (no se vuelve elipse)");
            }
            other => panic!("esperaba círculo, fue {other:?}"),
        }
        assert_eq!(
            stretch_geometry(&outside, window_around_10_0(), d).unwrap(),
            outside
        );
    }

    #[test]
    fn polyline_stretches_contained_vertices_keeps_bulge() {
        let g = EntityGeometry::Polyline(PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
                PolyVertex::new(Point2::new(10.0, 0.0), 0.5),
            ],
            false,
        ));
        match stretch_geometry(&g, window_around_10_0(), Vec2::new(0.0, 5.0)).unwrap() {
            EntityGeometry::Polyline(p) => {
                assert_eq!(p.vertices[0].pt, Point2::new(0.0, 0.0));
                assert_eq!(p.vertices[1].pt, Point2::new(10.0, 5.0));
                assert_eq!(p.vertices[1].bulge, 0.5, "bulge conservado");
                assert!(!p.closed);
            }
            other => panic!("esperaba polyline, fue {other:?}"),
        }
    }

    #[test]
    fn arc_translates_when_both_endpoints_inside() {
        let g = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 1.0, 0.0, FRAC_PI_2));
        let big = BBox::new(Point2::new(-2.0, -2.0), Point2::new(2.0, 2.0));
        match stretch_geometry(&g, big, Vec2::new(5.0, 0.0)).unwrap() {
            EntityGeometry::Arc(a) => {
                assert_eq!(a.center, Point2::new(5.0, 0.0));
                assert_eq!(a.radius, 1.0);
                assert_eq!(a.start_angle, 0.0);
                assert_eq!(a.end_angle, FRAC_PI_2);
            }
            other => panic!("esperaba arco, fue {other:?}"),
        }
    }

    #[test]
    fn arc_with_one_endpoint_inside_is_deferred_error() {
        let g = EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 1.0, 0.0, FRAC_PI_2));
        let win = BBox::new(Point2::new(0.5, -0.5), Point2::new(1.5, 0.5));
        let err = stretch_geometry(&g, win, Vec2::new(1.0, 0.0)).unwrap_err();
        match err {
            CmdError::Failed(msg) => assert!(msg.contains("arco"), "mensaje: {msg}"),
            other => panic!("esperaba Failed, fue {other:?}"),
        }
    }

    #[test]
    fn apply_stretch_is_atomic_on_arc_error() {
        let mut session = Session::new(Units::default());
        let layer = session.document().current_layer();
        let rec = |g| {
            EntityRecord::new(
                ObjectId::NIL.into(),
                layer,
                Color::ByLayer,
                LineTypeRef::ByLayer,
                Lineweight::ByLayer,
                g,
            )
        };
        let ids = session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![
                    tx.add_entity(
                        ContainerRef::ModelSpace,
                        rec(EntityGeometry::Line(LineGeo::new(
                            Point2::new(10.0, 0.0),
                            Point2::new(20.0, 0.0),
                        ))),
                    )?,
                    tx.add_entity(
                        ContainerRef::ModelSpace,
                        rec(EntityGeometry::Arc(ArcGeo::new(
                            Point2::ORIGIN,
                            1.0,
                            0.0,
                            FRAC_PI_2,
                        ))),
                    )?,
                ])
            })
            .expect("seed")
            .value;
        let before = serde_json::to_string(session.document()).unwrap();

        let win = BBox::new(Point2::new(0.5, -0.5), Point2::new(11.0, 0.5));
        let out = session.transact("Stretch", |tx| {
            apply_stretch(tx, &ids, win, Vec2::new(0.0, 5.0))
        });
        assert!(out.is_err(), "el arco-medio debe abortar");
        assert_eq!(
            before,
            serde_json::to_string(session.document()).unwrap(),
            "rollback atómico: nada se estiró"
        );
    }
}
