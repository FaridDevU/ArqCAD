//! Exact `pick` scenarios with hand-calculated distances.

mod common;

use af_math::Point2;
use af_model::entity::{Color, Lineweight};
use af_model::id::ObjectId;
use af_model::{ContainerRef, Layer, Session, TxError};
use af_select::{SpatialIndex, pick, pick_all};
use common::{add, circle_rec, line_rec, session};

/// Creates a layer, applies `modifier`, and returns its ID.
fn add_layer(
    session: &mut Session,
    name: &str,
    modifier: impl FnOnce(Layer) -> Layer,
) -> af_model::id::LayerId {
    let continuous = session.document().line_types().next().unwrap().id();
    let base = Layer::new(
        ObjectId::NIL.into(),
        name,
        Color::aci(1).unwrap(),
        continuous,
        Lineweight::ByLayer,
    );
    session
        .transact("mklayer", |tx| -> Result<_, TxError> {
            tx.add_layer_raw(modifier(base))
        })
        .expect("mklayer")
        .value
}

#[test]
fn pick_ordena_por_distancia_ascendente() {
    // Horizontal line at draw order 0 and circle at draw order 1. At (3,1):
    // line distance is 1.0; circle distance is `|sqrt(20) - 3|`.
    let mut s = session();
    let layer = s.document().current_layer();
    let line = add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
    );
    let circle = add(&mut s, circle_rec(layer, Point2::new(5.0, 5.0), 3.0));

    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    let hits = pick(s.document(), &idx, Point2::new(3.0, 1.0), 3.0);

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, line);
    assert!(
        (hits[0].dist - 1.0).abs() < 1e-12,
        "línea dist {}",
        hits[0].dist
    );
    assert_eq!(hits[1].id, circle);
    let expected = (20.0f64).sqrt() - 3.0;
    assert!(
        (hits[1].dist - expected).abs() < 1e-12,
        "círculo dist {} != {}",
        hits[1].dist,
        expected
    );
}

#[test]
fn pick_empate_por_draw_order_desc() {
    // A tangent line and circle both have zero distance at (5,0).
    let mut s = session();
    let layer = s.document().current_layer();
    let line = add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
    );
    let circle = add(&mut s, circle_rec(layer, Point2::new(5.0, 5.0), 5.0));

    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    let hits = pick(s.document(), &idx, Point2::new(5.0, 0.0), 0.1);

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].dist, 0.0);
    assert_eq!(hits[1].dist, 0.0);
    // Exact ties prefer the entity drawn later.
    assert_eq!(hits[0].id, circle);
    assert_eq!(hits[1].id, line);
}

#[test]
fn pick_usa_geometria_real_no_bbox() {
    // Bounds contain the point, but exact diagonal geometry is too far away.
    let mut s = session();
    let layer = s.document().current_layer();
    let _line = add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 10.0), Point2::new(10.0, 0.0)),
    );

    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    // The point is inside the bounds but about 6.36 units from `x+y=10`.
    let hits = pick(s.document(), &idx, Point2::new(0.5, 0.5), 0.2);
    assert!(hits.is_empty(), "no debe acertar por bbox: {hits:?}");
}

#[test]
fn pick_fuera_de_tolerancia_no_acierta() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    // Distance 0.5 exceeds tolerance 0.4.
    assert!(pick(s.document(), &idx, Point2::new(5.0, 0.5), 0.4).is_empty());
    // Tolerance 0.6 accepts distance 0.5.
    let hits = pick(s.document(), &idx, Point2::new(5.0, 0.5), 0.6);
    assert_eq!(hits.len(), 1);
    assert!((hits[0].dist - 0.5).abs() < 1e-12);
}

#[test]
fn pick_flag_locked_y_exclusion_off_frozen() {
    let mut s = session();
    let locked = add_layer(&mut s, "locked", |l| l.with_locked(true));
    let off = add_layer(&mut s, "off", |l| l.with_off(true));
    let frozen = add_layer(&mut s, "frozen", |l| l.with_frozen(true));

    // Four coincident circles on layers with different states.
    let normal_layer = s.document().current_layer();
    let c_normal = add(&mut s, circle_rec(normal_layer, Point2::new(5.0, 5.0), 5.0));
    let c_locked = add(&mut s, circle_rec(locked, Point2::new(5.0, 5.0), 5.0));
    let _c_off = add(&mut s, circle_rec(off, Point2::new(5.0, 5.0), 5.0));
    let _c_frozen = add(&mut s, circle_rec(frozen, Point2::new(5.0, 5.0), 5.0));

    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    let hits = pick(s.document(), &idx, Point2::new(5.0, 0.0), 0.1);

    // Off and frozen are excluded; normal and locked remain.
    let ids: Vec<_> = hits.iter().map(|h| h.id).collect();
    assert!(ids.contains(&c_normal));
    assert!(ids.contains(&c_locked));
    assert_eq!(hits.len(), 2, "solo normal + locked: {hits:?}");

    let locked_hit = hits.iter().find(|h| h.id == c_locked).unwrap();
    assert!(locked_hit.locked, "el círculo en capa locked lleva flag");
    let normal_hit = hits.iter().find(|h| h.id == c_normal).unwrap();
    assert!(!normal_hit.locked);
}

#[test]
fn pick_all_expone_el_ciclo_completo_en_orden_de_pick() {
    // `pick_all` preserves `pick` ranking when projected to IDs.
    let mut s = session();
    let layer = s.document().current_layer();
    let line = add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
    );
    let circle = add(&mut s, circle_rec(layer, Point2::new(5.0, 5.0), 5.0));

    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    let pt = Point2::new(5.0, 0.0);
    let ids = pick_all(s.document(), &idx, pt, 0.1);

    // At equal zero distance, the later-drawn circle ranks first.
    assert_eq!(ids, vec![circle, line]);
    // Result exactly matches projecting `pick` to IDs.
    let from_pick: Vec<_> = pick(s.document(), &idx, pt, 0.1)
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert_eq!(ids, from_pick);
}

#[test]
fn pick_vacio_sin_candidatos() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    // Query far from all geometry.
    assert!(pick(s.document(), &idx, Point2::new(1000.0, 1000.0), 0.5).is_empty());
}
