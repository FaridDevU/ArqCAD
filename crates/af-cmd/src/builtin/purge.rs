//! PURGE (`-PURGE`, `PU`) removes unused layers in one transaction and reports them.
//!
//! A layer is purgeable when no entity in any container references it and it is
//! neither layer `0` nor the current layer. [`TxContext::remove_layer_raw`]
//! rechecks these guards.
//!
//! Nothing to purge returns [`CmdError::Failed`] so a successful mutating command
//! never violates the one-transaction contract.
//!
//! Only layers are purged because other style catalogs lack reversible removal operations.

use std::collections::HashSet;

use af_model::TxContext;
use af_model::id::LayerId;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec};

/// Returns the PURGE specification with aliases `PU` and `-PURGE`.
#[must_use]
pub fn purge_spec() -> CommandSpec {
    CommandSpec::new("PURGE", "Purge", true, purge_exec)
        .alias("PU")
        .alias("-PURGE")
}

/// Registers PURGE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(purge_spec())
}

fn purge_exec(ctx: &mut CommandCtx<'_>, _args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let doc = ctx.document();

    // Count layer references across all entity containers.
    let mut referenced: HashSet<LayerId> = HashSet::new();
    for rec in doc.model_space().iter_records() {
        referenced.insert(rec.layer);
    }
    for layout in doc.layouts() {
        for rec in layout.entities().iter_records() {
            referenced.insert(rec.layer);
        }
    }
    for block in doc.blocks() {
        for rec in block.entities().iter_records() {
            referenced.insert(rec.layer);
        }
    }

    let current = doc.current_layer();
    // Keep protected layers and collect purgeable IDs with names for reporting.
    let purgeable: Vec<(LayerId, String)> = doc
        .layers()
        .filter(|l| {
            !referenced.contains(&l.id())
                && l.id() != current
                && !l.name().eq_ignore_ascii_case("0")
        })
        .map(|l| (l.id(), l.name().to_string()))
        .collect();

    if purgeable.is_empty() {
        return Err(CmdError::Failed(
            "PURGE: no hay capas sin uso que purgar".to_string(),
        ));
    }

    let ids: Vec<LayerId> = purgeable.iter().map(|(id, _)| *id).collect();
    ctx.transact("Purge", |tx: &mut TxContext<'_>| {
        for id in &ids {
            tx.remove_layer_raw(*id)?;
        }
        Ok::<(), CmdError>(())
    })?;

    let names: Vec<String> = purgeable.into_iter().map(|(_, name)| name).collect();
    Ok(CommandOutcome::message(format!(
        "Purged {} layer(s): {}",
        names.len(),
        names.join(", "),
    )))
}
