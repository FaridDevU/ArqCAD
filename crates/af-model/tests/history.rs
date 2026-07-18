//! Integration tests for the public undo/redo history API:
//! `Session::{transact, undo, redo, can_undo, can_redo, undo_label, redo_label,
//! history_labels, set_undo_limit}`.
//!
//! Tests use only the public surface and never access document internals.
//!
//! # Allocator behavior
//!
//! Undo never rewinds `nextObjectId`. State identity therefore compares the full
//! serialized document except for that monotonic field through [`canonical`].

use af_math::{Point2, Transform2, Vec2};
use af_model::entity::{
    Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Cause, ContainerRef, Document, RedoError, Session, TxError, UndoError};
use af_model::{apply_forward, apply_inverse};
use proptest::prelude::*;

// --------------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------------

fn point_rec(layer: LayerId, x: f64, y: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(Point2::new(x, y))),
    )
}

fn line_rec(layer: LayerId, x1: f64, y1: f64, x2: f64, y2: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2))),
    )
}

/// Canonical document serialization with `nextObjectId` neutralized to zero.
/// All remaining JSON, including model-space draw order, compares byte for byte.
fn canonical(doc: &Document) -> String {
    let mut v = serde_json::to_value(doc).expect("el documento serializa");
    if let Some(map) = v.as_object_mut() {
        map.insert("nextObjectId".to_string(), serde_json::Value::from(0u64));
    }
    serde_json::to_string(&v).expect("el valor serializa")
}

/// Adds a model-space point in its own transaction and returns its ID.
fn add_point(session: &mut Session, x: f64, y: f64) -> EntityId {
    let layer = session.document().current_layer();
    session
        .transact("add point", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, x, y))
        })
        .expect("commit")
        .value
}

// --------------------------------------------------------------------------
// Undo restores byte-identical state apart from `nextObjectId`.
// --------------------------------------------------------------------------

#[test]
fn undo_transact_is_byte_identical_except_for_next_object_id() {
    let mut session = Session::new(Units::default());
    let s0 = canonical(session.document());
    let n0 = session.document().next_object_id();

    // One transaction inserts three entities and advances the allocator.
    let layer = session.document().current_layer();
    session
        .transact("add three", |tx| -> Result<(), TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 0.0, 0.0))?;
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 1.0, 0.0))?;
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 2.0, 0.0))?;
            Ok(())
        })
        .unwrap();
    assert_ne!(canonical(session.document()), s0, "la tx sí cambia el doc");

    let cs = session.undo().expect("hay algo que deshacer");
    assert_eq!(cs.cause(), Cause::Undo);

    // State is byte-identical except for `nextObjectId`.
    assert_eq!(
        canonical(session.document()),
        s0,
        "undo after transact preserves bytes except for the monotonic nextObjectId allocator"
    );

    // Undo does not rewind the three allocated IDs.
    let n1 = session.document().next_object_id();
    assert!(n1 >= n0, "nextObjectId never decreases");
    assert_eq!(n1, n0 + 3, "avanzó por las 3 altas; el undo lo conserva");
}

#[test]
fn identidad_encadenada_3tx_3undo_3redo_draw_order_incluido() {
    let mut session = Session::new(Units::default());

    let s0 = canonical(session.document());
    let a = add_point(&mut session, 0.0, 0.0);
    let s1 = canonical(session.document());
    let _b = add_point(&mut session, 1.0, 0.0);
    let s2 = canonical(session.document());
    // The third transaction edits A without changing draw order.
    session
        .transact("move A", |tx| -> Result<(), TxError> {
            tx.modify_entity(a, |r| {
                let t = Transform2::translate(Vec2::new(10.0, 5.0));
                if let Ok(g) = r.geometry.transform(&t) {
                    r.geometry = g;
                }
            })
        })
        .unwrap();
    let s3 = canonical(session.document());

    // Three undos traverse s2, s1, then s0.
    assert_eq!(session.undo().unwrap().cause(), Cause::Undo);
    assert_eq!(canonical(session.document()), s2);
    session.undo().unwrap();
    assert_eq!(canonical(session.document()), s1);
    session.undo().unwrap();
    assert_eq!(canonical(session.document()), s0);
    assert!(!session.can_undo());
    assert_eq!(session.undo(), Err(UndoError::NothingToUndo));

    // Three redos traverse s1, s2, then s3.
    assert_eq!(session.redo().unwrap().cause(), Cause::Redo);
    assert_eq!(canonical(session.document()), s1);
    session.redo().unwrap();
    assert_eq!(canonical(session.document()), s2);
    session.redo().unwrap();
    assert_eq!(canonical(session.document()), s3);
    assert!(!session.can_redo());
    assert_eq!(session.redo(), Err(RedoError::NothingToRedo));
}

