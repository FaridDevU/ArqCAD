//! Builds render models and their deltas.
//!
//! This is the single boundary for style resolution, chord-error curve
//! flattening, and primitive batching.

use std::collections::{HashMap, HashSet};

use af_geom::flatten::{flatten_arc, flatten_circle, flatten_ellipse};
use af_math::Point2;
use af_model::ChangeSet;
use af_model::container::{ContainerRef, GeoRef};
use af_model::doc::Document;
use af_model::entity::{Color, LineTypeRef, Lineweight, PolylineGeo, SegKind, SplineGeo};
use af_model::id::{EntityId, LayerId, StyleId};
use indexmap::IndexMap;

use crate::{
    BatchKey, BatchUpdate, MarkerKind, PrimGeom, RenderBatch, RenderDelta, RenderModel, RenderOpts,
    RenderPrim, Rgba, WidthClass, palette,
};

/// Builds the complete render model for model space.
///
/// This function is pure with respect to the document: identical inputs produce
/// identical [`RenderModel`] values. Visible entities retain drawing order while
/// their styles, curves, and batches are resolved.
#[must_use]
pub fn build_full(doc: &Document, opts: &RenderOpts) -> RenderModel {
    let container = ContainerRef::ModelSpace;
    // IndexMap preserves first-seen drawing order.
    let mut map: IndexMap<BatchKey, Vec<RenderPrim>> = IndexMap::new();
    // Visit in drawing order without materializing records.
    doc.model_space().visit(|id, common, geo| {
        if !is_visible(doc, common.visible(), common.layer()) {
            return;
        }
        let key = resolve_key(
            doc,
            common.layer(),
            common.color(),
            common.line_type(),
            container,
        );
        let prim = build_prim(doc, id, common.lineweight(), common.layer(), geo, opts);
        map.entry(key).or_default().push(prim);
    });
    let batches = map
        .into_iter()
        .map(|(key, prims)| RenderBatch { key, prims })
        .collect();
    RenderModel {
        batches,
        ltscale: doc.ltscale(),
    }
}

/// Produces a [`RenderDelta`] for a [`ChangeSet`] relative to the pre-transaction
/// model in `prev`.
///
/// # Why `prev` is required
///
/// Removed and reclassified entities need their previous batch keys, which are
/// absent from the post-transaction document. `prev` supplies that mapping and
/// prevents stale batches after deletion.
///
/// # Rebuild policy
///
/// Each affected key is rebuilt from the current document:
/// - visible added and modified entities contribute their new keys;
/// - removed and modified entities contribute their previous keys;
/// - a changed layer contributes all its previous and current keys because its
///   style or visibility may affect every entity on that layer.
///
/// Changing only the current layer does not alter rendering and is ignored.
#[must_use]
pub fn apply_changeset(
    prev: &RenderModel,
    cs: &ChangeSet,
    doc: &Document,
    opts: &RenderOpts,
) -> RenderDelta {
    let container = ContainerRef::ModelSpace;

    // Map entity IDs to their previous batch keys.
    let mut old_key: HashMap<EntityId, BatchKey> = HashMap::new();
    for b in &prev.batches {
        for p in &b.prims {
            old_key.insert(p.entity, b.key);
        }
    }

    let mut affected: HashSet<BatchKey> = HashSet::new();

    // Removed entities affect their previous batches.
    for &id in cs.removed() {
        if let Some(k) = old_key.get(&id) {
            affected.insert(*k);
        }
    }
    // Modified entities affect both previous and current batches.
    for &id in cs.modified() {
        if let Some(k) = old_key.get(&id) {
            affected.insert(*k);
        }
        insert_new_key(&mut affected, doc, id, container);
    }
    // Added visible model-space entities affect their current batches.
    for &id in cs.added() {
        insert_new_key(&mut affected, doc, id, container);
    }
    // Layer changes rebuild every previous and current batch on that layer.
    for &lid in cs.layers_changed() {
        for b in &prev.batches {
            if b.key.layer == lid {
                affected.insert(b.key);
            }
        }
        doc.model_space().visit(|_id, common, _geo| {
            if common.layer() == lid && is_visible(doc, common.visible(), common.layer()) {
                affected.insert(resolve_key(
                    doc,
                    common.layer(),
                    common.color(),
                    common.line_type(),
                    container,
                ));
            }
        });
    }

    // Rebuild affected keys in deterministic order.
    let mut keys: Vec<BatchKey> = affected.into_iter().collect();
    keys.sort_by_key(key_ord);
    let mut batch_updates = Vec::with_capacity(keys.len());
    for key in keys {
        let prims = rebuild_batch_prims(doc, key, opts);
        if prims.is_empty() {
            batch_updates.push(BatchUpdate::Remove(key));
        } else {
            batch_updates.push(BatchUpdate::Upsert(RenderBatch { key, prims }));
        }
    }
    RenderDelta { batch_updates }
}

