//! Advanced polygon, fence, lasso, QSELECT, and SELECTSIMILAR scenarios.

mod common;

use af_math::Point2;
use af_model::entity::{
    Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
};
use af_model::id::{LayerId, ObjectId};
use af_model::{ContainerRef, Layer, Session, TxError};
use af_select::{
    EntityKind, SelectionFilter, SpatialIndex, WindowMode, apply_filter, select_fence,
    select_polygon, select_similar,
};
use common::{add, circle_rec, line_rec, point_rec, session};

/// Concave `[0,10]²` polygon with a triangular notch at the top.
fn concave() -> Vec<Point2> {
    vec![
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(10.0, 10.0),
        Point2::new(5.0, 5.0),
        Point2::new(0.0, 10.0),
    ]
}

fn idx(s: &Session) -> SpatialIndex {
    SpatialIndex::build(s.document(), ContainerRef::ModelSpace)
}

#[test]
fn wpolygon_contiene_solo_lo_totalmente_dentro() {
    let mut s = session();
    let layer = s.document().current_layer();
    let dentro = add(
        &mut s,
        line_rec(layer, Point2::new(2.0, 1.0), Point2::new(3.0, 1.0)),
    );
    // Crosses the right boundary at x=10 and is not contained.
    let cruza = add(
        &mut s,
        line_rec(layer, Point2::new(2.0, 2.0), Point2::new(12.0, 2.0)),
    );
    let ix = idx(&s);
    let poly = concave();

    let w = select_polygon(s.document(), &ix, &poly, WindowMode::Window);
    assert_eq!(w, vec![dentro], "window: solo la contenida");

    let c = select_polygon(s.document(), &ix, &poly, WindowMode::Crossing);
    assert!(c.contains(&dentro) && c.contains(&cruza), "crossing: ambas");
}

#[test]
fn concavidad_un_punto_en_la_muesca_queda_fuera() {
    let mut s = session();
    let layer = s.document().current_layer();
    let dentro = add(&mut s, point_rec(layer, Point2::new(5.0, 2.0)));
    let en_muesca = add(&mut s, point_rec(layer, Point2::new(5.0, 8.0)));
    let ix = idx(&s);
    let poly = concave();

    for mode in [WindowMode::Window, WindowMode::Crossing] {
        let sel = select_polygon(s.document(), &ix, &poly, mode);
        assert_eq!(
            sel,
            vec![dentro],
            "el punto en la muesca (5,8) está fuera del polígono cóncavo ({mode:?})"
        );
        assert!(!sel.contains(&en_muesca));
    }
}

#[test]
fn cpolygon_toca_un_lado() {
    // A circle centered at the corner crosses polygon sides but is not contained.
    let mut s = session();
    let layer = s.document().current_layer();
    let c = add(&mut s, circle_rec(layer, Point2::new(10.0, 10.0), 2.0));
    let ix = idx(&s);
    let poly = concave();

    assert!(
        select_polygon(s.document(), &ix, &poly, WindowMode::Window).is_empty(),
        "sobresale => window vacío"
    );
    assert_eq!(
        select_polygon(s.document(), &ix, &poly, WindowMode::Crossing),
        vec![c],
        "cruza el lado => crossing"
    );
}

#[test]
fn polygon_degenerado_devuelve_vacio() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(&mut s, point_rec(layer, Point2::new(5.0, 2.0)));
    let ix = idx(&s);
    // Fewer than three vertices do not bound an area.
    let two = vec![Point2::new(0.0, 0.0), Point2::new(10.0, 10.0)];
    assert!(select_polygon(s.document(), &ix, &two, WindowMode::Crossing).is_empty());
}

#[test]
fn fence_selecciona_lo_que_cruza_la_valla() {
    let mut s = session();
    let layer = s.document().current_layer();
    // Open horizontal fence across the full width at y=3.
    let cruza = add(
        &mut s,
        line_rec(layer, Point2::new(4.0, 0.0), Point2::new(4.0, 6.0)),
    );
    // Geometry above the fence does not cross it.
    let arriba = add(
        &mut s,
        line_rec(layer, Point2::new(1.0, 5.0), Point2::new(2.0, 5.0)),
    );
    let ix = idx(&s);
    let fence = vec![Point2::new(-1.0, 3.0), Point2::new(12.0, 3.0)];

    let sel = select_fence(s.document(), &ix, &fence);
    assert_eq!(sel, vec![cruza], "solo la que cruza la valla");
    assert!(!sel.contains(&arriba));
}

