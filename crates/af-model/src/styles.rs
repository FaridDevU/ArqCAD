//! Named document styles: [`LineType`], [`TextStyle`], and [`DimStyle`].
//!
//! Each type has its own case-insensitively unique document table. New documents
//! include `"Continuous"` and standard text/dimension styles.
//!
//! Fields are read through getters and mutated only through the document.

use serde::{Deserialize, Serialize};

use crate::id::StyleId;

/// Named line type.
///
/// Pattern values use drawing units: positive draws, negative skips, and zero
/// draws a point. An empty pattern is continuous. `ByLayer` and `ByBlock` are
/// inherited references, not table entries.
///
/// Uses `PartialEq` because patterns contain `f64` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineType {
    id: StyleId,
    name: String,
    description: String,
    /// Simplified `.lin` pattern; empty means continuous. Older files default empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pattern: Vec<f64>,
}

impl LineType {
    /// Creates a continuous line type with an empty pattern.
    #[must_use]
    pub(crate) fn new(
        id: StyleId,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self::with_pattern(id, name, description, Vec::new())
    }

    /// Creates a line type with a `.lin` pattern.
    #[must_use]
    pub(crate) fn with_pattern(
        id: StyleId,
        name: impl Into<String>,
        description: impl Into<String>,
        pattern: Vec<f64>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            description: description.into(),
            pattern,
        }
    }

    /// Stable line-type ID.
    #[must_use]
    pub fn id(&self) -> StyleId {
        self.id
    }

    /// Case-insensitively unique line-type name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Optional free-form description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Dash pattern; empty means continuous.
    #[must_use]
    pub fn pattern(&self) -> &[f64] {
        &self.pattern
    }

    /// Whether this is a continuous line.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        self.pattern.is_empty()
    }
}

/// ID-free line-type definition from the built-in library.
///
/// New drawings load only `"Continuous"`; other definitions load on demand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineTypeDef {
    /// Canonical name.
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// `.lin` pattern; empty means continuous.
    pub pattern: &'static [f64],
}

/// Built-in line-type definitions, including `"Continuous"`.
static LINETYPE_LIBRARY: &[LineTypeDef] = &[
    LineTypeDef {
        name: "Continuous",
        description: "Solid line",
        pattern: &[],
    },
    LineTypeDef {
        name: "DASHED",
        description: "Dashed __ __ __ __ __ __ __ __ __ __",
        pattern: &[0.5, -0.25],
    },
    LineTypeDef {
        name: "DASHDOT",
        description: "Dash dot __ . __ . __ . __ . __ . __",
        pattern: &[0.5, -0.25, 0.0, -0.25],
    },
    LineTypeDef {
        name: "DOTTED",
        description: "Dotted . . . . . . . . . . . . . . . .",
        pattern: &[0.0, -0.25],
    },
    LineTypeDef {
        name: "CENTER",
        description: "Center ____ _ ____ _ ____ _ ____ _ __",
        pattern: &[1.25, -0.25, 0.25, -0.25],
    },
    LineTypeDef {
        name: "HIDDEN",
        description: "Hidden __ __ __ __ __ __ __ __ __ __ __",
        pattern: &[0.25, -0.125],
    },
    LineTypeDef {
        name: "PHANTOM",
        description: "Phantom ______  __  __  ______  __  __",
        pattern: &[1.25, -0.25, 0.25, -0.25, 0.25, -0.25],
    },
];

/// Returns the built-in line-type library.
#[must_use]
pub fn linetype_library() -> &'static [LineTypeDef] {
    LINETYPE_LIBRARY
}

/// Finds a built-in definition by case-insensitive name.
#[must_use]
pub fn linetype_def(name: &str) -> Option<&'static LineTypeDef> {
    LINETYPE_LIBRARY
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
}

/// Named text style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextStyle {
    id: StyleId,
    name: String,
}

impl TextStyle {
    /// Creates a text style.
    #[must_use]
    pub(crate) fn new(id: StyleId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    /// Stable text-style ID.
    #[must_use]
    pub fn id(&self) -> StyleId {
        self.id
    }

    /// Case-insensitively unique text-style name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Named dimension style.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimStyle {
    id: StyleId,
    name: String,
}

impl DimStyle {
    /// Creates a dimension style.
    #[must_use]
    pub(crate) fn new(id: StyleId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    /// Stable dimension-style ID.
    #[must_use]
    pub fn id(&self) -> StyleId {
        self.id
    }

    /// Case-insensitively unique dimension-style name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_library_has_the_expected_linetypes() {
        let names: Vec<&str> = linetype_library().iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            vec![
                "Continuous",
                "DASHED",
                "DASHDOT",
                "DOTTED",
                "CENTER",
                "HIDDEN",
                "PHANTOM",
            ]
        );
    }

    #[test]
    fn only_continuous_is_solid_in_the_library() {
        for d in linetype_library() {
            let solid = d.pattern.is_empty();
            assert_eq!(
                solid,
                d.name == "Continuous",
                "{} solidez inesperada",
                d.name
            );
        }
    }

    #[test]
    fn linetype_def_lookup_is_case_insensitive() {
        assert_eq!(linetype_def("dashed").map(|d| d.name), Some("DASHED"));
        assert_eq!(linetype_def("HiDdEn").map(|d| d.name), Some("HIDDEN"));
        assert!(linetype_def("nope").is_none());
    }

    #[test]
    fn with_pattern_roundtrips_and_new_is_continuous() {
        let id: StyleId = crate::id::ObjectId(1).into();
        let solid = LineType::new(id, "Continuous", "Solid line");
        assert!(solid.is_continuous());
        assert!(solid.pattern().is_empty());

        let dashed = LineType::with_pattern(id, "DASHED", "d", vec![0.5, -0.25]);
        assert!(!dashed.is_continuous());
        assert_eq!(dashed.pattern(), &[0.5, -0.25]);
    }
}
