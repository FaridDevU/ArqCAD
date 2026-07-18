//! Standalone typed system-variable table.
//!
//! Each variable has a canonical uppercase name, typed [`SysvarValue`], factory
//! default, validation domain, and annotated [`SysvarScope`].
//!
//! This module defines values, defaults, and `set` validation.
//!
//! Angles use radians. Integer 0/1 values represent toggles. `Real2` represents
//! pairs such as `SNAPUNIT` and `GRIDUNIT`.

use std::collections::HashMap;
use std::fmt;

/// Annotated system-variable scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysvarScope {
    /// Application/session setting.
    Session,
    /// Drawing/document setting.
    Document,
}

/// Typed system-variable value.
///
/// Uses `PartialEq` because real variants contain `f64` values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SysvarValue {
    /// Integer toggles, bitcodes, ACI indexes, pixel sizes, or percentages.
    Int(i64),
    /// Nonnegative real value; angles use radians.
    Real(f64),
    /// Pair of real values.
    Real2(f64, f64),
}

impl SysvarValue {
    /// Variant label for type errors.
    const fn kind(self) -> &'static str {
        match self {
            SysvarValue::Int(_) => "int",
            SysvarValue::Real(_) => "real",
            SysvarValue::Real2(_, _) => "real2",
        }
    }
}

impl fmt::Display for SysvarValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysvarValue::Int(n) => write!(f, "{n}"),
            SysvarValue::Real(x) => write!(f, "{x}"),
            SysvarValue::Real2(x, y) => write!(f, "{x},{y}"),
        }
    }
}

/// Valid-value domain used by `set`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Domain {
    /// Integer in the inclusive `min..=max` range.
    IntRange { min: i64, max: i64 },
    /// Finite nonnegative real.
    RealNonNeg,
    /// Pair of finite nonnegative reals.
    Real2NonNeg,
}

impl Domain {
    /// Expected variant label.
    const fn kind(self) -> &'static str {
        match self {
            Domain::IntRange { .. } => "int",
            Domain::RealNonNeg => "real",
            Domain::Real2NonNeg => "real2",
        }
    }

    fn describe(self) -> String {
        match self {
            Domain::IntRange { min, max } => format!("entero en {min}..={max}"),
            Domain::RealNonNeg => "real finito >= 0".to_string(),
            Domain::Real2NonNeg => "par de reales finitos >= 0".to_string(),
        }
    }

    /// Validates `value` against this domain.
    fn validate(self, name: &str, value: SysvarValue) -> Result<(), SysvarError> {
        let ok = match (self, value) {
            (Domain::IntRange { min, max }, SysvarValue::Int(n)) => n >= min && n <= max,
            (Domain::RealNonNeg, SysvarValue::Real(x)) => x.is_finite() && x >= 0.0,
            (Domain::Real2NonNeg, SysvarValue::Real2(x, y)) => {
                x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0
            }
            // A wrong variant is a type error, not a range error.
            _ => {
                return Err(SysvarError::TypeMismatch {
                    name: name.to_string(),
                    expected: self.kind(),
                    got: value.kind(),
                });
            }
        };
        if ok {
            Ok(())
        } else {
            Err(SysvarError::OutOfRange {
                name: name.to_string(),
                value,
                allowed: self.describe(),
            })
        }
    }
}

/// Immutable system-variable metadata.
pub struct SysvarDef {
    /// Canonical uppercase name.
    pub name: &'static str,
    /// Annotated scope.
    pub scope: SysvarScope,
    /// Factory value.
    pub default: SysvarValue,
    /// Validation domain used by [`SysvarTable::set`].
    domain: Domain,
}

impl SysvarDef {
    /// Human-readable domain description.
    #[must_use]
    pub fn allowed(&self) -> String {
        self.domain.describe()
    }
}

/// Typed system-variable table error.
#[derive(Debug, Clone, PartialEq)]
pub enum SysvarError {
    /// Variable does not exist.
    Unknown(String),
    /// Value has an incompatible variant.
    TypeMismatch {
        /// Canonical variable name.
        name: String,
        /// Expected variant.
        expected: &'static str,
        /// Received variant.
        got: &'static str,
    },
    /// Value lies outside the valid domain.
    OutOfRange {
        /// Canonical variable name.
        name: String,
        /// Rejected value.
        value: SysvarValue,
        /// Valid-domain description.
        allowed: String,
    },
}

