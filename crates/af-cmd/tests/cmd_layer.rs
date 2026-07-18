//! End-to-end LAYER tests for all operations, protected-layer policies, aliases,
//! and the one-transaction contract.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::id::ObjectId;
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

// ---- new ---------------------------------------------------------------------

#[test]
fn new_creates_layer_with_factory_defaults_in_one_tx() {
    let (reg, mut session) = setup();
    let depth_before = session.history().undo_depth();

    reg.execute(&mut session, "LA", &json!({ "op": "new", "name": "Muros" }))
        .expect("LAYER new executes");

    assert_eq!(session.history().undo_depth(), depth_before + 1);
    let layer = session
        .document()
        .layer_by_name("Muros")
        .expect("layer created");
    assert_eq!(layer.color(), Color::aci(7).unwrap());
    assert_eq!(layer.lineweight(), Lineweight::ByLayer);
    assert!(!layer.is_off() && !layer.is_frozen() && !layer.is_locked());
    assert!(layer.is_plottable());
    let continuous = session
        .document()
        .line_types()
        .find(|lt| lt.name() == "Continuous")
        .unwrap();
    assert_eq!(layer.line_type(), continuous.id());
}

#[test]
fn new_without_name_is_missing_param() {
    let (reg, mut session) = setup();
    assert_eq!(
        reg.execute(&mut session, "LAYER", &json!({ "op": "new" }))
            .unwrap_err(),
        CmdError::MissingParam("name".to_string())
    );
}

// ---- delete --------------------------------------------------------------

#[test]
fn delete_removes_empty_layer() {
    let (reg, mut session) = setup();
    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "new", "name": "Vacia" }),
    )
    .expect("new");
    let id = session.document().layer_by_name("Vacia").unwrap().id();

    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "delete", "layer": id.raw().0 }),
    )
    .expect("delete executes");
    assert!(session.document().layer(id).is_none());
}

/// Verifies that used layers are rejected instead of deleting entities implicitly.
#[test]
fn delete_layer_with_entities_is_rejected_by_model_policy() {
    let (reg, mut session) = setup();
    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "new", "name": "Muros" }),
    )
    .expect("new");
    let layer = session.document().layer_by_name("Muros").unwrap().id();
    session
        .transact("seed", |tx| -> Result<(), TxError> {
            tx.add_entity(
                ContainerRef::ModelSpace,
                EntityRecord::new(
                    ObjectId::NIL.into(),
                    layer,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(1.0, 1.0),
                    )),
                ),
            )?;
            Ok(())
        })
        .expect("seed commits");

    let err = reg
        .execute(
            &mut session,
            "LAYER",
            &json!({ "op": "delete", "layer": layer.raw().0 }),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains("in use"), "msg: {m}"),
        other => panic!("expected Failed(in use), got {other:?}"),
    }
    assert!(session.document().layer(layer).is_some());
}

// ---- rename ----------------------------------------------------------------

#[test]
fn rename_layer_zero_is_rejected() {
    let (reg, mut session) = setup();
    let zero = session.document().layer_by_name("0").unwrap().id();
    let err = reg
        .execute(
            &mut session,
            "LAYER",
            &json!({ "op": "rename", "layer": zero.raw().0, "name": "Nope" }),
        )
        .unwrap_err();
    match err {
        CmdError::Failed(m) => assert!(m.contains('0'), "msg: {m}"),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert_eq!(session.document().layer(zero).unwrap().name(), "0");
}

#[test]
fn rename_happy_path() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LAYER", &json!({ "op": "new", "name": "A" }))
        .expect("new");
    let id = session.document().layer_by_name("A").unwrap().id();

    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "rename", "layer": id.raw().0, "name": "B" }),
    )
    .expect("rename executes");
    assert_eq!(session.document().layer(id).unwrap().name(), "B");
    assert!(session.document().layer_by_name("A").is_none());
}

// ---- color -----------------------------------------------------------------

#[test]
fn color_sets_layer_color_via_aci_and_rgb() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LAYER", &json!({ "op": "new", "name": "A" }))
        .expect("new");
    let id = session.document().layer_by_name("A").unwrap().id();

    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "color", "layer": id.raw().0, "color": "5" }),
    )
    .expect("color executes");
    assert_eq!(
        session.document().layer(id).unwrap().color(),
        Color::aci(5).unwrap()
    );

    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "color", "layer": id.raw().0, "color": "10,20,30" }),
    )
    .expect("color rgb executes");
    assert_eq!(
        session.document().layer(id).unwrap().color(),
        Color::Rgb(10, 20, 30)
    );
}

// ---- on/off/freeze/thaw/lock/unlock/plot/no-plot -----------------------------

#[test]
fn state_toggles_flip_the_matching_flag() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LAYER", &json!({ "op": "new", "name": "A" }))
        .expect("new");
    let id = session.document().layer_by_name("A").unwrap().id();

    let toggle = |session: &mut Session, op: &str| {
        reg.execute(session, "LAYER", &json!({ "op": op, "layer": id.raw().0 }))
            .unwrap_or_else(|e| panic!("op '{op}' failed: {e:?}"));
    };

    toggle(&mut session, "off");
    assert!(session.document().layer(id).unwrap().is_off());
    toggle(&mut session, "on");
    assert!(!session.document().layer(id).unwrap().is_off());

    toggle(&mut session, "freeze");
    assert!(session.document().layer(id).unwrap().is_frozen());
    toggle(&mut session, "thaw");
    assert!(!session.document().layer(id).unwrap().is_frozen());

    toggle(&mut session, "lock");
    assert!(session.document().layer(id).unwrap().is_locked());
    toggle(&mut session, "unlock");
    assert!(!session.document().layer(id).unwrap().is_locked());

    toggle(&mut session, "no-plot");
    assert!(!session.document().layer(id).unwrap().is_plottable());
    toggle(&mut session, "plot");
    assert!(session.document().layer(id).unwrap().is_plottable());
}

#[test]
fn no_plot_is_one_transaction_and_undo_restores_exact_document() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LAYER", &json!({ "op": "new", "name": "A" }))
        .expect("new");
    let id = session.document().layer_by_name("A").unwrap().id();
    let before = serde_json::to_string(session.document()).unwrap();
    let depth_before = session.history().undo_depth();

    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "no-plot", "layer": id.raw().0 }),
    )
    .expect("no-plot executes");

    assert!(!session.document().layer(id).unwrap().is_plottable());
    assert_eq!(session.history().undo_depth(), depth_before + 1);
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("undo no-plot");
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

// ---- set-current -------------------------------------------------------------

#[test]
fn set_current_changes_current_layer() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LAYER", &json!({ "op": "new", "name": "A" }))
        .expect("new");
    let id = session.document().layer_by_name("A").unwrap().id();

    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "set-current", "layer": id.raw().0 }),
    )
    .expect("set-current executes");
    assert_eq!(session.document().current_layer(), id);
}

#[test]
fn set_current_rejects_off_layer() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LAYER", &json!({ "op": "new", "name": "A" }))
        .expect("new");
    let id = session.document().layer_by_name("A").unwrap().id();
    reg.execute(
        &mut session,
        "LAYER",
        &json!({ "op": "off", "layer": id.raw().0 }),
    )
    .expect("off");

    let err = reg
        .execute(
            &mut session,
            "LAYER",
            &json!({ "op": "set-current", "layer": id.raw().0 }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_ne!(session.document().current_layer(), id);
}
