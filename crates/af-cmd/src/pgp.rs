//! Strict, dependency-free PGP parsing and atomic file editing.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// PGP source layers, from lowest to highest precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PgpLayer {
    System,
    User,
    Project,
    Session,
}

impl PgpLayer {
    pub(crate) const ALL: [Self; 4] = [Self::System, Self::User, Self::Project, Self::Session];

    /// Stable diagnostic name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Project => "project",
            Self::Session => "session",
        }
    }
}

impl core::fmt::Display for PgpLayer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A fatal PGP diagnostic with stable layer and one-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgpError {
    pub layer: PgpLayer,
    pub line: usize,
    pub cause: String,
}

impl PgpError {
    pub(crate) fn new(layer: PgpLayer, line: usize, cause: impl Into<String>) -> Self {
        Self {
            layer,
            line,
            cause: cause.into(),
        }
    }
}

impl core::fmt::Display for PgpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PGP {} linea {}: {}", self.layer, self.line, self.cause)
    }
}

impl std::error::Error for PgpError {}

/// Strict parse result. Spellings are retained; comparisons use [`normalize_token`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PgpParse {
    pub aliases: Vec<(String, String)>,
    pub(crate) lines: Vec<usize>,
}

/// Normalizes every command/alias comparison without locale or Unicode NFC/NFD.
pub(crate) fn normalize_token(token: &str) -> String {
    token.trim().to_uppercase().to_lowercase()
}

#[derive(Clone, Copy)]
pub(crate) struct RawLine<'a> {
    pub text: &'a str,
    pub ending: &'a str,
    pub start: usize,
    pub end: usize,
    pub bare_cr: bool,
}

fn raw_lines(content: &str) -> Vec<RawLine<'_>> {
    let bytes = content.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                let (ending_len, bare_cr) = if bytes.get(i + 1) == Some(&b'\n') {
                    (2, false)
                } else {
                    (1, true)
                };
                lines.push(RawLine {
                    text: &content[start..i],
                    ending: &content[i..i + ending_len],
                    start,
                    end: i + ending_len,
                    bare_cr,
                });
                i += ending_len;
                start = i;
            }
            b'\n' => {
                lines.push(RawLine {
                    text: &content[start..i],
                    ending: &content[i..i + 1],
                    start,
                    end: i + 1,
                    bare_cr: false,
                });
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < content.len() {
        lines.push(RawLine {
            text: &content[start..],
            ending: "",
            start,
            end: content.len(),
            bare_cr: false,
        });
    }
    lines
}

/// Parses one user layer. Kept as the compact Rust compatibility entry point.
pub fn parse_pgp(content: &str) -> Result<PgpParse, PgpError> {
    parse_pgp_layer(PgpLayer::User, content)
}

/// Parses one named layer with fatal, line-addressed diagnostics.
pub fn parse_pgp_layer(layer: PgpLayer, content: &str) -> Result<PgpParse, PgpError> {
    let (parsed, error) = parse_pgp_layer_prefix(layer, content);
    error.map_or(Ok(parsed), Err)
}

