//! End-to-end TRIM, EXTEND, FILLET, and OFFSET tests.
//!
//! Covers quick mode, aliases, transaction contracts, supported geometry, previews,
//! and numerical tangent/offset properties.

use af_cmd::builtin::modify;
use af_cmd::{CmdError, CommandRegistry};
use af_geom::bulge::bulge_to_arc;
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    ArcGeo, CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PolyVertex, PolylineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::json;

// ---- Helpers ----------------------------------------------------------------

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    modify::register(&mut reg).expect("register modify commands");
    reg
}

fn mk_record(layer: LayerId, geometry: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

fn seed(session: &mut Session, geoms: Vec<EntityGeometry>) -> Vec<EntityId> {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
            geoms
                .into_iter()
                .map(|g| tx.add_entity(ContainerRef::ModelSpace, mk_record(layer, g)))
                .collect()
        })
        .expect("seed commits")
        .value
}

fn line(a: [f64; 2], b: [f64; 2]) -> EntityGeometry {
    EntityGeometry::Line(LineGeo::new(
        Point2::new(a[0], a[1]),
        Point2::new(b[0], b[1]),
    ))
}

fn ids_json(ids: &[EntityId]) -> Vec<u64> {
    ids.iter().map(|id| id.raw().0).collect()
}

fn geom(session: &Session, id: EntityId) -> EntityGeometry {
    session
        .document()
        .entity(id)
        .expect("entity present")
        .0
        .geometry
        .clone()
}

fn as_line(g: &EntityGeometry) -> LineGeo {
    match g {
        EntityGeometry::Line(l) => *l,
        other => panic!("esperaba línea, fue {other:?}"),
    }
}

fn arc_geo(center: [f64; 2], r: f64, start: f64, end: f64) -> EntityGeometry {
    EntityGeometry::Arc(ArcGeo::new(
        Point2::new(center[0], center[1]),
        r,
        start,
        end,
    ))
}

fn circle_geo(center: [f64; 2], r: f64) -> EntityGeometry {
    EntityGeometry::Circle(CircleGeo::new(Point2::new(center[0], center[1]), r))
}

fn as_arc(g: &EntityGeometry) -> ArcGeo {
    match g {
        EntityGeometry::Arc(a) => *a,
        other => panic!("esperaba arco, fue {other:?}"),
    }
}

fn poly(verts: &[(f64, f64, f64)], closed: bool) -> EntityGeometry {
    EntityGeometry::Polyline(PolylineGeo::new(
        verts
            .iter()
            .map(|&(x, y, b)| PolyVertex::new(Point2::new(x, y), b))
            .collect(),
        closed,
    ))
}

fn as_poly(g: &EntityGeometry) -> PolylineGeo {
    match g {
        EntityGeometry::Polyline(p) => p.clone(),
        other => panic!("esperaba polilínea, fue {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn close_pt(a: Point2, b: Point2) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

// ============================================================================
// TRIM
// ============================================================================

/// Verifies that TRIM removes the picked middle interval and leaves two lines.
#[test]
fn trim_splits_a_line_crossed_by_two_edges() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            line([0.0, 0.0], [10.0, 0.0]), // Target.
            line([3.0, -5.0], [3.0, 5.0]), // Intersection at x=3.
            line([7.0, -5.0], [7.0, 5.0]), // Intersection at x=7.
        ],
    );
    let (target, cut1, cut2) = (ids[0], ids[1], ids[2]);

    let out = reg
        .execute(
            &mut session,
            "TRIM",
            &json!({
                "edges": ids_json(&[cut1, cut2]),
                "target": [target.raw().0],
                "pick": [5.0, 0.0],
            }),
        )
        .expect("trim succeeds");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(
        out.created.len(),
        1,
        "el segundo tramo es una entidad nueva"
    );

    let kept = as_line(&geom(&session, target));
    assert!(close_pt(kept.p1, Point2::new(0.0, 0.0)));
    assert!(close_pt(kept.p2, Point2::new(3.0, 0.0)));

    let extra = as_line(&geom(&session, out.created[0]));
    assert!(close_pt(extra.p1, Point2::new(7.0, 0.0)));
    assert!(close_pt(extra.p2, Point2::new(10.0, 0.0)));
}

/// Verifies quick mode with every other entity as a cutting edge.
#[test]
fn trim_quick_mode_uses_all_entities_as_edges() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([4.0, -5.0], [4.0, 5.0]), // Unique intersection at x=4.
        ],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "TRIM",
        &json!({ "target": [target.raw().0], "pick": [1.0, 0.0] }),
    )
    .expect("trim quick succeeds");

    let kept = as_line(&geom(&session, target));
    assert!(close_pt(kept.p1, Point2::new(4.0, 0.0)));
    assert!(close_pt(kept.p2, Point2::new(10.0, 0.0)));
}

