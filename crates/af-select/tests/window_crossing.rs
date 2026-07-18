//! Window and crossing semantics for inside, outside, and boundary contact.

mod common;

use af_math::{BBox, Point2};
use af_model::entity::{Color, Lineweight};
use af_model::id::ObjectId;
use af_model::{ContainerRef, Layer, Session, TxError};
use af_select::{SpatialIndex, WindowMode, select_window};
use common::{add, circle_rec, line_rec, session};

fn rect() -> BBox {
    BBox::new(Point2::new(0.0, 0.0), Point2::new(5.0, 5.0))
}

fn window(s: &Session, idx: &SpatialIndex) -> Vec<af_model::id::EntityId> {
    select_window(s.document(), idx, rect(), WindowMode::Window)
}
fn crossing(s: &Session, idx: &SpatialIndex) -> Vec<af_model::id::EntityId> {
    select_window(s.document(), idx, rect(), WindowMode::Crossing)
}

#[test]
fn dentro_contenida_ambos_modos() {
    let mut s = session();
    let layer = s.document().current_layer();
    let e = add(
        &mut s,
        line_rec(layer, Point2::new(1.0, 1.0), Point2::new(4.0, 4.0)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert_eq!(window(&s, &idx), vec![e]);
    assert_eq!(crossing(&s, &idx), vec![e]);
}

#[test]
fn cruza_borde_solo_crossing() {
    // A horizontal line exits the right side with degenerate zero-height bounds.
    let mut s = session();
    let layer = s.document().current_layer();
    let e = add(
        &mut s,
        line_rec(layer, Point2::new(3.0, 3.0), Point2::new(8.0, 3.0)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert!(window(&s, &idx).is_empty(), "no contenida => window vacío");
    assert_eq!(crossing(&s, &idx), vec![e], "cruza el borde => crossing");
}

#[test]
fn fuera_bbox_disjunta_ninguno() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(
        &mut s,
        line_rec(layer, Point2::new(7.0, 7.0), Point2::new(9.0, 9.0)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert!(window(&s, &idx).is_empty());
    assert!(crossing(&s, &idx).is_empty());
}

#[test]
fn fuera_bbox_solapa_pero_geometria_no_toca() {
    // A diagonal's bounds overlap the corner while exact geometry stays outside.
    let mut s = session();
    let layer = s.document().current_layer();
    add(
        &mut s,
        line_rec(layer, Point2::new(4.6, 6.0), Point2::new(6.0, 4.6)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert!(window(&s, &idx).is_empty());
    assert!(
        crossing(&s, &idx).is_empty(),
        "geometría fuera => crossing vacío"
    );
}

#[test]
fn circulo_que_rodea_el_rect_no_es_crossing() {
    // A large circle surrounds the rectangle without its ring touching it.
    let mut s = session();
    let layer = s.document().current_layer();
    add(&mut s, circle_rec(layer, Point2::new(2.5, 2.5), 10.0));
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert!(window(&s, &idx).is_empty());
    assert!(crossing(&s, &idx).is_empty());
}

#[test]
fn tocando_borde_desde_dentro_es_contenida() {
    // A segment on the right boundary is included by window containment.
    let mut s = session();
    let layer = s.document().current_layer();
    let e = add(
        &mut s,
        line_rec(layer, Point2::new(5.0, 1.0), Point2::new(5.0, 4.0)),
    );
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert_eq!(window(&s, &idx), vec![e]);
    assert_eq!(crossing(&s, &idx), vec![e]);
}

#[test]
fn crossing_circulo_que_cruza_el_borde() {
    // A corner-centered circle crosses two sides but is not contained.
    let mut s = session();
    let layer = s.document().current_layer();
    let e = add(&mut s, circle_rec(layer, Point2::new(5.0, 5.0), 2.0));
    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

    assert!(window(&s, &idx).is_empty());
    assert_eq!(crossing(&s, &idx), vec![e]);
}

#[test]
fn filtros_de_capa_y_orden_determinista() {
    // Normal and locked layers remain selectable; off is excluded.
    let mut s = session();
    let continuous = s.document().line_types().next().unwrap().id();
    let mk = |s: &mut Session, name: &str, f: fn(Layer) -> Layer| {
        let base = Layer::new(
            ObjectId::NIL.into(),
            name,
            Color::aci(1).unwrap(),
            continuous,
            Lineweight::ByLayer,
        );
        s.transact("mklayer", |tx| -> Result<_, TxError> {
            tx.add_layer_raw(f(base))
        })
        .unwrap()
        .value
    };
    let locked = mk(&mut s, "locked", |l| l.with_locked(true));
    let off = mk(&mut s, "off", |l| l.with_off(true));
    let normal = s.document().current_layer();

    let e_normal = add(
        &mut s,
        line_rec(normal, Point2::new(1.0, 1.0), Point2::new(2.0, 2.0)),
    );
    let e_locked = add(
        &mut s,
        line_rec(locked, Point2::new(1.0, 1.0), Point2::new(2.0, 2.0)),
    );
    let _e_off = add(
        &mut s,
        line_rec(off, Point2::new(1.0, 1.0), Point2::new(2.0, 2.0)),
    );

    let idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
    // Draw order places normal before locked; off remains excluded.
    assert_eq!(window(&s, &idx), vec![e_normal, e_locked]);
    assert_eq!(crossing(&s, &idx), vec![e_normal, e_locked]);
}
