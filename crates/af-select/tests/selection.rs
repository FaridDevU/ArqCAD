//! `SelectionState` ordering, toggling, retention, and callback behavior.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use af_math::Point2;
use af_model::id::{EntityId, ObjectId};
use af_select::SelectionState;
use common::{add, line_rec, session};

fn eid(n: u64) -> EntityId {
    ObjectId(n).into()
}

#[test]
fn orden_de_seleccion_estable() {
    let mut sel = SelectionState::new();
    sel.set([eid(30), eid(10), eid(20)]);
    assert_eq!(sel.items(), vec![eid(30), eid(10), eid(20)]);
    // Append new IDs; duplicates neither reorder nor duplicate.
    sel.add(eid(5));
    sel.add(eid(10));
    assert_eq!(sel.items(), vec![eid(30), eid(10), eid(20), eid(5)]);
}

#[test]
fn toggle_quita_o_anade_al_final() {
    let mut sel = SelectionState::new();
    sel.set([eid(1), eid(2), eid(3)]);
    sel.toggle(eid(2)); // Remove 2 while preserving remaining order.
    assert_eq!(sel.items(), vec![eid(1), eid(3)]);
    sel.toggle(eid(2)); // Re-add at the end.
    assert_eq!(sel.items(), vec![eid(1), eid(3), eid(2)]);
}

#[test]
fn remove_conserva_orden_y_clear_vacia() {
    let mut sel = SelectionState::new();
    sel.set([eid(1), eid(2), eid(3)]);
    sel.remove(eid(1));
    assert_eq!(sel.items(), vec![eid(2), eid(3)]);
    sel.remove(eid(99)); // Missing ID: no-op.
    assert_eq!(sel.items(), vec![eid(2), eid(3)]);
    sel.clear();
    assert!(sel.is_empty());
}

#[test]
fn callback_solo_dispara_en_cambios_efectivos() {
    let log: Rc<RefCell<Vec<Vec<EntityId>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&log);
    let mut sel = SelectionState::new();
    sel.on_change(move |ids| sink.borrow_mut().push(ids.to_vec()));

    sel.set([eid(1), eid(2)]); // A change triggers notification.
    sel.add(eid(2)); // Already present, so no notification.
    sel.add(eid(3)); // A change triggers notification.
    sel.remove(eid(99)); // A missing ID does not notify.
    sel.clear(); // A change triggers notification.
    sel.clear(); // Already empty, so no notification.

    let events = log.borrow();
    assert_eq!(events.len(), 3, "eventos: {events:?}");
    assert_eq!(events[0], vec![eid(1), eid(2)]);
    assert_eq!(events[1], vec![eid(1), eid(2), eid(3)]);
    assert_eq!(events[2], Vec::<EntityId>::new());
}

#[test]
fn set_reordena_y_notifica_si_cambia_el_orden() {
    let log: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let sink = Rc::clone(&log);
    let mut sel = SelectionState::new();
    sel.on_change(move |_| *sink.borrow_mut() += 1);

    sel.set([eid(1), eid(2)]);
    sel.set([eid(2), eid(1)]); // Same elements in a different order must notify.
    sel.set([eid(2), eid(1)]); // Identical order, so no notification.
    assert_eq!(*log.borrow(), 2);
    assert_eq!(sel.items(), vec![eid(2), eid(1)]);
}

#[test]
fn previous_archiva_y_no_notifica() {
    let log: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
    let sink = Rc::clone(&log);
    let mut sel = SelectionState::new();
    sel.on_change(move |_| *sink.borrow_mut() += 1);

    sel.set([eid(1), eid(2)]); // Notify once.
    sel.set_previous([eid(1), eid(2)]); // Archiving does not notify.
    sel.clear(); // Notify twice in total.

    // `Previous` remains available after clearing live selection.
    assert_eq!(sel.previous(), vec![eid(1), eid(2)]);
    assert!(sel.is_empty());
    assert_eq!(*log.borrow(), 2, "solo set + clear notifican");

    // `set_previous` deduplicates in order and replaces the archive.
    sel.set_previous([eid(5), eid(5), eid(3)]);
    assert_eq!(sel.previous(), vec![eid(5), eid(3)]);
}

#[test]
fn retain_existing_purga_tambien_previous() {
    let mut s = session();
    let layer = s.document().current_layer();
    let e1 = add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
    );
    let e2 = add(
        &mut s,
        line_rec(layer, Point2::new(2.0, 2.0), Point2::new(3.0, 3.0)),
    );

    let mut sel = SelectionState::new();
    sel.set_previous([e1, e2]);
    s.undo().expect("undo add e2"); // e2 disappears.

    sel.retain_existing(s.document());
    assert_eq!(
        sel.previous(),
        vec![e1],
        "Previous must not retain dangling IDs"
    );
}

#[test]
fn retain_existing_quita_ids_desaparecidos_tras_undo() {
    let mut s = session();
    let layer = s.document().current_layer();
    let e1 = add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)),
    );
    let e2 = add(
        &mut s,
        line_rec(layer, Point2::new(2.0, 2.0), Point2::new(3.0, 3.0)),
    );

    let mut sel = SelectionState::new();
    sel.set([e1, e2]);

    // Undoing insertion removes e2; IDs are never recycled.
    s.undo().expect("undo add e2");
    assert!(s.document().entity(e2).is_none());

    sel.retain_existing(s.document());
    assert_eq!(
        sel.items(),
        vec![e1],
        "e2 desaparecido => fuera de la selección"
    );

    // Redo restores e2, but selection is not undoable and remains unchanged.
    s.redo().expect("redo");
    sel.retain_existing(s.document());
    assert_eq!(sel.items(), vec![e1]);
}