/// Verifies that trimming a circle converts its complement to an arc.
#[test]
fn trim_circle_with_two_cuts_becomes_an_arc() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 1.0)),
            line([-2.0, 0.0], [2.0, 0.0]), // Intersects at (±1, 0): angles 0 and π.
        ],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "TRIM",
        &json!({ "target": [target.raw().0], "pick": [0.0, 1.0] }),
    )
    .expect("trim circle succeeds");

    let EntityGeometry::Arc(arc) = geom(&session, target) else {
        panic!("un círculo cortado debe volverse Arc");
    };
    assert!(close(arc.radius, 1.0));
    assert!(close_pt(arc.center, Point2::new(0.0, 0.0)));
    assert!(close(arc.arc_seg().sweep(), std::f64::consts::PI));
    assert!(close_pt(arc.arc_seg().midpoint(), Point2::new(0.0, -1.0)));
}

/// Verifies that a missing cut fails without mutation.
#[test]
fn trim_without_any_crossing_errors_and_makes_no_transaction() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([0.0, 5.0], [10.0, 5.0]), // Parallel: does not intersect.
        ],
    );
    let before = serde_json::to_string(session.document()).unwrap();
    let err = reg
        .execute(
            &mut session,
            "TRIM",
            &json!({ "target": [ids[0].raw().0], "pick": [5.0, 0.0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

// ============================================================================
// Polyline TRIM
// ============================================================================

/// Verifies middle trimming of an open polyline into two pieces.
#[test]
fn trim_open_polyline_in_the_middle_splits_in_two() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(
                &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)],
                false,
            ),
            line([3.0, -5.0], [3.0, 5.0]),
            line([7.0, -5.0], [7.0, 5.0]),
        ],
    );
    let (target, cut1, cut2) = (ids[0], ids[1], ids[2]);

    let out = reg
        .execute(
            &mut session,
            "TRIM",
            &json!({
                "edges": ids_json(&[cut1, cut2]),
                "target": [target.raw().0],
                "pick": [5.0, 0.0],
            }),
        )
        .expect("trim poly succeeds");
    assert!(out.tx_seq.is_some(), "exactamente 1 tx");
    assert_eq!(
        out.created.len(),
        1,
        "el segundo tramo es una entidad nueva"
    );

    let kept = as_poly(&geom(&session, target));
    assert!(!kept.closed);
    assert_eq!(kept.vertices.len(), 2);
    assert!(close_pt(kept.vertices[0].pt, Point2::new(0.0, 0.0)));
    assert!(close_pt(kept.vertices[1].pt, Point2::new(3.0, 0.0)));

    let extra = as_poly(&geom(&session, out.created[0]));
    assert_eq!(extra.vertices.len(), 3);
    assert!(close_pt(extra.vertices[0].pt, Point2::new(7.0, 0.0)));
    assert!(close_pt(extra.vertices[1].pt, Point2::new(10.0, 0.0)));
    assert!(close_pt(extra.vertices[2].pt, Point2::new(10.0, 10.0)));
}

/// Verifies endpoint trimming of an open polyline into one piece.
#[test]
fn trim_open_polyline_at_an_end_keeps_one_piece() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(
                &[(0.0, 0.0, 0.0), (10.0, 0.0, 0.0), (10.0, 10.0, 0.0)],
                false,
            ),
            line([3.0, -5.0], [3.0, 5.0]), // Unique intersection at x=3.
        ],
    );
    let target = ids[0];

    let out = reg
        .execute(
            &mut session,
            "TRIM",
            &json!({
                "edges": ids_json(&[ids[1]]),
                "target": [target.raw().0],
                "pick": [1.0, 0.0],
            }),
        )
        .expect("trim poly end succeeds");
    assert!(out.created.is_empty(), "recorte en extremo: 1 sola pieza");

    let kept = as_poly(&geom(&session, target));
    assert_eq!(kept.vertices.len(), 3);
    assert!(close_pt(kept.vertices[0].pt, Point2::new(3.0, 0.0)));
    assert!(close_pt(kept.vertices[1].pt, Point2::new(10.0, 0.0)));
    assert!(close_pt(kept.vertices[2].pt, Point2::new(10.0, 10.0)));
}

/// Verifies that trimming a closed polyline opens its remainder.
#[test]
fn trim_closed_polyline_opens_it() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(
                &[
                    (0.0, 0.0, 0.0),
                    (10.0, 0.0, 0.0),
                    (10.0, 10.0, 0.0),
                    (0.0, 10.0, 0.0),
                ],
                true,
            ),
            line([3.0, -5.0], [3.0, 5.0]),
            line([7.0, -5.0], [7.0, 5.0]),
        ],
    );
    let target = ids[0];

    let out = reg
        .execute(
            &mut session,
            "TRIM",
            &json!({
                "edges": ids_json(&[ids[1], ids[2]]),
                "target": [target.raw().0],
                "pick": [5.0, 0.0],
            }),
        )
        .expect("trim closed poly succeeds");
    assert!(
        out.created.is_empty(),
        "una cerrada recortada queda en 1 pieza"
    );

    let opened = as_poly(&geom(&session, target));
    assert!(!opened.closed, "la polilínea cerrada se abre al recortar");
    assert_eq!(opened.vertices.len(), 6);
    assert!(close_pt(opened.vertices[0].pt, Point2::new(7.0, 0.0)));
    assert!(close_pt(opened.vertices[5].pt, Point2::new(3.0, 0.0)));
    assert!(close_pt(opened.vertices[2].pt, Point2::new(10.0, 10.0)));
}

