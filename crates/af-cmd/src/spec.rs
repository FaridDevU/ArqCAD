//! Typed command schemas ([`ParamType`], [`ParamSpec`], [`CommandSpec`]), execution
//! context ([`CommandCtx`]), outcomes ([`CommandOutcome`]), and errors ([`CmdError`]).
//!
//! A [`CommandSpec`] declares a canonical name, aliases, UI/undo label, typed
//! parameters, mutation behavior, and the [`CommandFn`] that executes it. The
//! registry validates arguments before calling the function.

use af_model::entity::EntityGeometry;
use af_model::id::{EntityId, LayerId};
use af_model::{
    ChangeSet, Document, RedoError, Session, SysvarDef, SysvarError, SysvarValue, Transaction,
    TxContext, TxError, UndoError,
};

use crate::args::ParsedArgs;

/// The type of a command parameter.
///
/// The registry validates type, existence, and range before execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    /// A 2D point. JSON: `[x, y]` with two finite numbers.
    Point,
    /// A strictly positive distance. JSON: number greater than zero.
    Distance,
    /// An angle in radians. JSON callers supply a finite number; command-line
    /// input is converted from degrees by the parser.
    Angle,
    /// A repetition count. JSON: nonnegative integer.
    Count,
    /// Existing entities. JSON: an integer ID array whose members exist in the document.
    EntitySet,
    /// A case-insensitive keyword from a closed set. JSON: string.
    Enum(Vec<String>),
    /// Free-form text. JSON: string.
    Text,
    /// An existing layer reference. JSON: case-insensitive name or integer ID.
    LayerRef,
    /// A Boolean switch. JSON: bool.
    Flag,
    /// A nonempty polyline path of `{"pt":[x,y]}` vertices with optional finite
    /// `bulge` values. Missing bulges default to zero.
    Path,
}

impl ParamType {
    /// Returns a stable short type name for diagnostics.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            ParamType::Point => "Point",
            ParamType::Distance => "Distance",
            ParamType::Angle => "Angle",
            ParamType::Count => "Count",
            ParamType::EntitySet => "EntitySet",
            ParamType::Enum(_) => "Enum",
            ParamType::Text => "Text",
            ParamType::LayerRef => "LayerRef",
            ParamType::Flag => "Flag",
            ParamType::Path => "Path",
        }
    }
}

/// A parameter's name, type, optionality, and JSON default.
///
/// A parameter with a default is implicitly optional; the registry validates the
/// default exactly like a supplied value.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    /// Parameter name and JSON-object key.
    pub name: String,
    /// Expected type.
    pub ty: ParamType,
    /// Whether the parameter may be omitted.
    pub optional: bool,
    /// JSON default used when omitted; implies `optional`.
    pub default: Option<serde_json::Value>,
}

impl ParamSpec {
    /// Creates a required parameter.
    #[must_use]
    pub fn required(name: impl Into<String>, ty: ParamType) -> Self {
        Self {
            name: name.into(),
            ty,
            optional: false,
            default: None,
        }
    }

    /// Creates an optional parameter without a default.
    #[must_use]
    pub fn optional(name: impl Into<String>, ty: ParamType) -> Self {
        Self {
            name: name.into(),
            ty,
            optional: true,
            default: None,
        }
    }

    /// Creates an optional parameter with a validated JSON default.
    #[must_use]
    pub fn with_default(
        name: impl Into<String>,
        ty: ParamType,
        default: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            ty,
            optional: true,
            default: Some(default),
        }
    }
}

