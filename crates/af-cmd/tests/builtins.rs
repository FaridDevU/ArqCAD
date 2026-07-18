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
    let mut session = Session::new(Units::default());

    let err = reg
        .execute(&mut session, "_BADTX", &Value::Null)
        .unwrap_err();
    match err {
        CmdError::ContractViolation(msg) => {
            assert!(msg.contains("2 transactions"), "msg: {msg}");
        }
        other => panic!("esperaba ContractViolation, fue {other:?}"),
    }
}

#[test]
fn view_command_creating_tx_is_rejected() {
    let mut reg = CommandRegistry::new();
    reg.register(CommandSpec::new("_BADVIEW", "BadView", false, badview_exec))
        .unwrap();
    let mut session = Session::new(Units::default());

    let err = reg
        .execute(&mut session, "_BADVIEW", &Value::Null)
        .unwrap_err();
    assert!(matches!(err, CmdError::ContractViolation(_)));
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
