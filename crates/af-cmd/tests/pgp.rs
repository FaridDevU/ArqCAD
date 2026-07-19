use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use af_cmd::builtin::register_builtins;
use af_cmd::{
    CommandRegistry, PgpEdit, PgpEditError, PgpLayer, parse_pgp, parse_pgp_layer, standard_aliases,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "arccad-p1-006bu-pgp-test-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("unique test directory");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn registry() -> CommandRegistry {
    let mut registry = CommandRegistry::new();
    register_builtins(&mut registry).expect("builtins");
    registry
}

#[test]
fn parser_is_strict_and_diagnostics_name_layer_line_and_cause() {
    let accepted = parse_pgp_layer(
        PgpLayer::Project,
        "\u{feff}\r\n  ; comment \r\n  l\u{ed}nea  ,  *  LINE  \n",
    )
    .unwrap();
    assert_eq!(accepted.aliases, [("l\u{ed}nea".into(), "LINE".into())]);

    for (content, line, cause) in [
        ("A,*LINE,extra", 1, "exactamente una coma"),
        ("A,LINE", 1, "shell"),
        ("A,*", 1, "comando vacio"),
        (",*LINE", 1, "alias vacio"),
        ("; ok\nA,*LINE ; no", 2, "inline"),
        ("A,*LINE\rB,*LINE", 1, "CR desnudo"),
        ("A,*LINE\n\u{feff}B,*LINE", 2, "BOM"),
        ("A,*LINE\na,*MOVE", 2, "duplicado"),
    ] {
        let error = parse_pgp_layer(PgpLayer::Project, content).unwrap_err();
        assert_eq!(error.layer, PgpLayer::Project);
        assert_eq!(error.line, line);
        assert!(error.cause.contains(cause), "{error}");
        assert!(error.to_string().starts_with("PGP project linea "));
    }
}

#[test]
fn parser_reports_the_first_line_before_a_later_bare_cr() {
    let error = parse_pgp("BROKEN\nA,*LINE\rB,*MOVE").unwrap_err();
    assert_eq!(error.layer, PgpLayer::User);
    assert_eq!(error.line, 1);
    assert_eq!(error.cause, "fila activa requiere exactamente una coma");

    let error = parse_pgp("A,*LINE\nB,*MOVE\rC,*COPY").unwrap_err();
    assert_eq!(error.layer, PgpLayer::User);
    assert_eq!(error.line, 2);
    assert_eq!(error.cause, "CR desnudo no permitido");
}

#[test]
fn case_key_is_locale_independent_but_does_not_normalize_unicode() {
    assert!(parse_pgp("stra\u{df}e,*LINE\nSTRASSE,*MOVE").is_err());
    let parsed = parse_pgp("\u{e9},*LINE\ne\u{301},*MOVE").unwrap();
    assert_eq!(parsed.aliases.len(), 2);

    let mut registry = registry();
    registry
        .replace_pgp_layers("", "\u{e9},*LINE\ne\u{301},*MOVE", "", "")
        .unwrap();
    assert_eq!(registry.resolve_canonical_name("\u{c9}"), Some("LINE"));
    assert_eq!(registry.resolve_canonical_name("E\u{301}"), Some("MOVE"));
}

#[test]
fn editor_preserves_bom_untouched_bytes_and_mixed_endings() {
    let temp = TempDir::new();
    let path = temp.join("aliases.pgp");
    let sentinel = temp.join("sentinel.tmp");
    let original = "\u{feff}; head\r\n  Alfa  ,  *  LINE  \n; keep\r\nB,*CIRCLE";
    fs::write(&path, original.as_bytes()).unwrap();
    fs::write(&sentinel, b"do not touch").unwrap();
    let registry = registry();

    registry
        .edit_pgp_file(
            &path,
            PgpLayer::User,
            PgpEdit::Update {
                alias: "ALFA",
                target: "MOVE",
            },
        )
        .unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        "\u{feff}; head\r\n  Alfa  ,  *  MOVE  \n; keep\r\nB,*CIRCLE".as_bytes()
    );

    registry
        .edit_pgp_file(
            &path,
            PgpLayer::User,
            PgpEdit::Add {
                alias: "NUEVO",
                target: "COPY",
            },
        )
        .unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        "\u{feff}; head\r\n  Alfa  ,  *  MOVE  \n; keep\r\nB,*CIRCLE\r\nNUEVO,*COPY".as_bytes()
    );

    registry
        .edit_pgp_file(&path, PgpLayer::User, PgpEdit::Delete { alias: "b" })
        .unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        "\u{feff}; head\r\n  Alfa  ,  *  MOVE  \n; keep\r\nNUEVO,*COPY".as_bytes()
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"do not touch");
    assert_no_editor_temps(&temp.0, &path);
}

