//! End-to-end CHAMFER, BREAK, BREAKATPOINT, JOIN, LENGTHEN, and ALIGN tests for
//! successful execution, undo, atomic failure, and transaction invariants.

use af_cmd::{CmdError, CommandRegistry};
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
    af_cmd::builtin::register_builtins(&mut reg).expect("register builtins");
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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}
fn close_pt(a: Point2, b: Point2) -> bool {
    close(a.x, b.x) && close(a.y, b.y)
}

/// Returns sorted serialized model-space geometry for undo comparisons.
fn geoms_sorted(session: &Session) -> Vec<String> {
    let mut v: Vec<String> = session
        .document()
        .model_space()
        .iter()
        .map(|r| serde_json::to_string(&r.geometry).unwrap())
        .collect();
    v.sort();
    v
}

fn count(session: &Session) -> usize {
    session.document().model_space().iter().count()
}

// ============================================================================
// CHAMFER
// ============================================================================

#[test]
fn chamfer_trims_two_lines_and_inserts_the_bevel_segment() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![line([0.0, 0.0], [10.0, 0.0]), line([0.0, 0.0], [0.0, 10.0])],
    );
    let before = geoms_sorted(&session);

    let out = reg
        .execute(
            &mut session,
            "CHA", // alias
            &json!({ "entities": ids_json(&ids), "d1": 3.0, "d2": 4.0 }),
        )
        .expect("chamfer succeeds");
    assert!(out.tx_seq.is_some(), "affects_document => exactamente 1 tx");
    assert_eq!(
        out.created.len(),
        1,
        "el segmento de chaflán es una entidad nueva"
    );

    let seg = as_line(&geom(&session, out.created[0]));
    let es = [seg.p1, seg.p2];
    assert!(es.iter().any(|p| close_pt(*p, Point2::new(3.0, 0.0))));
    assert!(es.iter().any(|p| close_pt(*p, Point2::new(0.0, 4.0))));

    session.undo().expect("undo");
    assert_eq!(geoms_sorted(&session), before, "undo restaura la geometría");
}

#[test]
fn chamfer_parallel_lines_is_atomic_error() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![line([0.0, 0.0], [10.0, 0.0]), line([0.0, 5.0], [10.0, 5.0])],
    );
    let before = geoms_sorted(&session);

    let err = reg
        .execute(
            &mut session,
            "CHAMFER",
            &json!({ "entities": ids_json(&ids), "d1": 1.0, "d2": 1.0 }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(geoms_sorted(&session), before, "documento intacto (0 tx)");
    assert_ne!(session.undo_label(), Some("Chamfer"));
}

// ============================================================================
// BREAK / BREAKATPOINT
// ============================================================================

#[test]
fn break_removes_the_middle_and_leaves_two_pieces() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);
    let target = ids[0];
    let before = geoms_sorted(&session);

    let out = reg
        .execute(
            &mut session,
            "BR", // alias
            &json!({ "target": [target.raw().0], "p1": [3.0, 0.0], "p2": [7.0, 0.0] }),
        )
        .expect("break succeeds");
    assert!(out.tx_seq.is_some());
    assert_eq!(out.created.len(), 1, "el segundo tramo es nuevo");

    let kept = as_line(&geom(&session, target));
    assert!(close_pt(kept.p1, Point2::new(0.0, 0.0)) && close_pt(kept.p2, Point2::new(3.0, 0.0)));
    let extra = as_line(&geom(&session, out.created[0]));
    assert!(
        close_pt(extra.p1, Point2::new(7.0, 0.0)) && close_pt(extra.p2, Point2::new(10.0, 0.0))
    );

    session.undo().expect("undo");
    assert_eq!(geoms_sorted(&session), before);
}

#[test]
fn break_a_circle_becomes_an_arc() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(Point2::ORIGIN, 1.0))],
    );
    let target = ids[0];

    reg.execute(
        &mut session,
        "BREAK",
        &json!({ "target": [target.raw().0], "p1": [1.0, 0.0], "p2": [0.0, 1.0] }),
    )
    .expect("break circle succeeds");

    let EntityGeometry::Arc(arc) = geom(&session, target) else {
        panic!("un círculo roto debe volverse Arc");
    };
    assert!(close(arc.radius, 1.0));
    assert!(close(arc.sweep(), 1.5 * std::f64::consts::PI));
}