/// Verifies that trimming a bulged segment preserves its supporting circle.
#[test]
fn trim_polyline_arc_segment_keeps_cut_on_edge_and_circle() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], false),
            line([-2.0, -0.5], [2.0, -0.5]),
        ],
    );
    let target = ids[0];

    let out = reg
        .execute(
            &mut session,
            "TRIM",
            &json!({
                "edges": ids_json(&[ids[1]]),
                "target": [target.raw().0],
                "pick": [1.0, -1.0], // Bottom of the arc, between both intersections.
            }),
        )
        .expect("trim arc segment succeeds");
    assert_eq!(out.created.len(), 1, "recorte en medio del arco: 2 piezas");

    let center = Point2::new(1.0, 0.0);
    let piece1 = as_poly(&geom(&session, target)); // [(0,0,+b),(cut1,0)]
    let piece2 = as_poly(&geom(&session, out.created[0])); // [(cut2,+b),(2,0,0)]

    let cut1 = piece1.vertices[1].pt;
    let cut2 = piece2.vertices[0].pt;
    assert!(close(cut1.y, -0.5), "cut1 fuera de la arista: {cut1:?}");
    assert!(close(cut2.y, -0.5), "cut2 fuera de la arista: {cut2:?}");
    assert!(close(cut1.dist(center), 1.0));
    assert!(close(cut2.dist(center), 1.0));

    assert!(piece1.vertices[0].bulge > 0.0);
    assert!(piece2.vertices[0].bulge > 0.0);
    let a1 = bulge_to_arc(piece1.vertices[0].pt, cut1, piece1.vertices[0].bulge).unwrap();
    let a2 = bulge_to_arc(cut2, piece2.vertices[1].pt, piece2.vertices[0].bulge).unwrap();
    assert!(close_pt(a1.center, center) && close(a1.radius, 1.0));
    assert!(close_pt(a2.center, center) && close(a2.radius, 1.0));
}

// ============================================================================
// Polyline EXTEND
// ============================================================================

/// Verifies extension of an open polyline's final straight segment.
#[test]
fn extend_polyline_last_straight_segment() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(&[(0.0, 0.0, 0.0), (5.0, 0.0, 0.0), (5.0, 5.0, 0.0)], false),
            line([0.0, 10.0], [10.0, 10.0]), // Boundary at y=10.
        ],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "EXTEND",
        &json!({
            "edges": ids_json(&[ids[1]]),
            "target": [target.raw().0],
            "pick": [5.0, 4.9], // Near the final vertex.
        }),
    )
    .expect("extend last straight succeeds");

    let p = as_poly(&geom(&session, target));
    assert!(close_pt(p.vertices[0].pt, Point2::new(0.0, 0.0)));
    assert!(close_pt(p.vertices[1].pt, Point2::new(5.0, 0.0)));
    assert!(
        close_pt(p.vertices[2].pt, Point2::new(5.0, 10.0)),
        "el vértice final llega al límite: {:?}",
        p.vertices[2].pt
    );
}

/// Verifies extension of an open polyline's first straight segment.
#[test]
fn extend_polyline_first_straight_segment() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(&[(0.0, 0.0, 0.0), (5.0, 0.0, 0.0), (5.0, 5.0, 0.0)], false),
            line([-10.0, -5.0], [-10.0, 5.0]), // Boundary at x=-10.
        ],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "EXTEND",
        &json!({
            "edges": ids_json(&[ids[1]]),
            "target": [target.raw().0],
            "pick": [0.1, 0.0], // Near the initial vertex.
        }),
    )
    .expect("extend first straight succeeds");

    let p = as_poly(&geom(&session, target));
    assert!(
        close_pt(p.vertices[0].pt, Point2::new(-10.0, 0.0)),
        "el vértice inicial llega al límite: {:?}",
        p.vertices[0].pt
    );
    assert!(close_pt(p.vertices[1].pt, Point2::new(5.0, 0.0)));
    assert!(close_pt(p.vertices[2].pt, Point2::new(5.0, 5.0)));
}

/// Verifies final arc-segment extension with endpoint and bulge updates.
#[test]
fn extend_polyline_last_arc_segment() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(&[(-2.0, 0.0, 0.0), (0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], false),
            line([1.0, -3.0], [1.0, 3.0]), // Vertical at x=1.
        ],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "EXTEND",
        &json!({
            "edges": ids_json(&[ids[1]]),
            "target": [target.raw().0],
            "pick": [2.0, 0.0], // Final arc vertex.
        }),
    )
    .expect("extend last arc succeeds");

    let center = Point2::new(1.0, 0.0);
    let p = as_poly(&geom(&session, target));
    assert!(
        close_pt(p.vertices[2].pt, Point2::new(1.0, 1.0)),
        "vértice extendido: {:?}",
        p.vertices[2].pt
    );
    assert!(close(p.vertices[2].pt.dist(center), 1.0));
    assert!(p.vertices[1].bulge > 1.0, "bulge: {}", p.vertices[1].bulge);
    let arc = bulge_to_arc(p.vertices[1].pt, p.vertices[2].pt, p.vertices[1].bulge).unwrap();
    assert!(close_pt(arc.center, center) && close(arc.radius, 1.0));
}

