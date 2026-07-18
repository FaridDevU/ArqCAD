//! Tolerant parser for external `.pat` hatch-pattern files, mirroring
//! [`crate::lin::parse_lin`].
//!
//! Each pattern has a header followed by one or more parallel-line families:
//!
//! ```text
//! *NAME,human-readable description
//! angle,x-origin,y-origin,delta-x,delta-y[,dash1,dash2,...]
//! angle,x-origin,y-origin,delta-x,delta-y[,dash1,dash2,...]
//! ...
//! ```
//!
//! A family defines orientation, origin, translation between parallel lines,
//! and an optional dash pattern. Positive values draw dashes, negative values
//! create gaps, and zero draws points.
//!
//! File angles are degrees; [`PatFamily::angle_rad`] converts them to radians at
//! the input boundary.
//!
//! A malformed family is skipped with a warning. Headers without valid families
//! are discarded. Blank lines and semicolon comments are ignored silently.
//!
//! This module contains only the pure parser and parsed value types.

/// One parallel-line family from a `.pat` definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PatFamily {
    /// Family angle in radians.
    pub angle_rad: f64,
    /// Origin point for the first line.
    pub origin: (f64, f64),
    /// Translation between successive parallel lines.
    pub delta: (f64, f64),
    /// Optional dash pattern; empty means continuous.
    pub dashes: Vec<f64>,
}

/// Parsed `.pat` definition before document insertion.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchPattern {
    /// Name as written after `*`.
    pub name: String,
    /// Optional free-form description.
    pub description: String,
    /// Non-empty parsed line families.
    pub families: Vec<PatFamily>,
}

/// Result of parsing a complete `.pat` file.
///
/// `defs` contains definitions with a valid family; `warnings` describes ignored input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatParse {
    /// Valid definitions ordered by their final header occurrence.
    pub defs: Vec<HatchPattern>,
    /// Nonfatal warnings in input order.
    pub warnings: Vec<String>,
}

/// Header and valid families accumulated for the current definition.
type PendingDef = (String, String, Vec<PatFamily>);

/// Parses `.pat` file contents.
///
/// For case-insensitive duplicates, the last definition wins and adds a warning.
#[must_use]
pub fn parse_pat(content: &str) -> PatParse {
    let mut result = PatParse::default();
    let mut current: Option<PendingDef> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if let Some(rest) = line.strip_prefix('*') {
            finalize_pending(current.take(), &mut result);
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
            current = Some((name, description, Vec::new()));
            continue;
        }

        // Family lines are valid only within an open definition.
        let Some((name, _, families)) = current.as_mut() else {
            result.warnings.push(format!(
                "línea sin cabecera '*NOMBRE' precedente, ignorada: '{line}'"
            ));
            continue;
        };

        match parse_family_line(line) {
            Ok(family) => families.push(family),
            Err(reason) => {
                result.warnings.push(format!(
                    "hatch pattern '{name}': línea de familia inválida ({reason}), familia omitida: '{line}'"
                ));
            }
        }
    }

    finalize_pending(current, &mut result);
    result
}

/// Finishes the current definition, discarding empty ones and applying last-wins.
fn finalize_pending(pending: Option<PendingDef>, result: &mut PatParse) {
    let Some((name, description, families)) = pending else {
        return;
    };

    if families.is_empty() {
        result.warnings.push(format!(
            "hatch pattern '{name}': cabecera sin ninguna línea de familia válida, definición omitida"
        ));
        return;
    }

    if let Some(existing) = result
        .defs
        .iter()
        .position(|d| d.name.eq_ignore_ascii_case(&name))
    {
        result.warnings.push(format!(
            "hatch pattern '{name}' duplicado en el archivo, se conserva la última definición"
        ));
        result.defs.remove(existing);
    }

    result.defs.push(HatchPattern {
        name,
        description,
        families,
    });
}