fn assert_no_editor_temps(directory: &Path, destination: &Path) {
    let prefix = format!(
        ".{}.arccad-",
        destination.file_name().unwrap().to_string_lossy()
    );
    assert!(
        fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(&prefix))
    );
}

#[test]
fn editor_preconditions_and_invalid_candidates_are_fail_closed() {
    let temp = TempDir::new();
    let path = temp.join("aliases.pgp");
    let registry = registry();
    fs::write(&path, b"A1,*LINE\n").unwrap();
    let original = fs::read(&path).unwrap();

    let cases = [
        PgpEdit::Add {
            alias: "a1",
            target: "MOVE",
        },
        PgpEdit::Update {
            alias: "MISSING",
            target: "MOVE",
        },
        PgpEdit::Delete { alias: "MISSING" },
        PgpEdit::Update {
            alias: "A1",
            target: "NO_SUCH_COMMAND",
        },
        PgpEdit::Update {
            alias: "A1",
            target: "L",
        },
    ];
    for edit in cases {
        assert!(registry.edit_pgp_file(&path, PgpLayer::User, edit).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_no_editor_temps(&temp.0, &path);
    }

    fs::write(&path, b"bad row").unwrap();
    let invalid = fs::read(&path).unwrap();
    assert!(matches!(
        registry.edit_pgp_file(
            &path,
            PgpLayer::User,
            PgpEdit::Add {
                alias: "B",
                target: "LINE"
            }
        ),
        Err(PgpEditError::Invalid(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), invalid);
}

#[test]
fn editor_reports_semantic_prefix_before_later_syntax_without_writing() {
    let temp = TempDir::new();
    let path = temp.join("aliases.pgp");
    let original = b"LINE,*MOVE\nBROKEN";
    fs::write(&path, original).unwrap();
    let registry = registry();

    for edit in [
        PgpEdit::Add {
            alias: "NEW",
            target: "LINE",
        },
        PgpEdit::Update {
            alias: "LINE",
            target: "CIRCLE",
        },
        PgpEdit::Delete { alias: "LINE" },
    ] {
        let PgpEditError::Invalid(error) = registry
            .edit_pgp_file(&path, PgpLayer::System, edit)
            .unwrap_err()
        else {
            panic!("expected semantic PGP error");
        };
        assert_eq!(error.layer, PgpLayer::System);
        assert_eq!(error.line, 1);
        assert_eq!(error.cause, "alias 'LINE' sombrea comando canonico");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_no_editor_temps(&temp.0, &path);
    }
}

#[test]
fn editor_uses_lf_without_existing_terminator_and_reports_builtin_shadow() {
    let temp = TempDir::new();
    let path = temp.join("aliases.pgp");
    fs::write(&path, b"FIRST,*LINE").unwrap();
    let registry = registry();
    let diagnostics = registry
        .edit_pgp_file(
            &path,
            PgpLayer::Session,
            PgpEdit::Add {
                alias: "C",
                target: "MOVE",
            },
        )
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"FIRST,*LINE\nC,*MOVE");
    assert_eq!(
        diagnostics,
        ["PGP session linea 2: alias 'C' reemplaza builtin"]
    );
}

#[test]
fn every_standard_alias_target_is_canonical() {
    let registry = registry();
    for (alias, target) in standard_aliases() {
        assert_eq!(
            registry.resolve_canonical_name(target),
            Some(*target),
            "{alias} -> {target} must be canonical"
        );
    }
}