/// Verifies first arc-segment extension through its initial vertex.
#[test]
fn extend_polyline_first_arc_segment() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0), (4.0, 0.0, 0.0)], false),
            line([1.0, -3.0], [1.0, 3.0]), // Vertical at x=1.
        ],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "EXTEND",
        &json!({
            "edges": ids_json(&[ids[1]]),
            "target": [target.raw().0],
            "pick": [0.0, 0.0], // Initial arc vertex.
        }),
    )
    .expect("extend first arc succeeds");

    let center = Point2::new(1.0, 0.0);
    let p = as_poly(&geom(&session, target));
    assert!(
        close_pt(p.vertices[0].pt, Point2::new(1.0, 1.0)),
        "vértice inicial extendido: {:?}",
        p.vertices[0].pt
    );
    assert!(close(p.vertices[0].pt.dist(center), 1.0));
    assert!(p.vertices[0].bulge > 1.0, "bulge: {}", p.vertices[0].bulge);
    let arc = bulge_to_arc(p.vertices[0].pt, p.vertices[1].pt, p.vertices[0].bulge).unwrap();
    assert!(close_pt(arc.center, center) && close(arc.radius, 1.0));
}

/// Verifies rejection of interior polyline segments.
#[test]
fn extend_polyline_rejects_an_interior_segment() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            poly(
                &[
                    (0.0, 0.0, 0.0),
                    (5.0, 0.0, 0.0),
                    (5.0, 5.0, 0.0),
                    (10.0, 5.0, 0.0),
                ],
                false,
            ),
            line([100.0, -5.0], [100.0, 5.0]), // Distant edge used only to provide boundaries.
        ],
    );
    let target = ids[0];

    let err = reg
        .execute(
            &mut session,
            "EXTEND",
            &json!({
                "edges": ids_json(&[ids[1]]),
                "target": [target.raw().0],
                "pick": [5.0, 2.5], // Midway along the inner segment.
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, CmdError::Failed(ref m) if m.contains("first or last")),
        "esperaba rechazo de tramo interior, fue {err:?}"
    );
}

// ============================================================================
// EXTEND
// ============================================================================

/// Verifies extension to the first boundary in the selected direction.
#[test]
fn extend_stretches_to_the_first_boundary() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            line([0.0, 0.0], [5.0, 0.0]),    // Target.
            line([10.0, -5.0], [10.0, 5.0]), // Boundary at x=10 (the first).
            line([15.0, -5.0], [15.0, 5.0]), // Boundary at x=15 (farther away).
        ],
    );
    let target = ids[0];

    let out = reg
        .execute(
            &mut session,
            "EX", // alias
            &json!({
                "edges": ids_json(&[ids[1], ids[2]]),
                "target": [target.raw().0],
                "pick": [5.0, 0.0], // Near endpoint p2.
            }),
        )
        .expect("extend succeeds");
    assert!(out.tx_seq.is_some());

    let l = as_line(&geom(&session, target));
    assert!(close_pt(l.p1, Point2::new(0.0, 0.0)), "p1 no se toca");
    assert!(
        close_pt(l.p2, Point2::new(10.0, 0.0)),
        "p2 llega al PRIMER límite, no al segundo: {:?}",
        l.p2
    );
}

// ============================================================================
// FILLET
// ============================================================================

/// Verifies a positive-radius line-line fillet numerically.
#[test]
fn fillet_inserts_a_tangent_arc_between_two_lines() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            line([0.0, 0.0], [10.0, 0.0]), // X axis.
            line([0.0, 0.0], [0.0, 10.0]), // Y axis.
        ],
    );
    let r = 2.0;

    let out = reg
        .execute(
            &mut session,
            "F", // alias
            &json!({ "entities": ids_json(&ids), "radius": r }),
        )
        .expect("fillet succeeds");
    assert!(out.tx_seq.is_some());
    assert_eq!(out.created.len(), 1, "FILLET inserta un arco");

    let l0 = as_line(&geom(&session, ids[0]));
    let l1 = as_line(&geom(&session, ids[1]));
    let t0 = if close_pt(l0.p1, Point2::new(10.0, 0.0)) {
        l0.p2
    } else {
        l0.p1
    };
    let t1 = if close_pt(l1.p1, Point2::new(0.0, 10.0)) {
        l1.p2
    } else {
        l1.p1
    };
    assert!(
        close_pt(t0, Point2::new(2.0, 0.0)),
        "tangencia en X: {t0:?}"
    );
    assert!(
        close_pt(t1, Point2::new(0.0, 2.0)),
        "tangencia en Y: {t1:?}"
    );

    let EntityGeometry::Arc(arc) = geom(&session, out.created[0]) else {
        panic!("esperaba un Arc de empalme");
    };
    assert!(close(arc.radius, r));
    assert!(close_pt(arc.center, Point2::new(2.0, 2.0)));

    assert!(close(arc.center.y.abs(), r));
    assert!(close(arc.center.x.abs(), r));
    let seg = arc.arc_seg();
    let ends = [seg.start_point(), seg.end_point()];
    assert!(ends.iter().any(|p| close_pt(*p, Point2::new(2.0, 0.0))));
    assert!(ends.iter().any(|p| close_pt(*p, Point2::new(0.0, 2.0))));
    let to_t0 = Point2::new(2.0, 0.0) - arc.center; // Radius to the X tangent point.
    assert!(close(
        to_t0.dot(Point2::new(1.0, 0.0) - Point2::ORIGIN),
        0.0
    ));
}