#[test]
fn undo_de_remove_del_medio_restaura_draw_order() {
    let mut session = Session::new(Units::default());
    let layer = session.document().current_layer();
    let ids: Vec<EntityId> = session
        .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
            Ok(vec![
                tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 0.0, 0.0))?,
                tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 1.0, 0.0))?,
                tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 2.0, 0.0))?,
            ])
        })
        .unwrap()
        .value;
    let seeded = canonical(session.document());

    session
        .transact("erase middle", |tx| -> Result<(), TxError> {
            tx.remove_entity(ids[1])
        })
        .unwrap();
    // Removal leaves draw order [0, 2].
    let order: Vec<EntityId> = session
        .document()
        .model_space()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(order, vec![ids[0], ids[2]]);

    // Undo restores the middle entity at its exact position.
    session.undo().unwrap();
    assert_eq!(canonical(session.document()), seeded);
    assert_eq!(session.document().model_space().index_of(ids[1]), Some(1));
}

// --------------------------------------------------------------------------
// A new nonempty transaction clears redo.
// --------------------------------------------------------------------------

#[test]
fn nueva_tx_no_vacia_limpia_redo() {
    let mut session = Session::new(Units::default());
    add_point(&mut session, 0.0, 0.0);
    add_point(&mut session, 1.0, 0.0);
    session.undo().unwrap();
    assert!(session.can_redo(), "hay algo que rehacer tras el undo");

    // A new nonempty transaction invalidates redo.
    add_point(&mut session, 2.0, 0.0);
    assert!(!session.can_redo(), "la tx nueva limpió el redo");
    assert_eq!(session.redo(), Err(RedoError::NothingToRedo));

    // An empty transaction does not affect history.
    add_point(&mut session, 3.0, 0.0);
    session.undo().unwrap();
    assert!(session.can_redo());
    let empty = session
        .transact("noop", |_tx| -> Result<(), TxError> { Ok(()) })
        .unwrap();
    assert!(empty.transaction.is_none());
    assert!(session.can_redo(), "una tx vacía no limpia el redo");
}

// --------------------------------------------------------------------------
// History limits evict old entries without corrupting state.
// --------------------------------------------------------------------------

#[test]
fn limite_expulsa_las_mas_viejas_sin_corromper() {
    let mut session = Session::new(Units::default());
    session.set_undo_limit(3);
    assert_eq!(session.undo_limit(), 3);

    // The limit retains only the latest three of five labeled transactions.
    for i in 0..5 {
        let layer = session.document().current_layer();
        let label = format!("tx{i}");
        session
            .transact(label, |tx| -> Result<(), TxError> {
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    point_rec(layer, f64::from(i), 0.0),
                )?;
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(session.history_labels(), vec!["tx2", "tx3", "tx4"]);

    // Exactly three transactions can be undone.
    let snap_after_5 = canonical(session.document());
    assert!(session.undo().is_ok());
    assert!(session.undo().is_ok());
    assert!(session.undo().is_ok());
    assert!(!session.can_undo(), "solo 3 en la pila");
    assert_eq!(session.undo(), Err(UndoError::NothingToUndo));

    // Redoing all three restores the state after all five transactions.
    session.redo().unwrap();
    session.redo().unwrap();
    session.redo().unwrap();
    assert_eq!(canonical(session.document()), snap_after_5);
}

#[test]
fn set_undo_limit_0_deshabilita_limpio() {
    let mut session = Session::new(Units::default());
    add_point(&mut session, 0.0, 0.0);
    add_point(&mut session, 1.0, 0.0);
    session.undo().unwrap();
    assert!(session.can_undo() && session.can_redo());

    session.set_undo_limit(0);
    assert_eq!(session.undo_limit(), 0);
    assert!(!session.can_undo(), "deshabilitado: sin undo");
    assert!(!session.can_redo(), "deshabilitado: sin redo");
    assert!(session.history_labels().is_empty());

    // A zero limit still permits mutations but records no history.
    let before = canonical(session.document());
    add_point(&mut session, 2.0, 0.0);
    assert_ne!(canonical(session.document()), before, "el doc sí cambió");
    assert!(!session.can_undo(), "pero no hay nada que deshacer");
    assert_eq!(session.undo(), Err(UndoError::NothingToUndo));
}

// --------------------------------------------------------------------------
// Errors and labels.
// --------------------------------------------------------------------------

#[test]
fn nothing_to_undo_y_nothing_to_redo_en_sesion_limpia() {
    let mut session = Session::new(Units::default());
    assert!(!session.can_undo());
    assert!(!session.can_redo());
    assert_eq!(session.undo(), Err(UndoError::NothingToUndo));
    assert_eq!(session.redo(), Err(RedoError::NothingToRedo));
    assert_eq!(session.undo_label(), None);
    assert_eq!(session.redo_label(), None);
}

#[test]
fn labels_reflejan_las_transacciones() {
    let mut session = Session::new(Units::default());
    let layer = session.document().current_layer();
    session
        .transact("Move", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 0.0, 0.0))
        })
        .unwrap();
    session
        .transact("Erase", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 1.0, 0.0))
        })
        .unwrap();

    assert_eq!(session.undo_label(), Some("Erase"));
    assert_eq!(session.redo_label(), None);
    assert_eq!(session.history_labels(), vec!["Move", "Erase"]);

    session.undo().unwrap();
    assert_eq!(session.undo_label(), Some("Move"));
    assert_eq!(
        session.redo_label(),
        Some("Erase"),
        "lo deshecho es rehacible"
    );
    assert_eq!(session.history_labels(), vec!["Move"]);
}

