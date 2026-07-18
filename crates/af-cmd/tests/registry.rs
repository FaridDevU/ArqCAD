//! Registry collision, lookup, alias-precedence, and parameter-validation tests.

use af_cmd::{
    CmdError, CommandCtx, CommandOutcome, CommandRegistry, CommandSpec, ParamSpec, ParamType,
    ParsedArgs, RegisterError,
};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
use af_model::id::{EntityId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::json;

// ---- Test commands -----------------------------------------------------------

/// A read-only command that echoes recognized arguments.
fn echo_exec(_ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = args.point("p1") {
        parts.push(format!("p1={},{}", p.x, p.y));
    }
    if let Some(d) = args.distance("dist") {
        parts.push(format!("dist={d}"));
    }
    if let Some(a) = args.angle("ang") {
        parts.push(format!("ang={a}"));
    }
    if let Some(n) = args.count("n") {
        parts.push(format!("n={n}"));
    }
    if let Some(ids) = args.entity_set("sel") {
        parts.push(format!("sel={}", ids.len()));
    }
    if let Some(kw) = args.enum_value("mode") {
        parts.push(format!("mode={kw}"));
    }
    if let Some(t) = args.text("name") {
        parts.push(format!("name={t}"));
    }
    if let Some(layer) = args.layer("layer") {
        parts.push(format!("layer={}", layer.raw().0));
    }
    parts.push(format!("flag={}", args.flag("on")));
    Ok(CommandOutcome::message(parts.join(";")))
}

fn echo_spec() -> CommandSpec {
    CommandSpec::new("_ECHO", "Echo", false, echo_exec)
        .alias("_E")
        .param(ParamSpec::required("p1", ParamType::Point))
        .param(ParamSpec::optional("dist", ParamType::Distance))
        .param(ParamSpec::optional("ang", ParamType::Angle))
        .param(ParamSpec::optional("n", ParamType::Count))
        .param(ParamSpec::optional("sel", ParamType::EntitySet))
        .param(ParamSpec::optional(
            "mode",
            ParamType::Enum(vec!["fast".to_string(), "slow".to_string()]),
        ))
        .param(ParamSpec::optional("name", ParamType::Text))
        .param(ParamSpec::optional("layer", ParamType::LayerRef))
        .param(ParamSpec::optional("on", ParamType::Flag))
}

fn nop_exec(_ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    Ok(CommandOutcome::new())
}

fn seed_point(session: &mut Session) -> EntityId {
    let layer = session.document().current_layer();
    let rec = EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(Point2::new(1.0, 2.0))),
    );
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, rec)
        })
        .expect("seed commits")
        .value
}

// ---- Registration collisions ------------------------------------------------

#[test]
fn register_rejects_duplicate_name_case_insensitive() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec))
        .unwrap();
    let err = reg
        .register(CommandSpec::new("line", "Line2", false, nop_exec))
        .unwrap_err();
    assert!(matches!(err, RegisterError::Duplicate { .. }));
    assert_eq!(reg.commands().len(), 1);
}

#[test]
fn register_rejects_duplicate_alias() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec).alias("L"))
        .unwrap();
    let err = reg
        .register(CommandSpec::new("LIST", "List", false, nop_exec).alias("l"))
        .unwrap_err();
    assert_eq!(
        err,
        RegisterError::Duplicate {
            token: "l".to_string()
        }
    );
}

#[test]
fn register_rejects_alias_colliding_with_existing_name() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("MOVE", "Move", false, nop_exec))
        .unwrap();
    let err = reg
        .register(CommandSpec::new("COPY", "Copy", false, nop_exec).alias("move"))
        .unwrap_err();
    assert!(matches!(err, RegisterError::Duplicate { .. }));
}

#[test]
fn register_rejects_self_collision() {
    let mut reg = CommandRegistry::new();
    let err = reg
        .register(CommandSpec::new("ARC", "Arc", false, nop_exec).alias("arc"))
        .unwrap_err();
    assert!(matches!(err, RegisterError::Duplicate { .. }));
    assert!(reg.commands().is_empty()); // Registration is atomic: nothing was added.
}