/// Parses the valid prefix so semantic and syntax diagnostics can share line order.
pub(crate) fn parse_pgp_layer_prefix(
    layer: PgpLayer,
    content: &str,
) -> (PgpParse, Option<PgpError>) {
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut parsed = PgpParse::default();
    let mut seen = std::collections::HashMap::<String, usize>::new();

    for (offset, raw) in raw_lines(body).into_iter().enumerate() {
        let line_no = offset + 1;
        if raw.bare_cr {
            return (
                parsed,
                Some(PgpError::new(layer, line_no, "CR desnudo no permitido")),
            );
        }
        let line = raw.text.trim();
        if line.contains('\u{feff}') {
            return (
                parsed,
                Some(PgpError::new(
                    layer,
                    line_no,
                    "BOM solo permitido una vez al inicio",
                )),
            );
        }
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.bytes().filter(|byte| *byte == b',').count() != 1 {
            return (
                parsed,
                Some(PgpError::new(
                    layer,
                    line_no,
                    "fila activa requiere exactamente una coma",
                )),
            );
        }
        let (alias_field, command_field) = line.split_once(',').expect("comma counted above");
        let alias = alias_field.trim();
        if alias.is_empty() {
            return (parsed, Some(PgpError::new(layer, line_no, "alias vacio")));
        }
        let command_field = command_field.trim();
        if command_field.is_empty() {
            return (parsed, Some(PgpError::new(layer, line_no, "comando vacio")));
        }
        let Some(command) = command_field.strip_prefix('*') else {
            return (
                parsed,
                Some(PgpError::new(layer, line_no, "comando shell no permitido")),
            );
        };
        let command = command.trim();
        if command.is_empty() {
            return (parsed, Some(PgpError::new(layer, line_no, "comando vacio")));
        }
        if alias.contains(';') || command.contains(';') {
            return (
                parsed,
                Some(PgpError::new(
                    layer,
                    line_no,
                    "comentario inline no permitido",
                )),
            );
        }
        if alias.chars().any(char::is_whitespace)
            || command.chars().any(char::is_whitespace)
            || command.contains('*')
        {
            return (
                parsed,
                Some(PgpError::new(layer, line_no, "token PGP malformado")),
            );
        }

        let key = normalize_token(alias);
        if let Some(first) = seen.insert(key, line_no) {
            return (
                parsed,
                Some(PgpError::new(
                    layer,
                    line_no,
                    format!("alias duplicado/case-collision; primera linea {first}"),
                )),
            );
        }
        parsed.aliases.push((alias.to_owned(), command.to_owned()));
        parsed.lines.push(line_no);
    }
    (parsed, None)
}

/// One fail-closed edit to an existing PGP file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgpEdit<'a> {
    Add { alias: &'a str, target: &'a str },
    Update { alias: &'a str, target: &'a str },
    Delete { alias: &'a str },
}

/// File-edit failure. The destination is untouched for every error.
#[derive(Debug)]
pub enum PgpEditError {
    Io(io::Error),
    InvalidUtf8,
    Invalid(PgpError),
    AliasExists(String),
    AliasMissing(String),
}

impl core::fmt::Display for PgpEditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "PGP I/O: {error}"),
            Self::InvalidUtf8 => f.write_str("PGP file is not UTF-8"),
            Self::Invalid(error) => error.fmt(f),
            Self::AliasExists(alias) => write!(f, "PGP alias already exists: '{alias}'"),
            Self::AliasMissing(alias) => write!(f, "PGP alias not found: '{alias}'"),
        }
    }
}

