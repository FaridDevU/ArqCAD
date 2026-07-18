//! Valid and invalid command-line parser forms, including malformed numeric and
//! coordinate input.

use af_cmd::{ParamType, ParsedInput, parse_input};
use af_math::{Point2, Vec2};

const ORIGIN: Point2 = Point2::ORIGIN;

// --- Command names and aliases ------------------------------------------------

#[test]
fn command_by_name_and_alias() {
    assert_eq!(
        parse_input("LINE", &ParamType::Point, None).unwrap(),
        ParsedInput::Command("LINE".to_string())
    );
    // Parsing preserves trimmed spelling; registry lookup normalizes case.
    assert_eq!(
        parse_input("line", &ParamType::Point, None).unwrap(),
        ParsedInput::Command("line".to_string())
    );
    assert_eq!(
        parse_input("  L  ", &ParamType::Point, None).unwrap(),
        ParsedInput::Command("L".to_string())
    );
}

// --- Absolute point x,y -------------------------------------------------------

#[test]
fn absolute_point() {
    assert_eq!(
        parse_input("10,20", &ParamType::Point, None).unwrap(),
        ParsedInput::Point(Point2::new(10.0, 20.0))
    );
    assert_eq!(
        parse_input("1.5,-2.25", &ParamType::Point, None).unwrap(),
        ParsedInput::Point(Point2::new(1.5, -2.25))
    );
    assert_eq!(
        parse_input("0,0", &ParamType::Point, Some(Point2::new(9.0, 9.0))).unwrap(),
        ParsedInput::Point(Point2::ORIGIN)
    );
}

// --- Relative point @Δx,Δy ----------------------------------------------------

#[test]
fn relative_point() {
    assert_eq!(
        parse_input("@5,3", &ParamType::Point, Some(ORIGIN)).unwrap(),
        ParsedInput::RelativePoint(Vec2::new(5.0, 3.0))
    );
    let base = Point2::new(1.0, 1.0);
    let resolved = parse_input("@5,3", &ParamType::Point, Some(base))
        .unwrap()
        .resolve_point(base)
        .unwrap();
    assert_eq!(resolved, Point2::new(6.0, 4.0));
}

#[test]
fn relative_point_requires_reference() {
    let err = parse_input("@5,3", &ParamType::Point, None).unwrap_err();
    assert_eq!(err.pos, 0);
}

// --- Polar point @distance<angle ---------------------------------------------

#[test]
fn polar_point_degrees_to_radians() {
    let got = parse_input("@10<45", &ParamType::Point, Some(ORIGIN)).unwrap();
    match got {
        ParsedInput::PolarPoint { dist, angle_rad } => {
            assert_eq!(dist, 10.0);
            assert_eq!(angle_rad, 45.0_f64.to_radians());
        }
        other => panic!("esperaba PolarPoint, fue {other:?}"),
    }
    let resolved = parse_input("@10<45", &ParamType::Point, Some(ORIGIN))
        .unwrap()
        .resolve_point(ORIGIN)
        .unwrap();
    let expected = 10.0 * 45.0_f64.to_radians().cos();
    assert!((resolved.x - expected).abs() < 1e-12);
    assert!((resolved.y - expected).abs() < 1e-12);
}

// --- Prompt-dependent scalar --------------------------------------------------

#[test]
fn scalar_number() {
    assert_eq!(
        parse_input("7.5", &ParamType::Distance, None).unwrap(),
        ParsedInput::Number(7.5)
    );
    match parse_input("90", &ParamType::Angle, None).unwrap() {
        ParsedInput::Number(n) => assert_eq!(n, 90.0_f64.to_radians()),
        other => panic!("esperaba Number, fue {other:?}"),
    }
}

#[test]
fn bare_number_is_not_a_point() {
    assert!(parse_input("7.5", &ParamType::Point, None).is_err());
}

// --- Empty input --------------------------------------------------------------

#[test]
fn empty_is_enter() {
    assert_eq!(
        parse_input("", &ParamType::Point, None).unwrap(),
        ParsedInput::Empty
    );
    assert_eq!(
        parse_input("   \t ", &ParamType::Distance, None).unwrap(),
        ParsedInput::Empty
    );
}

// --- Commas separate coordinates, never decimals -----------------------------

#[test]
fn comma_is_never_a_decimal_separator() {
    assert_eq!(
        parse_input("7,5", &ParamType::Distance, None).unwrap(),
        ParsedInput::Point(Point2::new(7.0, 5.0))
    );
    assert_ne!(
        parse_input("7,5", &ParamType::Distance, None).unwrap(),
        ParsedInput::Number(7.5)
    );
}

// --- Literal text prompts -----------------------------------------------------

#[test]
fn text_prompt_is_verbatim() {
    assert_eq!(
        parse_input("10,20", &ParamType::Text, None).unwrap(),
        ParsedInput::Text("10,20".to_string())
    );
    assert_eq!(
        parse_input("Sala de máquinas", &ParamType::Text, None).unwrap(),
        ParsedInput::Text("Sala de máquinas".to_string())
    );
}

// --- Enum prompt keywords -----------------------------------------------------

#[test]
fn enum_prompt_yields_option_keyword() {
    let ty = ParamType::Enum(vec!["extents".to_string(), "window".to_string()]);
    assert_eq!(
        parse_input("window", &ty, None).unwrap(),
        ParsedInput::Option("window".to_string())
    );
}

// --- Malformed input ----------------------------------------------------------

#[test]
fn invalid_missing_y_coordinate() {
    let err = parse_input("10,", &ParamType::Point, None).unwrap_err();
    assert_eq!(err.pos, 3);
}

#[test]
fn invalid_polar_missing_distance() {
    let err = parse_input("@<45", &ParamType::Point, Some(ORIGIN)).unwrap_err();
    assert_eq!(err.pos, 1);
}

#[test]
fn invalid_non_finite_number() {
    let err = parse_input("1e999", &ParamType::Distance, None).unwrap_err();
    assert_eq!(err.pos, 0);
    assert!(parse_input("1e999", &ParamType::Point, None).is_err());
}

#[test]
fn invalid_double_dash() {
    let err = parse_input("--", &ParamType::Distance, None).unwrap_err();
    assert_eq!(err.pos, 0);
}

#[test]
fn invalid_too_many_coordinates() {
    assert!(parse_input("1,2,3", &ParamType::Point, None).is_err());
}

#[test]
fn invalid_comma_inside_coordinate_half() {
    let err = parse_input("1.5.2,3", &ParamType::Point, None).unwrap_err();
    assert_eq!(err.pos, 0);
}
