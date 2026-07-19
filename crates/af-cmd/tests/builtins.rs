//! End-to-end UNDO/REDO and transaction-contract tests.

use af_cmd::builtin::undo_redo::register_builtins;
use af_cmd::{CmdError, CommandCtx, CommandOutcome, CommandRegistry, CommandSpec, ParsedArgs};
use af_math::Point2;
use af_model::container::ContainerRef;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
use af_model::id::{EntityId, LayerId, ObjectId};
use af_model::units::Units;
use af_model::{Session, TxError};
use serde_json::Value;

// ---- Helpers ----------------------------------------------------------------

fn mk_point(layer: LayerId, x: f64, y: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(Point2::new(x, y))),
    )
}

/// Seeds one entity for undo tests.
fn seed(session: &mut Session) -> EntityId {
    let layer = session.document().current_layer();
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 1.0, 2.0))
        })
        .expect("seed commits")
        .value
}

fn assert_state_and_next_add_match_twin(
    reg: &CommandRegistry,
    session: &mut Session,
    twin: &mut Session,
) {
    assert_eq!(
        serde_json::to_string(session.document()).unwrap(),
        serde_json::to_string(twin.document()).unwrap()
    );
    assert_eq!(session.history_labels(), twin.history_labels());
    assert_eq!(session.history().redo_depth(), twin.history().redo_depth());

    let actual = reg.execute(session, "_ADD", &Value::Null).unwrap();
    let expected = reg.execute(twin, "_ADD", &Value::Null).unwrap();
    assert_eq!(
        actual, expected,
        "next ID, tx_seq and change set must match"
    );
    assert_eq!(
        serde_json::to_string(session.document()).unwrap(),
        serde_json::to_string(twin.document()).unwrap()
    );
    assert_eq!(session.history_labels(), twin.history_labels());
}

// ---- Transaction-contract commands ------------------------------------------

/// A conforming mutating command that creates exactly one transaction.
fn add_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    let rec = mk_point(layer, 3.0, 4.0);
    let id = ctx.transact("Add Point", |tx| -> Result<EntityId, CmdError> {
        Ok(tx.add_entity(ContainerRef::ModelSpace, rec)?)
    })?;
    Ok(CommandOutcome::created(vec![id]))
}

/// A mutating command that violates the contract with two transactions.
fn badtx_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    ctx.transact("bad-1", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 0.0, 0.0))?;
        Ok(())
    })?;
    ctx.transact("bad-2", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 1.0, 1.0))?;
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}

/// A view command that incorrectly creates a transaction.
fn badview_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    ctx.transact("sneaky", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 0.0, 0.0))?;
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}

/// A mutating command that incorrectly creates an empty transaction.
fn emptyadd_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    ctx.transact("empty", |_tx| -> Result<(), CmdError> { Ok(()) })?;
    Ok(CommandOutcome::new())
}

/// A mutating command whose failed transaction rolls back without changes.
fn fail_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    ctx.transact("will-rollback", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 5.0, 5.0))?;
        Err(CmdError::Failed("boom".to_string()))
    })?;
    Ok(CommandOutcome::new())
}

/// Commits once, then fails so the registry outer scope must roll it back.
fn late_fail_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    ctx.transact("committed-before-error", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 6.0, 6.0))?;
        Ok(())
    })?;
    Err(CmdError::Failed("late boom".to_string()))
}

/// Commits and then undoes; a mutating command must publish exactly one change set.
fn transact_then_undo_exec(
    ctx: &mut CommandCtx<'_>,
    _args: ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    ctx.transact("commit", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 7.0, 7.0))?;
        Ok(())
    })?;
    ctx.undo()?;
    Ok(CommandOutcome::new())
}

/// Captures a second non-empty failed transaction after one successful commit.
fn captured_second_failure_exec(
    ctx: &mut CommandCtx<'_>,
    _args: ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let layer = ctx.document().current_layer();
    ctx.transact("commit", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 8.0, 8.0))?;
        Ok(())
    })?;
    let second = ctx.transact("captured failure", |tx| -> Result<(), CmdError> {
        tx.add_entity(ContainerRef::ModelSpace, mk_point(layer, 9.0, 9.0))?;
        Err(CmdError::Failed("captured".to_string()))
    });
    assert_eq!(second, Err(CmdError::Failed("captured".to_string())));
    Ok(CommandOutcome::new())
}

// ---- UNDO / REDO ------------------------------------------------------------

#[test]
fn undo_redo_over_real_session() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).unwrap();
    let mut session = Session::new(Units::default());

    let id = seed(&mut session);
    assert!(session.document().entity(id).is_some());
    assert_eq!(session.history().undo_depth(), 1);

    let out = reg.execute(&mut session, "UNDO", &Value::Null).unwrap();
    assert_eq!(out.tx_seq, None);
    assert_eq!(out.message.as_deref(), Some("Undo seed"));
    assert!(session.document().entity(id).is_none());

    let out = reg.execute(&mut session, "REDO", &Value::Null).unwrap();
    assert_eq!(out.tx_seq, None);
    assert_eq!(out.message.as_deref(), Some("Redo seed"));
    assert!(session.document().entity(id).is_some());

    reg.execute(&mut session, "u", &Value::Null).unwrap();
    assert!(session.document().entity(id).is_none());
}

