//! Parser for the `acad.pgp`-compatible **PGP** command-alias format and
//! ArcForge's [`standard_aliases`] table.
//!
//! # PGP format
//!
//! The text file contains one entry per line:
//!
//! - Blank lines and lines beginning with `;` after trimming are ignored.
//! - Native aliases use `<Alias>,*<Command>`. A leading dash in `<Command>` is a
//!   valid command-line variant and is preserved.
//! - External shell commands omit the `*`. ArcForge never executes them; it skips
//!   the line and emits a warning.
//! - Lines without a comma or with an empty alias are skipped with a warning.
//!
//! Parsing is line-oriented and uses only standard-library string operations.
//!
//! Aliases and targets are normalized to uppercase, and an optional UTF-8 BOM is
//! accepted at the start of the content.
//!
//! Duplicate aliases use last-definition-wins semantics and emit a warning.

/// The result of parsing a PGP file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PgpParse {
    /// Uppercase `(ALIAS, COMMAND)` pairs without duplicate keys. Ordering follows
    /// each alias's first appearance, while its value comes from the last definition.
    pub aliases: Vec<(String, String)>,
    /// Nonfatal warnings for skipped shell commands, duplicates, and malformed lines.
    pub warnings: Vec<String>,
}

/// Parses PGP file content.
///
/// Target existence is validated later by `CommandRegistry::apply_user_aliases`.
#[must_use]
pub fn parse_pgp(content: &str) -> PgpParse {
    let mut aliases: Vec<(String, String)> = Vec::new();
    // Index aliases by output position to update duplicates without rescanning.
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    for (offset, raw_line) in content.lines().enumerate() {
        let line_no = offset + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        // Keep the remainder raw to distinguish native aliases from shell commands.
        let Some(comma) = line.find(',') else {
            warnings.push(format!(
                "línea {line_no}: sin coma, formato PGP inválido, ignorada: '{line}'"
            ));
            continue;
        };
        let alias_raw = line[..comma].trim();
        let rest = &line[comma + 1..];

        if alias_raw.is_empty() {
            warnings.push(format!(
                "línea {line_no}: alias vacío, línea ignorada: '{line}'"
            ));
            continue;
        }
        let alias = alias_raw.to_uppercase();

        // Native command fields begin with `*`; external executable fields do not.
        let cmd_field = rest.split(',').next().unwrap_or("").trim();

        match cmd_field.strip_prefix('*') {
            Some(cmd_raw) => {
                let cmd = cmd_raw.trim();
                if cmd.is_empty() {
                    warnings.push(format!(
                        "línea {line_no}: comando nativo vacío para alias '{alias}', ignorada"
                    ));
                    continue;
                }
                let cmd = cmd.to_uppercase();
                match index.get(&alias) {
                    Some(&pos) => {
                        warnings.push(format!(
                            "línea {line_no}: alias duplicado '{alias}' (redefine '{}' → '{cmd}'); el último gana",
                            aliases[pos].1
                        ));
                        aliases[pos].1 = cmd;
                    }
                    None => {
                        index.insert(alias.clone(), aliases.len());
                        aliases.push((alias, cmd));
                    }
                }
            }
            None => {
                // ArcForge never executes shell entries; skip them explicitly.
                warnings.push(format!(
                    "línea {line_no}: comando externo de shell ignorado (ArcForge nunca ejecuta shell): '{alias_raw}'"
                ));
            }
        }
    }

    PgpParse { aliases, warnings }
}

/// Standard AutoCAD-compatible aliases for commands in ArcForge's default registry.
///
/// This project-owned table records functional command conventions rather than
/// copying any third-party PGP file. It includes only implemented commands.
///
/// `standard_aliases_targets_exist_in_default_registry` verifies that every
/// target remains a canonical registered command name.
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
    fn parses_native_alias_lines() {
        let parsed = parse_pgp("L,*LINE\nC,*CIRCLE\n");
        assert_eq!(
            parsed.aliases,
            vec![
                ("L".to_string(), "LINE".to_string()),
                ("C".to_string(), "CIRCLE".to_string()),
            ]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn preserves_leading_dash_in_command_line_variant() {
        let parsed = parse_pgp("-B,*-BLOCK\n");
        assert_eq!(
            parsed.aliases,
            vec![("-B".to_string(), "-BLOCK".to_string())]
        );
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let parsed = parse_pgp("; comentario\n\nL,*LINE\n   ; otro comentario indentado\n");
        assert_eq!(parsed.aliases, vec![("L".to_string(), "LINE".to_string())]);
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn ignores_external_shell_commands_with_warning() {
        let parsed = parse_pgp("NOTEPAD,NOTEPAD,0,*Editar archivo:,\nL,*LINE\n");
        assert_eq!(parsed.aliases, vec![("L".to_string(), "LINE".to_string())]);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("NOTEPAD"));
        assert!(parsed.warnings[0].contains("shell"));
    }

    #[test]
    fn last_duplicate_alias_wins_with_warning() {
        let parsed = parse_pgp("L,*LINE\nL,*LENGTHEN\n");
        assert_eq!(
            parsed.aliases,
            vec![("L".to_string(), "LENGTHEN".to_string())]
        );
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains('L'));
    }

    #[test]
    fn malformed_lines_are_ignored_with_warning() {
        let parsed = parse_pgp("SINCOMA\n,*LINE\nL,*\n");
        assert!(parsed.aliases.is_empty());
        assert_eq!(parsed.warnings.len(), 3);
    }

    #[test]
    fn normalizes_alias_and_command_to_uppercase() {
        let parsed = parse_pgp("l,*line\n");
        assert_eq!(parsed.aliases, vec![("L".to_string(), "LINE".to_string())]);
    }

    #[test]
    fn standard_aliases_has_no_duplicate_keys() {
        let table = standard_aliases();
        let mut seen = std::collections::HashSet::new();
        for (alias, _) in table {
            assert!(seen.insert(*alias), "alias duplicado en la tabla: {alias}");
        }
    }
}
