//! End-to-end PGP parsing, alias-precedence integration, and standard-alias target tests.

use af_cmd::builtin::register_builtins;
use af_cmd::{CommandRegistry, parse_pgp, standard_aliases};

/// Project-authored PGP fixture covering comments, shell entries, dashed commands,
/// and last-definition-wins aliases.
const FIXTURE: &str = r#"
; ArcCAD test PGP fixture (not a real acad.pgp file)
; Native command aliases
L,*LINE
C,*CIRCLE
CO,*COPY
CP,*COPY

; Command-line variant (leading hyphen): it is not trimmed.
-B,*-BLOCK

; External shell command: ArcCAD never executes a shell, so this entry is ignored with a warning.
NOTEPAD,NOTEPAD,0,*Editar archivo externo:,

; "CO" is redefined below; the last entry wins, matching AutoCAD semantics.
CO,*ROTATE

; The blank and malformed lines below are ignored with warnings.

SINCOMA
"#;

#[test]
fn fixture_parses_expected_aliases_and_warnings() {
    let parsed = parse_pgp(FIXTURE);

    // A duplicate keeps first-appearance order but uses its last value.
    assert_eq!(
        parsed.aliases,
        vec![
            ("L".to_string(), "LINE".to_string()),
            ("C".to_string(), "CIRCLE".to_string()),
            ("CO".to_string(), "ROTATE".to_string()),
            ("CP".to_string(), "COPY".to_string()),
            ("-B".to_string(), "-BLOCK".to_string()),
        ]
    );

    assert_eq!(parsed.warnings.len(), 3);
    assert!(parsed.warnings.iter().any(|w| w.contains("CO")));
    assert!(
        parsed
            .warnings
            .iter()
            .any(|w| w.contains("NOTEPAD") && w.contains("shell"))
    );
    assert!(parsed.warnings.iter().any(|w| w.contains("SINCOMA")));
}

#[test]
fn fixture_applied_to_default_registry_respects_precedence() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("los builtins no colisionan entre sí");

    assert_eq!(reg.lookup("CO").unwrap().name(), "COPY");
    assert_eq!(reg.lookup("C").unwrap().name(), "CIRCLE");

    let parsed = parse_pgp(FIXTURE);
    let apply_warnings = reg.apply_user_aliases(parsed.aliases);

    assert_eq!(apply_warnings.len(), 1);
    assert!(apply_warnings[0].contains("-B"));

    assert_eq!(reg.lookup("CO").unwrap().name(), "ROTATE");
    assert_eq!(reg.lookup("CP").unwrap().name(), "COPY");
    assert_eq!(reg.lookup("C").unwrap().name(), "CIRCLE");
    assert_eq!(reg.lookup("L").unwrap().name(), "LINE");
    assert_eq!(reg.lookup("COPY").unwrap().name(), "COPY");
    assert_eq!(reg.lookup("ROTATE").unwrap().name(), "ROTATE");
    assert!(reg.lookup("-B").is_none());
}

#[test]
fn pgp_alias_cannot_shadow_a_canonical_command_name_in_the_real_registry() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("los builtins no colisionan entre sí");

    let parsed = parse_pgp("MOVE,*COPY\n");
    let warnings = reg.apply_user_aliases(parsed.aliases);
    assert_eq!(warnings.len(), 1);
    assert_eq!(reg.lookup("MOVE").unwrap().name(), "MOVE");
}

#[test]
fn replacing_pgp_aliases_is_complete_unicode_aware_and_preserves_precedence() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("los builtins no colisionan entre sí");

    let parsed = parse_pgp(
        "\u{feff}C,*COPY\nlínea,*LINE\nstraße,*LINE\nCIRCLE,*COPY\nHUERFANO,*NO_EXISTE\n",
    );
    assert!(parsed.warnings.is_empty());
    assert!(parsed.aliases.iter().any(|(alias, _)| alias == "LÍNEA"));
    assert!(parsed.aliases.iter().any(|(alias, _)| alias == "STRASSE"));

    let warnings = reg.replace_user_aliases(parsed.aliases);
    assert_eq!(warnings.len(), 2);
    assert!(warnings.iter().any(|warning| warning.contains("CIRCLE")));
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("HUERFANO") && warning.contains("desconocido"))
    );
    assert_eq!(reg.user_alias_count(), 3);
    assert_eq!(reg.resolve_canonical_name("C"), Some("COPY"));
    assert_eq!(reg.resolve_canonical_name("línea"), Some("LINE"));
    assert_eq!(reg.resolve_canonical_name("straße"), Some("LINE"));
    assert_eq!(reg.resolve_canonical_name("CIRCLE"), Some("CIRCLE"));

    let warnings = reg.replace_user_aliases([("MOVER", "MOVE")]);
    assert!(warnings.is_empty());
    assert_eq!(reg.user_alias_count(), 1);
    assert_eq!(reg.resolve_canonical_name("C"), Some("CIRCLE"));
    assert_eq!(reg.resolve_canonical_name("línea"), None);
    assert_eq!(reg.resolve_canonical_name("straße"), None);
    assert_eq!(reg.resolve_canonical_name("MOVER"), Some("MOVE"));
}

// ---- standard_aliases() target validity -------------------------------------

#[test]
fn standard_aliases_targets_exist_in_default_registry() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("los builtins no colisionan entre sí");

    for (alias, target) in standard_aliases() {
        let spec = reg.lookup(target).unwrap_or_else(|| {
            panic!(
                "standard_aliases(): el destino '{target}' (alias '{alias}') no existe en el registry por defecto"
            )
        });
        // Every table target must be canonical rather than another alias.
        assert_eq!(
            spec.name(),
            *target,
            "standard_aliases(): el destino '{target}' no es el nombre canónico de ningún comando (resolvió a '{}')",
            spec.name()
        );
    }
}

#[test]
fn standard_aliases_can_be_applied_to_the_default_registry_without_warnings() {
    let mut reg = CommandRegistry::new();
    register_builtins(&mut reg).expect("los builtins no colisionan entre sí");

    let warnings = reg.apply_user_aliases(
        standard_aliases()
            .iter()
            .map(|(alias, target)| (*alias, *target)),
    );
    assert!(
        warnings.is_empty(),
        "standard_aliases() no debería generar warnings contra el registry por defecto: {warnings:?}"
    );

    assert_eq!(reg.lookup("L").unwrap().name(), "LINE");
    assert_eq!(reg.lookup("c").unwrap().name(), "CIRCLE");
    assert_eq!(reg.lookup("UN").unwrap().name(), "UNITS");
}