/// Function signature for command execution.
///
/// Receives the mutation/read gateway and validated arguments, then returns a
/// [`CommandOutcome`] or [`CmdError`]. A plain function pointer lets plugins use
/// the same stateless registration path.
pub type CommandFn = fn(&mut CommandCtx<'_>, ParsedArgs) -> Result<CommandOutcome, CmdError>;

/// Function signature for a command preview.
///
/// Receives an immutable document and validated arguments, guaranteeing no
/// mutation or transaction. Returned result geometries can be drawn as a
/// transient overlay using the same planning phase as execution.
pub type PreviewFn = fn(&Document, ParsedArgs) -> Result<Vec<EntityGeometry>, CmdError>;

/// A declarative command specification.
pub struct CommandSpec {
    name: String,
    aliases: Vec<String>,
    label: String,
    params: Vec<ParamSpec>,
    affects_document: bool,
    execute: CommandFn,
    /// Optional zero-transaction preview function.
    preview: Option<PreviewFn>,
}

impl CommandSpec {
    /// Creates a specification with a canonical name, UI label, mutation flag,
    /// and execution function. Add aliases and parameters with the builder methods.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        label: impl Into<String>,
        affects_document: bool,
        execute: CommandFn,
    ) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            label: label.into(),
            params: Vec::new(),
            affects_document,
            execute,
            preview: None,
        }
    }

    /// Declares the command's preview function.
    #[must_use]
    pub fn preview(mut self, preview: PreviewFn) -> Self {
        self.preview = Some(preview);
        self
    }

    /// Adds a case-insensitive alias.
    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Adds a parameter in declaration order.
    #[must_use]
    pub fn param(mut self, param: ParamSpec) -> Self {
        self.params.push(param);
        self
    }

    /// Returns the stable canonical scripting name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns declared aliases.
    #[must_use]
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Returns the human-readable UI and history label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the parameter schema.
    #[must_use]
    pub fn params(&self) -> &[ParamSpec] {
        &self.params
    }

    /// Returns whether execution must attempt exactly one transaction and yield
    /// either one committed change or a non-empty structural no-op.
    #[must_use]
    pub fn affects_document(&self) -> bool {
        self.affects_document
    }

    /// Returns the execution function pointer.
    pub(crate) fn execute_fn(&self) -> CommandFn {
        self.execute
    }

    /// Returns the optional preview function pointer.
    pub(crate) fn preview_fn(&self) -> Option<PreviewFn> {
        self.preview
    }
}

/// The outcome of a successfully executed command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandOutcome {
    /// Sequence of the committed transaction, or `None` when none was created.
    pub tx_seq: Option<u64>,
    /// Entity IDs created by the command.
    pub created: Vec<EntityId>,
    /// Optional UI or console message.
    pub message: Option<String>,
    /// Observable [`ChangeSet`] values in application order. The registry moves
    /// them from [`CommandCtx`] so index, render, and selection observers can update
    /// incrementally from the model's own event.
    ///
    /// Mutating commands produce exactly one `Cause::Do` change set or, for a
    /// non-empty structural no-op, none with `tx_seq = None`. A truly empty
    /// mutating attempt remains a contract violation. UNDO and REDO create no
    /// transaction but publish one inverse or reapplied change set. Pure view
    /// commands publish none.
    pub change_sets: Vec<ChangeSet>,
}

impl CommandOutcome {
    /// Creates an outcome without new entities or a message.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an outcome containing only a message.
    #[must_use]
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            message: Some(msg.into()),
            ..Self::default()
        }
    }

    /// Creates an outcome reporting new entities.
    #[must_use]
    pub fn created(created: Vec<EntityId>) -> Self {
        Self {
            created,
            ..Self::default()
        }
    }
}

/// Context borrowed by a command during execution.
///
/// This is the command's only mutation surface. Transactions pass through
/// [`transact`](Self::transact), and undo/redo through [`undo`](Self::undo) and
/// [`redo`](Self::redo), allowing the registry to enforce exact transaction counts.
pub struct CommandCtx<'a> {
    session: &'a mut Session,
    tx_attempts: u32,
    tx_count: u32,
    semantic_noop_count: u32,
    last_tx_seq: Option<u64>,
    /// Observable change sets produced by transact, undo, or redo, in order.
    change_sets: Vec<ChangeSet>,
}

