//! Linear oracle comparing incremental indexing with container contents and scans.

mod common;

use af_math::{BBox, Point2};
use af_model::entity::{EntityGeometry, EntityOps, LineGeo};
use af_model::id::EntityId;
use af_model::units::Units;
use af_model::{ContainerRef, Session, TxError};
use af_select::SpatialIndex;
use common::line_rec;
use proptest::prelude::*;

/// Randomized script operation; indices are reduced modulo live entity count.
#[derive(Debug, Clone)]
enum Op {
    Add(f64, f64, f64, f64),
    Remove(usize),
    Modify(usize, f64, f64, f64, f64),
    Undo,
    Redo,
}

fn coord() -> impl Strategy<Value = f64> {
    -100.0f64..100.0
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (coord(), coord(), coord(), coord()).prop_map(|(a, b, c, d)| Op::Add(a, b, c, d)),
        any::<usize>().prop_map(Op::Remove),
        (any::<usize>(), coord(), coord(), coord(), coord())
            .prop_map(|(k, a, b, c, d)| Op::Modify(k, a, b, c, d)),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

/// Model-space IDs in draw order.
fn live_ids(s: &Session) -> Vec<EntityId> {
    s.document().model_space().iter().map(|r| r.id).collect()
}

/// Model-space IDs sorted for set comparison.
fn sorted_ids(s: &Session) -> Vec<EntityId> {
    let mut v = live_ids(s);
    v.sort_unstable_by_key(|id| id.raw().0);
    v
}

/// Linear oracle returning sorted IDs whose bounds intersect `rect`.
fn linear_query(s: &Session, rect: BBox) -> Vec<EntityId> {
    let mut v: Vec<EntityId> = s
        .document()
        .model_space()
        .iter()
        .filter(|r| r.geometry.bbox().intersects(rect))
        .map(|r| r.id)
        .collect();
    v.sort_unstable_by_key(|id| id.raw().0);
    v
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(120))]

    #[test]
    fn indice_equivale_al_contenido_y_a_la_query_lineal(
        ops in prop::collection::vec(arb_op(), 0..40),
        px in -150.0f64..150.0,
        py in -150.0f64..150.0,
        rw in 1.0f64..120.0,
        rh in 1.0f64..120.0,
    ) {
        let mut s = Session::new(Units::default());
        let layer = s.document().current_layer();
        let mut idx = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);

        for op in ops {
            match op {
                Op::Add(a, b, c, d) => {
                    let rec = line_rec(layer, Point2::new(a, b), Point2::new(c, d));
                    let out = s
                        .transact("add", |tx| -> Result<_, TxError> {
                            tx.add_entity(ContainerRef::ModelSpace, rec)
                        })
                        .expect("add");
                    if let Some(cs) = out.change_set {
                        idx.apply_changeset(&cs, s.document());
                    }
                }
                Op::Remove(k) => {
                    let ids = live_ids(&s);
                    if !ids.is_empty() {
                        let id = ids[k % ids.len()];
                        let out = s
                            .transact("rm", |tx| -> Result<_, TxError> { tx.remove_entity(id) })
                            .expect("rm");
                        if let Some(cs) = out.change_set {
                            idx.apply_changeset(&cs, s.document());
                        }
                    }
                }
                Op::Modify(k, a, b, c, d) => {
                    let ids = live_ids(&s);
                    if !ids.is_empty() {
                        let id = ids[k % ids.len()];
                        let out = s
                            .transact("mod", |tx| -> Result<_, TxError> {
                                tx.modify_entity(id, |r| {
                                    r.geometry = EntityGeometry::Line(LineGeo::new(
                                        Point2::new(a, b),
                                        Point2::new(c, d),
                                    ));
                                })
                            })
                            .expect("mod");
                        if let Some(cs) = out.change_set {
                            idx.apply_changeset(&cs, s.document());
                        }
                    }
                }
                Op::Undo => {
                    if let Ok(cs) = s.undo() {
                        idx.apply_changeset(&cs, s.document());
                    }
                }
                Op::Redo => {
                    if let Ok(cs) = s.redo() {
                        idx.apply_changeset(&cs, s.document());
                    }
                }
            }

            // Index IDs must exactly match container IDs.
            prop_assert_eq!(idx.ids(), sorted_ids(&s));

            // Rectangle queries must match the linear scan.
            let rect = BBox::new(
                Point2::new(px, py),
                Point2::new(px + rw, py + rh),
            );
            let mut got = idx.query_rect(rect);
            got.sort_unstable_by_key(|id| id.raw().0);
            prop_assert_eq!(got, linear_query(&s, rect));
        }

        // Incremental state must match a fresh bulk rebuild.
        let rebuilt = SpatialIndex::build(s.document(), ContainerRef::ModelSpace);
        prop_assert_eq!(idx.ids(), rebuilt.ids());
        prop_assert!(idx.len() <= 500);
    }
}