#[test]
fn undo_with_nothing_to_undo_errors() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).unwrap();
    let mut session = Session::new(Units::default());
    let err = reg.execute(&mut session, "UNDO", &Value::Null).unwrap_err();
    assert_eq!(err, CmdError::NothingToUndo);
}

#[test]
fn redo_with_nothing_to_redo_errors() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).unwrap();
    let mut session = Session::new(Units::default());
    seed(&mut session); // There is something to undo, but nothing to redo.
    let err = reg.execute(&mut session, "REDO", &Value::Null).unwrap_err();
    assert_eq!(err, CmdError::NothingToRedo);
}

// ---- One-transaction contract -----------------------------------------------

#[test]
fn well_behaved_command_creates_exactly_one_tx() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("_ADD", "Add", true, add_exec))
        .unwrap();
    let mut session = Session::new(Units::default());

    let out = reg.execute(&mut session, "_ADD", &Value::Null).unwrap();
    assert_eq!(out.created.len(), 1);
    assert!(out.tx_seq.is_some());
    assert!(session.document().entity(out.created[0]).is_some());
    assert_eq!(session.history().undo_depth(), 1);
}

#[test]
fn malicious_two_transaction_command_is_rejected() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("_BADTX", "BadTx", true, badtx_exec))
        .unwrap();
    reg.register(CommandSpec::new("_ADD", "Add", true, add_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    let mut twin = session.clone();

    let err = reg
        .execute(&mut session, "_BADTX", &Value::Null)
        .unwrap_err();
    match err {
        CmdError::ContractViolation(msg) => {
            assert!(msg.contains("2 transactions"), "msg: {msg}");
        }
        other => panic!("esperaba ContractViolation, fue {other:?}"),
    }
    assert_state_and_next_add_match_twin(&reg, &mut session, &mut twin);
}

#[test]
fn view_command_creating_tx_is_rejected() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("_BADVIEW", "BadView", false, badview_exec))
        .unwrap();
    reg.register(CommandSpec::new("_ADD", "Add", true, add_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    let mut twin = session.clone();

    let err = reg
        .execute(&mut session, "_BADVIEW", &Value::Null)
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)));
    assert_state_and_next_add_match_twin(&reg, &mut session, &mut twin);
}

#[test]
fn affects_document_command_with_empty_tx_is_rejected() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new(
        "_EMPTYADD",
        "EmptyAdd",
        true,
        emptyadd_exec,
    ))
    .unwrap();
    let mut session = Session::new(Units::default());

    let err = reg
        .execute(&mut session, "_EMPTYADD", &Value::Null)
        .unwrap_err();
    match err {
        CmdError::ContractViolation(msg) => assert!(msg.contains("0 transactions"), "msg: {msg}"),
        other => panic!("esperaba ContractViolation, fue {other:?}"),
    }
}

// ---- Failed command rollback -------------------------------------------------

#[test]
fn failed_command_leaves_zero_tx_and_intact_document() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("_FAIL", "Fail", true, fail_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "_FAIL", &Value::Null)
        .unwrap_err();
    assert_eq!(err, CmdError::Failed("boom".to_string()));

    assert_eq!(session.history().undo_depth(), 0);
    assert!(!session.can_undo());
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn error_after_inner_commit_rolls_back_registry_outer_scope() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new(
        "_LATEFAIL",
        "LateFail",
        true,
        late_fail_exec,
    ))
    .unwrap();
    reg.register(CommandSpec::new("_ADD", "Add", true, add_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    let before = serde_json::to_string(session.document()).unwrap();

    let err = reg
        .execute(&mut session, "_LATEFAIL", &Value::Null)
        .unwrap_err();
    assert_eq!(err, CmdError::Failed("late boom".to_string()));
    assert_eq!(serde_json::to_string(session.document()).unwrap(), before);
    assert_eq!(session.history().undo_depth(), 0);

    let success = reg.execute(&mut session, "_ADD", &Value::Null).unwrap();
    assert_eq!(success.tx_seq, Some(0));
    assert_eq!(session.history().undo_depth(), 1);
}

#[test]
fn transact_then_undo_is_rejected_without_publishing_candidate() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new(
        "_TXUNDO",
        "TxUndo",
        true,
        transact_then_undo_exec,
    ))
    .unwrap();
    reg.register(CommandSpec::new("_ADD", "Add", true, add_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    let mut twin = session.clone();

    let err = reg
        .execute(&mut session, "_TXUNDO", &Value::Null)
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)));
    assert_state_and_next_add_match_twin(&reg, &mut session, &mut twin);
}

#[test]
fn captured_second_transaction_failure_is_still_a_contract_violation() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new(
        "_CAPTURED2",
        "Captured2",
        true,
        captured_second_failure_exec,
    ))
    .unwrap();
    reg.register(CommandSpec::new("_ADD", "Add", true, add_exec))
        .unwrap();
    let mut session = Session::new(Units::default());
    let mut twin = session.clone();

    let err = reg
        .execute(&mut session, "_CAPTURED2", &Value::Null)
        .unwrap_err();
    match err {
        CmdError::ContractViolation(message) => {
            assert!(message.contains("2 transaction attempts"), "{message}");
            assert!(message.contains("1 transactions"), "{message}");
        }
        other => panic!("expected ContractViolation, got {other:?}"),
    }
    assert_state_and_next_add_match_twin(&reg, &mut session, &mut twin);
}
