//! Integration tests for `.pat` parsing, warnings, and the built-in pattern library.
//!
//! Tests use only the public `af_model` surface and remain independent of
//! `Document` and `TxContext`.

use af_model::{HatchPattern, PatFamily, parse_pat, standard_patterns};

const FIXTURE: &str = include_str!("fixtures/arcforge_sample.pat");

#[test]
fn parses_the_arccad_fixture() {
    let parsed = parse_pat(FIXTURE);

    // Skip empty EMPTY_DEF and retain the final LINE45 definition.
    let names: Vec<&str> = parsed.defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["CROSSHATCH", "DOTGRID", "PARTIALLY_BROKEN", "LINE45"]
    );

    let line45 = parsed.defs.iter().find(|d| d.name == "LINE45").unwrap();
    assert_eq!(
        line45.description,
        "ArcCAD 45-degree hatch redefined for the fixture"
    );
    assert_eq!(
        line45.families,
        vec![PatFamily {
            angle_rad: 45.0_f64.to_radians(),
            origin: (1.0, 1.0),
            delta: (0.0, 0.2),
            dashes: vec![],
        }]
    );

    let crosshatch = parsed.defs.iter().find(|d| d.name == "CROSSHATCH").unwrap();
    assert_eq!(crosshatch.families.len(), 2);
    assert_eq!(crosshatch.families[0].angle_rad, 0.0);
    assert_eq!(
        crosshatch.families[1].angle_rad,
        std::f64::consts::FRAC_PI_2
    );
    assert_eq!(crosshatch.families[0].delta, (0.0, 0.25));

    let dotgrid = parsed.defs.iter().find(|d| d.name == "DOTGRID").unwrap();
    assert_eq!(dotgrid.families.len(), 1);
    assert_eq!(dotgrid.families[0].dashes, vec![0.0, -0.125]);

    // PARTIALLY_BROKEN keeps its valid 0- and 60-degree families.
    let partial = parsed
        .defs
        .iter()
        .find(|d| d.name == "PARTIALLY_BROKEN")
        .unwrap();
    assert_eq!(partial.families.len(), 2);
    assert_eq!(partial.families[0].angle_rad, 0.0);
    assert_eq!(partial.families[1].angle_rad, 60.0_f64.to_radians());
}

#[test]
fn fixture_warnings_cover_the_broken_family_the_empty_def_and_the_duplicate_name() {
    let parsed = parse_pat(FIXTURE);
    assert_eq!(parsed.warnings.len(), 3, "warnings: {:?}", parsed.warnings);

    let broken_family_warning = parsed
        .warnings
        .iter()
        .find(|w| w.contains("PARTIALLY_BROKEN"))
        .expect("aviso de familia inválida para PARTIALLY_BROKEN");
    assert!(broken_family_warning.contains("inválida"));

    let empty_def_warning = parsed
        .warnings
        .iter()
        .find(|w| w.contains("EMPTY_DEF"))
        .expect("aviso de definición sin familias para EMPTY_DEF");
    assert!(empty_def_warning.contains("omitida"));

    let dup_warning = parsed
        .warnings
        .iter()
        .find(|w| w.contains("duplicado"))
        .expect("aviso de nombre duplicado");
    assert!(dup_warning.contains("LINE45"));
}

#[test]
fn warnings_are_reported_in_file_order() {
    let parsed = parse_pat(FIXTURE);
    // Warnings follow source order: malformed family, empty definition, duplicate.
    assert!(parsed.warnings[0].contains("PARTIALLY_BROKEN"));
    assert!(parsed.warnings[1].contains("EMPTY_DEF"));
    assert!(parsed.warnings[2].contains("duplicado"));
}

#[test]
fn standard_patterns_are_documented_as_own_definitions_and_parse_clean() {
    let defs: Vec<HatchPattern> = standard_patterns();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["SOLID", "ANSI31", "GRID", "DOTS", "BRICK"]);

    // SOLID uses a normal family with an empty dash sequence; fill rendering
    // recognizes its name rather than drawing literal lines.
    let solid = defs.iter().find(|d| d.name == "SOLID").unwrap();
    assert_eq!(solid.families.len(), 1);
    assert!(solid.families[0].dashes.is_empty());

    let ansi31 = defs.iter().find(|d| d.name == "ANSI31").unwrap();
    assert_eq!(ansi31.families[0].angle_rad, 45.0_f64.to_radians());
}