impl RenderModel {
    /// Applies a [`RenderDelta`] and returns the resulting model.
    ///
    /// `Upsert` replaces or creates a key while preserving an existing position;
    /// `Remove` deletes it. New keys are appended, so callers comparing a fresh
    /// full build should compare by key rather than position.
    #[must_use]
    pub fn apply_delta(&self, delta: &RenderDelta) -> RenderModel {
        let mut map: IndexMap<BatchKey, RenderBatch> = IndexMap::new();
        for b in &self.batches {
            map.insert(b.key, b.clone());
        }
        for u in &delta.batch_updates {
            match u {
                BatchUpdate::Upsert(b) => {
                    map.insert(b.key, b.clone());
                }
                BatchUpdate::Remove(k) => {
                    map.shift_remove(k);
                }
            }
        }
        RenderModel {
            batches: map.into_values().collect(),
            ltscale: self.ltscale,
        }
    }
}

/// Adds an entity's current key to `affected` when it is visible in model space.
fn insert_new_key(
    affected: &mut HashSet<BatchKey>,
    doc: &Document,
    id: EntityId,
    container: ContainerRef,
) {
    if let Some((rec, ContainerRef::ModelSpace)) = doc.entity(id)
        && is_visible(doc, rec.visible, rec.layer)
    {
        affected.insert(resolve_key(
            doc,
            rec.layer,
            rec.color,
            rec.line_type,
            container,
        ));
    }
}

/// Rebuilds the primitives for `key` in drawing order.
fn rebuild_batch_prims(doc: &Document, key: BatchKey, opts: &RenderOpts) -> Vec<RenderPrim> {
    let Some(container) = doc.container(key.container) else {
        return Vec::new();
    };
    let mut prims = Vec::new();
    // Use the same zero-copy drawing-order visit as a full build.
    container.visit(|id, common, geo| {
        if is_visible(doc, common.visible(), common.layer())
            && resolve_key(
                doc,
                common.layer(),
                common.color(),
                common.line_type(),
                key.container,
            ) == key
        {
            prims.push(build_prim(
                doc,
                id,
                common.lineweight(),
                common.layer(),
                geo,
                opts,
            ));
        }
    });
    prims
}

/// Deterministic sort key for a [`BatchKey`].
fn key_ord(k: &BatchKey) -> (u8, u64, u64, [u8; 4], u64) {
    let (tag, cid) = match k.container {
        ContainerRef::ModelSpace => (0u8, 0u64),
        ContainerRef::Layout(id) => (1, id.raw().0),
        ContainerRef::Block(id) => (2, id.raw().0),
    };
    (
        tag,
        cid,
        k.layer.raw().0,
        [k.color.r, k.color.g, k.color.b, k.color.a],
        k.linetype.raw().0,
    )
}

/// Returns whether the entity and its layer are visible for rendering. Locked
/// layers remain visible.
///
/// Unknown layers in corrupt documents conservatively leave geometry visible.
fn is_visible(doc: &Document, visible: bool, layer: LayerId) -> bool {
    if !visible {
        return false;
    }
    match doc.layer(layer) {
        Some(l) => !(l.is_off() || l.is_frozen()),
        None => true,
    }
}

/// Resolves an entity's container, layer, color, and line type into a [`BatchKey`].
fn resolve_key(
    doc: &Document,
    layer: LayerId,
    color: Color,
    line_type: LineTypeRef,
    container: ContainerRef,
) -> BatchKey {
    BatchKey {
        container,
        layer,
        color: resolve_color(doc, color, layer),
        linetype: resolve_linetype(doc, line_type, layer),
    }
}

