//! Current line properties: LINETYPE (`LT`), LWEIGHT (`LW`), and LTSCALE (`LTS`).
//!
//! Each updates a document property in one reversible transaction, and new
//! entities inherit the value.
//!
//! # LINETYPE
//!
//! A library pattern is loaded and selected in one transaction. `BYLAYER`,
//! `BYBLOCK`, and already-loaded names only change the current reference.
//!
//! # FILLMODE
//!
//! FILLMODE remains a system variable and is not duplicated as a command here.

use af_model::entity::LineTypeRef;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::style_value::parse_lineweight;

/// Returns the LINETYPE specification with alias `LT`.
#[must_use]
pub fn linetype_spec() -> CommandSpec {
    CommandSpec::new("LINETYPE", "Linetype", true, linetype_exec)
        .alias("LT")
        .param(ParamSpec::required("linetype", ParamType::Text))
}

/// Returns the LWEIGHT specification with alias `LW`.
#[must_use]
pub fn lweight_spec() -> CommandSpec {
    CommandSpec::new("LWEIGHT", "Lweight", true, lweight_exec)
        .alias("LW")
        .param(ParamSpec::required("lineweight", ParamType::Text))
}

/// Returns the LTSCALE specification with alias `LTS`.
#[must_use]
pub fn ltscale_spec() -> CommandSpec {
    CommandSpec::new("LTSCALE", "Ltscale", true, ltscale_exec)
        .alias("LTS")
        // Registry `Distance` validation keeps LTSCALE positive.
        .param(ParamSpec::required("scale", ParamType::Distance))
}

/// Registers LINETYPE, LWEIGHT, and LTSCALE.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(linetype_spec())?;
    registry.register(lweight_spec())?;
    registry.register(ltscale_spec())?;
    Ok(())
}

/// A LINETYPE plan that selects an existing reference or loads and selects a library pattern.
enum LtPlan {
    Set(LineTypeRef),
    Load {
        name: &'static str,
        description: &'static str,
        pattern: Vec<f64>,
    },
}

fn linetype_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let raw = args
        .text("linetype")
        .ok_or_else(|| CmdError::MissingParam("linetype".to_string()))?;
    let s = raw.trim();

    let plan = if s.eq_ignore_ascii_case("bylayer") {
        LtPlan::Set(LineTypeRef::ByLayer)
    } else if s.eq_ignore_ascii_case("byblock") {
        LtPlan::Set(LineTypeRef::ByBlock)
    } else if let Some(lt) = ctx.document().line_type_by_name(s) {
        // Select an already-loaded line type directly.
        LtPlan::Set(LineTypeRef::Style(lt.id()))
    } else if let Some(def) = af_model::linetype_def(s) {
        // Load and select a library line type in the same transaction.
        LtPlan::Load {
            name: def.name,
            description: def.description,
            pattern: def.pattern.to_vec(),
        }
    } else {
        return Err(CmdError::Failed(format!(
            "unknown line type '{raw}' (not loaded and not in the factory library)"
        )));
    };

    ctx.transact("Linetype", |tx| -> Result<(), CmdError> {
        let lt = match plan {
            LtPlan::Set(r) => r,
            LtPlan::Load {
                name,
                description,
                pattern,
            } => LineTypeRef::Style(tx.add_line_type_raw(name, description, pattern)?),
        };
        tx.set_current_line_type(lt)?;
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}

fn lweight_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let raw = args
        .text("lineweight")
        .ok_or_else(|| CmdError::MissingParam("lineweight".to_string()))?;
    let lw = parse_lineweight(raw)?;
    ctx.transact("Lweight", |tx| -> Result<(), CmdError> {
        tx.set_current_lineweight(lw);
        Ok(())
    })?;
    Ok(CommandOutcome::new())
}

fn ltscale_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let scale = args
        .distance("scale")
        .ok_or_else(|| CmdError::MissingParam("scale".to_string()))?;
    ctx.transact("Ltscale", |tx| {
        tx.set_ltscale(scale).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}
