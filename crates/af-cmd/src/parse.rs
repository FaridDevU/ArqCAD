//! Dependency-free command-line parser.
//!
//! [`parse_input`] converts raw input to [`ParsedInput`] using the expected
//! [`ParamType`] and the last reference point. Keeping it in the core lets the UI,
//! scripts, and plugins share the same parser.
//!
//! # Decimal separator invariant
//!
//! A comma always separates coordinates and is never a decimal separator. `7,5`
//! is point `(7, 5)`, not number `7.5`; decimals always use a period.
//!
//! # Accepted forms
//!
//! | Input      | Result |
//! |------------|-----------|
//! | `LINE` / `l` outside an `Enum` prompt  | [`ParsedInput::Command`] |
//! | word in an `Enum` prompt               | [`ParsedInput::Option`] |
//! | `10,20`    | [`ParsedInput::Point`] |
//! | `@5,3`     | [`ParsedInput::RelativePoint`] (Δx,Δy) |
//! | `@10<45`   | [`ParsedInput::PolarPoint`] (degrees converted to radians) |
//! | `7.5`      | [`ParsedInput::Number`] (prompt-dependent) |
//! | empty      | [`ParsedInput::Empty`] |
//!
//! Relative and polar forms require `last_point`; otherwise parsing returns a
//! [`ParseError`].

use af_math::{Point2, Vec2};
use af_model::units::{parse_angle_deg, parse_linear};

use crate::spec::ParamType;

/// The result of parsing one input line.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedInput {
    /// A case-insensitive command name or alias, resolved by the registry.
    Command(String),
    /// An absolute `(x, y)` point.
    Point(Point2),
    /// A displacement `Δ = (Δx, Δy)` relative to the last point.
    RelativePoint(Vec2),
    /// A polar point relative to the last point, with its angle in radians.
    PolarPoint {
        /// Distance from the reference point.
        dist: f64,
        /// Angle in radians, converted from user-entered degrees.
        angle_rad: f64,
    },
    /// A scalar interpreted by prompt context; angles are stored in radians.
    Number(f64),
    /// Free-form text from a `Text` prompt.
    Text(String),
    /// Empty input, used to repeat a command or accept a default.
    Empty,
    /// A keyword from an `Enum` prompt.
    Option(String),
}

impl ParsedInput {
    /// Resolves point-like input to an absolute point from reference `base`
    /// ([`Point`](Self::Point),
    /// [`RelativePoint`](Self::RelativePoint)/[`PolarPoint`](Self::PolarPoint)).
    ///
    /// Returns `None` for non-point variants.
    #[must_use]
    pub fn resolve_point(&self, base: Point2) -> Option<Point2> {
        match self {
            ParsedInput::Point(p) => Some(*p),
            ParsedInput::RelativePoint(d) => Some(base + *d),
            ParsedInput::PolarPoint { dist, angle_rad } => {
                Some(base + Vec2::new(dist * angle_rad.cos(), dist * angle_rad.sin()))
            }
            _ => None,
        }
    }
}

/// A parse error with a byte position in the original input and an expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset of the problem in the original input.
    pub pos: usize,
    /// What was expected or why parsing failed.
    pub msg: String,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "posición {}: {}", self.pos, self.msg)
    }
}

impl std::error::Error for ParseError {}

