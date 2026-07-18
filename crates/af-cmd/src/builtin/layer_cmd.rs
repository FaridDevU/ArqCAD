//! LAYER (`LA`) applies one headless layer operation per invocation in one
//! transaction: create, delete, rename, set color, toggle state, or set current.
//!
//! Business rules live in [`af_model::layers_ops`]; this module maps typed command
//! input to those operations.
//!
//! # Operations
//!
//! Supported values are `new`, `delete`, `rename`, `color`, `on`, `off`, `freeze`,
//! `thaw`, `lock`, `unlock`, `plot`, `no-plot`, and `set-current`. Delete uses
//! [`DeletePolicy::RejectIfUsed`] and never removes entities implicitly.
//!
//! # New layers
//!
//! New layers use ACI 7, `Continuous`, and `ByLayer`. Styling is a separate operation.

use af_model::entity::Color;
use af_model::layers_ops::{self, DeletePolicy, LayerOpError, LayerPatch, LayerProps};

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::style_value::parse_color;

/// Converts a contextual [`LayerOpError`] to a user-facing command error.
impl From<LayerOpError> for CmdError {
    fn from(e: LayerOpError) -> Self {
        CmdError::Failed(e.to_string())
    }
}

/// Accepted `op` keywords.
const OPS: &[&str] = &[
    "new",
    "delete",
    "rename",
    "color",
    "on",
    "off",
    "freeze",
    "thaw",
    "lock",
    "unlock",
    "plot",
    "no-plot",
    "set-current",
];

/// Returns the LAYER specification with alias `LA`.
#[must_use]
pub fn layer_spec() -> CommandSpec {
    CommandSpec::new("LAYER", "Layer", true, layer_exec)
        .alias("LA")
        .param(ParamSpec::required(
            "op",
            ParamType::Enum(OPS.iter().map(|s| (*s).to_string()).collect()),
        ))
        .param(ParamSpec::optional("layer", ParamType::LayerRef))
        .param(ParamSpec::optional("name", ParamType::Text))
        .param(ParamSpec::optional("color", ParamType::Text))
}

/// Registers LAYER.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(layer_spec())
}

fn layer_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Keep this match aligned with OPS so additions require an explicit implementation.
    let op = args
        .enum_value("op")
        .ok_or_else(|| CmdError::MissingParam("op".to_string()))?;

    match op {
        "new" => cmd_new(ctx, &args),
        "delete" => cmd_delete(ctx, &args),
        "rename" => cmd_rename(ctx, &args),
        "color" => cmd_color(ctx, &args),
        "on" => cmd_toggle(ctx, &args, "Layer on", |p| p.off = Some(false)),
        "off" => cmd_toggle(ctx, &args, "Layer off", |p| p.off = Some(true)),
        "freeze" => cmd_toggle(ctx, &args, "Layer freeze", |p| p.frozen = Some(true)),
        "thaw" => cmd_toggle(ctx, &args, "Layer thaw", |p| p.frozen = Some(false)),
        "lock" => cmd_toggle(ctx, &args, "Layer lock", |p| p.locked = Some(true)),
        "unlock" => cmd_toggle(ctx, &args, "Layer unlock", |p| p.locked = Some(false)),
        "plot" => cmd_toggle(ctx, &args, "Layer plot", |p| p.plot = Some(true)),
        "no-plot" => cmd_toggle(ctx, &args, "Layer no-plot", |p| p.plot = Some(false)),
        "set-current" => cmd_set_current(ctx, &args),
        other => unreachable!("op fuera del schema Enum de LAYER: {other}"),
    }
}

fn require_layer(args: &ParsedArgs) -> Result<af_model::id::LayerId, CmdError> {
    args.layer("layer")
        .ok_or_else(|| CmdError::MissingParam("layer".to_string()))
}

fn require_name(args: &ParsedArgs) -> Result<&str, CmdError> {
    args.text("name")
        .ok_or_else(|| CmdError::MissingParam("name".to_string()))
}

fn cmd_new(ctx: &mut CommandCtx<'_>, args: &ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let name = require_name(args)?;
    let continuous = ctx
        .document()
        .line_types()
        .find(|lt| lt.name().eq_ignore_ascii_case("continuous"))
        .map(af_model::LineType::id)
        .ok_or_else(|| {
            CmdError::Failed("LAYER new: document has no 'Continuous' line type".to_string())
        })?;
    let props = LayerProps::new(
        name,
        Color::aci(7).expect("7 is in range 1..=255"),
        continuous,
        af_model::entity::Lineweight::ByLayer,
    );
    let id = ctx.transact("Layer new", |tx| {
        layers_ops::create_layer(tx, props).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::message(format!(
        "layer '{name}' created (id {})",
        id.raw().0
    )))
}

fn cmd_delete(ctx: &mut CommandCtx<'_>, args: &ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = require_layer(args)?;
    ctx.transact("Layer delete", |tx| {
        layers_ops::delete_layer(tx, layer, DeletePolicy::RejectIfUsed).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

fn cmd_rename(ctx: &mut CommandCtx<'_>, args: &ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = require_layer(args)?;
    let name = require_name(args)?.to_string();
    ctx.transact("Layer rename", |tx| {
        layers_ops::rename_layer(tx, layer, name).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

fn cmd_color(ctx: &mut CommandCtx<'_>, args: &ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let layer = require_layer(args)?;
    let raw = args
        .text("color")
        .ok_or_else(|| CmdError::MissingParam("color".to_string()))?;
    let color = parse_color(raw)?;
    let patch = LayerPatch {
        color: Some(color),
        ..LayerPatch::default()
    };
    ctx.transact("Layer color", |tx| {
        layers_ops::set_layer_props(tx, layer, patch).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

/// Applies a one-field Boolean [`LayerPatch`] in a transaction labeled `label`.
fn cmd_toggle(
    ctx: &mut CommandCtx<'_>,
    args: &ParsedArgs,
    label: &'static str,
    patch_field: impl FnOnce(&mut LayerPatch),
) -> Result<CommandOutcome, CmdError> {
    let layer = require_layer(args)?;
    let mut patch = LayerPatch::default();
    patch_field(&mut patch);
    ctx.transact(label, |tx| {
        layers_ops::set_layer_props(tx, layer, patch).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}

fn cmd_set_current(
    ctx: &mut CommandCtx<'_>,
    args: &ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let layer = require_layer(args)?;
    ctx.transact("Layer set-current", |tx| {
        layers_ops::set_current_layer(tx, layer).map_err(CmdError::from)
    })?;
    Ok(CommandOutcome::new())
}
