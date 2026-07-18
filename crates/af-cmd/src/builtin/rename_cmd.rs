//! RENAME changes a named document element.
//!
//! `kind` currently supports only `layer`, the sole typed named-element reference.
//! Additional kinds can extend the existing Enum dispatch without a new abstraction.
//!
//! It has no standard alias in this registry.

use af_model::layers_ops;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Named element kinds currently supported by RENAME.
const KINDS: &[&str] = &["layer"];

/// Returns the RENAME specification without aliases.
#[must_use]
pub fn rename_spec() -> CommandSpec {
    CommandSpec::new("RENAME", "Rename", true, rename_exec)
        .param(ParamSpec::required(
            "kind",
            ParamType::Enum(KINDS.iter().map(|s| (*s).to_string()).collect()),
        ))
        .param(ParamSpec::required("target", ParamType::LayerRef))
        .param(ParamSpec::required("name", ParamType::Text))
}

/// Registers RENAME.
///
/// # Errors
/// Returns [`RegisterError`] on a name collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(rename_spec())
}

fn rename_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let kind = args
        .enum_value("kind")
        .ok_or_else(|| CmdError::MissingParam("kind".to_string()))?;
    let target = args
        .layer("target")
        .ok_or_else(|| CmdError::MissingParam("target".to_string()))?;
    let name = args
        .text("name")
        .ok_or_else(|| CmdError::MissingParam("name".to_string()))?
        .to_string();

    match kind {
        "layer" => {
            ctx.transact("Rename", |tx| {
                layers_ops::rename_layer(tx, target, name).map_err(CmdError::from)
            })?;
            Ok(CommandOutcome::new())
        }
        other => unreachable!("kind fuera del schema Enum de RENAME: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_model::Session;
    use af_model::entity::{Color, Lineweight};
    use af_model::layers_ops::LayerProps;
    use af_model::units::Units;

    #[test]
    fn rename_layer_happy_path() {
        let mut session = Session::new(Units::default());
        let id = session
            .transact("mk A", |tx| {
                let continuous = tx.doc().line_types().next().unwrap().id();
                layers_ops::create_layer(
                    tx,
                    LayerProps::new("A", Color::aci(1).unwrap(), continuous, Lineweight::ByLayer),
                )
                .map_err(CmdError::from)
            })
            .expect("commits")
            .value;

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "kind".to_string(),
            crate::args::ArgValue::Enum("layer".to_string()),
        );
        args.insert("target".to_string(), crate::args::ArgValue::LayerRef(id));
        args.insert(
            "name".to_string(),
            crate::args::ArgValue::Text("B".to_string()),
        );
        rename_exec(&mut ctx, args).expect("rename executes");

        assert_eq!(session.document().layer(id).unwrap().name(), "B");
        assert!(session.document().layer_by_name("A").is_none());
    }

    #[test]
    fn rename_layer_zero_is_rejected() {
        let mut session = Session::new(Units::default());
        let zero = session.document().layer_by_name("0").unwrap().id();

        let mut ctx = CommandCtx::new(&mut session);
        let mut args = ParsedArgs::new();
        args.insert(
            "kind".to_string(),
            crate::args::ArgValue::Enum("layer".to_string()),
        );
        args.insert("target".to_string(), crate::args::ArgValue::LayerRef(zero));
        args.insert(
            "name".to_string(),
            crate::args::ArgValue::Text("Nope".to_string()),
        );
        let err = rename_exec(&mut ctx, args).unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));
        assert_eq!(session.document().layer(zero).unwrap().name(), "0");
    }
}
