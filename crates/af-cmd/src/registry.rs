//! [`CommandRegistry`] provides command registration, case-insensitive lookup,
//! and headless execution with transaction-contract enforcement.
//!
//! Runtime commands and plugins use the same [`CommandSpec`] registration path.
//! Case-insensitive name or alias collisions are [`RegisterError`] values rather
//! than last-definition-wins replacements.
//!
//! Runtime user/PGP aliases loaded through
//! [`CommandRegistry::apply_user_aliases`] may replace built-in aliases, but never
//! canonical command names.

use std::collections::{HashMap, HashSet};

use af_model::Session;
use af_model::entity::EntityGeometry;
use serde_json::Value;

use crate::args::validate_args;
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec};

/// Shared caseless normalization for command names and aliases. Uppercasing before
/// lowercasing preserves equivalence for Unicode expansions such as `ß` to `SS`
/// and contextual forms such as final sigma.
fn normalize_token(token: &str) -> String {
    token.trim().to_uppercase().to_lowercase()
}

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
/// 2. **User/PGP alias** (`user_aliases`), which may replace a built-in alias.
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
    /// Normalized user/PGP alias to index in `specs`.
    user_aliases: HashMap<String, usize>,
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
            .or_else(|| self.user_aliases.get(&key))
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

    /// Returns the number of active user/PGP aliases.
    #[must_use]
    pub fn user_alias_count(&self) -> usize {
        self.user_aliases.len()
    }

    /// Applies runtime aliases between canonical names and built-in aliases in
    /// lookup precedence.
    ///
    /// For each `(alias, target)` pair:
    /// - Empty aliases are skipped with a warning.
    /// - Aliases matching canonical names are skipped with a warning.
    /// - Unknown targets are skipped with a warning.
    /// - Valid aliases replace earlier entries using last-definition-wins semantics.
    ///
    /// Valid pairs are applied even when other pairs produce warnings.
    pub fn apply_user_aliases<I, A, T>(&mut self, pairs: I) -> Vec<String>
    where
        I: IntoIterator<Item = (A, T)>,
        A: AsRef<str>,
        T: AsRef<str>,
    {
        let mut warnings = Vec::new();
        for (alias, target) in pairs {
            let alias = alias.as_ref();
            let target = target.as_ref();
            let key = normalize_token(alias);

            if key.is_empty() {
                warnings.push(format!(
                    "alias de usuario vacío ignorado (destino '{target}')"
                ));
                continue;
            }
            if self.canonical.contains_key(&key) {
                warnings.push(format!(
                    "alias de usuario '{alias}' ignorado: coincide con el nombre canónico de un comando ya registrado"
                ));
                continue;
            }
            match self.lookup_index(target) {
                Some(idx) => {
                    self.user_aliases.insert(key, idx);
                }
                None => {
                    warnings.push(format!(
                        "alias de usuario '{alias}' -> '{target}' ignorado: comando destino desconocido"
                    ));
                }
            }
        }
        warnings
    }

    /// Replaces the entire user-alias table in one swap.
    ///
    /// Pairs are validated against an initially empty candidate table, so aliases
    /// absent from `pairs` are removed. Validation and precedence match
    /// [`apply_user_aliases`](Self::apply_user_aliases).
    pub fn replace_user_aliases<I, A, T>(&mut self, pairs: I) -> Vec<String>
    where
        I: IntoIterator<Item = (A, T)>,
        A: AsRef<str>,
        T: AsRef<str>,
    {
        let mut candidate = HashMap::new();
        let mut warnings = Vec::new();

        for (alias, target) in pairs {
            let alias = alias.as_ref();
            let target = target.as_ref();
            let key = normalize_token(alias);

            if key.is_empty() {
                warnings.push(format!(
                    "alias de usuario vacío ignorado (destino '{target}')"
                ));
                continue;
            }
            if self.canonical.contains_key(&key) {
                warnings.push(format!(
                    "alias de usuario '{alias}' ignorado: coincide con el nombre canónico de un comando ya registrado"
                ));
                continue;
            }

            let target_key = normalize_token(target);
            match self
                .canonical
                .get(&target_key)
                .or_else(|| candidate.get(&target_key))
                .or_else(|| self.index.get(&target_key))
                .copied()
            {
                Some(idx) => {
                    candidate.insert(key, idx);
                }
                None => warnings.push(format!(
                    "alias de usuario '{alias}' -> '{target}' ignorado: comando destino desconocido"
                )),
            }
        }

        self.user_aliases = candidate;
        warnings
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
            let last_seq = ctx.last_tx_seq();
            let change_sets = ctx.take_change_sets();
            let change_set_count = change_sets.len();

            // Enforce the declared transaction contract.
            if spec.affects_document() {
                if tx_attempts != 1 || tx_count != 1 || change_set_count != 1 {
                    return Err(CmdError::ContractViolation(format!(
                        "command '{}' declares affects_document but produced {tx_attempts} transaction attempts, {tx_count} transactions, and {change_set_count} change sets (exactly 1 of each required)",
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