#[test]
fn register_rejects_empty_name() {
    let mut reg = CommandRegistry::new();
    assert_eq!(
        reg.register(CommandSpec::new("  ", "Blank", false, nop_exec))
            .unwrap_err(),
        RegisterError::EmptyName
    );
    assert_eq!(
        reg.register(CommandSpec::new("OK", "Ok", false, nop_exec).alias("  "))
            .unwrap_err(),
        RegisterError::EmptyName
    );
}

// ---- Lookup -----------------------------------------------------------------

#[test]
fn lookup_is_case_insensitive_and_trims() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("CIRCLE", "Circle", false, nop_exec).alias("C"))
        .unwrap();
    assert!(reg.lookup("circle").is_some());
    assert!(reg.lookup("  CiRcLe ").is_some());
    assert!(reg.lookup("c").is_some());
    assert!(reg.lookup(" C ").is_some());
    assert!(reg.lookup("nope").is_none());
    assert_eq!(reg.lookup("c").unwrap().name(), "CIRCLE");
}

// ---- apply_user_aliases precedence ------------------------------------------

#[test]
fn user_alias_overrides_builtin_alias_but_not_canonical_names() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("CIRCLE", "Circle", false, nop_exec).alias("C"))
        .unwrap();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec).alias("L"))
        .unwrap();

    assert_eq!(reg.lookup("C").unwrap().name(), "CIRCLE");

    let warnings = reg.apply_user_aliases([("C".to_string(), "LINE".to_string())]);
    assert!(warnings.is_empty());
    assert_eq!(reg.lookup("C").unwrap().name(), "LINE");
    assert_eq!(reg.lookup("c").unwrap().name(), "LINE"); // Lookup remains case-insensitive.

    assert_eq!(reg.lookup("CIRCLE").unwrap().name(), "CIRCLE");
    assert_eq!(reg.lookup("LINE").unwrap().name(), "LINE");
    assert_eq!(reg.lookup("L").unwrap().name(), "LINE");
}

#[test]
fn user_alias_cannot_shadow_a_canonical_command_name() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("CIRCLE", "Circle", false, nop_exec))
        .unwrap();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec))
        .unwrap();

    let warnings = reg.apply_user_aliases([("LINE".to_string(), "CIRCLE".to_string())]);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("canónico") || warnings[0].contains("canonico"));
    assert_eq!(reg.lookup("LINE").unwrap().name(), "LINE");
}

#[test]
fn user_alias_with_unknown_target_is_ignored_with_warning() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec))
        .unwrap();

    let warnings = reg.apply_user_aliases([("Z".to_string(), "ZOOM".to_string())]);
    assert_eq!(warnings.len(), 1);
    assert!(reg.lookup("Z").is_none());
}

#[test]
fn user_alias_empty_key_is_ignored_with_warning() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec))
        .unwrap();

    let warnings = reg.apply_user_aliases([("   ".to_string(), "LINE".to_string())]);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn user_alias_can_target_another_user_alias_applied_earlier_in_the_same_call() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec))
        .unwrap();

    // Later pairs may target user aliases established earlier in the same call.
    let warnings = reg.apply_user_aliases([
        ("L1".to_string(), "LINE".to_string()),
        ("L2".to_string(), "L1".to_string()),
    ]);
    assert!(warnings.is_empty());
    assert_eq!(reg.lookup("L2").unwrap().name(), "LINE");
}

#[test]
fn user_alias_last_pair_wins_when_the_same_key_repeats() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("LINE", "Line", false, nop_exec))
        .unwrap();
    reg.register(CommandSpec::new("CIRCLE", "Circle", false, nop_exec))
        .unwrap();

    reg.apply_user_aliases([
        ("Q".to_string(), "LINE".to_string()),
        ("Q".to_string(), "CIRCLE".to_string()),
    ]);
    assert_eq!(reg.lookup("Q").unwrap().name(), "CIRCLE");
}

// ---- Argument validation by type --------------------------------------------

#[test]
fn valid_point_arg_executes() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let out = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": [3.0, 4.0] }))
        .unwrap();
    assert_eq!(out.tx_seq, None); // Does not touch the document.
    assert!(out.message.unwrap().contains("p1=3,4"));
}

#[test]
fn missing_required_param_errors() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let err = reg.execute(&mut session, "_ECHO", &json!({})).unwrap_err();
    assert_eq!(err, CmdError::MissingParam("p1".to_string()));
}

#[test]
fn unknown_param_errors() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let err = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "bogus": 1 }))
        .unwrap_err();
    assert_eq!(err, CmdError::UnknownParam("bogus".to_string()));
}