/// Verifies an exact line-line corner when radius is omitted.
#[test]
fn fillet_without_radius_makes_an_exact_corner() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![line([2.0, 0.0], [10.0, 0.0]), line([0.0, 2.0], [0.0, 10.0])],
    );

    let out = reg
        .execute(
            &mut session,
            "FILLET",
            &json!({ "entities": ids_json(&ids) }),
        )
        .expect("corner fillet succeeds");
    assert!(out.created.is_empty(), "R=0 no inserta arco");

    let l0 = as_line(&geom(&session, ids[0]));
    let l1 = as_line(&geom(&session, ids[1]));
    assert!(close_pt(l0.p2, Point2::new(0.0, 0.0)) || close_pt(l0.p1, Point2::new(0.0, 0.0)));
    assert!(close_pt(l1.p2, Point2::new(0.0, 0.0)) || close_pt(l1.p1, Point2::new(0.0, 0.0)));
}

// ============================================================================
// Line-arc and arc-arc FILLET
// ============================================================================

/// Returns tangency on an arc's supporting circle from the fillet center.
fn on_circle(o: Point2, r: f64, from: Point2) -> Point2 {
    let d = from - o;
    let len = d.norm();
    Point2::new(o.x + r * d.x / len, o.y + r * d.y / len)
}

/// Verifies that line-arc picks select distinct tangent fillets.
#[test]
fn fillet_line_arc_picks_different_sides_give_different_arcs() {
    use std::f64::consts::PI;
    let reg = registry();
    let seed_scene = |s: &mut Session| {
        seed(
            s,
            vec![
                line([-10.0, 0.0], [10.0, 0.0]),
                arc_geo([0.0, 5.0], 3.0, PI, 2.0 * PI),
            ],
        )
    };
    let o = Point2::new(0.0, 5.0);

    let mut s_r = Session::new(Units::default());
    let ids_r = seed_scene(&mut s_r);
    let out_r = reg
        .execute(
            &mut s_r,
            "FILLET",
            &json!({
                "entities": ids_json(&ids_r),
                "radius": 2.0,
                "pick0": [5.0, 0.0],
                "pick1": [2.5, 3.0],
            }),
        )
        .expect("fillet line-arc derecha");
    assert_eq!(out_r.created.len(), 1, "inserta exactamente un arco");
    let arc_r = as_arc(&geom(&s_r, out_r.created[0]));
    assert!(close(arc_r.radius, 2.0));
    assert!(
        close_pt(arc_r.center, Point2::new(4.0, 2.0)),
        "{:?}",
        arc_r.center
    );
    assert!(close(arc_r.center.y.abs(), 2.0), "tangente a la recta");
    assert!(
        close(arc_r.center.dist(o), 5.0),
        "tangente externa al arco (R+r)"
    );
    let t0 = Point2::new(4.0, 0.0);
    let t1 = Point2::new(2.4, 3.2);
    let ends = [arc_r.arc_seg().start_point(), arc_r.arc_seg().end_point()];
    assert!(
        ends.iter().any(|p| close_pt(*p, t0)),
        "falta t0 en {ends:?}"
    );
    assert!(
        ends.iter().any(|p| close_pt(*p, t1)),
        "falta t1 en {ends:?}"
    );
    assert!(arc_r.arc_seg().sweep() < PI, "debe ser el arco menor");
    let l_r = as_line(&geom(&s_r, ids_r[0]));
    assert!(close_pt(l_r.p1, Point2::new(-10.0, 0.0)) || close_pt(l_r.p2, Point2::new(-10.0, 0.0)));
    assert!(
        close_pt(l_r.p1, t0) || close_pt(l_r.p2, t0),
        "recta junta en t0: {l_r:?}"
    );
    let a_r = as_arc(&geom(&s_r, ids_r[1]));
    let a_ends = [a_r.arc_seg().start_point(), a_r.arc_seg().end_point()];
    assert!(
        a_ends.iter().any(|p| close_pt(*p, t1)),
        "arco junta en t1: {a_ends:?}"
    );

    let mut s_l = Session::new(Units::default());
    let ids_l = seed_scene(&mut s_l);
    let out_l = reg
        .execute(
            &mut s_l,
            "FILLET",
            &json!({
                "entities": ids_json(&ids_l),
                "radius": 2.0,
                "pick0": [-5.0, 0.0],
                "pick1": [-2.5, 3.0],
            }),
        )
        .expect("fillet line-arc izquierda");
    let arc_l = as_arc(&geom(&s_l, out_l.created[0]));
    assert!(
        close_pt(arc_l.center, Point2::new(-4.0, 2.0)),
        "{:?}",
        arc_l.center
    );
    assert!(
        !close_pt(arc_r.center, arc_l.center),
        "pick en lados distintos debe dar arcos distintos"
    );
}