/// Parses one `angle,x-origin,y-origin,delta-x,delta-y[,dash,...]` family line.
///
/// Returns a readable error for missing or nonnumeric fields.
fn parse_family_line(line: &str) -> Result<PatFamily, String> {
    let mut tokens = line.split(',').map(str::trim);

    let angle_deg = next_f64(&mut tokens, "angle")?;
    let x0 = next_f64(&mut tokens, "x-origin")?;
    let y0 = next_f64(&mut tokens, "y-origin")?;
    let dx = next_f64(&mut tokens, "delta-x")?;
    let dy = next_f64(&mut tokens, "delta-y")?;

    let mut dashes = Vec::new();
    for tok in tokens {
        if tok.is_empty() {
            continue;
        }
        match tok.parse::<f64>() {
            Ok(v) => dashes.push(v),
            Err(_) => return Err(format!("elemento de guiones inválido '{tok}'")),
        }
    }

    Ok(PatFamily {
        angle_rad: angle_deg.to_radians(),
        origin: (x0, y0),
        delta: (dx, dy),
        dashes,
    })
}

/// Parses the next required numeric field with a named error.
fn next_f64<'a>(tokens: &mut impl Iterator<Item = &'a str>, field: &str) -> Result<f64, String> {
    let tok = tokens
        .next()
        .ok_or_else(|| format!("falta el campo '{field}'"))?;
    tok.parse::<f64>()
        .map_err(|_| format!("'{field}' no numérico: '{tok}'"))
}

/// Original `.pat` source for the built-in ArcCAD hatch patterns.
///
/// **Clean-room provenance:** these definitions were written from scratch from
/// public-domain ISO/ANSI conventions. They do not copy values or text from
/// proprietary pattern files; only the interoperable `.pat` format is reused.
///
/// `SOLID` is encoded as a normal definition for parser consistency. Fill engines
/// must interpret its case-insensitive name as a full solid fill.
const STANDARD_PATTERNS_SRC: &str = r#"
; ArcCAD built-in .pat hatch patterns.
; Original definitions derived from public-domain ISO/ANSI conventions.
; These are not copied from acad.pat or zwcad.pat.
; See standard_patterns for interpretation rules.

*SOLID,Relleno solido (caso especial, ver doc de standard_patterns)
0,0,0,0,.001

*ANSI31,Lineas a 45 grados (hachura tipo hierro/ladrillo/piedra)
45,0,0,0,.125

*GRID,Cuadricula ortogonal (dos familias perpendiculares)
0,0,0,0,.25
90,0,0,0,.25

*DOTS,Puntos en cuadricula (dos familias de puntos cruzadas)
0,0,0,0,.125,0,-.125
90,0,0,0,.125,0,-.125

*BRICK,Aparejo de ladrillo a hiladas (ilustrativo, definicion propia)
0,0,0,0,.25
90,0,0,.5,.5,.25,-.25
"#;

