//! Tolerant parser for external `.lin` line-type files.
//!
//! Each definition uses a header and pattern line:
//!
//! ```text
//! *NAME,human-readable description
//! A,e1,e2,e3,...
//! ```
//!
//! `A` is the supported alignment code. Positive elements draw dashes, negative
//! elements create gaps, and zero draws points. Definitions with unsupported
//! complex elements are skipped with a warning rather than loaded incompletely.
//!
//! Malformed lines produce warnings and are ignored. Blank lines and semicolon
//! comments are ignored silently.

/// Parsed `.lin` definition before document insertion and ID assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLinetype {
    /// Name as written after `*`.
    pub name: String,
    /// Optional free-form description.
    pub description: String,
    /// Numeric pattern; empty means continuous.
    pub pattern: Vec<f64>,
}

/// Result of parsing a complete `.lin` file.
///
/// `defs` contains parsed numeric definitions; `warnings` describes ignored input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinParse {
    /// Valid definitions ordered by their final header occurrence.
    pub defs: Vec<ParsedLinetype>,
    /// Nonfatal warnings in input order.
    pub warnings: Vec<String>,
}

/// Parses `.lin` file contents.
///
/// For case-insensitive duplicates, the last definition wins and adds a warning.
#[must_use]
pub fn parse_lin(content: &str) -> LinParse {
    let mut result = LinParse::default();
    // Parsed header awaiting its pattern line.
    let mut pending: Option<(String, String)> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(rest) = line.strip_prefix('*') {
            if let Some((name, _)) = pending.take() {
                result.warnings.push(format!(
                    "linetype '{name}': cabecera sin línea de patrón antes de la siguiente cabecera, definición omitida"
                ));
            }
            let (name, description) = match rest.split_once(',') {
                Some((n, d)) => (n.trim().to_string(), d.trim().to_string()),
                None => (rest.trim().to_string(), String::new()),
            };
            if name.is_empty() {
                result
                    .warnings
                    .push("cabecera '*' sin nombre, línea ignorada".to_string());
                continue;
            }
            pending = Some((name, description));
            continue;
        }

        // A pattern line is valid only immediately after a header.
        let Some((name, description)) = pending.take() else {
            result.warnings.push(format!(
                "línea sin cabecera '*NOMBRE' precedente, ignorada: '{line}'"
            ));
            continue;
        };

        if line.contains('[') {
            result.warnings.push(format!(
                "linetype '{name}': elemento complejo [...] no soportado (v0), definición omitida"
            ));
            continue;
        }

        // The first field is alignment; the model currently stores only the pattern.
        let mut tokens = line.split(',');
        if tokens.next().is_none() {
            result.warnings.push(format!(
                "linetype '{name}': línea de patrón vacía, definición omitida"
            ));
            continue;
        }

        let mut pattern = Vec::new();
        let mut malformed = false;
        for tok in tokens {
            let t = tok.trim();
            if t.is_empty() {
                continue;
            }
            match t.parse::<f64>() {
                Ok(v) => pattern.push(v),
                Err(_) => {
                    result.warnings.push(format!(
                        "linetype '{name}': elemento de patrón inválido '{t}', definición omitida"
                    ));
                    malformed = true;
                    break;
                }
            }
        }
        if malformed {
            continue;
        }

        if let Some(existing) = result
            .defs
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(&name))
        {
            result.warnings.push(format!(
                "linetype '{name}' duplicado en el archivo, se conserva la última definición"
            ));
            result.defs.remove(existing);
        }

        result.defs.push(ParsedLinetype {
            name,
            description,
            pattern,
        });
    }

    if let Some((name, _)) = pending {
        result.warnings.push(format!(
            "linetype '{name}': cabecera sin línea de patrón al final del archivo, definición omitida"
        ));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_two_line_definition() {
        let parsed = parse_lin("*FOO,Foo linetype\nA,0.5,-0.25\n");
        assert!(parsed.warnings.is_empty());
        assert_eq!(
            parsed.defs,
            vec![ParsedLinetype {
                name: "FOO".to_string(),
                description: "Foo linetype".to_string(),
                pattern: vec![0.5, -0.25],
            }]
        );
    }

    #[test]
    fn header_without_description_is_allowed() {
        let parsed = parse_lin("*FOO\nA,0.5,-0.25\n");
        assert_eq!(parsed.defs[0].description, "");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let parsed = parse_lin(
            "; comentario inicial\n\n*FOO,Foo\n; comentario entre cabecera y patrón\nA,0.5,-0.25\n\n",
        );
        assert_eq!(parsed.defs.len(), 1);
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn complex_segment_is_skipped_with_warning() {
        let parsed = parse_lin(
            r#"*GAS_LINE,Gas line ----GAS----GAS----
A,.5,-.2,["GAS",STANDARD,S=.1,R=0.0,X=-0.1,Y=-.05],-.25
"#,
        );
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("GAS_LINE"));
        assert!(parsed.warnings[0].contains("complejo"));
    }

    #[test]
    fn duplicate_name_case_insensitive_keeps_last_and_warns() {
        let parsed = parse_lin("*FOO,first\nA,1.0\n*foo,second\nA,2.0,-1.0\n");
        assert_eq!(parsed.defs.len(), 1);
        assert_eq!(parsed.defs[0].name, "foo");
        assert_eq!(parsed.defs[0].description, "second");
        assert_eq!(parsed.defs[0].pattern, vec![2.0, -1.0]);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("duplicado"));
    }

    #[test]
    fn header_without_pattern_line_before_next_header_warns_and_is_skipped() {
        let parsed = parse_lin("*FOO,Foo\n*BAR,Bar\nA,1.0\n");
        assert_eq!(parsed.defs.len(), 1);
        assert_eq!(parsed.defs[0].name, "BAR");
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("FOO"));
    }

    #[test]
    fn header_without_pattern_at_eof_warns_and_is_skipped() {
        let parsed = parse_lin("*FOO,Foo\n");
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("FOO"));
    }

    #[test]
    fn stray_pattern_line_without_header_warns() {
        let parsed = parse_lin("A,1.0,-0.5\n");
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn invalid_numeric_element_skips_definition_with_warning() {
        let parsed = parse_lin("*FOO,Foo\nA,1.0,not-a-number,-0.5\n");
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("inválido"));
    }

    #[test]
    fn dot_element_zero_is_a_valid_pattern_value() {
        let parsed = parse_lin("*DASHDOT2,Dash dot\nA,0.5,-0.25,0.0,-0.25\n");
        assert_eq!(parsed.defs[0].pattern, vec![0.5, -0.25, 0.0, -0.25]);
    }

    #[test]
    fn empty_content_yields_no_defs_and_no_warnings() {
        let parsed = parse_lin("");
        assert!(parsed.defs.is_empty());
        assert!(parsed.warnings.is_empty());
    }
}