/// Verifies an arc-arc fillet tangent to both supporting circles.
#[test]
fn fillet_arc_arc_inserts_a_tangent_arc() {
    use std::f64::consts::PI;
    let reg = registry();
    let mut s = Session::new(Units::default());
    let ids = seed(
        &mut s,
        vec![
            arc_geo([0.0, 0.0], 2.0, 0.0, PI),
            arc_geo([4.0, 0.0], 2.0, 0.0, PI),
        ],
    );
    let out = reg
        .execute(
            &mut s,
            "FILLET",
            &json!({
                "entities": ids_json(&ids),
                "radius": 1.0,
                "pick0": [0.0, 2.0],
                "pick1": [4.0, 2.0],
            }),
        )
        .expect("fillet arco-arco");
    assert_eq!(out.created.len(), 1);
    let arc = as_arc(&geom(&s, out.created[0]));
    assert!(close(arc.radius, 1.0));
    let c1 = Point2::new(0.0, 0.0);
    let c2 = Point2::new(4.0, 0.0);
    let s5 = 5.0_f64.sqrt();
    assert!(
        close_pt(arc.center, Point2::new(2.0, s5)),
        "{:?}",
        arc.center
    );
    assert!(close(arc.center.dist(c1), 3.0));
    assert!(close(arc.center.dist(c2), 3.0));
    let t0 = on_circle(c1, 2.0, arc.center);
    let t1 = on_circle(c2, 2.0, arc.center);
    let ends = [arc.arc_seg().start_point(), arc.arc_seg().end_point()];
    assert!(ends.iter().any(|p| close_pt(*p, t0)));
    assert!(ends.iter().any(|p| close_pt(*p, t1)));
    assert!(arc.arc_seg().sweep() < PI, "debe ser el arco menor");
    let a0 = as_arc(&geom(&s, ids[0]));
    let a1 = as_arc(&geom(&s, ids[1]));
    let e0 = [a0.arc_seg().start_point(), a0.arc_seg().end_point()];
    let e1 = [a1.arc_seg().start_point(), a1.arc_seg().end_point()];
    assert!(
        e0.iter().any(|p| close_pt(*p, t0)),
        "arco0 junta en t0: {e0:?}"
    );
    assert!(
        e1.iter().any(|p| close_pt(*p, t1)),
        "arco1 junta en t1: {e1:?}"
    );
}