// --------------------------------------------------------------------------
// Undo and redo do not revalidate command rules.
// --------------------------------------------------------------------------

#[test]
fn undo_redo_no_revalidan_capa_locked_posterior() {
    // Snapshot application ignores a layer locked after the original command.
    // Build that state through the public serialized document form.
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let out = session
        .transact("add on layer 0", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, line_rec(l0, 0.0, 0.0, 1.0, 1.0))
        })
        .unwrap();
    let id = out.value;
    let tx = out.transaction.unwrap();

    let mut locked = lock_all_layers(session.document());
    assert!(locked.layer(l0).unwrap().is_locked());
    assert!(locked.entity(id).is_some());

    // Inverse application succeeds on the locked layer.
    apply_inverse(&mut locked, &tx).expect("undo ignora el lock posterior");
    assert!(locked.entity(id).is_none());
    assert!(locked.layer(l0).unwrap().is_locked(), "el lock sigue ahí");

    // Forward application recreates the entity with its original ID.
    apply_forward(&mut locked, &tx).expect("redo ignora el lock posterior");
    assert!(locked.entity(id).is_some());
}

/// Round-trips through public JSON while marking every layer as locked.
fn lock_all_layers(doc: &Document) -> Document {
    let mut v = serde_json::to_value(doc).expect("doc serializa");
    let layers = v
        .get_mut("layers")
        .and_then(serde_json::Value::as_object_mut)
        .expect("layers es un objeto");
    for (_id, layer) in layers.iter_mut() {
        let obj = layer.as_object_mut().expect("cada capa es un objeto");
        obj.insert("locked".to_string(), serde_json::Value::Bool(true));
    }
    serde_json::from_value(v).expect("doc con capa bloqueada deserializa")
}

// --------------------------------------------------------------------------
// History is not serialized with the document.
// --------------------------------------------------------------------------

#[test]
fn el_json_del_doc_no_cambia_por_tener_historia() {
    // Pending undo and redo entries do not alter document serialization.
    let mut session = Session::new(Units::default());
    add_point(&mut session, 0.0, 0.0); // Entity X.
    let just_x = canonical(session.document());

    add_point(&mut session, 9.0, 9.0); // Entity Y.
    session.undo().unwrap(); // Undo Y, leaving X and one redo entry.

    assert!(session.can_redo(), "hay historia (redo pendiente)");
    assert_eq!(
        canonical(session.document()),
        just_x,
        "el doc neto es {{X}}: la historia no altera el JSON del documento"
    );

    // Raw JSON contains no history or redo state.
    let raw = serde_json::to_string(session.document()).unwrap();
    assert!(!raw.contains("history"), "sin campo history");
    assert!(!raw.contains("redo"), "sin campo redo");
    assert!(!raw.contains("undo"), "sin campo undo");
}

// --------------------------------------------------------------------------
// Property test against reference serialization stacks.
// --------------------------------------------------------------------------

/// Simulated command or history navigation action.
#[derive(Debug, Clone)]
enum Op {
    AddPoint(f64, f64),
    AddLine(f64, f64),
    MoveFirst(f64, f64),
    ToggleFirst,
    RemoveFirst,
    Undo,
    Redo,
}

fn coord() -> impl Strategy<Value = f64> {
    // Bounded integer coordinates avoid floating-point serialization edges.
    (-50i32..=50).prop_map(f64::from)
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (coord(), coord()).prop_map(|(x, y)| Op::AddPoint(x, y)),
        2 => (coord(), coord()).prop_map(|(x, y)| Op::AddLine(x, y)),
        2 => (coord(), coord()).prop_map(|(dx, dy)| Op::MoveFirst(dx, dy)),
        1 => Just(Op::ToggleFirst),
        2 => Just(Op::RemoveFirst),
        3 => Just(Op::Undo),
        3 => Just(Op::Redo),
    ]
}

