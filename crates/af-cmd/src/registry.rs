//! [`CommandRegistry`] provides command registration, case-insensitive lookup,
//! and headless execution with transaction-contract enforcement.
//!
//! Runtime commands and plugins use the same [`CommandSpec`] registration path.
//! Case-insensitive name or alias collisions are [`RegisterError`] values rather
//! than last-definition-wins replacements.
//!
//! Runtime PGP aliases are published from four validated layers in one swap.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use af_model::Session;
use af_model::entity::EntityGeometry;
use serde_json::Value;

use crate::args::validate_args;
use crate::pgp::{
    PgpEdit, PgpEditError, PgpError, PgpLayer, PgpParse, normalize_token, parse_pgp_layer_prefix,
    prepare_edit, write_atomic,
};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec};

/// An error raised while registering a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// A name or alias collides case-insensitively with a registered token or
    /// another token in the same command.
    Duplicate {
        /// The colliding token as declared.
        token: String,
    },
    /// A name or alias is empty after trimming.
    EmptyName,
}

impl core::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RegisterError::Duplicate { token } => {
                write!(
                    f,
                    "duplicate command name/alias (case-insensitive): '{token}'"
                )
            }
            RegisterError::EmptyName => write!(f, "command name/alias must not be empty"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// A command registry with case-insensitive name and alias lookup.
///
/// # [`lookup`](Self::lookup) precedence
///
/// The first matching layer wins:
///
/// 1. **Canonical name** (`canonical`), which aliases can never shadow.
/// 2. **Effective PGP alias** (`pgp_aliases`), with session-to-system precedence.
/// 3. **Built-in alias** (`index`), declared through `CommandSpec::alias`.
///
/// `index` also stores canonical names so registration detects all collisions in
/// one table, but lookup resolves those names through `canonical` first.
#[derive(Default)]
pub struct CommandRegistry {
    specs: Vec<CommandSpec>,
    /// Normalized built-in token to index in `specs`, including canonical names.
    index: HashMap<String, usize>,
    /// Normalized canonical name to index in `specs`.
    canonical: HashMap<String, usize>,
    /// Normalized effective PGP alias to index in `specs`.
    pgp_aliases: HashMap<String, usize>,
}

impl CommandRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a command.
    ///
    /// Names and aliases must be nonempty and case-insensitively unique after trimming.
    ///
    /// # Errors
    /// - [`RegisterError::EmptyName`] for an empty name or alias.
    /// - [`RegisterError::Duplicate`] for any case-insensitive collision.
    pub fn register(&mut self, spec: CommandSpec) -> Result<(), RegisterError> {
        if spec.name().trim().is_empty() {
            return Err(RegisterError::EmptyName);
        }

        // Keep original spellings for diagnostics while validating normalized tokens.
        let mut tokens: Vec<&str> = Vec::with_capacity(1 + spec.aliases().len());
        tokens.push(spec.name());
        for alias in spec.aliases() {
            if alias.trim().is_empty() {
                return Err(RegisterError::EmptyName);
            }
            tokens.push(alias);
        }

        // Validate every collision before mutation so registration is atomic.
        let mut seen: HashSet<String> = HashSet::new();
        for token in &tokens {
            let key = normalize_token(token);
            if self.index.contains_key(&key) || !seen.insert(key) {
                return Err(RegisterError::Duplicate {
                    token: (*token).to_string(),
                });
            }
        }

        let idx = self.specs.len();
        for (i, token) in tokens.iter().enumerate() {
            let key = normalize_token(token);
            self.index.insert(key.clone(), idx);
            if i == 0 {
                // `tokens[0]` is always `spec.name()`.
                self.canonical.insert(key, idx);
            }
        }
        self.specs.push(spec);
        Ok(())
    }

    /// Resolves trimmed, case-insensitive `name` to its index using documented precedence.
    fn lookup_index(&self, name: &str) -> Option<usize> {
        let key = normalize_token(name);
        self.canonical
            .get(&key)
            .or_else(|| self.pgp_aliases.get(&key))
            .or_else(|| self.index.get(&key))
            .copied()
    }

    /// Finds a command by trimmed, case-insensitive name or alias.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&CommandSpec> {
        self.lookup_index(name).map(|i| &self.specs[i])
    }

    /// Resolves a name or alias to the command's canonical name.
    #[must_use]
    pub fn resolve_canonical_name(&self, name: &str) -> Option<&str> {
        self.lookup(name).map(|spec| spec.name())
    }

    /// Returns the number of active aliases after layer precedence.
    #[must_use]
    pub fn pgp_alias_count(&self) -> usize {
        self.pgp_aliases.len()
    }

    /// Parses, validates, and publishes all four layers in one fail-closed swap.
    ///
    /// # Errors
    /// Returns the first stable layer/line diagnostic. The prior table is retained.
    pub fn replace_pgp_layers(
        &mut self,
        system: &str,
        user: &str,
        project: &str,
        session: &str,
    ) -> Result<Vec<String>, PgpError> {
        let contents = [system, user, project, session];
        let parsed: Vec<(PgpLayer, PgpParse, Option<PgpError>)> = PgpLayer::ALL
            .into_iter()
            .zip(contents)
            .map(|(layer, content)| {
                let (parsed, error) = parse_pgp_layer_prefix(layer, content);
                (layer, parsed, error)
            })
            .collect();
        let (candidate, diagnostics) = self.validate_pgp_layers(&parsed)?;
        self.pgp_aliases = candidate;
        Ok(diagnostics)
    }

    /// Atomically edits an existing UTF-8 PGP file after syntax and target validation.
    ///
    /// This Rust-only API never crosses JSON/FFI. Any error leaves the destination
    /// byte-identical; the returned strings are nonfatal builtin-shadow diagnostics.
    ///
    /// # Errors
    /// Returns strict parse, semantic, edit-precondition, or I/O errors.
    pub fn edit_pgp_file(
        &self,
        path: &Path,
        layer: PgpLayer,
        edit: PgpEdit<'_>,
    ) -> Result<Vec<String>, PgpEditError> {
        let (bytes, diagnostics) = prepare_edit(path, layer, edit, |content| {
            let (parsed, error) = parse_pgp_layer_prefix(layer, content);
            self.validate_pgp_layers(&[(layer, parsed, error)])
                .map(|(_, diagnostics)| diagnostics)
        })?;
        write_atomic(path, &bytes)?;
        Ok(diagnostics)
    }

    fn validate_pgp_layers(
        &self,
        layers: &[(PgpLayer, PgpParse, Option<PgpError>)],
    ) -> Result<(HashMap<String, usize>, Vec<String>), PgpError> {
        let all_aliases: HashSet<String> = layers
            .iter()
            .flat_map(|(_, parsed, _)| parsed.aliases.iter())
            .map(|(alias, _)| normalize_token(alias))
            .collect();
        let mut candidate = HashMap::new();
        let mut owners = HashMap::<String, PgpLayer>::new();
        let mut diagnostics = Vec::new();

        for (layer, parsed, syntax_error) in layers {
            for ((alias, target), line) in parsed.aliases.iter().zip(&parsed.lines) {
                let key = normalize_token(alias);
                if self.canonical.contains_key(&key) {
                    return Err(PgpError::new(
                        *layer,
                        *line,
                        format!("alias '{alias}' sombrea comando canonico"),
                    ));
                }

                let target_key = normalize_token(target);
                let Some(&target_index) = self.canonical.get(&target_key) else {
                    let cause = if all_aliases.contains(&target_key)
                        || self.index.contains_key(&target_key)
                    {
                        format!("target '{target}' es alias/no canonico")
                    } else {
                        format!("target '{target}' desconocido")
                    };
                    return Err(PgpError::new(*layer, *line, cause));
                };

                if self.index.contains_key(&key) {
                    diagnostics.push(format!(
                        "PGP {layer} linea {line}: alias '{alias}' reemplaza builtin"
                    ));
                }
                if let Some(previous) = owners.insert(key.clone(), *layer) {
                    diagnostics.push(format!(
                        "PGP {layer} linea {line}: alias '{alias}' reemplaza capa {previous}"
                    ));
                }
                candidate.insert(key, target_index);
            }
            if let Some(error) = syntax_error {
                return Err(error.clone());
            }
        }
        Ok((candidate, diagnostics))
    }

    /// Returns all commands in registration order.
    #[must_use]
    pub fn commands(&self) -> &[CommandSpec] {
        &self.specs
    }

    /// Executes a command by name with JSON arguments.
    ///
    /// The registry resolves the command, validates arguments, executes it, and
    /// enforces its transaction contract. It sets `tx_seq` from the committed
    /// transaction, or to `None` when no transaction was created.
    ///
    /// # Errors
    /// Returns lookup, argument-validation, command, or transaction-contract errors.
    pub fn execute(
        &self,
        session: &mut Session,
        name: &str,
        args: &Value,
    ) -> Result<CommandOutcome, CmdError> {
        let spec = self
            .lookup(name)
            .ok_or_else(|| CmdError::UnknownCommand(name.to_string()))?;

        // End the immutable session borrow before command execution.
        let parsed = validate_args(spec.params(), args, session.document())?;

        // Dispatch mutates only a candidate; the real session is published after
        // command execution and all transaction-contract checks succeed.
        // ponytail: use a session snapshot until profiling justifies staged dispatch.
        let mut candidate = session.clone();
        let result = (|| {
            // `CommandCtx` is the only mutation gateway and counts transactions.
            let mut ctx = CommandCtx::new(&mut candidate);
            let mut outcome = (spec.execute_fn())(&mut ctx, parsed)?;
            let tx_attempts = ctx.tx_attempts();
            let tx_count = ctx.tx_count();
            let semantic_noops = ctx.semantic_noop_count();
            let last_seq = ctx.last_tx_seq();
            let change_sets = ctx.take_change_sets();
            let change_set_count = change_sets.len();

            // Enforce the declared transaction contract.
            if spec.affects_document() {
                let real_change = tx_count == 1 && change_set_count == 1 && semantic_noops == 0;
                let semantic_noop = tx_count == 0 && change_set_count == 0 && semantic_noops == 1;
                if tx_attempts != 1 || !(real_change || semantic_noop) {
                    return Err(CmdError::ContractViolation(format!(
                        "command '{}' declares affects_document but produced {tx_attempts} transaction attempts, {tx_count} transactions, {semantic_noops} semantic no-ops, and {change_set_count} change sets (exactly one non-empty attempt yielding 1/1 or semantic no-op 0/0 required)",
                        spec.name()
                    )));
                }
            } else if tx_attempts != 0 {
                return Err(CmdError::ContractViolation(format!(
                    "command '{}' declares affects_document=false but produced {tx_attempts} transaction attempts (0 required)",
                    spec.name()
                )));
            }

            // Publish the committed transaction and observed changes uniformly.
            outcome.tx_seq = last_seq;
            outcome.change_sets = change_sets;
            Ok(outcome)
        })();
        if result.is_ok() {
            *session = candidate;
        }
        result
    }

    /// Validates and previews a command without creating a transaction. The
    /// immutable `&Session` signature prevents document mutation.
    ///
    /// # Errors
    /// Returns lookup, preview-support, argument-validation, or planning errors.
    pub fn preview(
        &self,
        session: &Session,
        name: &str,
        args: &Value,
    ) -> Result<Vec<EntityGeometry>, CmdError> {
        let spec = self
            .lookup(name)
            .ok_or_else(|| CmdError::UnknownCommand(name.to_string()))?;
        let preview_fn = spec
            .preview_fn()
            .ok_or_else(|| CmdError::NotPreviewable(spec.name().to_string()))?;
        let parsed = validate_args(spec.params(), args, session.document())?;
        preview_fn(session.document(), parsed)
    }
}