#[test]
fn lazo_es_un_poligono_de_puntos_densos() {
    // A dense cursor trace should match the equivalent four-vertex polygon.
    let mut s = session();
    let layer = s.document().current_layer();
    let dentro = add(
        &mut s,
        line_rec(layer, Point2::new(2.0, 2.0), Point2::new(3.0, 3.0)),
    );
    let ix = idx(&s);

    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(10.0, 10.0),
        Point2::new(0.0, 10.0),
    ];
    // Subdivide each side into 40 segments to model a lasso trace.
    let mut lasso = Vec::new();
    for k in 0..corners.len() {
        let a = corners[k];
        let b = corners[(k + 1) % corners.len()];
        for i in 0..40 {
            let f = i as f64 / 40.0;
            lasso.push(Point2::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f));
        }
    }
    assert!(lasso.len() > 100, "traza densa");
    assert_eq!(
        select_polygon(s.document(), &ix, &lasso, WindowMode::Window),
        vec![dentro],
        "el lazo denso contiene la línea igual que el cuadrado de 4 vértices"
    );
}

// ---- QSELECT and SELECTSIMILAR filters ----

fn add_layer(session: &mut Session, name: &str) -> LayerId {
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
            tx.add_layer_raw(base)
        })
        .expect("mklayer")
        .value
}

/// Builds a line with an explicit color on one layer.
fn colored_line(layer: LayerId, color: Color, a: Point2, b: Point2) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        color,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Line(LineGeo::new(a, b)),
    )
}

#[test]
fn apply_filter_por_tipo_capa_y_color() {
    let mut s = session();
    let a = s.document().current_layer();
    let b = add_layer(&mut s, "B");
    let red = Color::aci(1).unwrap();

    let l_bylayer = add(
        &mut s,
        line_rec(a, Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
    );
    let l_red = add(
        &mut s,
        colored_line(a, red, Point2::new(0.0, 1.0), Point2::new(1.0, 1.0)),
    );
    let c_bylayer = add(&mut s, circle_rec(a, Point2::new(5.0, 5.0), 1.0));
    let p_on_b = add(
        &mut s,
        EntityRecord::new(
            ObjectId::NIL.into(),
            b,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(9.0, 9.0))),
        ),
    );
    let all = vec![l_bylayer, l_red, c_bylayer, p_on_b];

    // Lines only.
    let f = SelectionFilter {
        kinds: Some(vec![EntityKind::Line]),
        ..Default::default()
    };
    assert_eq!(
        apply_filter(all.clone(), s.document(), &f),
        vec![l_bylayer, l_red]
    );

    // Layer B only.
    let f = SelectionFilter {
        layers: Some(vec![b]),
        ..Default::default()
    };
    assert_eq!(apply_filter(all.clone(), s.document(), &f), vec![p_on_b]);

    // Explicit red only.
    let f = SelectionFilter {
        colors: Some(vec![red]),
        ..Default::default()
    };
    assert_eq!(apply_filter(all.clone(), s.document(), &f), vec![l_red]);

    // Populated fields combine with AND semantics.
    let f = SelectionFilter {
        kinds: Some(vec![EntityKind::Line]),
        layers: Some(vec![a]),
        colors: Some(vec![red]),
    };
    assert_eq!(apply_filter(all, s.document(), &f), vec![l_red]);
}

#[test]
fn select_similar_tipo_capa_y_color_si_no_bylayer() {
    let mut s = session();
    let a = s.document().current_layer();
    let b = add_layer(&mut s, "B");
    let red = Color::aci(1).unwrap();

    let l_bylayer = add(
        &mut s,
        line_rec(a, Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
    );
    let l_red = add(
        &mut s,
        colored_line(a, red, Point2::new(0.0, 1.0), Point2::new(1.0, 1.0)),
    );
    let _c_on_a = add(&mut s, circle_rec(a, Point2::new(5.0, 5.0), 1.0));
    let _l_on_b = add(
        &mut s,
        line_rec(b, Point2::new(2.0, 2.0), Point2::new(3.0, 3.0)),
    );

    // A ByLayer reference ignores color and matches every line on layer A.
    assert_eq!(
        select_similar(s.document(), l_bylayer),
        vec![l_bylayer, l_red],
    );

    // An explicit reference color adds color matching.
    assert_eq!(select_similar(s.document(), l_red), vec![l_red]);
}
