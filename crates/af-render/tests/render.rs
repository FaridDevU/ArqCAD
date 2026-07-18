//! Integration tests for render-model goldens, bounding boxes, ChangeSet deltas,
//! determinism, and chord-error policy.
//!
//! Documents are built through the same public transaction boundary used by UI
//! callers.

use std::collections::HashMap;

use af_math::Point2;
use af_model::Session;
use af_model::container::ContainerRef;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo, PolyVertex, PolylineGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId, StyleId};
use af_model::layers::Layer;
use af_model::tx::TxError;
use af_model::units::Units;
use af_render::{
    BatchKey, BatchUpdate, MarkerKind, PrimGeom, RenderModel, RenderOpts, Rgba, apply_changeset,
    build_full,
};

const RED: Rgba = Rgba::new(255, 0, 0, 255);
const GREEN: Rgba = Rgba::new(0, 255, 0, 255);
const BLUE: Rgba = Rgba::new(0, 0, 255, 255);
const POINT_RGB: Rgba = Rgba::new(10, 20, 30, 255);

/// Notable fixture IDs.
struct Fx {
    layer0: LayerId,
    /// Continuous line type resolved from fixture ByLayer values.
    continuous: StyleId,
    walls: LayerId,
    hidden: LayerId,
    line: EntityId,
    poly: EntityId,
    circle: EntityId,
    point: EntityId,
    hidden_line: EntityId,
}

/// Golden document with two styled layers, four entity classes, and one entity
/// on an off layer to exercise visibility.
///
/// - `Walls`: on, ACI 1 red, 0.5 mm.
/// - `Hidden`: off, ACI 5 blue.
/// - `line` and `poly`: Walls with ByLayer red.
/// - `circle`: Walls with explicit ACI 3 green.
/// - `point`: layer 0 with explicit RGB(10, 20, 30).
/// - `hidden_line`: Hidden and therefore not rendered.
fn fixture() -> (Session, Fx) {
    let mut session = Session::new(Units::default());
    let continuous = session.document().line_types().next().unwrap().id();
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    let placeholder: LayerId = ObjectId(0).into();

    let out = session
        .transact::<_, TxError, _>("fixture", |tx| {
            let walls = tx.add_layer_raw(
                Layer::new(
                    placeholder,
                    "Walls",
                    Color::aci(1).unwrap(),
                    continuous,
                    Lineweight::Mm(0.5),
                )
                .with_off(false),
            )?;
            let hidden = tx.add_layer_raw(
                Layer::new(
                    placeholder,
                    "Hidden",
                    Color::aci(5).unwrap(),
                    continuous,
                    Lineweight::Mm(0.35),
                )
                .with_off(true),
            )?;

            let line = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    walls,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer, // Walls resolves to 0.5 mm.
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(4.0, 0.0),
                    )),
                ),
            )?;
            let poly = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    walls,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::Mm(0.25),
                    EntityGeometry::Polyline(PolylineGeo::new(
                        vec![
                            PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
                            PolyVertex::new(Point2::new(10.0, 0.0), 1.0), // Arc segment.
                            PolyVertex::new(Point2::new(20.0, 0.0), 0.0),
                        ],
                        false,
                    )),
                ),
            )?;
            let circle = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    walls,
                    Color::aci(3).unwrap(), // Explicit green.
                    LineTypeRef::ByLayer,
                    Lineweight::Mm(0.25),
                    EntityGeometry::Circle(CircleGeo::new(Point2::new(5.0, 5.0), 3.0)),
                ),
            )?;
            let point = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    layer0,
                    Color::Rgb(10, 20, 30),
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Point(PointGeo::new(Point2::new(-1.0, -2.0))),
                ),
            )?;
            let hidden_line = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    hidden,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(1.0, 1.0),
                    )),
                ),
            )?;
            Ok((walls, hidden, line, poly, circle, point, hidden_line))
        })
        .unwrap();

    let (walls, hidden, line, poly, circle, point, hidden_line) = out.value;
    (
        session,
        Fx {
            layer0,
            continuous,
            walls,
            hidden,
            line,
            poly,
            circle,
            point,
            hidden_line,
        },
    )
}