impl<'a> CommandCtx<'a> {
    /// Creates a context for a session.
    pub(crate) fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            tx_attempts: 0,
            tx_count: 0,
            semantic_noop_count: 0,
            last_tx_seq: None,
            change_sets: Vec::new(),
        }
    }

    /// Returns the document for read-only change planning.
    #[must_use]
    pub fn document(&self) -> &Document {
        self.session.document()
    }

    /// Returns whether a transaction can be undone.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.session.can_undo()
    }

    /// Returns whether a transaction can be redone.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.session.can_redo()
    }

    /// Returns the next undo label, if any.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.session.undo_label()
    }

    /// Returns the next redo label, if any.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.session.redo_label()
    }

    /// Returns committed undo entries from newest to oldest without modifying history.
    #[must_use]
    pub fn undo_transactions(&self) -> impl DoubleEndedIterator<Item = &Transaction> + '_ {
        self.session.undo_transactions()
    }

    /// Executes a session transaction, counting the call before it runs and the
    /// commit only when it succeeds with a semantic document change.
    ///
    /// A closure returning `Ok` with a semantic document change commits and
    /// increments the counter. Empty transactions, structural no-ops, and
    /// rollbacks do not count as commits; non-empty structural no-ops are tracked
    /// separately for registry validation.
    ///
    /// # Errors
    /// Propagates the closure's [`CmdError`] after atomic rollback.
    pub fn transact<T, F>(&mut self, label: impl Into<String>, f: F) -> Result<T, CmdError>
    where
        F: FnOnce(&mut TxContext<'_>) -> Result<T, CmdError>,
    {
        self.tx_attempts += 1;
        let mut had_operations = false;
        let outcome = self.session.transact(label, |tx| {
            let result = f(tx);
            had_operations = tx.has_operations();
            result
        })?;
        if let Some(tx) = outcome.transaction.as_ref() {
            self.tx_count += 1;
            self.last_tx_seq = Some(tx.seq());
        } else if had_operations {
            self.semantic_noop_count += 1;
        }
        if let Some(cs) = outcome.change_set {
            self.change_sets.push(cs);
        }
        Ok(outcome.value)
    }

    /// Undoes the last transaction without creating a new one.
    ///
    /// # Errors
    /// Returns [`CmdError::NothingToUndo`] when history is empty.
    pub fn undo(&mut self) -> Result<ChangeSet, CmdError> {
        let cs = self.session.try_undo()?.ok_or(CmdError::NothingToUndo)?;
        self.change_sets.push(cs.clone());
        Ok(cs)
    }

    /// Redoes the last undone transaction without creating a new one.
    ///
    /// # Errors
    /// Returns [`CmdError::NothingToRedo`] when no redo entry exists.
    pub fn redo(&mut self) -> Result<ChangeSet, CmdError> {
        let cs = self.session.try_redo()?.ok_or(CmdError::NothingToRedo)?;
        self.change_sets.push(cs.clone());
        Ok(cs)
    }

    /// Returns case-insensitive system variable `name`, if it exists.
    ///
    /// System variables are session state; reading or writing them creates no transaction.
    #[must_use]
    pub fn sysvar(&self, name: &str) -> Option<SysvarValue> {
        self.session.sysvar(name)
    }

    /// Returns metadata for system variable `name`.
    #[must_use]
    pub fn sysvar_def(&self, name: &str) -> Option<&'static SysvarDef> {
        self.session.sysvar_def(name)
    }

    /// Sets system variable `name` without creating a transaction.
    ///
    /// # Errors
    /// Returns [`CmdError::Failed`] for unknown variables, type mismatch, or
    /// out-of-domain values.
    pub fn set_sysvar(&mut self, name: &str, value: SysvarValue) -> Result<(), SysvarError> {
        self.session.set_sysvar(name, value)
    }

    /// Returns the number of committed transactions created by the command.
    pub(crate) fn tx_count(&self) -> u32 {
        self.tx_count
    }

    /// Returns the number of command-level transaction calls, including failures.
    pub(crate) fn tx_attempts(&self) -> u32 {
        self.tx_attempts
    }

    /// Returns the number of non-empty attempts discarded as structural no-ops.
    pub(crate) fn semantic_noop_count(&self) -> u32 {
        self.semantic_noop_count
    }

    /// Returns the sequence of the command's last committed transaction.
    pub(crate) fn last_tx_seq(&self) -> Option<u64> {
        self.last_tx_seq
    }

    /// Drains accumulated change sets for transfer to [`CommandOutcome`].
    pub(crate) fn take_change_sets(&mut self) -> Vec<ChangeSet> {
        std::mem::take(&mut self.change_sets)
    }

    /// Returns the pending LAYISO backup. This is session state and does not
    /// participate in transactions or undo/redo.
    #[must_use]
    pub fn layer_iso_backup(&self) -> Option<&[(LayerId, bool)]> {
        self.session.layer_iso_backup()
    }

    /// Replaces the pending LAYISO backup.
    pub fn set_layer_iso_backup(&mut self, backup: Vec<(LayerId, bool)>) {
        self.session.set_layer_iso_backup(backup);
    }

    /// Takes the pending LAYISO backup for restoration.
    pub fn take_layer_iso_backup(&mut self) -> Option<Vec<(LayerId, bool)>> {
        self.session.take_layer_iso_backup()
    }
}