#[test]
fn break_at_point_splits_a_line_without_a_gap() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);
    let target = ids[0];

    let out = reg
        .execute(
            &mut session,
            "BREAKATPOINT",
            &json!({ "target": [target.raw().0], "point": [4.0, 0.0] }),
        )
        .expect("break-at-point succeeds");
    assert_eq!(out.created.len(), 1);

    let a = as_line(&geom(&session, target));
    let b = as_line(&geom(&session, out.created[0]));
    assert!(close_pt(a.p2, Point2::new(4.0, 0.0)));
    assert!(close_pt(b.p1, Point2::new(4.0, 0.0)));
    assert!(close_pt(a.p1, Point2::new(0.0, 0.0)) && close_pt(b.p2, Point2::new(10.0, 0.0)));
}

#[test]
fn break_at_point_on_a_circle_is_an_error() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Circle(CircleGeo::new(Point2::ORIGIN, 1.0))],
    );
    let before = geoms_sorted(&session);
    let err = reg
        .execute(
            &mut session,
            "BREAKATPOINT",
            &json!({ "target": [ids[0].raw().0], "point": [1.0, 0.0] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_eq!(geoms_sorted(&session), before);
}

// ============================================================================
// JOIN
// ============================================================================

#[test]
fn join_collinear_lines_into_one() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![line([0.0, 0.0], [4.0, 0.0]), line([4.0, 0.0], [8.0, 0.0])],
    );
    let before = geoms_sorted(&session);
    assert_eq!(count(&session), 2);

    let out = reg
        .execute(&mut session, "J", &json!({ "entities": ids_json(&ids) }))
        .expect("join succeeds");
    assert!(out.tx_seq.is_some());
    assert_eq!(count(&session), 1, "las demás entidades se eliminan");

    let merged = as_line(&geom(&session, ids[0]));
    assert!(
        close_pt(merged.p1, Point2::new(0.0, 0.0)) && close_pt(merged.p2, Point2::new(8.0, 0.0))
    );

    session.undo().expect("undo");
    assert_eq!(count(&session), 2);
    assert_eq!(geoms_sorted(&session), before);
}

#[test]
fn join_two_arcs_into_one() {
    use std::f64::consts::{FRAC_PI_2, PI};
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![
            EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 3.0, 0.0, FRAC_PI_2)),
            EntityGeometry::Arc(ArcGeo::new(Point2::ORIGIN, 3.0, FRAC_PI_2, PI)),
        ],
    );
    reg.execute(&mut session, "JOIN", &json!({ "entities": ids_json(&ids) }))
        .expect("join arcs succeeds");
    assert_eq!(count(&session), 1);
    let EntityGeometry::Arc(arc) = geom(&session, ids[0]) else {
        panic!("arco");
    };
    assert!(close(arc.sweep(), PI));
}

#[test]
fn join_non_collinear_lines_is_atomic_error() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![line([0.0, 0.0], [4.0, 0.0]), line([0.0, 1.0], [4.0, 1.0])],
    );
    let before = geoms_sorted(&session);
    let err = reg
        .execute(&mut session, "JOIN", &json!({ "entities": ids_json(&ids) }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_eq!(geoms_sorted(&session), before, "documento intacto");
}

#[test]
fn join_polylines_into_one() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let a = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(2.0, 0.0), 0.0),
        ],
        false,
    );
    let b = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(2.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(2.0, 3.0), 0.0),
        ],
        false,
    );
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Polyline(a), EntityGeometry::Polyline(b)],
    );
    reg.execute(&mut session, "JOIN", &json!({ "entities": ids_json(&ids) }))
        .expect("join polylines succeeds");
    assert_eq!(count(&session), 1);
    let EntityGeometry::Polyline(p) = geom(&session, ids[0]) else {
        panic!("polilínea");
    };
    assert_eq!(p.vertices.len(), 3);
}

// ============================================================================
// LENGTHEN
// ============================================================================