fn opts() -> RenderOpts {
    RenderOpts::new(0.1)
}

/// Key-to-batch index for comparisons that ignore batch order.
fn as_map(m: &RenderModel) -> HashMap<BatchKey, &af_render::RenderBatch> {
    m.batches.iter().map(|b| (b.key, b)).collect()
}

// ===================== Golden snapshot =====================

#[test]
fn golden_render_model() {
    let (session, fx) = fixture();
    let model = build_full(session.document(), &opts());

    // Three visible batches; the off-layer line is absent.
    assert_eq!(model.batches.len(), 3, "batches: {:#?}", model.batches);

    // Batch 0: red Walls line and polyline in drawing order.
    let b0 = &model.batches[0];
    assert_eq!(b0.key.container, ContainerRef::ModelSpace);
    assert_eq!(b0.key.layer, fx.walls);
    assert_eq!(b0.key.color, RED);
    assert_eq!(b0.prims.len(), 2);
    assert_eq!(b0.prims[0].entity, fx.line);
    assert_eq!(b0.prims[1].entity, fx.poly);
    // The line has two endpoints and its layer's resolved 0.5 mm width.
    match &b0.prims[0].geom {
        PrimGeom::PolylineStrip {
            points,
            width_class,
            ..
        } => {
            assert_eq!(points, &vec![Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)]);
            assert!((width_class.0 - 0.5).abs() < 1e-6, "grosor ByLayer=0.5");
        }
        other => panic!("se esperaba PolylineStrip, fue {other:?}"),
    }
    // The flattened polyline starts at (0, 0), ends at (20, 0), and gains
    // intermediate vertices from its arc segment.
    match &b0.prims[1].geom {
        PrimGeom::PolylineStrip { points, .. } => {
            assert_eq!(points[0], Point2::new(0.0, 0.0));
            assert_eq!(*points.last().unwrap(), Point2::new(20.0, 0.0));
            assert!(points.len() > 3, "el arco debe aportar vértices");
            assert!(
                points.contains(&Point2::new(10.0, 0.0)),
                "vértice compartido"
            );
        }
        other => panic!("se esperaba PolylineStrip, fue {other:?}"),
    }

    // Batch 1: green Walls circle, flattened and closed.
    let b1 = &model.batches[1];
    assert_eq!(b1.key.layer, fx.walls);
    assert_eq!(b1.key.color, GREEN);
    assert_eq!(b1.prims.len(), 1);
    assert_eq!(b1.prims[0].entity, fx.circle);
    match &b1.prims[0].geom {
        PrimGeom::PolylineStrip { points, .. } => {
            assert!(points.len() >= 4);
            assert_eq!(*points.last().unwrap(), points[0], "círculo cerrado exacto");
            assert_eq!(points[0], Point2::new(8.0, 5.0)); // East quadrant: (5 + 3, 5).
        }
        other => panic!("se esperaba PolylineStrip, fue {other:?}"),
    }

    // Batch 2: layer 0 point with an explicit RGB color.
    let b2 = &model.batches[2];
    assert_eq!(b2.key.layer, fx.layer0);
    assert_eq!(b2.key.color, POINT_RGB);
    assert_eq!(b2.prims.len(), 1);
    assert_eq!(b2.prims[0].entity, fx.point);
    assert_eq!(
        b2.prims[0].geom,
        PrimGeom::Marker {
            at: Point2::new(-1.0, -2.0),
            kind: MarkerKind::Node,
        }
    );

    // The entity on the off layer appears in no batch.
    assert!(
        model
            .batches
            .iter()
            .all(|b| b.prims.iter().all(|p| p.entity != fx.hidden_line)),
        "la entidad de la capa off no debe renderizarse"
    );
}

// ===================== Determinism =====================