impl fmt::Display for SysvarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SysvarError::Unknown(name) => write!(f, "sysvar desconocida: '{name}'"),
            SysvarError::TypeMismatch {
                name,
                expected,
                got,
            } => write!(
                f,
                "sysvar '{name}': tipo incompatible (se esperaba {expected}, se recibió {got})"
            ),
            SysvarError::OutOfRange {
                name,
                value,
                allowed,
            } => write!(
                f,
                "sysvar '{name}': valor {value} fuera de rango (válido: {allowed})"
            ),
        }
    }
}

impl std::error::Error for SysvarError {}

// Const constructors keep the table compact.

const fn int(
    name: &'static str,
    scope: SysvarScope,
    default: i64,
    min: i64,
    max: i64,
) -> SysvarDef {
    SysvarDef {
        name,
        scope,
        default: SysvarValue::Int(default),
        domain: Domain::IntRange { min, max },
    }
}

const fn real(name: &'static str, scope: SysvarScope, default: f64) -> SysvarDef {
    SysvarDef {
        name,
        scope,
        default: SysvarValue::Real(default),
        domain: Domain::RealNonNeg,
    }
}

const fn real2(name: &'static str, scope: SysvarScope, dx: f64, dy: f64) -> SysvarDef {
    SysvarDef {
        name,
        scope,
        default: SysvarValue::Real2(dx, dy),
        domain: Domain::Real2NonNeg,
    }
}

use SysvarScope::{Document, Session};

/// Factory system-variable definitions.
///
/// `POLARANG` stores its 90-degree default as π/2 radians.
static DEFS: &[SysvarDef] = &[
    // Snap and grid.
    int("ORTHOMODE", Document, 0, 0, 1),
    int("SNAPMODE", Document, 0, 0, 1),
    real2("SNAPUNIT", Document, 0.5, 0.5),
    // Start new drawings with the familiar visible grid.
    int("GRIDMODE", Document, 1, 0, 1),
    real2("GRIDUNIT", Document, 0.5, 0.5),
    // Object snap.
    int("OSMODE", Session, 4133, 0, 16383), // 4133 = End+Cen+Int+Ext (bitcode)
    int("AUTOSNAP", Session, 63, 0, 63),    // 63 = all enabled (bitcode)
    int("APERTURE", Session, 10, 1, 50),
    int("APBOX", Session, 0, 0, 1),
    // Cursor selection.
    int("PICKBOX", Session, 3, 0, 255),
    int("PICKFIRST", Session, 1, 0, 1),
    int("PICKADD", Session, 2, 0, 2),
    int("PICKAUTO", Session, 5, 0, 7), // 5 = 1+4 (bitcode)
    int("PICKDRAG", Session, 2, 0, 2),
    // Dynamic input.
    int("DYNMODE", Session, 3, -3, 3), // 3 = pointer+dimensional
    int("DYNPROMPT", Session, 1, 0, 1),
    int("DYNTOOLTIPS", Session, 1, 0, 1),
    int("TOOLTIPMERGE", Session, 0, 0, 1),
    int("TEMPOVERRIDES", Session, 1, 0, 1),
    // Polar tracking; `POLARANG` uses radians.
    int("POLARMODE", Session, 0, 0, 15), // bitcode
    real("POLARANG", Session, std::f64::consts::FRAC_PI_2), // 90°
    real("POLARDIST", Session, 0.0),
    int("TRACKPATH", Session, 0, 0, 3), // bitcode
    // Grips.
    int("GRIPS", Session, 2, 0, 2),
    int("GRIPSIZE", Session, 5, 1, 255),
    int("GRIPCOLOR", Session, 150, 1, 255), // ACI, inactive grip
    int("GRIPHOT", Session, 12, 1, 255),    // ACI, active grip
    int("GRIPHOVER", Session, 3, 1, 255),   // ACI, hovered grip
    int("GRIPCONTOUR", Session, 251, 1, 255), // ACI
    int("GRIPBLOCK", Session, 0, 0, 1),
    // Cursor.
    int("CURSORSIZE", Session, 5, 1, 100), // Screen percentage.
    int("CURSORBADGE", Session, 2, 1, 2),
    // Selection highlighting and window/crossing areas.
    int("HIGHLIGHT", Session, 1, 0, 1),
    int("SELECTIONPREVIEW", Session, 3, 0, 3), // bitcode
    int("SELECTIONCYCLING", Session, -2, -2, 2),
    int("SELECTIONAREA", Session, 1, 0, 1),
    int("SELECTIONAREAOPACITY", Session, 25, 0, 100), // %
    int("WINDOWAREACOLOR", Session, 150, 1, 255),     // ACI
    int("CROSSINGAREACOLOR", Session, 100, 1, 255),   // ACI; 2019 default is 100.
    // Editing behavior toggles.
    int("LWDISPLAY", Document, 0, 0, 1),
    int("FILLMODE", Document, 1, 0, 1),
    int("MBUTTONPAN", Session, 1, 0, 1),
    int("ZOOMFACTOR", Session, 60, 3, 100), // Percent per wheel step.
    int("ZOOMWHEEL", Session, 0, 0, 1),
    int("UCSICON", Document, 3, 0, 3), // bitcode: ON + AT ORIGIN
    int("MIRRTEXT", Document, 0, 0, 1),
    int("PELLIPSE", Document, 0, 0, 1),
    int("PLINEGEN", Document, 0, 0, 1),
    // Context menus and command preview.
    int("SHORTCUTMENU", Session, 11, 0, 31), // 11 = 1+2+8 (bitcode)
    int("SHORTCUTMENUDURATION", Session, 250, 100, 10000), // ms
    int("COMMANDPREVIEW", Session, 1, 0, 1),
    int("TRIMEXTENDMODE", Session, 1, 0, 1),
    int("PREVIEWEFFECT", Session, 2, 0, 2),
    int("CMDECHO", Session, 1, 0, 1), // Not persisted between sessions.
];

