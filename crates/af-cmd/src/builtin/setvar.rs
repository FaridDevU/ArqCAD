//! SETVAR (`-SETVAR`) reads or writes a system variable.
//!
//! System variables belong to [`af_model::Session`], not the document, so reads
//! and writes create no transaction or undo entry.
//!
//! A name alone reports the value; adding `value` parses, validates, and assigns it.
//!
//! The default [`SysvarValue`] variant determines the expected type. `Real2` input
//! uses `x,y`, with the comma separating values rather than decimals.

use af_model::units::parse_linear;
use af_model::{SysvarValue, sysvar::SysvarDef};

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the SETVAR specification with alias `-SETVAR`.
#[must_use]
pub fn setvar_spec() -> CommandSpec {
    CommandSpec::new("SETVAR", "Setvar", false, setvar_exec)
        .alias("-SETVAR")
        .param(ParamSpec::required("name", ParamType::Text))
        .param(ParamSpec::optional("value", ParamType::Text))
}

/// Registers SETVAR.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(setvar_spec())
}

fn setvar_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let name = args
        .text("name")
        .ok_or_else(|| CmdError::MissingParam("name".to_string()))?;
    let def = ctx
        .sysvar_def(name)
        .ok_or_else(|| CmdError::Failed(format!("SETVAR: unknown sysvar '{name}'")))?;
    let canonical = def.name;

    match args.text("value") {
        // Reading reports the current value without a transaction.
        None => {
            let cur = ctx
                .sysvar(name)
                .expect("la sysvar existe (def resuelto): tiene valor");
            Ok(CommandOutcome::message(format!("{canonical} = {cur}")))
        }
        // Assignment parses by type and lets the model enforce its range.
        Some(raw) => {
            let value = parse_sysvar_value(def, raw)?;
            ctx.set_sysvar(name, value)?; // SysvarError -> CmdError::Failed
            Ok(CommandOutcome::message(format!("{canonical} = {value}")))
        }
    }
}

/// Parses `raw` to the type implied by `def`; assignment validates the range.
fn parse_sysvar_value(def: &SysvarDef, raw: &str) -> Result<SysvarValue, CmdError> {
    match def.default {
        SysvarValue::Int(_) => {
            let n: i64 = raw.trim().parse().map_err(|_| {
                CmdError::Failed(format!(
                    "SETVAR {}: '{raw}' no es un entero válido",
                    def.name
                ))
            })?;
            Ok(SysvarValue::Int(n))
        }
        SysvarValue::Real(_) => {
            let x = parse_linear(raw)
                .map_err(|e| CmdError::Failed(format!("SETVAR {}: {e}", def.name)))?;
            Ok(SysvarValue::Real(x))
        }
        SysvarValue::Real2(_, _) => {
            let mut parts = raw.split(',');
            let (Some(sx), Some(sy), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(CmdError::Failed(format!(
                    "SETVAR {}: se esperaba 'x,y' (dos reales separados por coma)",
                    def.name
                )));
            };
            let x = parse_linear(sx)
                .map_err(|e| CmdError::Failed(format!("SETVAR {}: {e}", def.name)))?;
            let y = parse_linear(sy)
                .map_err(|e| CmdError::Failed(format!("SETVAR {}: {e}", def.name)))?;
            Ok(SysvarValue::Real2(x, y))
        }
    }
}