impl std::error::Error for PgpEditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Invalid(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for PgpEditError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<PgpError> for PgpEditError {
    fn from(error: PgpError) -> Self {
        Self::Invalid(error)
    }
}

pub(crate) fn prepare_edit(
    path: &Path,
    layer: PgpLayer,
    edit: PgpEdit<'_>,
    mut validate: impl FnMut(&str) -> Result<Vec<String>, PgpError>,
) -> Result<(Vec<u8>, Vec<String>), PgpEditError> {
    let bytes = fs::read(path)?;
    let content = std::str::from_utf8(&bytes).map_err(|_| PgpEditError::InvalidUtf8)?;
    let has_bom = content.starts_with('\u{feff}');
    let body = content.strip_prefix('\u{feff}').unwrap_or(content);
    let _ = validate(content)?;
    let parsed = parse_pgp_layer(layer, content)?;
    let lines = raw_lines(body);

    let (alias, target, action) = match edit {
        PgpEdit::Add { alias, target } => (alias, Some(target), 0),
        PgpEdit::Update { alias, target } => (alias, Some(target), 1),
        PgpEdit::Delete { alias } => (alias, None, 2),
    };
    let key = normalize_token(alias);
    let found = parsed
        .aliases
        .iter()
        .position(|(candidate, _)| normalize_token(candidate) == key);

    let mut candidate = body.to_owned();
    match (action, found) {
        (0, Some(_)) => return Err(PgpEditError::AliasExists(alias.to_owned())),
        (0, None) => {
            let target = target.expect("add target").trim();
            let alias = alias.trim();
            let eol = lines
                .iter()
                .find(|line| !line.ending.is_empty())
                .map_or("\n", |line| line.ending);
            let ends_with_eol = lines.last().is_some_and(|line| !line.ending.is_empty());
            if !body.is_empty() && !ends_with_eol {
                candidate.push_str(eol);
            }
            candidate.push_str(alias);
            candidate.push_str(",*");
            candidate.push_str(target);
            if ends_with_eol {
                candidate.push_str(eol);
            }
        }
        (1, None) | (2, None) => {
            return Err(PgpEditError::AliasMissing(alias.to_owned()));
        }
        (1, Some(index)) => {
            let line = lines[parsed.lines[index] - 1];
            let comma = line.text.find(',').expect("parsed active row has comma");
            let after_comma = &line.text[comma + 1..];
            let star = comma + 1 + (after_comma.len() - after_comma.trim_start().len());
            let after_star = &line.text[star + 1..];
            let target_start = star + 1 + (after_star.len() - after_star.trim_start().len());
            let target_end = line.text.trim_end().len();
            candidate.replace_range(
                line.start + target_start..line.start + target_end,
                target.expect("update target").trim(),
            );
        }
        (2, Some(index)) => {
            let line = lines[parsed.lines[index] - 1];
            candidate.replace_range(line.start..line.end, "");
        }
        _ => unreachable!(),
    }

    let mut output = Vec::with_capacity(bytes.len().max(candidate.len() + 3));
    if has_bom {
        output.extend_from_slice("\u{feff}".as_bytes());
    }
    output.extend_from_slice(candidate.as_bytes());
    // Validate the complete candidate before creating any temporary file.
    let diagnostics = validate(std::str::from_utf8(&output).expect("constructed from UTF-8"))?;
    Ok((output, diagnostics))
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PgpEditError> {
    write_atomic_with(path, bytes, &TEMP_SEQUENCE, |_| Ok(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomicStage {
    Created,
    Written,
    Synced,
    Closed,
    Renaming,
}

fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    sequence: &AtomicU64,
    mut stage: impl FnMut(AtomicStage) -> io::Result<()>,
) -> Result<(), PgpEditError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "PGP path has no file name"))?;

    let (temp_path, mut file) = loop {
        let sequence = sequence.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(file_name);
        name.push(format!(".arccad-{}-{sequence}.tmp", std::process::id()));
        let candidate: PathBuf = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };

    if let Err(error) = stage(AtomicStage::Created) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = file.write_all(bytes) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = stage(AtomicStage::Written) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = file.sync_all() {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = stage(AtomicStage::Synced) {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    drop(file);
    if let Err(error) = stage(AtomicStage::Closed) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = stage(AtomicStage::Renaming) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

/// Project-owned default aliases. Every target is tested as a canonical command.
#[must_use]
pub fn standard_aliases() -> &'static [(&'static str, &'static str)] {
    &[
        ("A", "ARC"),
        ("AA", "AREA"),
        ("AL", "ALIGN"),
        ("AR", "ARRAY"),
        ("BR", "BREAK"),
        ("C", "CIRCLE"),
        ("CHA", "CHAMFER"),
        ("CO", "COPY"),
        ("COL", "COLOR"),
        ("CP", "COPY"),
        ("DI", "DIST"),
        ("E", "ERASE"),
        ("EL", "ELLIPSE"),
        ("EX", "EXTEND"),
        ("F", "FILLET"),
        ("G", "GROUP"),
        ("J", "JOIN"),
        ("L", "LINE"),
        ("LA", "LAYER"),
        ("LEN", "LENGTHEN"),
        ("LI", "LIST"),
        ("LT", "LINETYPE"),
        ("LTS", "LTSCALE"),
        ("LW", "LWEIGHT"),
        ("M", "MOVE"),
        ("MA", "MATCHPROP"),
        ("MEA", "MEASUREGEOM"),
        ("MI", "MIRROR"),
        ("O", "OFFSET"),
        ("PL", "PLINE"),
        ("PO", "POINT"),
        ("POL", "POLYGON"),
        ("PU", "PURGE"),
        ("REC", "RECTANG"),
        ("REN", "RENAME"),
        ("RO", "ROTATE"),
        ("S", "STRETCH"),
        ("SC", "SCALE"),
        ("TR", "TRIM"),
        ("U", "UNDO"),
        ("UN", "UNITS"),
        ("X", "EXPLODE"),
        ("XL", "XLINE"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_parser_accepts_contractual_whitespace_and_eol() {
        let parsed = parse_pgp("\u{feff} ; comment\r\n  l\u{ed}nea  ,  *  LINE  \n").unwrap();
        assert_eq!(parsed.aliases, [("l\u{ed}nea".into(), "LINE".into())]);
        assert_eq!(parsed.lines, [2]);
    }

    #[test]
    fn strict_parser_rejects_every_non_row_form() {
        for (content, cause) in [
            ("A,*LINE,extra", "exactamente una coma"),
            ("A,LINE", "shell"),
            ("A,*", "comando vacio"),
            (",*LINE", "alias vacio"),
            ("A,*LINE ; no", "inline"),
            ("A,*LI NE", "malformado"),
            ("A,*LINE\rB,*LINE", "CR desnudo"),
            ("A,*LINE\n\u{feff}B,*LINE", "BOM"),
        ] {
            assert!(parse_pgp(content).unwrap_err().cause.contains(cause));
        }
    }

    #[test]
    fn duplicate_uses_contractual_unicode_key_without_nfc() {
        assert!(parse_pgp("stra\u{df}e,*LINE\nSTRASSE,*LINE").is_err());
        let distinct = parse_pgp("\u{e9},*LINE\ne\u{301},*LINE").unwrap();
        assert_eq!(distinct.aliases.len(), 2);
    }

    fn test_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "arccad-p1-006bu-{label}-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    fn assert_no_own_temp(root: &Path, destination: &Path) {
        let prefix = format!(
            ".{}.arccad-",
            destination.file_name().unwrap().to_string_lossy()
        );
        assert!(
            fs::read_dir(root)
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| { !entry.file_name().to_string_lossy().starts_with(&prefix) })
        );
    }

    #[test]
    fn every_atomic_stage_failure_preserves_destination_and_cleans_own_temp() {
        let root = test_dir("atomic-stages");
        let destination = root.join("aliases.pgp");
        let sentinel = root.join("foreign.tmp");
        fs::write(&destination, b"ORIGINAL,*LINE").unwrap();
        fs::write(&sentinel, b"untouched").unwrap();

        for failed in [
            AtomicStage::Created,
            AtomicStage::Written,
            AtomicStage::Synced,
            AtomicStage::Closed,
            AtomicStage::Renaming,
        ] {
            let sequence = AtomicU64::new(0);
            let mut observed = false;
            let result =
                write_atomic_with(&destination, b"REPLACEMENT,*MOVE", &sequence, |stage| {
                    if stage == failed {
                        observed = true;
                        Err(io::Error::other(format!("injected {stage:?}")))
                    } else {
                        Ok(())
                    }
                });
            assert!(result.is_err());
            assert!(observed);
            assert_eq!(fs::read(&destination).unwrap(), b"ORIGINAL,*LINE");
            assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
            assert_no_own_temp(&root, &destination);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn temp_collision_keeps_foreign_file_and_uses_the_next_counter() {
        let root = test_dir("temp-collision");
        let destination = root.join("aliases.pgp");
        fs::write(&destination, b"ORIGINAL,*LINE").unwrap();
        let counter = 41;
        let collision = root.join(format!(
            ".aliases.pgp.arccad-{}-{counter}.tmp",
            std::process::id()
        ));
        fs::write(&collision, b"foreign sentinel").unwrap();
        let sequence = AtomicU64::new(counter);

        write_atomic_with(&destination, b"REPLACEMENT,*MOVE", &sequence, |_| Ok(())).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"REPLACEMENT,*MOVE");
        assert_eq!(fs::read(&collision).unwrap(), b"foreign sentinel");
        assert_eq!(sequence.load(Ordering::Relaxed), counter + 2);
        let next_temp = root.join(format!(
            ".aliases.pgp.arccad-{}-{}.tmp",
            std::process::id(),
            counter + 1
        ));
        assert!(!next_temp.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_rename_removes_only_its_own_temp() {
        let root = test_dir("rename-failure");
        let destination = root.join("aliases.pgp");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("sentinel"), b"untouched").unwrap();
        fs::write(root.join("foreign.tmp"), b"untouched").unwrap();

        assert!(write_atomic(&destination, b"candidate").is_err());
        assert_eq!(
            fs::read(destination.join("sentinel")).unwrap(),
            b"untouched"
        );
        assert_eq!(fs::read(root.join("foreign.tmp")).unwrap(), b"untouched");
        assert_no_own_temp(&root, &destination);
        fs::remove_dir_all(root).unwrap();
    }
}
