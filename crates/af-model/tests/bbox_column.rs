//! Bounding-box column invariant under randomized public transaction mutations.
//!
//! For every live entity, `EntityContainer::bbox` and `visit_bboxes` must exactly
//! equal `geometry.bbox()` after additions, same- or cross-variant edits,
//! removals, undo, and redo.

use af_math::Point2;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight,
    PointGeo, PolyVertex, PolylineGeo,
};
use af_model::id::EntityId;
use af_model::units::Units;
use af_model::{ContainerRef, Session, TxError};
use proptest::collection::vec;
use proptest::prelude::*;

/// Deterministically generated valid geometry.
#[derive(Debug, Clone)]
enum Geom {
    Line(f64, f64, f64, f64),
    Circle(f64, f64, f64),
    Point(f64, f64),
    Polyline(u8),
}

impl Geom {
    fn build(&self, seed: u64) -> EntityGeometry {
        match *self {
            Geom::Line(x1, y1, x2, y2) => {
                EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2)))
            }
            Geom::Circle(x, y, r) => {
                // Keep the radius above merge tolerance.
                EntityGeometry::Circle(CircleGeo::new(Point2::new(x, y), r.abs() + 1.0))
            }
            Geom::Point(x, y) => EntityGeometry::Point(PointGeo::new(Point2::new(x, y))),
            Geom::Polyline(vcount) => {
                // Deterministic nonzero steps keep vertices distinct.
                let n = (vcount % 7) as usize + 2; // 2..=8
                let mut verts = Vec::with_capacity(n);
                let mut s = seed | 1;
                let mut x = 0.0f64;
                let mut y = 0.0f64;
                for i in 0..n {
                    // Use a nonzero pseudodeterministic step.
                    s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let step = 10.0 + ((s >> 32) % 90) as f64; // 10..=99
                    x += step;
                    y += if i % 2 == 0 { step } else { -step };
                    let bulge = ((s % 100) as f64) / 200.0 - 0.25; // -0.25..0.25
                    verts.push(PolyVertex::new(Point2::new(x, y), bulge));
                }
                EntityGeometry::Polyline(PolylineGeo::new(verts, vcount % 2 == 0))
            }
        }
    }
}

fn geom_strategy() -> impl Strategy<Value = Geom> {
    let coord = -5_000.0f64..5_000.0;
    prop_oneof![
        (coord.clone(), coord.clone(), coord.clone(), coord.clone())
            .prop_map(|(a, b, c, d)| Geom::Line(a, b, c, d)),
        (coord.clone(), coord.clone(), 1.0f64..500.0).prop_map(|(x, y, r)| Geom::Circle(x, y, r)),
        (coord.clone(), coord.clone()).prop_map(|(x, y)| Geom::Point(x, y)),
        any::<u8>().prop_map(Geom::Polyline),
    ]
}

#[derive(Debug, Clone)]
enum Op {
    Add(Geom),
    /// Replaces one live entity with same- or cross-variant geometry.
    Modify(usize, Geom),
    Remove(usize),
    Undo,
    Redo,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => geom_strategy().prop_map(Op::Add),
        3 => (any::<usize>(), geom_strategy()).prop_map(|(i, g)| Op::Modify(i, g)),
        2 => any::<usize>().prop_map(Op::Remove),
        1 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

/// Live model-space IDs in draw order.
fn live_ids(session: &Session) -> Vec<EntityId> {
    session
        .document()
        .container(ContainerRef::ModelSpace)
        .map(|c| c.iter_records().map(|r| r.id).collect())
        .unwrap_or_default()
}

/// Verifies cached boxes against materialized geometry through both read paths.
fn assert_bbox_column_matches(session: &Session) {
    let c = session
        .document()
        .container(ContainerRef::ModelSpace)
        .expect("model space existe");

    for rec in c.iter_records() {
        assert_eq!(
            c.bbox(rec.id),
            Some(rec.geometry.bbox()),
            "columna bbox != geo.bbox() (lookup) para id {:?}",
            rec.id
        );
    }

    let visited: Vec<(EntityId, af_math::BBox)> = {
        let mut v = Vec::new();
        c.visit_bboxes(|id, _common, bb| v.push((id, *bb)));
        v
    };
    let expected: Vec<(EntityId, af_math::BBox)> = c
        .iter_records()
        .map(|r| (r.id, r.geometry.bbox()))
        .collect();
    assert_eq!(
        visited, expected,
        "visit_bboxes no coincide con geo.bbox() en orden de dibujo"
    );
}

fn new_record(session: &Session, geometry: EntityGeometry) -> EntityRecord {
    // `add_entity` replaces this placeholder ID; use the current layer.
    EntityRecord::new(
        EntityId::from(af_model::id::ObjectId::NIL),
        session.document().current_layer(),
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(80))]

    #[test]
    fn columna_bbox_es_geo_bbox_bajo_mutaciones_via_tx(ops in vec(op_strategy(), 0..70)) {
        let mut session = Session::new(Units::default());
        let mut counter: u64 = 0;

        for op in ops {
            match op {
                Op::Add(g) => {
                    counter += 1;
                    let geometry = g.build(counter);
                    let record = new_record(&session, geometry);
                    // A rejected insertion does not mutate the document.
                    let _ = session.transact("add", |tx| -> Result<(), TxError> {
                        tx.add_entity(ContainerRef::ModelSpace, record.clone())?;
                        Ok(())
                    });
                }
                Op::Modify(i, g) => {
                    let ids = live_ids(&session);
                    if ids.is_empty() {
                        continue;
                    }
                    counter += 1;
                    let id = ids[i % ids.len()];
                    let geometry = g.build(counter);
                    let _ = session.transact("modify", |tx| -> Result<(), TxError> {
                        tx.modify_entity(id, |rec| rec.geometry = geometry.clone())?;
                        Ok(())
                    });
                }
                Op::Remove(i) => {
                    let ids = live_ids(&session);
                    if ids.is_empty() {
                        continue;
                    }
                    let id = ids[i % ids.len()];
                    let _ = session.transact("remove", |tx| -> Result<(), TxError> {
                        tx.remove_entity(id)?;
                        Ok(())
                    });
                }
                Op::Undo => {
                    let _ = session.undo();
                }
                Op::Redo => {
                    let _ = session.redo();
                }
            }

            // Check the invariant after every operation, including rollback and history.
            assert_bbox_column_matches(&session);
        }
    }
}