#[test]
fn build_full_omite_entidades_visible_false() {
    // An entity with visible=false is not rendered even when its layer is on.
    let mut session = Session::new(Units::default());
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    let id = session
        .transact::<_, TxError, _>("add", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId(0).into(),
                    layer0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(4.0, 0.0))),
                ),
            )
        })
        .unwrap()
        .value;
    // A visible entity produces one batch.
    assert_eq!(build_full(session.document(), &opts()).batches.len(), 1);
    // Hiding the entity removes it from rendering.
    session
        .transact::<_, TxError, _>("hide", |tx| tx.modify_entity(id, |rec| rec.visible = false))
        .unwrap();
    assert!(
        build_full(session.document(), &opts()).batches.is_empty(),
        "una entidad visible=false no produce batches"
    );
}

#[test]
fn build_full_es_puro() {
    let (session, _) = fixture();
    let a = build_full(session.document(), &opts());
    let b = build_full(session.document(), &opts());
    assert_eq!(a, b, "dos llamadas con el mismo doc deben coincidir");
}

// ===================== ChangeSet deltas =====================

/// Applies a delta to `prev`, compares it with a fresh full build, and verifies
/// that only `expected_keys` changed.
fn assert_delta(
    prev: &RenderModel,
    cs: &af_model::ChangeSet,
    doc: &af_model::Document,
    expected_keys: &[BatchKey],
) -> RenderModel {
    let delta = apply_changeset(prev, cs, doc, &opts());

    // The delta mentions exactly the expected keys.
    let touched: Vec<BatchKey> = delta
        .batch_updates
        .iter()
        .map(|u| match u {
            BatchUpdate::Upsert(b) => b.key,
            BatchUpdate::Remove(k) => *k,
        })
        .collect();
    assert_eq!(
        touched.len(),
        expected_keys.len(),
        "claves tocadas: {touched:?}, esperadas: {expected_keys:?}"
    );
    for k in expected_keys {
        assert!(touched.contains(k), "falta la clave {k:?} en el delta");
    }

    // Applying the delta reproduces a fresh build of the new document.
    let applied = prev.apply_delta(&delta);
    let full = build_full(doc, &opts());
    let am = as_map(&applied);
    let fm = as_map(&full);
    assert_eq!(
        am.len(),
        fm.len(),
        "nº de batches difiere del rebuild total"
    );
    for (k, fb) in &fm {
        assert_eq!(am.get(k).map(|b| &b.prims), Some(&fb.prims), "batch {k:?}");
    }

    // Unaffected batches remain identical between previous and applied models.
    let pm = as_map(prev);
    for (k, pb) in &pm {
        if !expected_keys.contains(k) {
            assert_eq!(
                am.get(k).map(|b| &b.prims),
                Some(&pb.prims),
                "batch no afectado {k:?} cambió"
            );
        }
    }
    applied
}

#[test]
fn delta_add_solo_toca_su_batch() {
    let (mut session, fx) = fixture();
    let prev = build_full(session.document(), &opts());

    // Adding another ByLayer line affects only the red Walls batch.
    let out = session
        .transact::<_, TxError, _>("add", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId(0).into(),
                    fx.walls,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 9.0),
                        Point2::new(9.0, 9.0),
                    )),
                ),
            )?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    let key = BatchKey {
        container: ContainerRef::ModelSpace,
        layer: fx.walls,
        color: RED,
        linetype: fx.continuous,
    };
    assert_delta(&prev, &cs, session.document(), &[key]);
}