/// Verifies that an impossible fillet radius fails without mutation.
#[test]
fn fillet_impossible_radius_errors_without_transaction() {
    use std::f64::consts::PI;
    let reg = registry();
    let mut s = Session::new(Units::default());
    let ids = seed(
        &mut s,
        vec![
            line([-10.0, 0.0], [10.0, 0.0]),
            arc_geo([0.0, 5.0], 3.0, PI, 2.0 * PI),
        ],
    );
    let before = serde_json::to_string(s.document()).unwrap();
    let err = reg
        .execute(
            &mut s,
            "FILLET",
            &json!({ "entities": ids_json(&ids), "radius": 0.5 }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(
        before,
        serde_json::to_string(s.document()).unwrap(),
        "un empalme imposible no debe mutar el documento (0 tx)"
    );
}

/// Verifies an exact line-arc corner when radius is omitted.
#[test]
fn fillet_zero_radius_joins_line_and_arc_at_the_real_intersection() {
    use std::f64::consts::PI;
    let reg = registry();
    let mut s = Session::new(Units::default());
    let ids = seed(
        &mut s,
        vec![
            line([-10.0, 0.0], [10.0, 0.0]),
            arc_geo([0.0, 3.0], 5.0, 1.5 * PI, 2.0 * PI),
        ],
    );
    let out = reg
        .execute(
            &mut s,
            "FILLET",
            &json!({
                "entities": ids_json(&ids),
                "pick0": [5.0, 0.0],
                "pick1": [5.0, 3.0],
            }),
        )
        .expect("fillet R=0 línea-arco");
    assert!(out.created.is_empty(), "R=0 no inserta arco");
    assert!(out.tx_seq.is_some(), "R=0 confirma exactamente una tx");

    let corner = Point2::new(4.0, 0.0);
    let l = as_line(&geom(&s, ids[0]));
    assert!(
        close_pt(l.p1, corner) || close_pt(l.p2, corner),
        "la recta se une en la intersección: {l:?}"
    );
    assert!(close_pt(l.p1, Point2::new(-10.0, 0.0)) || close_pt(l.p2, Point2::new(-10.0, 0.0)));
    let a = as_arc(&geom(&s, ids[1]));
    let ends = [a.arc_seg().start_point(), a.arc_seg().end_point()];
    assert!(
        ends.iter().any(|p| close_pt(*p, corner)),
        "el arco se une en la intersección: {ends:?}"
    );
}

/// Verifies rejection of an exact corner involving a complete circle.
#[test]
fn fillet_zero_radius_with_a_full_circle_errors() {
    let reg = registry();
    let mut s = Session::new(Units::default());
    let ids = seed(
        &mut s,
        vec![line([-10.0, 0.0], [10.0, 0.0]), circle_geo([0.0, 0.0], 3.0)],
    );
    let before = serde_json::to_string(s.document()).unwrap();
    let err = reg
        .execute(&mut s, "FILLET", &json!({ "entities": ids_json(&ids) }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(s.document()).unwrap());
}

/// Checks that generated fillets are tangent to both supporting curves within tolerance.
#[test]
fn property_fillet_arc_is_tangent_to_both_supports() {
    use std::f64::consts::PI;
    let reg = registry();

    fn line_res(center: Point2, a: Point2, b: Point2, r: f64) -> f64 {
        let d = b - a;
        let len = d.norm();
        ((center - a).cross(d) / len).abs() - r
    }
    fn circle_res(center: Point2, o: Point2, rr: f64, r: f64) -> f64 {
        ((center.dist(o) - rr).abs() - r).abs()
    }

    enum Sup {
        Line(Point2, Point2),
        Circle(Point2, f64),
    }
    let l = |a: [f64; 2], b: [f64; 2]| Sup::Line(Point2::new(a[0], a[1]), Point2::new(b[0], b[1]));
    let c = |o: [f64; 2], r: f64| Sup::Circle(Point2::new(o[0], o[1]), r);

    let cases: Vec<(Vec<EntityGeometry>, Sup, Sup, f64)> = vec![
        (
            vec![
                line([-10.0, 0.0], [10.0, 0.0]),
                arc_geo([0.0, 5.0], 3.0, PI, 2.0 * PI),
            ],
            l([-10.0, 0.0], [10.0, 0.0]),
            c([0.0, 5.0], 3.0),
            2.0,
        ),
        (
            vec![
                line([-8.0, 0.0], [8.0, 0.0]),
                arc_geo([0.0, 4.0], 3.0, PI, 2.0 * PI),
            ],
            l([-8.0, 0.0], [8.0, 0.0]),
            c([0.0, 4.0], 3.0),
            1.5,
        ),
        (
            vec![
                arc_geo([0.0, 0.0], 2.0, 0.0, PI),
                arc_geo([4.0, 0.0], 2.0, 0.0, PI),
            ],
            c([0.0, 0.0], 2.0),
            c([4.0, 0.0], 2.0),
            1.0,
        ),
        (
            vec![line([-10.0, 0.0], [10.0, 0.0]), circle_geo([0.0, 5.0], 3.0)],
            l([-10.0, 0.0], [10.0, 0.0]),
            c([0.0, 5.0], 3.0),
            2.0,
        ),
    ];

    let mut ok = 0usize;
    for (geoms, sup0, sup1, r) in cases {
        let mut s = Session::new(Units::default());
        let ids = seed(&mut s, geoms);
        let Ok(out) = reg.execute(
            &mut s,
            "FILLET",
            &json!({ "entities": ids_json(&ids), "radius": r }),
        ) else {
            continue;
        };
        assert_eq!(out.created.len(), 1);
        let arc = as_arc(&geom(&s, out.created[0]));
        assert!(close(arc.radius, r), "radio del empalme");
        let res0 = match &sup0 {
            Sup::Line(a, b) => line_res(arc.center, *a, *b, r),
            Sup::Circle(o, rr) => circle_res(arc.center, *o, *rr, r),
        };
        let res1 = match &sup1 {
            Sup::Line(a, b) => line_res(arc.center, *a, *b, r),
            Sup::Circle(o, rr) => circle_res(arc.center, *o, *rr, r),
        };
        assert!(res0.abs() <= 1e-6, "no tangente al soporte 0 (res={res0})");
        assert!(res1.abs() <= 1e-6, "no tangente al soporte 1 (res={res1})");
        ok += 1;
    }
    assert!(ok >= 3, "esperaba varios empalmes tangentes, hubo {ok}");
}

// ============================================================================
// OFFSET
// ============================================================================

/// Verifies that bulged-polyline offsets preserve parallel and concentric distance.
#[test]
fn offset_polyline_with_bulge_keeps_distance() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let poly = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(4.0, 0.0), 1.0),
            PolyVertex::new(Point2::new(4.0, 4.0), 0.0),
        ],
        false,
    );
    let arc0 = bulge_to_arc(Point2::new(4.0, 0.0), Point2::new(4.0, 4.0), 1.0).unwrap();
    let ids = seed(&mut session, vec![EntityGeometry::Polyline(poly)]);
    let d = 0.5;

    let out = reg
        .execute(
            &mut session,
            "O", // alias
            &json!({
                "entities": ids_json(&ids),
                "distance": d,
                "side": [2.0, 2.0], // Above the straight segment (to its left).
            }),
        )
        .expect("offset succeeds");
    assert!(out.tx_seq.is_some());
    assert_eq!(out.created.len(), 1, "OFFSET crea una entidad nueva");

    let EntityGeometry::Polyline(new_poly) = geom(&session, out.created[0]) else {
        panic!("esperaba polyline");
    };
    assert_eq!(new_poly.vertices.len(), 3);
    assert!(!new_poly.closed);

    assert!(
        close(new_poly.vertices[0].pt.y, d),
        "el tramo recto no está a distancia d: {}",
        new_poly.vertices[0].pt.y
    );

    let v1 = new_poly.vertices[1];
    let v2 = new_poly.vertices[2];
    assert!(v1.bulge.abs() > 0.1, "el tramo curvo perdió su bulge");
    let arc1 = bulge_to_arc(v1.pt, v2.pt, v1.bulge).unwrap();
    assert!(
        close_pt(arc1.center, arc0.center),
        "el arco desplazado no es concéntrico: {:?} vs {:?}",
        arc1.center,
        arc0.center
    );
    assert!(
        close((arc1.radius - arc0.radius).abs(), d),
        "el radio no cambió en exactamente d: {} vs {}",
        arc1.radius,
        arc0.radius
    );
}

