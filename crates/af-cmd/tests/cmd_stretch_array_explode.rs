//! End-to-end STRETCH, ARRAY, and EXPLODE tests for transactions, IDs, undo, and
//! unsupported geometry.

use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{
    ArcGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PolyVertex,
    PolylineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use core::f64::consts::TAU;
use serde_json::{Value, json};

fn registry() -> CommandRegistry {
    let mut reg = CommandRegistry::new();
    af_cmd::builtin::register_builtins(&mut reg).expect("register builtins");
    reg
}

fn mk(layer: LayerId, g: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        g,
    )
}

fn seed(session: &mut Session, geoms: Vec<EntityGeometry>) -> Vec<EntityId> {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
            geoms
                .into_iter()
                .map(|g| tx.add_entity(ContainerRef::ModelSpace, mk(layer, g)))
                .collect()
        })
        .expect("seed commits")
        .value
}

fn ids_json(ids: &[EntityId]) -> Vec<u64> {
    ids.iter().map(|id| id.raw().0).collect()
}

fn geom(session: &Session, id: EntityId) -> EntityGeometry {
    session.document().entity(id).unwrap().0.geometry.clone()
}

fn model_count(session: &Session) -> usize {
    session.document().model_space().iter().count()
}

// ---- STRETCH ---------------------------------------------------------------

#[test]
fn stretch_moves_contained_endpoint_one_tx_and_undo_byte_identical() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();

    let args: Value = json!({
        "entities": ids_json(&ids),
        "corner1": [9.0, -1.0],
        "corner2": [11.0, 1.0],
        "base": [0.0, 0.0],
        "to": [0.0, 5.0],
    });
    let out = reg
        .execute(&mut session, "STRETCH", &args)
        .expect("stretch ok");
    assert!(out.tx_seq.is_some(), "affects_document => 1 tx");
    assert!(out.created.is_empty(), "STRETCH no crea entidades");

    match geom(&session, ids[0]) {
        EntityGeometry::Line(l) => {
            assert_eq!(l.p1, Point2::new(0.0, 0.0), "extremo fuera fijo");
            assert_eq!(l.p2, Point2::new(10.0, 5.0), "extremo dentro estirado");
        }
        other => panic!("esperaba línea, fue {other:?}"),
    }

    session.undo().expect("undo");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn stretch_via_alias_s_works() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::ORIGIN,
            Point2::new(10.0, 0.0),
        ))],
    );
    let args = json!({
        "entities": ids_json(&ids),
        "corner1": [9.0, -1.0], "corner2": [11.0, 1.0],
        "base": [0.0, 0.0], "to": [1.0, 0.0],
    });
    assert!(reg.execute(&mut session, "S", &args).is_ok(), "alias S");
}

#[test]
fn stretch_arc_single_endpoint_is_rejected_atomically() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Arc(ArcGeo::new(
            Point2::ORIGIN,
            1.0,
            0.0,
            core::f64::consts::FRAC_PI_2,
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();
    let args = json!({
        "entities": ids_json(&ids),
        "corner1": [0.5, -0.5], "corner2": [1.5, 0.5],
        "base": [0.0, 0.0], "to": [1.0, 0.0],
    });
    let err = reg.execute(&mut session, "STRETCH", &args).unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(
        before,
        serde_json::to_string(session.document()).unwrap(),
        "rollback atómico"
    );
}

// ---- ARRAY -----------------------------------------------------------------

#[test]
fn array_rect_creates_grid_minus_origin_one_tx() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::ORIGIN,
            Point2::new(1.0, 0.0),
        ))],
    );
    let args = json!({
        "entities": ids_json(&ids),
        "mode": "rect",
        "rows": 2, "cols": 3, "spacing": [10.0, 20.0],
    });
    let out = reg.execute(&mut session, "ARRAY", &args).expect("array ok");
    assert!(out.tx_seq.is_some());
    assert_eq!(out.created.len(), 5, "2*3 - 1 copias");
    assert_eq!(model_count(&session), 6, "original + 5 copias");

    session.undo().expect("undo");
    assert_eq!(model_count(&session), 1);
}

#[test]
fn array_polar_full_turn_via_alias_ar() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::ORIGIN,
            Point2::new(1.0, 0.0),
        ))],
    );
    let args = json!({
        "entities": ids_json(&ids),
        "mode": "polar",
        "center": [0.0, 0.0], "items": 4, "angle": TAU, "rotate": true,
    });
    let out = reg
        .execute(&mut session, "AR", &args)
        .expect("array polar ok");
    assert_eq!(out.created.len(), 3, "4 items => 3 copias");
    let l = match geom(&session, out.created[0]) {
        EntityGeometry::Line(g) => g,
        other => panic!("esperaba línea, fue {other:?}"),
    };
    let tol = 1e-9;
    assert!(l.p2.x.abs() < tol && (l.p2.y - 1.0).abs() < tol);
}

#[test]
fn array_rect_single_cell_errors() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::ORIGIN,
            Point2::new(1.0, 0.0),
        ))],
    );
    let args = json!({ "entities": ids_json(&ids), "mode": "rect", "rows": 1, "cols": 1 });
    let err = reg.execute(&mut session, "ARRAY", &args).unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(model_count(&session), 1, "nada creado");
}

// ---- EXPLODE ---------------------------------------------------------------

#[test]
fn explode_polyline_into_pieces_one_tx() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let poly = PolylineGeo::new(
        vec![
            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
            PolyVertex::new(Point2::new(10.0, 0.0), 1.0),
            PolyVertex::new(Point2::new(10.0, 10.0), 0.0),
        ],
        false,
    );
    let ids = seed(&mut session, vec![EntityGeometry::Polyline(poly)]);

    let args = json!({ "entities": ids_json(&ids) });
    let out = reg
        .execute(&mut session, "EXPLODE", &args)
        .expect("explode ok");
    assert!(out.tx_seq.is_some());
    assert_eq!(out.created.len(), 2, "2 tramos => 2 piezas");
    assert!(session.document().entity(ids[0]).is_none());
    assert_eq!(model_count(&session), 2);
    let kinds: Vec<&str> = out
        .created
        .iter()
        .map(|&id| match geom(&session, id) {
            EntityGeometry::Line(_) => "line",
            EntityGeometry::Arc(_) => "arc",
            _ => "other",
        })
        .collect();
    assert!(
        kinds.contains(&"line") && kinds.contains(&"arc"),
        "{kinds:?}"
    );

    session.undo().expect("undo");
    assert_eq!(model_count(&session), 1, "undo restaura la polilínea");
    assert!(session.document().entity(ids[0]).is_some());
}

#[test]
fn explode_non_polyline_is_rejected_atomically_via_alias_x() {
    let reg = registry();
    let mut session = Session::new(Units::default());
    let ids = seed(
        &mut session,
        vec![EntityGeometry::Line(LineGeo::new(
            Point2::ORIGIN,
            Point2::new(1.0, 1.0),
        ))],
    );
    let before = serde_json::to_string(session.document()).unwrap();
    let err = reg
        .execute(&mut session, "X", &json!({ "entities": ids_json(&ids) }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)), "fue {err:?}");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}