/// Parses an input line according to the current prompt.
///
/// - `s` is trimmed before parsing.
/// - `expecting` guides scalar, text, and keyword interpretation.
/// - `last_point` is required for relative and polar `@…` forms.
///
/// # Errors
/// Returns [`ParseError`] with a position and reason for malformed input.
pub fn parse_input(
    s: &str,
    expecting: &ParamType,
    last_point: Option<Point2>,
) -> Result<ParsedInput, ParseError> {
    // Preserve the trimmed content's offset for exact error positions.
    let leading = s.len() - s.trim_start().len();
    let t = s.trim();

    if t.is_empty() {
        return Ok(ParsedInput::Empty);
    }

    // Text prompts keep input literal instead of interpreting coordinates.
    if matches!(expecting, ParamType::Text) {
        return Ok(ParsedInput::Text(t.to_string()));
    }

    // Relative and polar forms: `@Δx,Δy` or `@distance<angle`.
    if let Some(rest) = t.strip_prefix('@') {
        if last_point.is_none() {
            return Err(ParseError {
                pos: leading,
                msg: "entrada relativa '@' sin punto de referencia previo".to_string(),
            });
        }
        let rest_off = leading + 1;

        if let Some(lt) = rest.find('<') {
            let dist = scalar_linear(&rest[..lt], rest_off, "la distancia del punto polar")?;
            let angle_rad = scalar_angle(
                &rest[lt + 1..],
                rest_off + lt + 1,
                "el ángulo (en grados) del punto polar",
            )?;
            return Ok(ParsedInput::PolarPoint { dist, angle_rad });
        }

        if let Some(comma) = rest.find(',') {
            let dy_str = &rest[comma + 1..];
            if let Some(extra) = dy_str.find(',') {
                return Err(ParseError {
                    pos: rest_off + comma + 1 + extra,
                    msg: "demasiadas coordenadas: se esperaba '@Δx,Δy' (2D)".to_string(),
                });
            }
            let dx = scalar_linear(
                &rest[..comma],
                rest_off,
                "la componente Δx del punto relativo",
            )?;
            let dy = scalar_linear(
                dy_str,
                rest_off + comma + 1,
                "la componente Δy del punto relativo",
            )?;
            return Ok(ParsedInput::RelativePoint(Vec2::new(dx, dy)));
        }

        return Err(ParseError {
            pos: leading,
            msg: "entrada '@' malformada: se esperaba '@Δx,Δy' o '@dist<ángulo'".to_string(),
        });
    }

    // Absolute point `x,y`.
    if let Some(comma) = t.find(',') {
        let y_str = &t[comma + 1..];
        if let Some(extra) = y_str.find(',') {
            return Err(ParseError {
                pos: leading + comma + 1 + extra,
                msg: "demasiadas coordenadas: se esperaba 'x,y' (2D)".to_string(),
            });
        }
        let x = scalar_linear(&t[..comma], leading, "la coordenada X")?;
        let y = scalar_linear(y_str, leading + comma + 1, "la coordenada Y")?;
        return Ok(ParsedInput::Point(Point2::new(x, y)));
    }

    // A single token is either a number or a command/option word.
    if looks_numeric(t) {
        let n = if matches!(expecting, ParamType::Angle) {
            scalar_angle(t, leading, "un ángulo en grados")?
        } else {
            scalar_linear(t, leading, "un número")?
        };
        if matches!(expecting, ParamType::Point) {
            return Err(ParseError {
                pos: leading,
                msg: "se esperaba un punto 'x,y', no un único número".to_string(),
            });
        }
        return Ok(ParsedInput::Number(n));
    }

    // Words are options in `Enum` prompts and commands elsewhere.
    if matches!(expecting, ParamType::Enum(_)) {
        Ok(ParsedInput::Option(t.to_string()))
    } else {
        Ok(ParsedInput::Command(t.to_string()))
    }
}

/// Returns `true` when `t` looks numeric, so a parse failure is reported as a
/// malformed number instead of treating it as a word.
fn looks_numeric(t: &str) -> bool {
    matches!(t.as_bytes().first(), Some(b'0'..=b'9' | b'+' | b'-' | b'.'))
}

/// Parses a period-decimal linear scalar and maps failures to [`ParseError`].
fn scalar_linear(tok: &str, pos: usize, expected: &str) -> Result<f64, ParseError> {
    parse_linear(tok).map_err(|e| ParseError {
        pos,
        msg: format!("se esperaba {expected}; {e}"),
    })
}

/// Parses degrees, converts them to radians, and maps parse failures.
fn scalar_angle(tok: &str, pos: usize, expected: &str) -> Result<f64, ParseError> {
    parse_angle_deg(tok).map_err(|e| ParseError {
        pos,
        msg: format!("se esperaba {expected}; {e}"),
    })
}