/// ID of the first model-space entity in draw order, if any.
fn first_id(session: &Session) -> Option<EntityId> {
    session.document().model_space().iter().next().map(|r| r.id)
}

/// Runs a simulated command and reports whether it committed a nonempty transaction.
fn run_command(session: &mut Session, op: &Op) -> bool {
    let layer = session.document().current_layer();
    let outcome = match op {
        Op::AddPoint(x, y) => session.transact("add point", |tx| -> Result<(), TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, *x, *y))?;
            Ok(())
        }),
        Op::AddLine(x, y) => session.transact("add line", |tx| -> Result<(), TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                line_rec(layer, *x, *y, *x + 1.0, *y + 1.0),
            )?;
            Ok(())
        }),
        Op::MoveFirst(dx, dy) => match first_id(session) {
            None => return false,
            Some(id) => session.transact("move", |tx| -> Result<(), TxError> {
                tx.modify_entity(id, |r| {
                    let t = Transform2::translate(Vec2::new(*dx, *dy));
                    if let Ok(g) = r.geometry.transform(&t) {
                        r.geometry = g;
                    }
                })
            }),
        },
        Op::ToggleFirst => match first_id(session) {
            None => return false,
            Some(id) => session.transact("toggle", |tx| -> Result<(), TxError> {
                tx.modify_entity(id, |r| r.visible = !r.visible)
            }),
        },
        Op::RemoveFirst => match first_id(session) {
            None => return false,
            Some(id) => session.transact("erase", |tx| -> Result<(), TxError> {
                tx.remove_entity(id)
            }),
        },
        Op::Undo | Op::Redo => unreachable!("undo/redo se manejan en el bucle"),
    };
    // Rollback preserves the document if an unexpected command error occurs.
    matches!(outcome, Ok(o) if o.transaction.is_some())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Random command, undo, and redo sequences against reference state stacks.
    #[test]
    fn undo_redo_contra_oraculo(
        ops in prop::collection::vec(op_strategy(), 1..70),
        limit in 1usize..=5,
    ) {
        let mut session = Session::new(Units::default());
        session.set_undo_limit(limit);

        // Reference stacks use canonical states with a neutralized allocator.
        // `states_undo[i]` is the state before transaction i; redo is LIFO.
        let mut states_undo: Vec<String> = Vec::new();
        let mut states_redo: Vec<String> = Vec::new();

        // `nextObjectId` never decreases throughout the sequence.
        let mut max_next = session.document().next_object_id();

        for op in &ops {
            match op {
                Op::Undo => {
                    if session.can_undo() {
                        let current = canonical(session.document());
                        let cs = session.undo().expect("can_undo => Ok");
                        prop_assert_eq!(cs.cause(), Cause::Undo);
                        let expected = states_undo.pop().expect("oráculo tiene destino");
                        prop_assert_eq!(canonical(session.document()), expected);
                        states_redo.push(current);
                    } else {
                        prop_assert!(states_undo.is_empty());
                        prop_assert_eq!(session.undo(), Err(UndoError::NothingToUndo));
                    }
                }
                Op::Redo => {
                    if session.can_redo() {
                        let current = canonical(session.document());
                        let cs = session.redo().expect("can_redo => Ok");
                        prop_assert_eq!(cs.cause(), Cause::Redo);
                        let expected = states_redo.pop().expect("oráculo tiene destino");
                        prop_assert_eq!(canonical(session.document()), expected);
                        states_undo.push(current);
                        // Redo cannot exceed the fixed undo limit.
                        prop_assert!(states_undo.len() <= limit);
                    } else {
                        prop_assert!(states_redo.is_empty());
                        prop_assert_eq!(session.redo(), Err(RedoError::NothingToRedo));
                    }
                }
                command => {
                    let before = canonical(session.document());
                    let committed = run_command(&mut session, command);
                    if committed {
                        states_undo.push(before);
                        states_redo.clear();
                        // Mirror `History::record` by evicting the oldest state.
                        while states_undo.len() > limit {
                            states_undo.remove(0);
                        }
                    }
                }
            }

            // Session and reference stacks match after every action.
            prop_assert_eq!(session.can_undo(), !states_undo.is_empty());
            prop_assert_eq!(session.can_redo(), !states_redo.is_empty());
            prop_assert_eq!(session.history().undo_depth(), states_undo.len());
            prop_assert_eq!(session.history().redo_depth(), states_redo.len());

            // `nextObjectId` never decreases.
            let n = session.document().next_object_id();
            prop_assert!(n >= max_next, "nextObjectId decreased: {} < {}", n, max_next);
            max_next = n;
        }
    }
}
