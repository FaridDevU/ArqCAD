//! Integration tests for `af_model::extents::doc_extents` through the public API.
//!
//! Entity and layer state setup goes through `Session` and `TxContext`.

use af_math::{BBox, Point2};
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo,
};
use af_model::extents::{ExtentsFilter, doc_extents};
use af_model::id::{BlockId, EntityId, LayerId, ObjectId, StyleId};
use af_model::units::Units;
use af_model::{ContainerRef, Layer, Session, TxError};
use proptest::prelude::*;

// --------------------------------------------------------------------------
// Public geometry and record setup.
// --------------------------------------------------------------------------

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> EntityGeometry {
    EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2)))
}

fn point(x: f64, y: f64) -> EntityGeometry {
    EntityGeometry::Point(PointGeo::new(Point2::new(x, y)))
}

fn circle(x: f64, y: f64, r: f64) -> EntityGeometry {
    EntityGeometry::Circle(CircleGeo::new(Point2::new(x, y), r))
}

fn rec(layer: LayerId, geo: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geo,
    )
}

fn continuous(session: &Session) -> StyleId {
    session.document().line_types().next().unwrap().id()
}

/// Model-space extents under `filter`.
fn ms_extents(session: &Session, filter: ExtentsFilter) -> Option<BBox> {
    doc_extents(session.document(), ContainerRef::ModelSpace, filter)
}

fn add(session: &mut Session, layer: LayerId, geo: EntityGeometry) -> EntityId {
    session
        .transact("add", move |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, rec(layer, geo))
        })
        .expect("commit add")
        .value
}

fn add_layer(session: &mut Session, name: &str, lt: StyleId) -> LayerId {
    let layer = Layer::new(
        ObjectId::NIL.into(),
        name,
        Color::aci(1).unwrap(),
        lt,
        Lineweight::ByLayer,
    );
    session
        .transact("add layer", move |tx| -> Result<LayerId, TxError> {
            tx.add_layer_raw(layer)
        })
        .expect("commit add layer")
        .value
}

fn set_layer_states(session: &mut Session, id: LayerId, off: bool, frozen: bool) {
    let derived = session
        .document()
        .layer(id)
        .unwrap()
        .clone()
        .with_off(off)
        .with_frozen(frozen);
    session
        .transact("layer flags", move |tx| -> Result<(), TxError> {
            tx.modify_layer_raw(id, derived)
        })
        .expect("commit layer flags");
}

fn hide_entity(session: &mut Session, id: EntityId) {
    session
        .transact("hide", move |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |r| r.visible = false)
        })
        .expect("commit hide");
}

fn ent_bbox(session: &Session, id: EntityId) -> BBox {
    session.document().entity(id).unwrap().0.geometry.bbox()
}

// --------------------------------------------------------------------------
// Empty, fully hidden, and missing containers return `None`.
// --------------------------------------------------------------------------

#[test]
fn documento_vacio_no_tiene_extents() {
    let session = Session::new(Units::default());
    assert!(ms_extents(&session, ExtentsFilter::Visible).is_none());
    assert!(ms_extents(&session, ExtentsFilter::All).is_none());
}

#[test]
fn contenedor_inexistente_es_none() {
    let session = Session::new(Units::default());
    let ghost: BlockId = ObjectId(9999).into();
    let doc = session.document();
    let ext = doc_extents(doc, ContainerRef::Block(ghost), ExtentsFilter::All);
    assert!(ext.is_none());
}

#[test]
fn todo_oculto_es_none_en_visible_pero_all_lo_ve() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add(&mut session, l0, point(5.0, 5.0));
    hide_entity(&mut session, id);

    // No visible geometry remains, so no origin box is fabricated.
    assert!(ms_extents(&session, ExtentsFilter::Visible).is_none());
    // The unfiltered path includes the hidden entity.
    let all = ms_extents(&session, ExtentsFilter::All).expect("All ve la entidad oculta");
    assert!(all.contains_point(Point2::new(5.0, 5.0)));
}