/// Resolves an entity to a concrete document [`LineType`] ID. A valid explicit
/// style wins; other modes and dangling IDs fall back to the layer default.
fn resolve_linetype(doc: &Document, line_type: LineTypeRef, layer: LayerId) -> StyleId {
    if let LineTypeRef::Style(id) = line_type
        && doc.line_type(id).is_some()
    {
        return id;
    }
    doc.layer(layer)
        .map(af_model::layers::Layer::line_type)
        .or_else(|| doc.line_types().next().map(af_model::LineType::id))
        .unwrap_or_else(|| layer.raw().into())
}

/// Resolves an entity color, including ByLayer and ByBlock, to final [`Rgba`].
fn resolve_color(doc: &Document, color: Color, layer: LayerId) -> Rgba {
    match color {
        Color::Rgb(r, g, b) => Rgba::new(r, g, b, 255),
        Color::Aci(a) => palette::aci_to_rgba(a.get()),
        Color::ByLayer => layer_color(doc, layer),
        // Without a block-reference context, ByBlock falls back to white.
        Color::ByBlock => palette::FALLBACK,
    }
}

/// Resolves a layer default color, falling back to white for corrupt recursive modes.
fn layer_color(doc: &Document, layer: LayerId) -> Rgba {
    match doc.layer(layer).map(af_model::layers::Layer::color) {
        Some(Color::Rgb(r, g, b)) => Rgba::new(r, g, b, 255),
        Some(Color::Aci(a)) => palette::aci_to_rgba(a.get()),
        Some(Color::ByLayer | Color::ByBlock) | None => palette::FALLBACK,
    }
}

/// Resolves entity lineweight to millimeters. Negative or unresolvable values
/// fall back to a 0.0 hairline.
fn resolve_width(doc: &Document, lineweight: Lineweight, layer: LayerId) -> WidthClass {
    let mm = match lineweight {
        Lineweight::Mm(v) => v,
        Lineweight::ByLayer => layer_width(doc, layer),
        // Without a block-reference context, ByBlock falls back to hairline.
        Lineweight::ByBlock => 0.0,
    };
    WidthClass(mm.max(0.0))
}

/// Resolves a layer's default lineweight in millimeters.
fn layer_width(doc: &Document, layer: LayerId) -> f32 {
    match doc.layer(layer).map(af_model::layers::Layer::lineweight) {
        Some(Lineweight::Mm(v)) => v,
        _ => 0.0,
    }
}

/// Builds an entity primitive from a zero-copy [`GeoRef`], flattening curves by
/// `opts.chord_err`.
fn build_prim(
    doc: &Document,
    id: EntityId,
    lineweight: Lineweight,
    layer: LayerId,
    geo: GeoRef<'_>,
    opts: &RenderOpts,
) -> RenderPrim {
    let geom = match geo {
        GeoRef::Line(g) => PrimGeom::PolylineStrip {
            points: vec![g.p1, g.p2],
            width_class: resolve_width(doc, lineweight, layer),
            poly_width: 0.0,
            analytic_length: Some(g.length()),
        },
        GeoRef::Point(g) => PrimGeom::Marker {
            at: g.position,
            kind: MarkerKind::Node,
        },
        GeoRef::Circle(g) => PrimGeom::PolylineStrip {
            points: flatten_circle(g.center, g.radius, opts.chord_err),
            width_class: resolve_width(doc, lineweight, layer),
            poly_width: 0.0,
            analytic_length: Some(g.circumference()),
        },
        GeoRef::Arc(g) => PrimGeom::PolylineStrip {
            points: flatten_arc(&g.arc_seg(), opts.chord_err),
            width_class: resolve_width(doc, lineweight, layer),
            poly_width: 0.0,
            analytic_length: Some(g.length()),
        },
        GeoRef::Ellipse(g) => PrimGeom::PolylineStrip {
            points: flatten_ellipse(&g.ellipse(), opts.chord_err),
            width_class: resolve_width(doc, lineweight, layer),
            poly_width: 0.0,
            analytic_length: Some(g.length()),
        },
        GeoRef::Polyline(g) => PrimGeom::PolylineStrip {
            points: flatten_polyline(g, opts.chord_err),
            width_class: resolve_width(doc, lineweight, layer),
            // Geometric polyline width uses the absolute value of corrupt negatives.
            poly_width: g.width.abs() as f32,
            analytic_length: Some(g.length()),
        },
        // The viewport-free render model represents infinite entities with a
        // fixed segment long enough to exceed any normal screen.
        GeoRef::Xline(g) => {
            let (a, b) = g.endpoints();
            PrimGeom::PolylineStrip {
                points: vec![a, b],
                width_class: resolve_width(doc, lineweight, layer),
                poly_width: 0.0,
                analytic_length: None,
            }
        }
        GeoRef::Ray(g) => {
            let (a, b) = g.endpoints();
            PrimGeom::PolylineStrip {
                points: vec![a, b],
                width_class: resolve_width(doc, lineweight, layer),
                poly_width: 0.0,
                analytic_length: None,
            }
        }
        GeoRef::Spline(g) => PrimGeom::PolylineStrip {
            points: flatten_spline(g, opts.chord_err),
            width_class: resolve_width(doc, lineweight, layer),
            poly_width: 0.0,
            analytic_length: None,
        },
        // Masks use an implicitly closed filled polygon without stroke width.
        GeoRef::Wipeout(g) => PrimGeom::MaskPolygon {
            points: g.points.clone(),
        },
    };
    RenderPrim { entity: id, geom }
}

