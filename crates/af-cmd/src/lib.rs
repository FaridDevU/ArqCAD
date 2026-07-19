#![forbid(unsafe_code)]
//! The headless ArcCAD command system.
//!
//! A [`CommandSpec`] is an executable, scriptable, testable unit: a name and
//! complete arguments produce exactly one transaction, or none. This crate has
//! no mouse, prompt, or UI concerns. Interactive tools collect complete JSON
//! arguments, and [`CommandRegistry`] validates them before execution.
//!
//! # Components
//! - [`ParamType`], [`ParamSpec`], and [`CommandSpec`] define the typed schema.
//! - [`CommandRegistry`] registers, resolves, validates, executes, and enforces
//!   the one-transaction contract.
//! - [`parse_input`] parses command-line input without parser dependencies. A
//!   comma always separates coordinates and is never a decimal separator.
//! - [`parse_pgp`], [`PgpLayer`], and [`PgpEdit`] provide strict layered PGP
//!   loading and atomic Rust-only editing.
//! - [`builtin`] contains the commands implemented by this crate.
//!
//! # Transaction invariant
//!
//! A successful command with `affects_document` must produce exactly one
//! committed, nonempty transaction. [`CommandCtx`] is the only mutation gateway
//! exposed to commands and counts calls through [`CommandCtx::transact`].
//! UNDO and REDO use [`CommandCtx::undo`] and [`CommandCtx::redo`] instead.
//! [`CommandRegistry`] rejects any deviation with
//! [`CmdError::ContractViolation`].

mod args;
mod parse;
mod pgp;
mod registry;
mod spec;

/// Built-in commands, registered together through
/// [`register_builtins`](builtin::register_builtins).
pub mod builtin;

pub use args::{ArgValue, ParsedArgs};
pub use parse::{ParseError, ParsedInput, parse_input};
pub use pgp::{
    PgpEdit, PgpEditError, PgpError, PgpLayer, PgpParse, parse_pgp, parse_pgp_layer,
    standard_aliases,
};
pub use registry::{CommandRegistry, RegisterError};
pub use spec::{
    CmdError, CommandCtx, CommandFn, CommandOutcome, CommandSpec, ParamSpec, ParamType,
};