/// Built-in ArcCAD hatch patterns.
///
/// Parses [`STANDARD_PATTERNS_SRC`] through the same path as user files.
#[must_use]
pub fn standard_patterns() -> Vec<HatchPattern> {
    parse_pat(STANDARD_PATTERNS_SRC).defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_family_definition() {
        let parsed = parse_pat("*FOO,Foo pattern\n45,0,0,0,.125\n");
        assert!(parsed.warnings.is_empty());
        assert_eq!(
            parsed.defs,
            vec![HatchPattern {
                name: "FOO".to_string(),
                description: "Foo pattern".to_string(),
                families: vec![PatFamily {
                    angle_rad: 45.0_f64.to_radians(),
                    origin: (0.0, 0.0),
                    delta: (0.0, 0.125),
                    dashes: vec![],
                }],
            }]
        );
    }

    #[test]
    fn angle_conversion_from_degrees_to_radians_is_exact() {
        let parsed = parse_pat("*FOO\n0,0,0,0,.1\n90,0,0,0,.1\n180,0,0,0,.1\n");
        let angles: Vec<f64> = parsed.defs[0]
            .families
            .iter()
            .map(|f| f.angle_rad)
            .collect();
        assert_eq!(
            angles,
            vec![0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
        );
    }

    #[test]
    fn header_without_description_is_allowed() {
        let parsed = parse_pat("*FOO\n0,0,0,0,.1\n");
        assert_eq!(parsed.defs[0].description, "");
    }

    #[test]
    fn a_definition_may_have_several_families() {
        let parsed = parse_pat("*GRID2,Grid\n0,0,0,0,.25\n90,0,0,0,.25\n");
        assert_eq!(parsed.defs[0].families.len(), 2);
        assert_eq!(
            parsed.defs[0].families[1].angle_rad,
            std::f64::consts::FRAC_PI_2
        );
    }

    #[test]
    fn dashes_are_parsed_in_order_and_absence_means_continuous() {
        let parsed = parse_pat("*DASHY\n0,0,0,0,.2,.3,-.1,0\n*PLAIN\n0,0,0,0,.2\n");
        let dashy = parsed.defs.iter().find(|d| d.name == "DASHY").unwrap();
        assert_eq!(dashy.families[0].dashes, vec![0.3, -0.1, 0.0]);
        let plain = parsed.defs.iter().find(|d| d.name == "PLAIN").unwrap();
        assert!(plain.families[0].dashes.is_empty());
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let parsed = parse_pat(
            "; comentario inicial\n\n*FOO,Foo\n; comentario entre cabecera y familia\n0,0,0,0,.1\n\n",
        );
        assert_eq!(parsed.defs.len(), 1);
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn malformed_family_line_is_skipped_but_definition_survives_with_other_families() {
        let parsed = parse_pat("*FOO,Foo\n0,0,0,0,.1\n30,0,0,not-a-number,.2\n60,0,0,0,.3\n");
        assert_eq!(parsed.defs.len(), 1);
        assert_eq!(parsed.defs[0].families.len(), 2);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("FOO"));
        assert!(parsed.warnings[0].contains("inválida"));
    }

    #[test]
    fn family_line_missing_fields_is_skipped_with_warning() {
        let parsed = parse_pat("*FOO,Foo\n0,0,0\n45,0,0,0,.1\n");
        assert_eq!(parsed.defs[0].families.len(), 1);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("delta-x"));
    }

    #[test]
    fn duplicate_name_case_insensitive_keeps_last_and_warns() {
        let parsed = parse_pat("*FOO,first\n0,0,0,0,.1\n*foo,second\n45,0,0,0,.2\n90,1,1,0,.3\n");
        assert_eq!(parsed.defs.len(), 1);
        assert_eq!(parsed.defs[0].name, "foo");
        assert_eq!(parsed.defs[0].description, "second");
        assert_eq!(parsed.defs[0].families.len(), 2);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("duplicado"));
    }

    #[test]
    fn header_without_any_family_before_next_header_warns_and_is_skipped() {
        let parsed = parse_pat("*FOO,Foo\n*BAR,Bar\n0,0,0,0,.1\n");
        assert_eq!(parsed.defs.len(), 1);
        assert_eq!(parsed.defs[0].name, "BAR");
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("FOO"));
    }

    #[test]
    fn header_without_any_family_at_eof_warns_and_is_skipped() {
        let parsed = parse_pat("*FOO,Foo\n");
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("FOO"));
    }

    #[test]
    fn stray_family_line_without_header_warns() {
        let parsed = parse_pat("0,0,0,0,.1\n");
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn header_without_name_is_ignored_with_warning() {
        // No following family line, so only the missing-name warning is emitted.
        let parsed = parse_pat("*,sin nombre\n");
        assert!(parsed.defs.is_empty());
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("sin nombre"));
    }

    #[test]
    fn empty_content_yields_no_defs_and_no_warnings() {
        let parsed = parse_pat("");
        assert!(parsed.defs.is_empty());
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn standard_patterns_parse_without_warnings_and_cover_the_expected_names() {
        let parsed = parse_pat(STANDARD_PATTERNS_SRC);
        assert!(
            parsed.warnings.is_empty(),
            "standard_patterns no debería producir avisos: {:?}",
            parsed.warnings
        );
        let names: Vec<&str> = parsed.defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["SOLID", "ANSI31", "GRID", "DOTS", "BRICK"]);
    }

    #[test]
    fn standard_patterns_public_helper_matches_direct_parse() {
        let defs = standard_patterns();
        assert_eq!(defs, parse_pat(STANDARD_PATTERNS_SRC).defs);

        let ansi31 = defs.iter().find(|d| d.name == "ANSI31").unwrap();
        assert_eq!(ansi31.families.len(), 1);
        assert_eq!(ansi31.families[0].angle_rad, 45.0_f64.to_radians());

        let grid = defs.iter().find(|d| d.name == "GRID").unwrap();
        assert_eq!(grid.families.len(), 2);
    }
}