/// Flattens a polyline into one point strip without duplicating shared vertices.
/// Closed polylines end exactly at their first vertex.
fn flatten_polyline(g: &PolylineGeo, chord_err: f64) -> Vec<Point2> {
    let mut pts: Vec<Point2> = Vec::new();
    let mut first = true;
    for seg in g.segments() {
        match seg {
            SegKind::Line { a, b } => {
                if first {
                    pts.push(a);
                    first = false;
                }
                pts.push(b);
            }
            SegKind::Arc(arc) => {
                let ap = flatten_arc(&arc, chord_err); // [start, …, end]
                if first {
                    pts.push(ap[0]);
                    first = false;
                }
                // Skip ap[0] because the previous segment already supplied it.
                pts.extend_from_slice(&ap[1..]);
            }
        }
    }
    if pts.is_empty() {
        // Invalid polylines with fewer than two vertices retain their raw points.
        pts.extend(g.vertices.iter().map(|v| v.pt));
    }
    pts
}

/// Flattens a cubic spline by chord error. Degenerate geometry falls back to raw
/// fit points.
fn flatten_spline(g: &SplineGeo, chord_err: f64) -> Vec<Point2> {
    g.fit_spline()
        .map_or_else(|| g.fit_points.clone(), |sp| sp.flatten(chord_err))
}

#[cfg(test)]
mod tests {
    use super::*;

    use af_math::Point2;
    use af_math::Vec2;
    use af_model::entity::{
        ArcGeo, CircleGeo, Color, EllipseGeo, EntityGeometry, EntityRecord, LineGeo, LineTypeRef,
        Lineweight, PointGeo, PolyVertex, PolylineGeo, RayGeo, SplineGeo, WipeoutGeo, XlineGeo,
    };
    use af_model::id::{LayerId, ObjectId};
    use af_model::layers::Layer;
    use af_model::tx::TxError;
    use af_model::units::Units;
    use af_model::{ContainerRef, Session};

    /// Materialized reference path. It matches `build_full` but clones geometry
    /// through `iter_records()` instead of using the zero-copy visit path.
    fn build_full_materialized(doc: &Document, opts: &RenderOpts) -> RenderModel {
        let container = ContainerRef::ModelSpace;
        let mut map: IndexMap<BatchKey, Vec<RenderPrim>> = IndexMap::new();
        for rec in doc.model_space().iter_records() {
            if !is_visible(doc, rec.visible, rec.layer) {
                continue;
            }
            let key = resolve_key(doc, rec.layer, rec.color, rec.line_type, container);
            let prim = build_prim(
                doc,
                rec.id,
                rec.lineweight,
                rec.layer,
                GeoRef::of(&rec.geometry),
                opts,
            );
            map.entry(key).or_default().push(prim);
        }
        let batches = map
            .into_iter()
            .map(|(key, prims)| RenderBatch { key, prims })
            .collect();
        RenderModel {
            batches,
            ltscale: doc.ltscale(),
        }
    }