/// Finds a system-variable definition by case-insensitive ASCII name.
#[must_use]
pub fn find_def(name: &str) -> Option<&'static SysvarDef> {
    DEFS.iter().find(|d| d.name.eq_ignore_ascii_case(name))
}

/// All factory definitions.
#[must_use]
pub fn defs() -> &'static [SysvarDef] {
    DEFS
}

/// Mutable current system-variable values.
///
/// [`Default`] loads factory values. Names are case-insensitive.
#[derive(Debug, Clone)]
pub struct SysvarTable {
    values: HashMap<&'static str, SysvarValue>,
}

impl Default for SysvarTable {
    fn default() -> Self {
        Self {
            values: DEFS.iter().map(|d| (d.name, d.default)).collect(),
        }
    }
}

impl SysvarTable {
    /// Current value for `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<SysvarValue> {
        let def = find_def(name)?;
        self.values.get(def.name).copied()
    }

    /// Metadata for `name`.
    #[must_use]
    pub fn def(&self, name: &str) -> Option<&'static SysvarDef> {
        find_def(name)
    }

    /// Sets `name` after validating type and range.
    ///
    /// # Errors
    /// Returns [`SysvarError`] for unknown names, wrong variants, or invalid ranges.
    pub fn set(&mut self, name: &str, value: SysvarValue) -> Result<(), SysvarError> {
        let def = find_def(name).ok_or_else(|| SysvarError::Unknown(name.to_string()))?;
        def.domain.validate(def.name, value)?;
        self.values.insert(def.name, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn defaults_sample_exact() {
        let t = SysvarTable::default();
        assert_eq!(t.get("OSMODE"), Some(SysvarValue::Int(4133)));
        assert_eq!(t.get("AUTOSNAP"), Some(SysvarValue::Int(63)));
        assert_eq!(t.get("PICKBOX"), Some(SysvarValue::Int(3)));
        assert_eq!(t.get("PICKFIRST"), Some(SysvarValue::Int(1)));
        assert_eq!(t.get("DYNMODE"), Some(SysvarValue::Int(3)));
        assert_eq!(t.get("GRIPSIZE"), Some(SysvarValue::Int(5)));
        assert_eq!(t.get("GRIPCOLOR"), Some(SysvarValue::Int(150)));
        assert_eq!(t.get("GRIPHOT"), Some(SysvarValue::Int(12)));
        assert_eq!(t.get("CURSORSIZE"), Some(SysvarValue::Int(5)));
        assert_eq!(t.get("ZOOMFACTOR"), Some(SysvarValue::Int(60)));
        assert_eq!(t.get("SELECTIONAREAOPACITY"), Some(SysvarValue::Int(25)));
        assert_eq!(t.get("SHORTCUTMENU"), Some(SysvarValue::Int(11)));
        assert_eq!(t.get("SELECTIONCYCLING"), Some(SysvarValue::Int(-2)));
        assert_eq!(t.get("SNAPUNIT"), Some(SysvarValue::Real2(0.5, 0.5)));
        // `POLARANG` stores 90 degrees as π/2 radians.
        assert_eq!(t.get("POLARANG"), Some(SysvarValue::Real(FRAC_PI_2)));
    }

    #[test]
    fn every_default_is_valid_under_its_own_domain() {
        // Every default must satisfy its own domain.
        for d in DEFS {
            assert!(
                d.domain.validate(d.name, d.default).is_ok(),
                "default inválido para {}",
                d.name
            );
        }
    }

    #[test]
    fn names_are_unique() {
        for (i, a) in DEFS.iter().enumerate() {
            for b in &DEFS[i + 1..] {
                assert_ne!(a.name, b.name, "sysvar duplicada: {}", a.name);
            }
        }
    }

    #[test]
    fn set_out_of_range_is_err() {
        let mut t = SysvarTable::default();
        // Integer range.
        assert!(matches!(
            t.set("APERTURE", SysvarValue::Int(99)),
            Err(SysvarError::OutOfRange { .. })
        ));
        assert!(matches!(
            t.set("ORTHOMODE", SysvarValue::Int(2)),
            Err(SysvarError::OutOfRange { .. })
        ));
        // Negative and nonfinite real values.
        assert!(matches!(
            t.set("POLARANG", SysvarValue::Real(-1.0)),
            Err(SysvarError::OutOfRange { .. })
        ));
        assert!(matches!(
            t.set("SNAPUNIT", SysvarValue::Real2(-0.5, 1.0)),
            Err(SysvarError::OutOfRange { .. })
        ));
        assert!(matches!(
            t.set("POLARANG", SysvarValue::Real(f64::NAN)),
            Err(SysvarError::OutOfRange { .. })
        ));
        // Failed writes preserve the previous value.
        assert_eq!(t.get("APERTURE"), Some(SysvarValue::Int(10)));
    }

    #[test]
    fn set_type_mismatch_is_err() {
        let mut t = SysvarTable::default();
        assert!(matches!(
            t.set("OSMODE", SysvarValue::Real(1.0)),
            Err(SysvarError::TypeMismatch { .. })
        ));
        assert!(matches!(
            t.set("SNAPUNIT", SysvarValue::Int(1)),
            Err(SysvarError::TypeMismatch { .. })
        ));
    }

    #[test]
    fn unknown_var_is_err_or_none() {
        let mut t = SysvarTable::default();
        assert_eq!(t.get("NOPE"), None);
        assert!(matches!(
            t.set("NOPE", SysvarValue::Int(1)),
            Err(SysvarError::Unknown(_))
        ));
    }

    #[test]
    fn set_get_roundtrip() {
        let mut t = SysvarTable::default();
        t.set("OSMODE", SysvarValue::Int(191)).unwrap();
        assert_eq!(t.get("OSMODE"), Some(SysvarValue::Int(191)));
        t.set("SNAPUNIT", SysvarValue::Real2(1.0, 2.0)).unwrap();
        assert_eq!(t.get("SNAPUNIT"), Some(SysvarValue::Real2(1.0, 2.0)));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        let mut t = SysvarTable::default();
        assert_eq!(t.get("osmode"), t.get("OSMODE"));
        assert_eq!(t.get("OsMode"), Some(SysvarValue::Int(4133)));
        t.set("OrthoMode", SysvarValue::Int(1)).unwrap();
        assert_eq!(t.get("ORTHOMODE"), Some(SysvarValue::Int(1)));
    }

    #[test]
    fn scope_annotation_present() {
        assert_eq!(find_def("ORTHOMODE").unwrap().scope, SysvarScope::Document);
        assert_eq!(find_def("OSMODE").unwrap().scope, SysvarScope::Session);
    }
}
