//! COLOR tests for reversible CECOLOR changes and immediate LINE inheritance.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_model::Session;
use af_model::entity::Color;
use af_model::units::Units;
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

#[test]
fn default_current_color_is_bylayer() {
    let (_reg, session) = setup();
    assert_eq!(session.document().current_color(), Color::ByLayer);
}

#[test]
fn color_sets_current_color_in_one_tx() {
    let (reg, mut session) = setup();
    let depth_before = session.history().undo_depth();

    reg.execute(&mut session, "COL", &json!({ "color": "3" }))
        .expect("COLOR executes via alias COL");

    assert_eq!(session.document().current_color(), Color::aci(3).unwrap());
    assert_eq!(session.history().undo_depth(), depth_before + 1);
}

#[test]
fn subsequent_line_picks_up_current_color() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "COLOR", &json!({ "color": "1" }))
        .expect("COLOR executes");

    let out = reg
        .execute(&mut session, "LINE", &json!({ "p1": [0, 0], "p2": [1, 1] }))
        .expect("LINE executes");
    let (rec, _) = session.document().entity(out.created[0]).unwrap();
    assert_eq!(rec.color, Color::aci(1).unwrap());
}

#[test]
fn color_undo_restores_bylayer() {
    let (reg, mut session) = setup();
    reg.execute(&mut session, "COLOR", &json!({ "color": "byblock" }))
        .expect("COLOR executes");
    assert_eq!(session.document().current_color(), Color::ByBlock);

    reg.execute(&mut session, "UNDO", &serde_json::Value::Null)
        .expect("UNDO executes");
    assert_eq!(session.document().current_color(), Color::ByLayer);
}

#[test]
fn invalid_color_text_is_rejected() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "COLOR", &json!({ "color": "not-a-color" }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    assert_eq!(session.document().current_color(), Color::ByLayer);
}
