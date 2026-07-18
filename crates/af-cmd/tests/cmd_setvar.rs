//! SETVAR tests for zero-transaction session state and model type/range validation.

use af_cmd::builtin::register_builtins;
use af_cmd::{CmdError, CommandRegistry};
use af_model::Session;
use af_model::units::Units;
use serde_json::json;

fn setup() -> (CommandRegistry, Session) {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("builtins register");
    (reg, Session::new(Units::default()))
}

#[test]
fn setvar_reports_current_value_zero_tx() {
    let (reg, mut session) = setup();
    let before = serde_json::to_string(session.document()).unwrap();
    let out = reg
        .execute(&mut session, "SETVAR", &json!({ "name": "OSMODE" }))
        .expect("SETVAR reports");
    assert!(out.tx_seq.is_none(), "leer una sysvar es 0 tx");
    assert_eq!(out.message.as_deref(), Some("OSMODE = 4133"));
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    assert!(!session.can_undo());
}

#[test]
fn setvar_sets_int_zero_tx() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "SETVAR",
            &json!({ "name": "osmode", "value": "191" }),
        )
        .expect("SETVAR sets");
    assert!(out.tx_seq.is_none(), "escribir una sysvar es 0 tx");
    assert_eq!(out.message.as_deref(), Some("OSMODE = 191"));
    let back = reg
        .execute(&mut session, "SETVAR", &json!({ "name": "OSMODE" }))
        .unwrap();
    assert_eq!(back.message.as_deref(), Some("OSMODE = 191"));
    assert!(!session.can_undo(), "las sysvars no entran en el undo");
}

#[test]
fn setvar_sets_real2_comma_separates_pair() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(
            &mut session,
            "SETVAR",
            &json!({ "name": "SNAPUNIT", "value": "1.5,2.25" }),
        )
        .expect("SETVAR sets real2");
    assert!(out.tx_seq.is_none());
    assert_eq!(out.message.as_deref(), Some("SNAPUNIT = 1.5,2.25"));
}

#[test]
fn setvar_out_of_range_is_error() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "SETVAR",
            &json!({ "name": "APERTURE", "value": "999" }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
    let back = reg
        .execute(&mut session, "SETVAR", &json!({ "name": "APERTURE" }))
        .unwrap();
    assert_eq!(back.message.as_deref(), Some("APERTURE = 10"));
}

#[test]
fn setvar_unknown_sysvar_is_error() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(&mut session, "SETVAR", &json!({ "name": "NOPE" }))
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
}

#[test]
fn setvar_type_mismatch_is_error() {
    let (reg, mut session) = setup();
    let err = reg
        .execute(
            &mut session,
            "SETVAR",
            &json!({ "name": "OSMODE", "value": "1.5" }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::Failed(_)));
}

#[test]
fn setvar_dash_alias_works() {
    let (reg, mut session) = setup();
    let out = reg
        .execute(&mut session, "-SETVAR", &json!({ "name": "OSMODE" }))
        .expect("-SETVAR alias");
    assert_eq!(out.message.as_deref(), Some("OSMODE = 4133"));
}
