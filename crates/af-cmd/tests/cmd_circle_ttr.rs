//! End-to-end CIRCLE tangent-tangent-radius tests.
//!
//! Covers line-line, line-circle, circle-circle, no-solution, invalid-entity, and
//! tolerance-level tangency cases.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandOutcome, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    ArcGeo, CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use core::f64::consts::PI;
use serde_json::json;

// ---- Helpers ----------------------------------------------------------------

const TOL: f64 = 1e-6;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
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

/// Seeds tangent-source entities and returns their IDs.
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

fn ids_json(ids: &[EntityId]) -> Vec<u64> {
    ids.iter().map(|id| id.raw().0).collect()
}

fn line(a: [f64; 2], b: [f64; 2]) -> EntityGeometry {
    EntityGeometry::Line(LineGeo::new(
        Point2::new(a[0], a[1]),
        Point2::new(b[0], b[1]),
    ))
}

fn circle(c: [f64; 2], r: f64) -> EntityGeometry {
    EntityGeometry::Circle(CircleGeo::new(Point2::new(c[0], c[1]), r))
}

/// Returns the sole created circle after checking the transaction contract.
fn created_circle(session: &Session, out: &CommandOutcome) -> CircleGeo {
    assert_eq!(out.created.len(), 1, "TTR crea exactamente una entidad");
    assert!(out.tx_seq.is_some(), "TTR confirma exactamente 1 tx");
    let id = out.created[0];
    let (rec, _) = session.document().entity(id).expect("entity exists");
    match &rec.geometry {
        EntityGeometry::Circle(c) => *c,
        other => panic!("se esperaba un círculo, fue {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn close_pt(a: Point2, b: Point2) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

/// Returns perpendicular distance from `p` to infinite line `a` to `b`.
fn perp_dist_line(p: Point2, a: Point2, b: Point2) -> f64 {
    let d = b - a;
    let len = d.norm();
    (p - a).cross(d / len).abs()
}

/// Returns the smallest internal or external circle-tangency residual.
fn circle_residual(center: Point2, o: Point2, big_r: f64, r: f64) -> f64 {
    let d = center.dist(o);
    (d - (big_r + r)).abs().min((d - (big_r - r).abs()).abs())
}

/// Returns tangency residual between `(center, r)` and supported geometry `g`.
fn residual(g: &EntityGeometry, center: Point2, r: f64) -> f64 {
    match g {
        EntityGeometry::Line(l) => (perp_dist_line(center, l.p1, l.p2) - r).abs(),
        EntityGeometry::Circle(c) => circle_residual(center, c.center, c.radius, r),
        EntityGeometry::Arc(a) => circle_residual(center, a.center, a.radius, r),
        other => panic!("residual no definido para {other:?}"),
    }
}

// ---- Line-line TTR -----------------------------------------------------------

#[test]
fn ttr_line_line_perpendicular_pick_selects_center() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![line([-5.0, 0.0], [5.0, 0.0]), line([0.0, -5.0], [0.0, 5.0])],
    );

    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [2, 0],
                "p2": [0, 2],
                "radius": 1,
            }),
        )
        .expect("TTR línea-línea (+X,+Y)");
    let c = created_circle(&session, &out);
    assert!(
        close_pt(c.center, Point2::new(1.0, 1.0)),
        "centro {:?}",
        c.center
    );
    assert!(close(c.radius, 1.0));

    let out2 = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [2, 0],
                "p2": [0, -2],
                "radius": 1,
            }),
        )
        .expect("TTR línea-línea (+X,−Y)");
    let c2 = created_circle(&session, &out2);
    assert!(
        close_pt(c2.center, Point2::new(1.0, -1.0)),
        "centro {:?}",
        c2.center
    );

    for cc in [c, c2] {
        assert!(residual(&line([-5.0, 0.0], [5.0, 0.0]), cc.center, 1.0) < TOL);
        assert!(residual(&line([0.0, -5.0], [0.0, 5.0]), cc.center, 1.0) < TOL);
    }
}

#[test]
fn ttr_pick_association_pair_swap_and_exact_tie() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![line([-5.0, 0.0], [5.0, 0.0]), line([0.0, -5.0], [0.0, 5.0])],
    );
    for (pair, p1, p2, want) in [
        ([ids[0], ids[1]], [3.0, 4.0], [-2.0, 5.0], [1.0, 1.0]),
        ([ids[1], ids[0]], [-2.0, 5.0], [3.0, 4.0], [1.0, 1.0]),
        ([ids[0], ids[1]], [-2.0, 5.0], [3.0, 4.0], [-1.0, 1.0]),
        ([ids[0], ids[1]], [0.0, 0.0], [0.0, 0.0], [-1.0, -1.0]),
    ] {
        let out = reg
            .execute(
                &mut session,
                "CIRCLE",
                &json!({
                    "mode": "ttr",
                    "entities": ids_json(&pair),
                    "p1": p1,
                    "p2": p2,
                    "radius": 1,
                }),
            )
            .expect("TTR pick association case");
        let center = created_circle(&session, &out).center;
        assert!(
            close_pt(center, Point2::new(want[0], want[1])),
            "{center:?}"
        );
    }
}

// ---- Line-circle TTR ---------------------------------------------------------

#[test]
fn ttr_line_circle_external() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![line([-3.0, 0.0], [3.0, 0.0]), circle([0.0, 4.0], 2.0)],
    );
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [0, 0],
                "p2": [0, 2],
                "radius": 1,
            }),
        )
        .expect("TTR línea-círculo");
    let c = created_circle(&session, &out);
    assert!(
        close_pt(c.center, Point2::new(0.0, 1.0)),
        "centro {:?}",
        c.center
    );
    assert!(close(c.radius, 1.0));
    assert!(residual(&line([-3.0, 0.0], [3.0, 0.0]), c.center, 1.0) < TOL);
    assert!(close(c.center.dist(Point2::new(0.0, 4.0)), 3.0));
}