#[test]
fn delta_modify_solo_toca_su_batch() {
    let (mut session, fx) = fixture();
    let prev = build_full(session.document(), &opts());

    // Changing the circle radius affects only the green Walls key.
    let out = session
        .transact::<_, TxError, _>("modify", |tx| {
            tx.modify_entity(fx.circle, |rec| {
                if let EntityGeometry::Circle(c) = &mut rec.geometry {
                    c.radius = 7.0;
                }
            })?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    let key = BatchKey {
        container: ContainerRef::ModelSpace,
        layer: fx.walls,
        color: GREEN,
        linetype: fx.continuous,
    };
    let applied = assert_delta(&prev, &cs, session.document(), &[key]);
    // The resized circle starts at the east quadrant of its new radius.
    let b = applied.batch(&key).unwrap();
    if let PrimGeom::PolylineStrip { points, .. } = &b.prims[0].geom {
        assert_eq!(points[0], Point2::new(12.0, 5.0));
    } else {
        panic!("se esperaba PolylineStrip");
    }
}

#[test]
fn delta_modify_color_toca_dos_batches() {
    let (mut session, fx) = fixture();
    let prev = build_full(session.document(), &opts());

    // Changing the circle from explicit green to ByLayer moves it into the red
    // Walls batch and removes the now-empty green batch.
    let out = session
        .transact::<_, TxError, _>("recolor", |tx| {
            tx.modify_entity(fx.circle, |rec| {
                rec.color = Color::ByLayer;
            })?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    let old_key = BatchKey {
        container: ContainerRef::ModelSpace,
        layer: fx.walls,
        color: GREEN,
        linetype: fx.continuous,
    };
    let new_key = BatchKey {
        container: ContainerRef::ModelSpace,
        layer: fx.walls,
        color: RED,
        linetype: fx.continuous,
    };
    let applied = assert_delta(&prev, &cs, session.document(), &[old_key, new_key]);
    // The green batch disappears and red now holds line, polyline, and circle.
    assert!(
        applied.batch(&old_key).is_none(),
        "batch verde vacío eliminado"
    );
    assert_eq!(applied.batch(&new_key).unwrap().prims.len(), 3);
}

#[test]
fn delta_remove_vacia_su_batch() {
    let (mut session, fx) = fixture();
    let prev = build_full(session.document(), &opts());

    // Removing the point empties its layer-0 RGB batch.
    let out = session
        .transact::<_, TxError, _>("remove", |tx| {
            tx.remove_entity(fx.point)?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    let key = BatchKey {
        container: ContainerRef::ModelSpace,
        layer: fx.layer0,
        color: POINT_RGB,
        linetype: fx.continuous,
    };
    let delta = apply_changeset(&prev, &cs, session.document(), &opts());
    assert_eq!(delta.batch_updates, vec![BatchUpdate::Remove(key)]);
    let applied = assert_delta(&prev, &cs, session.document(), &[key]);
    assert!(applied.batch(&key).is_none());
}

#[test]
fn delta_layer_on_revela_entidades() {
    let (mut session, fx) = fixture();
    let prev = build_full(session.document(), &opts());
    // The previous model has no Hidden batch because that layer was off.
    assert_eq!(prev.batches.len(), 3);

    // Turning on Hidden adds its blue ByLayer line.
    let out = session
        .transact::<_, TxError, _>("layer on", |tx| {
            let l = tx.doc().layer(fx.hidden).unwrap().clone().with_off(false);
            tx.modify_layer_raw(fx.hidden, l)?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert!(!cs.layers_changed().is_empty(), "cambió la capa");
    let key = BatchKey {
        container: ContainerRef::ModelSpace,
        layer: fx.hidden,
        color: BLUE,
        linetype: fx.continuous,
    };
    let applied = assert_delta(&prev, &cs, session.document(), &[key]);
    assert_eq!(applied.batches.len(), 4, "aparece el batch de Hidden");
    assert_eq!(applied.batch(&key).unwrap().prims[0].entity, fx.hidden_line);
}

/// ByLayer line types resolve from the layer, and different resolved patterns use
/// different batches. LTSCALE flows to `RenderModel::ltscale`.
#[test]
fn linetype_bylayer_se_resuelve_al_de_la_capa() {
    let mut session = Session::new(Units::default());
    let continuous = session.document().line_types().next().unwrap().id();
    let placeholder: LayerId = ObjectId(0).into();

    let out = session
        .transact::<_, TxError, _>("setup", |tx| {
            // Load DASHED and create a layer that uses it by default.
            let dashed = tx.add_line_type_raw("DASHED", "d", vec![0.5, -0.25])?;
            let l = tx.add_layer_raw(
                Layer::new(
                    placeholder,
                    "L",
                    Color::aci(1).unwrap(),
                    dashed,
                    Lineweight::ByLayer,
                )
                .with_off(false),
            )?;
            // A ByLayer entity resolves to the layer's DASHED pattern.
            let a = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    l,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(1.0, 0.0))),
                ),
            )?;
            // The same layer and color with explicit Continuous uses another batch.
            let b = tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    placeholder.raw().into(),
                    l,
                    Color::ByLayer,
                    LineTypeRef::Style(continuous),
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(0.0, 1.0))),
                ),
            )?;
            tx.set_ltscale(2.5)?;
            Ok((dashed, a, b))
        })
        .unwrap();
    let (dashed, a, b) = out.value;

    let model = build_full(session.document(), &opts());
    // LTSCALE flows into the model.
    assert_eq!(model.ltscale, 2.5);
    // Two batches share container, layer, and color but differ in line type.
    assert_eq!(model.batches.len(), 2);
    let map = as_map(&model);
    let ka = map.keys().find(|k| k.linetype == dashed).unwrap();
    let kc = map.keys().find(|k| k.linetype == continuous).unwrap();
    assert_eq!(ka.layer, kc.layer, "misma capa");
    assert_eq!(ka.color, kc.color, "mismo color resuelto");
    assert_ne!(ka.linetype, kc.linetype, "distinto tipo de línea");
    assert_eq!(model.batch(ka).unwrap().prims[0].entity, a);
    assert_eq!(model.batch(kc).unwrap().prims[0].entity, b);
}

