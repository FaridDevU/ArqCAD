//! AUDIT exposes [`Document::validate_full`](af_model::Document::validate_full) as
//! a read-only command.
//!
//! Validation runs on a document clone and reports [`Issue`] values without
//! changing the live document or creating a transaction.
//!
//! In-place repair is intentionally absent because `validate_full` repairs are not
//! represented as reversible document operations. File loading handles recovery.

use af_model::Severity;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec};

/// Returns the AUDIT specification without aliases.
#[must_use]
pub fn audit_spec() -> CommandSpec {
    CommandSpec::new("AUDIT", "Audit", false, audit_exec)
}

/// Registers AUDIT.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(audit_spec())
}

fn audit_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // `validate_full` mutates repairs, so run it on a clone to guarantee zero transactions.
    let mut probe = ctx.document().clone();
    let issues = probe.validate_full();

    if issues.is_empty() {
        return Ok(CommandOutcome::message(
            "AUDIT: 0 issues found. Document is valid.".to_string(),
        ));
    }

    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let repaired = issues
        .iter()
        .filter(|i| i.severity == Severity::Repaired)
        .count();
    let warnings = issues
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .count();

    let mut out = format!(
        "AUDIT: {} issue(s) — {errors} error(s), {repaired} repairable, {warnings} warning(s) \
         (view-only: no changes applied)\n",
        issues.len(),
    );
    for i in &issues {
        out.push_str(&format!("  [{:?}] {}\n", i.severity, i.message));
    }
    if out.ends_with('\n') {
        out.pop();
    }
    Ok(CommandOutcome::message(out))
}