// ---- Circle-circle TTR -------------------------------------------------------

#[test]
fn ttr_circle_circle() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![circle([0.0, 0.0], 2.0), circle([4.0, 0.0], 2.0)],
    );
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [0, 2],
                "p2": [4, 2],
                "radius": 1,
            }),
        )
        .expect("TTR círculo-círculo");
    let c = created_circle(&session, &out);
    let s5 = 5.0_f64.sqrt();
    assert!(
        close_pt(c.center, Point2::new(2.0, s5)),
        "centro {:?}",
        c.center
    );
    assert!(residual(&circle([0.0, 0.0], 2.0), c.center, 1.0) < TOL);
    assert!(residual(&circle([4.0, 0.0], 2.0), c.center, 1.0) < TOL);
}

#[test]
fn ttr_candidate_enclosing_sources_uses_opposite_side_contacts() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![circle([0.0, 0.0], 1.0), circle([8.0, 0.0], 1.0)],
    );
    let out = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [-1, 0],
                "p2": [9, 0],
                "radius": 5,
            }),
        )
        .expect("candidate encloses both sources");
    let c = created_circle(&session, &out);
    assert!(close_pt(c.center, Point2::new(4.0, 0.0)));
    assert!(close(c.radius, 5.0));
}

// ---- No solution -------------------------------------------------------------

#[test]
fn ttr_no_solution_errors_and_zero_tx() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![circle([0.0, 0.0], 1.0), circle([10.0, 0.0], 1.0)],
    );
    let before = session.history().undo_depth();
    let entities = session.document().model_space().len();

    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [1, 0],
                "p2": [9, 0],
                "radius": 1,
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, CmdError::Failed(ref m) if m.contains("tangente")),
        "err {err:?}"
    );
    assert_eq!(session.document().model_space().len(), entities);
    assert_eq!(session.history().undo_depth(), before);
}

#[test]
fn ttr_nonfinite_scores_error_without_mutation() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![line([-5.0, 0.0], [5.0, 0.0]), line([0.0, -5.0], [0.0, 5.0])],
    );
    let before = session.history().undo_depth();
    let entities = session.document().model_space().len();

    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [f64::MAX, f64::MAX],
                "p2": [f64::MAX, f64::MAX],
                "radius": 1,
            }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "err {err:?}");
    assert_eq!(session.document().model_space().len(), entities);
    assert_eq!(session.history().undo_depth(), before);
}

// ---- Invalid entity ----------------------------------------------------------

#[test]
fn ttr_invalid_entity_point_errors_and_zero_tx() {
    let (reg, mut session) = setup();
    let ids = seed(
        &mut session,
        vec![
            EntityGeometry::Point(PointGeo::new(Point2::new(1.0, 1.0))),
            line([-3.0, 0.0], [3.0, 0.0]),
        ],
    );
    let before = session.history().undo_depth();
    let entities = session.document().model_space().len();

    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [1, 1],
                "p2": [0, 0],
                "radius": 1,
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, CmdError::Failed(ref m) if m.contains("tangente válida")),
        "err {err:?}"
    );
    assert_eq!(session.document().model_space().len(), entities);
    assert_eq!(session.history().undo_depth(), before);
}

// ---- Entity cardinality ------------------------------------------------------

#[test]
fn ttr_requires_exactly_two_entities() {
    let (reg, mut session) = setup();
    let ids = seed(&mut session, vec![line([-3.0, 0.0], [3.0, 0.0])]);
    let err = reg
        .execute(
            &mut session,
            "CIRCLE",
            &json!({
                "mode": "ttr",
                "entities": ids_json(&ids),
                "p1": [0, 0],
                "p2": [0, 1],
                "radius": 1,
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, CmdError::Failed(ref m) if m.contains("exactamente dos")),
        "err {err:?}"
    );
    assert_eq!(session.document().model_space().len(), 1);
}

// ---- Tangency property -------------------------------------------------------

#[test]
fn ttr_property_created_circle_is_tangent_to_both() {
    let curves = [
        line([0.0, 0.0], [1.0, 0.0]),
        line([0.0, 0.0], [0.0, 1.0]),
        line([-1.0, -1.0], [2.0, 1.0]),
        circle([0.0, 0.0], 1.0),
        circle([3.0, 0.0], 2.0),
        EntityGeometry::Arc(ArcGeo::new(Point2::new(-1.0, 2.5), 1.5, 0.0, PI)),
    ];
    let radii = [0.5, 1.0, 2.0];
    let mut total = 0usize;

    for i in 0..curves.len() {
        for j in (i + 1)..curves.len() {
            for &r in &radii {
                let (reg, mut session) = setup();
                let ids = seed(&mut session, vec![curves[i].clone(), curves[j].clone()]);
                let res = reg.execute(
                    &mut session,
                    "CIRCLE",
                    &json!({
                        "mode": "ttr",
                        "entities": ids_json(&ids),
                        "p1": [5, 5],
                        "p2": [-5, -5],
                        "radius": r,
                    }),
                );
                // Skip radii without a solution; dedicated tests cover that path.
                let Ok(out) = res else { continue };
                let c = created_circle(&session, &out);
                assert!(close(c.radius, r));
                assert!(
                    residual(&curves[i], c.center, r) < TOL,
                    "no tangente a curva {i} (r={r})"
                );
                assert!(
                    residual(&curves[j], c.center, r) < TOL,
                    "no tangente a curva {j} (r={r})"
                );
                total += 1;
            }
        }
    }
    assert!(total > 15, "esperaba varios TTR con solución, hubo {total}");
}
