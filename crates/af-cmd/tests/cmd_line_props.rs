//! LINETYPE, LWEIGHT, and LTSCALE tests for reversible changes and LINE inheritance.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_model::Session;
use af_model::entity::{LineTypeRef, Lineweight};
use af_model::units::Units;
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

#[test]
fn defaults_are_bylayer_and_ltscale_one() {
    let (_reg, session) = setup();
    assert_eq!(session.document().current_line_type(), LineTypeRef::ByLayer);
    assert_eq!(session.document().current_lineweight(), Lineweight::ByLayer);
    assert_eq!(session.document().ltscale(), 1.0);
    assert_eq!(session.document().line_types().count(), 1);
}

#[test]
fn linetype_loads_from_library_and_sets_current_in_one_tx() {
    let (reg, mut session) = setup();
    let depth_before = session.history().undo_depth();

    reg.execute(&mut session, "LT", &json!({ "linetype": "dashed" }))
        .expect("LINETYPE via alias LT");

    let dashed = session
        .document()
        .line_type_by_name("DASHED")
        .expect("DASHED cargado");
    assert!(!dashed.is_continuous());
    assert_eq!(dashed.pattern(), &[0.5, -0.25]);
    assert_eq!(
        session.document().current_line_type(),
        LineTypeRef::Style(dashed.id())
    );
    assert_eq!(session.history().undo_depth(), depth_before + 1);
    assert_eq!(session.document().line_types().count(), 2);
}

#[test]
fn linetype_set_bylayer_and_unknown_is_rejected() {
    let (reg, mut session) = setup();
    reg.execute(
        &mut session,
        "LINETYPE",
        &json!({ "linetype": "Continuous" }),
    )
    .expect("set Continuous");
    let cont = session
        .document()
        .line_type_by_name("Continuous")
        .unwrap()
        .id();
    assert_eq!(
        session.document().current_line_type(),
        LineTypeRef::Style(cont)
    );

    let err = reg
        .execute(&mut session, "LINETYPE", &json!({ "linetype": "NOPE" }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
}

#[test]
fn subsequent_line_inherits_celtype_and_celweight() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LT", &json!({ "linetype": "hidden" }))
        .expect("LINETYPE");
    reg.execute(&mut session, "LW", &json!({ "lineweight": "0.5" }))
        .expect("LWEIGHT");

    let hidden = session.document().line_type_by_name("HIDDEN").unwrap().id();

    let out = reg
        .execute(&mut session, "LINE", &json!({ "p1": [0, 0], "p2": [1, 1] }))
        .expect("LINE executes");
    let (rec, _) = session.document().entity(out.created[0]).unwrap();
    assert_eq!(rec.line_type, LineTypeRef::Style(hidden));
    assert_eq!(rec.lineweight, Lineweight::Mm(0.5));
}

#[test]
fn lweight_sets_current_and_is_reversible() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LWEIGHT", &json!({ "lineweight": "byblock" }))
        .expect("LWEIGHT");
    assert_eq!(session.document().current_lineweight(), Lineweight::ByBlock);
    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO");
    assert_eq!(session.document().current_lineweight(), Lineweight::ByLayer);
}

#[test]
fn ltscale_sets_doc_prop_and_rejects_non_positive() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "LTS", &json!({ "scale": 2.5 }))
        .expect("LTSCALE via alias LTS");
    assert_eq!(session.document().ltscale(), 2.5);

    let err = reg
        .execute(&mut session, "LTSCALE", &json!({ "scale": 0 }))
        .unwrap_err();
    assert!(!matches!(err, CmdError::ContractViolation(_)));
    assert_eq!(
        session.document().ltscale(),
        2.5,
        "sin cambio tras el rechazo"
    );

    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO");
    assert_eq!(session.document().ltscale(), 1.0);
}