/// Verifies that a line offset lies `d` toward `side`.
#[test]
fn offset_line_to_the_picked_side() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);

    let out = reg
        .execute(
            &mut session,
            "OFFSET",
            &json!({ "entities": ids_json(&ids), "distance": 3.0, "side": [5.0, -1.0] }),
        )
        .expect("offset line succeeds");

    let l = as_line(&geom(&session, out.created[0]));
    assert!(
        close(l.p1.y, -3.0) && close(l.p2.y, -3.0),
        "paralela: {l:?}"
    );
    assert!(close(l.p1.x, 0.0) && close(l.p2.x, 10.0));
}

/// Verifies inward and outward circle offsets selected by `side`.
#[test]
fn offset_circle_outward_and_inward() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(
            Point2::new(0.0, 0.0),
            5.0,
        ))],
    );

    let out = reg
        .execute(
            &mut session,
            "OFFSET",
            &json!({ "entities": ids_json(&ids), "distance": 2.0, "side": [10.0, 0.0] }),
        )
        .expect("offset outward");
    let EntityGeometry::Circle(c) = geom(&session, out.created[0]) else {
        panic!("círculo");
    };
    assert!(close(c.radius, 7.0));

    let out = reg
        .execute(
            &mut session,
            "OFFSET",
            &json!({ "entities": ids_json(&ids), "distance": 2.0, "side": [1.0, 0.0] }),
        )
        .expect("offset inward");
    let EntityGeometry::Circle(c) = geom(&session, out.created[0]) else {
        panic!("círculo");
    };
    assert!(close(c.radius, 3.0));
}

// ============================================================================
// Aliases
// ============================================================================

#[test]
fn commands_expose_their_autocad_aliases() {
    let reg = registry();
    for (canon, alias) in [
        ("TRIM", "TR"),
        ("EXTEND", "EX"),
        ("FILLET", "F"),
        ("OFFSET", "O"),
    ] {
        let by_name = reg.lookup(canon).map(|s| s.name().to_string());
        let by_alias = reg.lookup(alias).map(|s| s.name().to_string());
        assert_eq!(by_name, Some(canon.to_string()), "falta {canon}");
        assert_eq!(by_alias, by_name, "el alias {alias} debe apuntar a {canon}");
    }
}

// ============================================================================
// PREVIEW (dry run)
// ============================================================================

/// Verifies that TRIM preview matches execution without mutation.
#[test]
fn preview_trim_is_pure_and_matches_execute() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([3.0, -5.0], [3.0, 5.0]),
            line([7.0, -5.0], [7.0, 5.0]),
        ],
    );
    let (target, cut1, cut2) = (ids[0], ids[1], ids[2]);
    let args = json!({
        "edges": ids_json(&[cut1, cut2]),
        "target": [target.raw().0],
        "pick": [5.0, 0.0],
    });

    let before = geom(&session, target);
    let can_undo_before = session.can_undo();
    let geoms = reg.preview(&session, "TRIM", &args).expect("preview ok");

    assert_eq!(
        geom(&session, target),
        before,
        "preview no muta el documento"
    );
    assert_eq!(
        session.can_undo(),
        can_undo_before,
        "preview no crea tx (0 tx)"
    );

    assert_eq!(geoms.len(), 2);
    let l0 = as_line(&geoms[0]);
    let l1 = as_line(&geoms[1]);
    assert!(close_pt(l0.p1, Point2::new(0.0, 0.0)) && close_pt(l0.p2, Point2::new(3.0, 0.0)));
    assert!(close_pt(l1.p1, Point2::new(7.0, 0.0)) && close_pt(l1.p2, Point2::new(10.0, 0.0)));
}

/// Verifies that OFFSET preview returns its parallel without mutation.
#[test]
fn preview_offset_returns_the_parallel_without_creating() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);
    let count_before = session.document().model_space().iter().count();

    let geoms = reg
        .preview(
            &session,
            "OFFSET",
            &json!({ "entities": ids_json(&ids), "distance": 3.0, "side": [5.0, -1.0] }),
        )
        .expect("preview offset ok");

    assert_eq!(
        session.document().model_space().iter().count(),
        count_before,
        "preview no crea entidades"
    );
    assert_eq!(geoms.len(), 1);
    let l = as_line(&geoms[0]);
    assert!(
        close(l.p1.y, -3.0) && close(l.p2.y, -3.0),
        "paralela: {l:?}"
    );
}

/// Verifies typed rejection for commands without preview support.
#[test]
fn preview_of_a_non_previewable_command_is_typed_error() {
    let mut reg = CommandRegistry::new();
    af_cmd::builtin::register_builtins(&mut reg).expect("builtins");
    let session = Session::new(Units::default());
    let err = reg.preview(&session, "MOVE", &json!({})).unwrap_err();
    assert!(
        matches!(err, CmdError::NotPreviewable(ref name) if name == "MOVE"),
        "esperaba NotPreviewable(MOVE), fue {err:?}"
    );
}