// --------------------------------------------------------------------------
// Visible extents filter inactive layers and hidden entities.
// --------------------------------------------------------------------------

#[test]
fn extents_visible_filtra_off_frozen_y_entity_visible() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let lt = continuous(&session);

    let apagada = add_layer(&mut session, "Apagada", lt);
    set_layer_states(&mut session, apagada, true, false);
    let congelada = add_layer(&mut session, "Congelada", lt);
    set_layer_states(&mut session, congelada, false, true);

    // The line on layer "0" is the only visible entity.
    add(&mut session, l0, line(0.0, 0.0, 10.0, 10.0));
    // Entity on an off layer.
    add(&mut session, apagada, point(100.0, 100.0));
    // Entity on a frozen layer.
    add(&mut session, congelada, point(-50.0, -50.0));
    // Hidden entity on layer "0".
    let hidden = add(&mut session, l0, point(200.0, 200.0));
    hide_entity(&mut session, hidden);

    // Visible extents contain only the line.
    let visible = ms_extents(&session, ExtentsFilter::Visible).expect("hay una entidad visible");
    let expected_visible = BBox::new(Point2::new(0.0, 0.0), Point2::new(10.0, 10.0));
    assert_eq!(visible, expected_visible);

    // Unfiltered extents include every entity.
    let all = ms_extents(&session, ExtentsFilter::All).expect("hay entidades");
    let expected_all = BBox::new(Point2::new(-50.0, -50.0), Point2::new(200.0, 200.0));
    assert_eq!(all, expected_all);

    // Visible extents are contained by unfiltered extents.
    assert!(all.contains_bbox(visible));
}

// --------------------------------------------------------------------------
// Exact bounding-box union.
// --------------------------------------------------------------------------

#[test]
fn extents_es_union_de_las_cajas_de_las_entidades() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();

    let a = add(&mut session, l0, line(-3.0, 2.0, 4.0, 2.0));
    let b = add(&mut session, l0, point(1.0, -7.0));
    let c = add(&mut session, l0, circle(10.0, 0.0, 5.0));

    let expected = ent_bbox(&session, a)
        .union(ent_bbox(&session, b))
        .union(ent_bbox(&session, c));

    let got = ms_extents(&session, ExtentsFilter::Visible).expect("hay entidades");
    assert_eq!(got, expected);
    // Circle spans x=[5,15], y=[-5,5]; point reaches y=-7; line reaches x=-3.
    let expected_box = BBox::new(Point2::new(-3.0, -7.0), Point2::new(15.0, 5.0));
    assert_eq!(got, expected_box);
}

// --------------------------------------------------------------------------
// Property: visible extents contain every visible entity box.
// --------------------------------------------------------------------------

fn coord() -> impl Strategy<Value = f64> {
    -1.0e4f64..1.0e4
}

fn arb_geo() -> impl Strategy<Value = EntityGeometry> {
    prop_oneof![
        (coord(), coord(), coord(), coord()).prop_map(|(a, b, c, d)| line(a, b, c, d)),
        (coord(), coord()).prop_map(|(x, y)| point(x, y)),
        (coord(), coord(), 1.0f64..1.0e4).prop_map(|(x, y, r)| circle(x, y, r)),
    ]
}

proptest! {
    #[test]
    fn extents_visible_contiene_cada_entidad_visible(geos in prop::collection::vec(arb_geo(), 1..12)) {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();

        let mut ids = Vec::new();
        for g in geos {
            ids.push(add(&mut session, l0, g));
        }

        let visible = ms_extents(&session, ExtentsFilter::Visible).expect("todas visibles -> Some");
        for id in &ids {
            let bb = ent_bbox(&session, *id);
            // Scale tolerance with coordinates for floating-point box unions.
            prop_assert!(
                visible.expand(1e-6).contains_bbox(bb),
                "extents {visible:?} no contiene la caja {bb:?}"
            );
        }

        // Visible extents remain within unfiltered extents.
        let all = ms_extents(&session, ExtentsFilter::All).expect("hay entidades");
        prop_assert!(all.expand(1e-9).contains_bbox(visible));
    }
}