#[test]
fn lengthen_total_sets_absolute_length_from_the_near_end() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);
    let target = ids[0];
    let before = geoms_sorted(&session);

    let out = reg
        .execute(
            &mut session,
            "LEN", // alias
            &json!({ "target": [target.raw().0], "pick": [9.5, 0.0], "total": 15.0 }),
        )
        .expect("lengthen succeeds");
    assert!(out.tx_seq.is_some());

    let l = as_line(&geom(&session, target));
    assert!(
        close_pt(l.p1, Point2::new(0.0, 0.0)),
        "el extremo lejano no se mueve"
    );
    assert!(close_pt(l.p2, Point2::new(15.0, 0.0)));

    session.undo().expect("undo");
    assert_eq!(geoms_sorted(&session), before);
}

#[test]
fn lengthen_delta_shrink_shortens() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);
    let target = ids[0];

    reg.execute(
        &mut session,
        "LENGTHEN",
        &json!({ "target": [target.raw().0], "pick": [9.5, 0.0], "delta": 4.0, "shrink": true }),
    )
    .expect("lengthen shrink succeeds");

    let l = as_line(&geom(&session, target));
    assert!(
        close(l.length(), 6.0),
        "10 − 4 = 6, longitud {}",
        l.length()
    );
    assert!(close_pt(l.p1, Point2::new(0.0, 0.0)) && close_pt(l.p2, Point2::new(6.0, 0.0)));
}

#[test]
fn lengthen_to_non_positive_is_atomic_error() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [10.0, 0.0])]);
    let before = geoms_sorted(&session);
    let err = reg
        .execute(
            &mut session,
            "LENGTHEN",
            &json!({ "target": [ids[0].raw().0], "pick": [9.5, 0.0], "delta": 20.0, "shrink": true }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_eq!(geoms_sorted(&session), before);
}

// ============================================================================
// ALIGN
// ============================================================================

#[test]
fn align_one_pair_translates() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [2.0, 0.0])]);
    let target = ids[0];
    let before = geoms_sorted(&session);

    let out = reg
        .execute(
            &mut session,
            "AL", // alias
            &json!({ "entities": ids_json(&ids), "src1": [0.0, 0.0], "dst1": [5.0, 5.0] }),
        )
        .expect("align translate succeeds");
    assert!(out.tx_seq.is_some());

    let l = as_line(&geom(&session, target));
    assert!(close_pt(l.p1, Point2::new(5.0, 5.0)) && close_pt(l.p2, Point2::new(7.0, 5.0)));

    session.undo().expect("undo");
    assert_eq!(geoms_sorted(&session), before);
}

#[test]
fn align_two_pairs_with_scale_maps_source_onto_destination() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(&mut session, vec![line([0.0, 0.0], [1.0, 0.0])]);
    let target = ids[0];

    reg.execute(
        &mut session,
        "ALIGN",
        &json!({
            "entities": ids_json(&ids),
            "src1": [0.0, 0.0], "dst1": [0.0, 0.0],
            "src2": [1.0, 0.0], "dst2": [0.0, 2.0],
            "scale": true,
        }),
    )
    .expect("align 2-pairs succeeds");

    let l = as_line(&geom(&session, target));
    assert!(close_pt(l.p1, Point2::new(0.0, 0.0)));
    assert!(close_pt(l.p2, Point2::new(0.0, 2.0)));
}

// ============================================================================
// Aliases
// ============================================================================

#[test]
fn commands_expose_their_autocad_aliases() {
    let reg = registry();
    for (canon, alias) in [
        ("CHAMFER", "CHA"),
        ("BREAK", "BR"),
        ("JOIN", "J"),
        ("LENGTHEN", "LEN"),
        ("ALIGN", "AL"),
    ] {
        let by_name = reg.lookup(canon).map(|s| s.name().to_string());
        let by_alias = reg.lookup(alias).map(|s| s.name().to_string());
        assert_eq!(by_name, Some(canon.to_string()), "falta {canon}");
        assert_eq!(by_alias, by_name, "el alias {alias} debe apuntar a {canon}");
    }
    assert!(reg.lookup("BREAKATPOINT").is_some());
}