/// A command validation or execution error.
///
/// Messages are suitable for direct UI display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CmdError {
    /// No command has the requested name or alias.
    UnknownCommand(String),
    /// JSON arguments are neither an object nor `null`.
    NotAnObject,
    /// An argument does not match any command parameter.
    UnknownParam(String),
    /// A required parameter is missing.
    MissingParam(String),
    /// An argument type does not match its parameter.
    TypeMismatch {
        /// Affected parameter.
        param: String,
        /// Expected type description.
        expected: &'static str,
        /// Received JSON type description.
        found: String,
    },
    /// An argument has the correct type but is outside its valid range.
    OutOfRange {
        /// Affected parameter.
        param: String,
        /// Rejection reason.
        message: String,
    },
    /// A keyword is not in its `Enum` set.
    InvalidEnum {
        /// Affected parameter.
        param: String,
        /// Received value.
        value: String,
        /// Accepted keywords.
        allowed: Vec<String>,
    },
    /// An `EntitySet` references an ID absent from the document.
    UnknownEntity(EntityId),
    /// A `LayerRef` references an unknown layer name or ID.
    UnknownLayer(String),
    /// The underlying transaction failed.
    Tx(TxError),
    /// UNDO has no transaction to undo.
    NothingToUndo,
    /// REDO has no transaction to redo.
    NothingToRedo,
    /// A command violated the transaction count implied by `affects_document`.
    ContractViolation(String),
    /// Preview was requested for a command without preview support.
    NotPreviewable(String),
    /// A command-specific failure.
    Failed(String),
}

impl core::fmt::Display for CmdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CmdError::UnknownCommand(name) => write!(f, "unknown command '{name}'"),
            CmdError::NotAnObject => {
                write!(
                    f,
                    "command arguments must be a JSON object (or null for none)"
                )
            }
            CmdError::UnknownParam(p) => write!(f, "unknown argument '{p}'"),
            CmdError::MissingParam(p) => write!(f, "missing required argument '{p}'"),
            CmdError::TypeMismatch {
                param,
                expected,
                found,
            } => write!(
                f,
                "argument '{param}': expected {expected}, found JSON {found}"
            ),
            CmdError::OutOfRange { param, message } => {
                write!(f, "argument '{param}' out of range: {message}")
            }
            CmdError::InvalidEnum {
                param,
                value,
                allowed,
            } => write!(
                f,
                "argument '{param}': '{value}' is not one of [{}]",
                allowed.join(", ")
            ),
            CmdError::UnknownEntity(id) => write!(f, "unknown entity id {}", id.raw().0),
            CmdError::UnknownLayer(r) => write!(f, "unknown layer '{r}'"),
            CmdError::Tx(e) => write!(f, "{e}"),
            CmdError::NothingToUndo => write!(f, "nothing to undo"),
            CmdError::NothingToRedo => write!(f, "nothing to redo"),
            CmdError::ContractViolation(m) => write!(f, "command contract violation: {m}"),
            CmdError::NotPreviewable(name) => {
                write!(f, "command '{name}' does not support preview")
            }
            CmdError::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CmdError {}

impl From<TxError> for CmdError {
    fn from(e: TxError) -> Self {
        CmdError::Tx(e)
    }
}

impl From<UndoError> for CmdError {
    fn from(_: UndoError) -> Self {
        CmdError::NothingToUndo
    }
}

impl From<RedoError> for CmdError {
    fn from(_: RedoError) -> Self {
        CmdError::NothingToRedo
    }
}

impl From<SysvarError> for CmdError {
    /// System variables are session state, so their errors become readable
    /// [`CmdError::Failed`] values rather than transaction errors.
    fn from(e: SysvarError) -> Self {
        CmdError::Failed(e.to_string())
    }
}
