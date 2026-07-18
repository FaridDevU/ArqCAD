//! DXF adapter options, reports, and shared [`DxfError`] values.

use std::collections::BTreeMap;

/// DXF export options.
///
/// Currently empty because R2000 ASCII is the only output format. The type keeps
/// the public export signature ready for future compatible options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportOptions {}

/// Summary of written, skipped, and warned export content.
///
/// Counts use DXF type names. [`BTreeMap`] keeps stable ordering for tests and
/// diffs. Unsupported future geometry must appear in both `skipped` and
/// `warnings` instead of being silently omitted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// Written entities by DXF type.
    pub exported: BTreeMap<String, usize>,
    /// Skipped entities by internal type.
    pub skipped: BTreeMap<String, usize>,
    /// Human-readable warnings.
    pub warnings: Vec<String>,
}

impl ExportReport {
    /// Increments the exported count for `kind`.
    pub(crate) fn bump_exported(&mut self, kind: &str) {
        *self.exported.entry(kind.to_string()).or_insert(0) += 1;
    }

    /// Increments the skipped count for `kind`.
    #[allow(dead_code)]
    pub(crate) fn bump_skipped(&mut self, kind: &str) {
        *self.skipped.entry(kind.to_string()).or_insert(0) += 1;
    }

    /// Records a warning.
    pub(crate) fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    /// Total exported entities.
    #[must_use]
    pub fn total_exported(&self) -> usize {
        self.exported.values().sum()
    }
}

/// DXF import options.
///
/// Currently empty while import uses the tolerant supported subset. The type
/// preserves the public signature for future compatible options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportOptions {}

/// Summary of imported, skipped, and warned content.
///
/// Counts use source DXF type names and stable [`BTreeMap`] order. Every omission
/// increments `skipped` and records a warning. Layers merge by name and do not
/// count as entities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Imported entities by source DXF type.
    pub imported: BTreeMap<String, usize>,
    /// Skipped unsupported or invalid entities by DXF type.
    pub skipped: BTreeMap<String, usize>,
    /// Human-readable warnings.
    pub warnings: Vec<String>,
}

impl ImportReport {
    /// Increments the imported count for `kind`.
    pub(crate) fn bump_imported(&mut self, kind: &str) {
        *self.imported.entry(kind.to_string()).or_insert(0) += 1;
    }

    /// Increments the skipped count for `kind`.
    pub(crate) fn bump_skipped(&mut self, kind: &str) {
        *self.skipped.entry(kind.to_string()).or_insert(0) += 1;
    }

    /// Records a warning.
    pub(crate) fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    /// Total imported entities.
    #[must_use]
    pub fn total_imported(&self) -> usize {
        self.imported.values().sum()
    }

    /// Total skipped entities.
    #[must_use]
    pub fn total_skipped(&self) -> usize {
        self.skipped.values().sum()
    }
}

/// DXF adapter error shared by import and export.
///
/// Unmappable geometry is reported as skipped rather than forcing the model into
/// DXF constraints. Import skips individual invalid entities and errors only for
/// structurally unreadable or oversized input.
#[derive(Debug)]
pub enum DxfError {
    /// I/O error while reading or writing.
    Io(std::io::Error),
    /// Input exceeds the import size limit.
    TooLarge {
        /// Exceeded byte limit.
        limit: u64,
    },
    /// Input violates the ASCII DXF code/value pair structure.
    Malformed(String),
}

impl core::fmt::Display for DxfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DxfError::Io(e) => write!(f, "DXF I/O error: {e}"),
            DxfError::TooLarge { limit } => {
                write!(f, "DXF input exceeds the {limit}-byte import limit")
            }
            DxfError::Malformed(why) => write!(f, "malformed DXF input: {why}"),
        }
    }
}

impl std::error::Error for DxfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DxfError::Io(e) => Some(e),
            DxfError::TooLarge { .. } | DxfError::Malformed(_) => None,
        }
    }
}

impl From<std::io::Error> for DxfError {
    fn from(e: std::io::Error) -> Self {
        DxfError::Io(e)
    }
}
