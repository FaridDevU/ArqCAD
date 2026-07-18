//! [`Layer`] provides named default properties and display/edit states to entities.
//!
//! Every entity references one layer. Layer `"0"` is permanent. The
//! [`Document`](crate::doc::Document) enforces case-insensitive name uniqueness.
//!
//! Getters expose private fields. Builder-style `with_*` methods create inert
//! values; changing a stored layer requires a transaction.

use serde::{Deserialize, Serialize};

use crate::entity::{Color, Lineweight};
use crate::id::{LayerId, StyleId};

/// A document layer.
///
/// `color`, `line_type`, and `lineweight` are concrete defaults inherited by
/// `ByLayer` entities. Off layers are hidden; frozen layers also skip spatial
/// operations; locked layers remain visible but cannot be edited; plot controls output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    id: LayerId,
    name: String,
    color: Color,
    line_type: StyleId,
    lineweight: Lineweight,
    off: bool,
    frozen: bool,
    locked: bool,
    plot: bool,
    description: String,
}

impl Layer {
    /// Creates a visible, unfrozen, unlocked, printable layer without a description.
    ///
    /// This creates an inert value; transactions assign its persistent ID and
    /// enforce document policy.
    #[must_use]
    pub fn new(
        id: LayerId,
        name: impl Into<String>,
        color: Color,
        line_type: StyleId,
        lineweight: Lineweight,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            color,
            line_type,
            lineweight,
            off: false,
            frozen: false,
            locked: false,
            plot: true,
            description: String::new(),
        }
    }

    /// Returns a copy with a replacement ID.
    ///
    /// Transactions assign IDs on insertion and preserve them on modification.
    #[must_use]
    pub(crate) fn with_id(mut self, id: LayerId) -> Self {
        self.id = id;
        self
    }

    /// Returns a copy with a replacement name.
    ///
    /// This inert value does not enforce document naming policy.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Returns a copy with a replacement default color.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Returns a copy with a replacement default line type.
    #[must_use]
    pub fn with_line_type(mut self, line_type: StyleId) -> Self {
        self.line_type = line_type;
        self
    }

    /// Returns a copy with a replacement default lineweight.
    #[must_use]
    pub fn with_lineweight(mut self, lineweight: Lineweight) -> Self {
        self.lineweight = lineweight;
        self
    }

    /// Returns a copy with a replacement `off` state.
    #[must_use]
    pub fn with_off(mut self, off: bool) -> Self {
        self.off = off;
        self
    }

    /// Returns a copy with a replacement `frozen` state.
    #[must_use]
    pub fn with_frozen(mut self, frozen: bool) -> Self {
        self.frozen = frozen;
        self
    }

    /// Returns a copy with a replacement `locked` state.
    #[must_use]
    pub fn with_locked(mut self, locked: bool) -> Self {
        self.locked = locked;
        self
    }

    /// Returns a copy with a replacement `plot` state.
    #[must_use]
    pub fn with_plot(mut self, plot: bool) -> Self {
        self.plot = plot;
        self
    }

    /// Returns a copy with a replacement description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Stable layer ID.
    #[must_use]
    pub fn id(&self) -> LayerId {
        self.id
    }

    /// Case-insensitively unique layer name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Default layer color.
    #[must_use]
    pub fn color(&self) -> Color {
        self.color
    }

    /// Default line type.
    #[must_use]
    pub fn line_type(&self) -> StyleId {
        self.line_type
    }

    /// Default lineweight.
    #[must_use]
    pub fn lineweight(&self) -> Lineweight {
        self.lineweight
    }

    /// Whether the layer is off.
    #[must_use]
    pub fn is_off(&self) -> bool {
        self.off
    }

    /// Whether the layer is frozen.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Whether the layer is locked.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Whether the layer is included in plotted output.
    #[must_use]
    pub fn is_plottable(&self) -> bool {
        self.plot
    }

    /// Optional layer description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}