#[test]
fn apply_changeset_es_determinista() {
    let (mut session, fx) = fixture();
    let prev = build_full(session.document(), &opts());
    let out = session
        .transact::<_, TxError, _>("mod", |tx| {
            tx.modify_entity(fx.circle, |rec| {
                if let EntityGeometry::Circle(c) = &mut rec.geometry {
                    c.radius = 2.0;
                }
            })?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    let d1 = apply_changeset(&prev, &cs, session.document(), &opts());
    let d2 = apply_changeset(&prev, &cs, session.document(), &opts());
    assert_eq!(d1, d2);
}

// ===================== Chord-error policy =====================

#[test]
fn menos_chord_err_mas_segmentos() {
    // Lower chord error creates more vertices for the same large circle.
    let mut session = Session::new(Units::default());
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    session
        .transact::<_, TxError, _>("circle", |tx| {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId(0).into(),
                    layer0,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Circle(CircleGeo::new(Point2::new(0.0, 0.0), 100.0)),
                ),
            )?;
            Ok(())
        })
        .unwrap();

    let n_coarse = circle_point_count(&build_full(session.document(), &RenderOpts::new(5.0)));
    let n_fine = circle_point_count(&build_full(session.document(), &RenderOpts::new(0.05)));
    assert!(
        n_fine > n_coarse,
        "menos error debe dar más segmentos: coarse={n_coarse}, fine={n_fine}"
    );
}

fn circle_point_count(m: &RenderModel) -> usize {
    match &m.batches[0].prims[0].geom {
        PrimGeom::PolylineStrip { points, .. } => points.len(),
        other => panic!("se esperaba PolylineStrip, fue {other:?}"),
    }
}

#[test]
fn polilinea_ancha_transporta_poly_width_en_el_prim() {
    // A DONUT is a wide polyline represented by `poly_width` in world units.
    let geo = EntityGeometry::Polyline(
        PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(-3.0, 0.0), 1.0),
                PolyVertex::new(Point2::new(3.0, 0.0), 1.0),
            ],
            true,
        )
        .with_width(2.0),
    );
    let (session, _) = single_entity(geo).unwrap();
    let model = build_full(session.document(), &RenderOpts::new(0.05));
    match &model.batches[0].prims[0].geom {
        PrimGeom::PolylineStrip { poly_width, .. } => {
            assert!((poly_width - 2.0).abs() < 1e-6, "poly_width resuelto = 2.0");
        }
        other => panic!("se esperaba PolylineStrip, fue {other:?}"),
    }
    // A normal line has no geometric polyline width.
    let (ls, _) = single_entity(EntityGeometry::Line(LineGeo::new(
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 1.0),
    )))
    .unwrap();
    let lm = build_full(ls.document(), &RenderOpts::new(0.05));
    match &lm.batches[0].prims[0].geom {
        PrimGeom::PolylineStrip { poly_width, .. } => assert_eq!(*poly_width, 0.0),
        other => panic!("se esperaba PolylineStrip, fue {other:?}"),
    }
}