    fn rec(layer: LayerId, color: Color, lw: Lineweight, geom: EntityGeometry) -> EntityRecord {
        EntityRecord::new(
            ObjectId::NIL.into(),
            layer,
            color,
            LineTypeRef::ByLayer,
            lw,
            geom,
        )
    }

    /// Synthetic document containing all ten geometry variants across varied
    /// layers, colors, and widths.
    fn doc_con_las_diez_variantes() -> Session {
        let mut session = Session::new(Units::default());
        let continuous = session.document().line_types().next().unwrap().id();
        let layer0 = session.document().layer_by_name("0").unwrap().id();
        let placeholder: LayerId = ObjectId(0).into();

        session
            .transact::<_, TxError, _>("fixture-10", |tx| {
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
                let off = tx.add_layer_raw(
                    Layer::new(
                        placeholder,
                        "Off",
                        Color::aci(5).unwrap(),
                        continuous,
                        Lineweight::Mm(0.35),
                    )
                    .with_off(true),
                )?;

                // 1. Line using the Walls layer color and width.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Line(LineGeo::new(
                            Point2::new(0.0, 0.0),
                            Point2::new(4.0, 0.0),
                        )),
                    ),
                )?;
                // 2. Point on layer 0 with an explicit RGB color.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        layer0,
                        Color::Rgb(10, 20, 30),
                        Lineweight::ByLayer,
                        EntityGeometry::Point(PointGeo::new(Point2::new(-1.0, -2.0))),
                    ),
                )?;
                // 3. Circle with explicit ACI 3.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::aci(3).unwrap(),
                        Lineweight::Mm(0.25),
                        EntityGeometry::Circle(CircleGeo::new(Point2::new(5.0, 5.0), 3.0)),
                    ),
                )?;
                // 4. Arc.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Arc(ArcGeo::new(Point2::new(0.0, 0.0), 2.0, 0.0, 1.5)),
                    ),
                )?;
                // 5. Ellipse.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Ellipse(EllipseGeo::new(
                            Point2::new(1.0, 1.0),
                            4.0,
                            0.5,
                            0.3,
                            0.0,
                            2.0,
                        )),
                    ),
                )?;
                // 6. Polyline with bulges.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::Mm(0.6),
                        EntityGeometry::Polyline(PolylineGeo::new(
                            vec![
                                PolyVertex::new(Point2::new(0.0, 0.0), 0.5),
                                PolyVertex::new(Point2::new(10.0, 0.0), -0.75),
                                PolyVertex::new(Point2::new(20.0, 5.0), 0.0),
                            ],
                            true,
                        )),
                    ),
                )?;
                // 7. Infinite construction line.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Xline(XlineGeo::new(
                            Point2::new(0.0, 0.0),
                            Vec2::new(1.0, 1.0),
                        )),
                    ),
                )?;
                // 8. Infinite ray.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Ray(RayGeo::new(
                            Point2::new(2.0, 2.0),
                            Vec2::new(0.0, 1.0),
                        )),
                    ),
                )?;
                // 9. Spline.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Spline(SplineGeo::new(
                            vec![
                                Point2::new(0.0, 0.0),
                                Point2::new(1.0, 2.0),
                                Point2::new(3.0, -1.0),
                                Point2::new(5.0, 0.0),
                            ],
                            false,
                        )),
                    ),
                )?;
                // 10. Wipeout.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        walls,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Wipeout(WipeoutGeo::new(vec![
                            Point2::new(0.0, 0.0),
                            Point2::new(2.0, 0.0),
                            Point2::new(2.0, 2.0),
                            Point2::new(0.0, 2.0),
                        ])),
                    ),
                )?;
                // An extra entity on an off layer exercises both visibility paths.
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    rec(
                        off,
                        Color::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Line(LineGeo::new(
                            Point2::new(9.0, 9.0),
                            Point2::new(9.0, 10.0),
                        )),
                    ),
                )?;
                Ok(())
            })
            .unwrap();
        session
    }

    // The zero-copy and materialized paths must produce identical render models.
    #[test]
    fn build_full_visit_equivale_a_ruta_materializada() {
        let session = doc_con_las_diez_variantes();
        let doc = session.document();
        let opts = RenderOpts::new(0.05);

        let by_visit = build_full(doc, &opts);
        let by_materialized = build_full_materialized(doc, &opts);

        assert_eq!(
            by_visit, by_materialized,
            "el render model por visit debe ser idéntico al de la ruta materializada"
        );
        // Visible variants must produce at least one batch.
        assert!(!by_visit.batches.is_empty());
    }

    #[test]
    fn build_full_carries_contracted_analytic_lengths() {
        let session = doc_con_las_diez_variantes();
        let doc = session.document();
        let model = build_full(doc, &RenderOpts::new(0.05));
        let mut strips = 0;
        let mut analytic = 0;

        for prim in model.batches.iter().flat_map(|batch| &batch.prims) {
            let PrimGeom::PolylineStrip {
                analytic_length, ..
            } = &prim.geom
            else {
                continue;
            };
            strips += 1;
            let (record, _) = doc.entity(prim.entity).expect("render entity");
            let expected = match &record.geometry {
                EntityGeometry::Line(g) => Some(g.length()),
                EntityGeometry::Circle(g) => Some(g.circumference()),
                EntityGeometry::Arc(g) => Some(g.length()),
                EntityGeometry::Polyline(g) => Some(g.length()),
                EntityGeometry::Ellipse(g) => Some(g.length()),
                EntityGeometry::Xline(_) | EntityGeometry::Ray(_) | EntityGeometry::Spline(_) => {
                    None
                }
                EntityGeometry::Point(_) | EntityGeometry::Wipeout(_) => {
                    panic!("non-strip geometry emitted as a strip")
                }
            };

            match (*analytic_length, expected) {
                (Some(actual), Some(expected)) => {
                    analytic += 1;
                    assert!((actual - expected).abs() < 1e-12);
                }
                (None, None) => {}
                pair => panic!("analytic length mismatch: {pair:?}"),
            }
        }

        assert_eq!(strips, 8);
        assert_eq!(analytic, 5);
    }

    /// Informal benchmark, ignored by default. Run with
    /// `cargo test -p af-render --lib -- --ignored --nocapture bench_build_full`).
    /// Compares the zero-copy and materialized paths on 10,000 polyline entities.
    /// It prints timing information without gating CI.
    #[test]
    #[ignore = "manual informal benchmark with nondeterministic timing"]
    fn bench_build_full_visit_vs_materializado_10k() {
        use std::time::Instant;

        let mut session = Session::new(Units::default());
        let layer0 = session.document().layer_by_name("0").unwrap().id();
        session
            .transact::<_, TxError, _>("bench-10k", |tx| {
                for i in 0..10_000u32 {
                    let x = f64::from(i);
                    let geom = if i % 2 == 0 {
                        EntityGeometry::Line(LineGeo::new(
                            Point2::new(x, 0.0),
                            Point2::new(x + 1.0, 1.0),
                        ))
                    } else {
                        EntityGeometry::Polyline(PolylineGeo::new(
                            vec![
                                PolyVertex::new(Point2::new(x, 0.0), 0.3),
                                PolyVertex::new(Point2::new(x + 1.0, 1.0), -0.4),
                                PolyVertex::new(Point2::new(x + 2.0, 0.0), 0.0),
                            ],
                            false,
                        ))
                    };
                    tx.add_entity(
                        ContainerRef::ModelSpace,
                        rec(layer0, Color::ByLayer, Lineweight::ByLayer, geom),
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let doc = session.document();
        let opts = RenderOpts::new(0.05);

        // Warm up and verify equality before measuring.
        assert_eq!(build_full(doc, &opts), build_full_materialized(doc, &opts));

        const ITERS: u32 = 30;
        let t0 = Instant::now();
        for _ in 0..ITERS {
            std::hint::black_box(build_full_materialized(doc, &opts));
        }
        let materialized = t0.elapsed() / ITERS;

        let t1 = Instant::now();
        for _ in 0..ITERS {
            std::hint::black_box(build_full(doc, &opts));
        }
        let visit = t1.elapsed() / ITERS;

        println!(
            "build_full 10k (media de {ITERS}): materializado={materialized:?}  visit={visit:?}"
        );
    }
}
