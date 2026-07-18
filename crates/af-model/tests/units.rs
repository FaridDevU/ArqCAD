//! Integration tests for `af_model::units`.

use std::f64::consts::PI;

use af_model::units::{
    LinearUnit, ParseValueError, Units, format_angle_deg, format_linear, parse_angle_deg,
    parse_linear,
};

/// `"mm"` maps to `LinearUnit::Mm`, which is also the default.
#[test]
fn linear_unit_serde_strings_y_default() {
    assert_eq!(LinearUnit::default(), LinearUnit::Mm);

    let cases = [
        (LinearUnit::Mm, "\"mm\""),
        (LinearUnit::Cm, "\"cm\""),
        (LinearUnit::M, "\"m\""),
        (LinearUnit::In, "\"in\""),
        (LinearUnit::Ft, "\"ft\""),
        (LinearUnit::Unitless, "\"unitless\""),
    ];

    for (unit, json) in cases {
        let serialized = serde_json::to_string(&unit).unwrap();
        assert_eq!(serialized, json, "serializando {unit:?}");

        let back: LinearUnit = serde_json::from_str(json).unwrap();
        assert_eq!(back, unit, "deserializando {json}");
    }
}

/// `Units` serializes as `{"linear": "<unit>"}` and defaults to millimeters.
#[test]
fn units_serde_roundtrip_y_default() {
    assert_eq!(
        Units::default(),
        Units {
            linear: LinearUnit::Mm
        }
    );

    let u = Units {
        linear: LinearUnit::In,
    };
    let json = serde_json::to_string(&u).unwrap();
    assert_eq!(json, r#"{"linear":"in"}"#);

    let back: Units = serde_json::from_str(&json).unwrap();
    assert_eq!(back, u);
}

/// DXF `$INSUNITS` codes for every variant.
#[test]
fn dxf_insunits_code_por_variante() {
    assert_eq!(LinearUnit::Unitless.dxf_insunits_code(), 0);
    assert_eq!(LinearUnit::In.dxf_insunits_code(), 1);
    assert_eq!(LinearUnit::Ft.dxf_insunits_code(), 2);
    assert_eq!(LinearUnit::Mm.dxf_insunits_code(), 4);
    assert_eq!(LinearUnit::Cm.dxf_insunits_code(), 5);
    assert_eq!(LinearUnit::M.dxf_insunits_code(), 6);
}

/// `to_mm_factor` is absent only for `Unitless`; all others are fixed.
#[test]
fn to_mm_factor_por_variante() {
    assert_eq!(LinearUnit::Mm.to_mm_factor(), Some(1.0));
    assert_eq!(LinearUnit::Cm.to_mm_factor(), Some(10.0));
    assert_eq!(LinearUnit::M.to_mm_factor(), Some(1000.0));
    assert_eq!(LinearUnit::In.to_mm_factor(), Some(25.4));
    assert_eq!(LinearUnit::Ft.to_mm_factor(), Some(304.8));
    assert_eq!(LinearUnit::Unitless.to_mm_factor(), None);
}

/// `format_linear` handles fixed precision, negatives, and zero without suffixes.
#[test]
fn format_linear_precision_negativos_y_ceros() {
    let mm = Units {
        linear: LinearUnit::Mm,
    };
    assert_eq!(format_linear(124.5, mm, 2), "124.50");
    assert_eq!(format_linear(-7.0, mm, 3), "-7.000");
    assert_eq!(format_linear(0.0, mm, 2), "0.00");
    assert_eq!(format_linear(-0.0, mm, 2), "0.00");
    // A negative value rounded to zero must not retain its sign.
    assert_eq!(format_linear(-0.0004, mm, 2), "0.00");
    // Units are metadata, so the same raw value formats identically.
    let inch = Units {
        linear: LinearUnit::In,
    };
    assert_eq!(format_linear(124.5, inch, 2), "124.50");
}

/// `parse_linear` accepts plain numbers and rejects comma decimals and invalid text.
#[test]
fn parse_linear_casos_validos_e_invalidos() {
    assert_eq!(parse_linear("7.25").unwrap(), 7.25);
    assert_eq!(parse_linear("-3.5").unwrap(), -3.5);
    assert_eq!(parse_linear("0").unwrap(), 0.0);
    assert_eq!(parse_linear("  10.0  ").unwrap(), 10.0);

    assert!(matches!(
        parse_linear("7,25"),
        Err(ParseValueError::CommaDecimalSeparator(_))
    ));
    assert!(matches!(
        parse_linear("abc"),
        Err(ParseValueError::NotANumber(_))
    ));

    // Errors provide nonempty messages.
    let err = parse_linear("abc").unwrap_err();
    assert!(!err.to_string().is_empty());
    let err = parse_linear("7,25").unwrap_err();
    assert!(!err.to_string().is_empty());
}

/// Parsing rejects unit suffixes.
#[test]
fn parse_linear_rechaza_sufijos_de_unidad() {
    assert!(parse_linear("10mm").is_err());
    assert!(parse_linear("10 mm").is_err());
    assert!(parse_linear("10in").is_err());
}

/// `NaN` and infinity are invalid linear values.
#[test]
fn parse_linear_rechaza_no_finitos() {
    assert!(matches!(
        parse_linear("NaN"),
        Err(ParseValueError::NotFinite(_))
    ));
    assert!(matches!(
        parse_linear("inf"),
        Err(ParseValueError::NotFinite(_))
    ));
}

/// Angles format in degrees and round-trip through `parse_angle_deg`.
#[test]
fn format_and_parse_angle_deg() {
    assert_eq!(format_angle_deg(PI / 2.0, 1), "90.0");
    assert_eq!(format_angle_deg(0.0, 2), "0.00");
    assert_eq!(format_angle_deg(PI, 0), "180");

    let back = parse_angle_deg("90.0").unwrap();
    assert!((back - PI / 2.0).abs() < 1e-12);

    let back = parse_angle_deg("-45.0").unwrap();
    assert!((back - (-PI / 4.0)).abs() < 1e-12);

    assert!(parse_angle_deg("7,5").is_err());
}

/// `parse(format(x))` approximates `x` within display precision.
#[test]
fn roundtrip_format_parse_linear() {
    let mm = Units {
        linear: LinearUnit::Mm,
    };
    for &x in &[0.0, 124.5, -7.333, 1000.0, -0.005] {
        let s = format_linear(x, mm, 3);
        let back = parse_linear(&s).unwrap();
        assert!((back - x).abs() <= 5e-4, "x={x} s={s} back={back}");
    }
}

/// Degree-to-radian-to-degree round trips stay within display precision.
#[test]
fn roundtrip_format_parse_angle() {
    for &deg in &[0.0_f64, 90.0, -45.0, 179.999, 359.5] {
        let rad = deg.to_radians();
        let s = format_angle_deg(rad, 3);
        let back = parse_angle_deg(&s).unwrap();
        let back_deg = back.to_degrees();
        assert!(
            (back_deg - deg).abs() <= 5e-3,
            "deg={deg} s={s} back_deg={back_deg}"
        );
    }
}