// ===================== Primitive bounds property =====================

use proptest::prelude::*;

/// Builds a one-entity document and returns its session and ID.
fn single_entity(geo: EntityGeometry) -> Option<(Session, EntityId)> {
    let mut session = Session::new(Units::default());
    let layer0 = session.document().layer_by_name("0").unwrap().id();
    let res = session.transact::<_, TxError, _>("one", |tx| {
        tx.add_entity(
            ContainerRef::ModelSpace,
            EntityRecord::new(
                ObjectId(0).into(),
                layer0,
                Color::ByLayer,
                LineTypeRef::ByLayer,
                Lineweight::ByLayer,
                geo,
            ),
        )
    });
    match res {
        Ok(out) => Some((session, out.value)),
        Err(_) => None, // Discard invalid geometry rejected by the transaction.
    }
}

/// Verifies that every primitive point falls within its entity bounding box plus
/// half-width margin.
fn assert_points_in_bbox(session: &Session, id: EntityId) {
    let (rec, _) = session.document().entity(id).unwrap();
    let bb = rec.geometry.bbox();
    let model = build_full(session.document(), &RenderOpts::new(0.05));
    for batch in &model.batches {
        for prim in &batch.prims {
            let margin = match &prim.geom {
                PrimGeom::PolylineStrip { width_class, .. } => f64::from(width_class.0) * 0.5,
                PrimGeom::Marker { .. } | PrimGeom::MaskPolygon { .. } => 0.0,
            };
            let padded = bb.expand(margin + 1e-6);
            let pts: Vec<Point2> = match &prim.geom {
                PrimGeom::PolylineStrip { points, .. } => points.clone(),
                PrimGeom::Marker { at, .. } => vec![*at],
                PrimGeom::MaskPolygon { points } => points.clone(),
            };
            for p in pts {
                assert!(
                    padded.contains_point(p),
                    "punto {p:?} fuera de bbox {bb:?} (+margen {margin})"
                );
            }
        }
    }
}

proptest! {
    #[test]
    fn prop_line_prim_en_bbox(
        x1 in -1.0e5f64..1.0e5, y1 in -1.0e5f64..1.0e5,
        x2 in -1.0e5f64..1.0e5, y2 in -1.0e5f64..1.0e5,
    ) {
        let geo = EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2)));
        if let Some((s, id)) = single_entity(geo) {
            assert_points_in_bbox(&s, id);
        }
    }

    #[test]
    fn prop_circle_prim_en_bbox(
        cx in -1.0e5f64..1.0e5, cy in -1.0e5f64..1.0e5,
        r in 1.0e-2f64..1.0e4,
    ) {
        let geo = EntityGeometry::Circle(CircleGeo::new(Point2::new(cx, cy), r));
        if let Some((s, id)) = single_entity(geo) {
            assert_points_in_bbox(&s, id);
        }
    }

    #[test]
    fn prop_polyline_prim_en_bbox(
        n in 2usize..=5,
        seed in any::<u64>(),
    ) {
        // Separate vertices by 50 units on X to avoid coincident geometry.
        let mut bulges = seed;
        let verts: Vec<PolyVertex> = (0..n)
            .map(|i| {
                bulges = bulges.wrapping_mul(6364136223846793005).wrapping_add(1);
                let b = ((bulges >> 33) as f64 / u32::MAX as f64) * 2.0 - 1.0; // [-1,1]
                let y = ((bulges >> 11) as u32 as f64 / u32::MAX as f64) * 20.0 - 10.0;
                PolyVertex::new(Point2::new(i as f64 * 50.0, y), b)
            })
            .collect();
        let geo = EntityGeometry::Polyline(PolylineGeo::new(verts, false));
        if let Some((s, id)) = single_entity(geo) {
            assert_points_in_bbox(&s, id);
        }
    }
}