#[test]
fn point_type_mismatch_errors() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let err = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": "x,y" }))
        .unwrap_err();
    assert!(matches!(err, CmdError::TypeMismatch { .. }));
}

#[test]
fn distance_must_be_positive() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let err = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "dist": 0.0 }))
        .unwrap_err();
    assert!(matches!(err, CmdError::OutOfRange { .. }));
    assert!(
        reg.execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "dist": 2.5 }))
            .is_ok()
    );
}

#[test]
fn args_must_be_object() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let err = reg
        .execute(&mut session, "_ECHO", &json!([1, 2, 3]))
        .unwrap_err();
    assert_eq!(err, CmdError::NotAnObject);
}

#[test]
fn entity_set_checks_existence() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());

    let err = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "sel": [99999] }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::UnknownEntity(_)));

    let id = seed_point(&mut session);
    let out = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "sel": [id.raw().0] }),
        )
        .unwrap();
    assert!(out.message.unwrap().contains("sel=1"));
}

#[test]
fn enum_validates_case_insensitive() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());

    let out = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "mode": "FAST" }),
        )
        .unwrap();
    assert!(out.message.unwrap().contains("mode=fast"));

    let err = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "mode": "turbo" }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::InvalidEnum { .. }));
}

#[test]
fn layer_ref_by_name_and_id_and_unknown() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();

    let by_name = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "layer": "0" }),
        )
        .unwrap();
    assert!(
        by_name
            .message
            .unwrap()
            .contains(&format!("layer={}", l0.raw().0))
    );

    let by_id = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "layer": l0.raw().0 }),
        )
        .unwrap();
    assert!(
        by_id
            .message
            .unwrap()
            .contains(&format!("layer={}", l0.raw().0))
    );

    let err = reg
        .execute(
            &mut session,
            "_ECHO",
            &json!({ "p1": [0, 0], "layer": "Ghost" }),
        )
        .unwrap_err();
    assert!(matches!(err, CmdError::UnknownLayer(_)));
}

#[test]
fn count_accepts_integers_rejects_floats_and_negatives() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());

    let out = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "n": 3 }))
        .unwrap();
    assert!(out.message.unwrap().contains("n=3"));

    assert!(matches!(
        reg.execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "n": 2.5 }))
            .unwrap_err(),
        CmdError::TypeMismatch { .. }
    ));
    assert!(matches!(
        reg.execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "n": -1 }))
            .unwrap_err(),
        CmdError::TypeMismatch { .. }
    ));
}

#[test]
fn flag_defaults_false_and_reads_true() {
    let mut reg = CommandRegistry::new();
    reg.register(echo_spec()).unwrap();
    let mut session = Session::new(Units::default());

    let off = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": [0, 0] }))
        .unwrap();
    assert!(off.message.unwrap().contains("flag=false"));

    let on = reg
        .execute(&mut session, "_ECHO", &json!({ "p1": [0, 0], "on": true }))
        .unwrap();
    assert!(on.message.unwrap().contains("flag=true"));
}

#[test]
fn default_value_is_applied_when_absent() {
    fn spec() -> CommandSpec {
        CommandSpec::new("_DEF", "Def", false, echo_exec)
            .param(ParamSpec::required("p1", ParamType::Point))
            .param(ParamSpec::with_default(
                "dist",
                ParamType::Distance,
                json!(5.0),
            ))
    }
    let mut reg = CommandRegistry::new();
    reg.register(spec()).unwrap();
    let mut session = Session::new(Units::default());
    let out = reg
        .execute(&mut session, "_DEF", &json!({ "p1": [0, 0] }))
        .unwrap();
    assert!(out.message.unwrap().contains("dist=5"));
}

#[test]
fn args_null_is_treated_as_empty() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("_NP", "NoParams", false, nop_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    assert!(
        reg.execute(&mut session, "_NP", &serde_json::Value::Null)
            .is_ok()
    );
}

#[test]
fn execute_unknown_command_errors() {
    let reg = CommandRegistry::new();
    let mut session = Session::new(Units::default());
    let err = reg
        .execute(&mut session, "NOPE", &serde_json::Value::Null)
        .unwrap_err();
    assert_eq!(err, CmdError::UnknownCommand("NOPE".to_string()));
}
