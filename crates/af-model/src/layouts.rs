//! [`Layout`] represents a presentation and print sheet.
//!
//! It combines a name, [`PaperSettings`], an [`EntityContainer`] paper space,
//! and viewport entity IDs.
//!
//! Fields are read through getters and mutated only through the document.

use serde::{Deserialize, Serialize};

use crate::container::EntityContainer;
use crate::id::{EntityId, LayoutId};

/// Layout sheet orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    /// Portrait orientation.
    #[default]
    Portrait,
    /// Landscape orientation.
    Landscape,
}

/// Sheet settings for a [`Layout`].
///
/// Sizes and margins use paper millimeters. Margins are left, right, top, bottom.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperSettings {
    /// Sheet size in millimeters as `(width, height)`.
    size_mm: (f64, f64),
    /// Margins in millimeters as `(left, right, top, bottom)`.
    margins_mm: (f64, f64, f64, f64),
    /// Sheet orientation.
    orientation: Orientation,
    /// Plot scale; `1.0` is 1:1.
    plot_scale: f64,
}

impl PaperSettings {
    /// Creates explicit paper settings.
    #[must_use]
    pub(crate) fn new(
        size_mm: (f64, f64),
        margins_mm: (f64, f64, f64, f64),
        orientation: Orientation,
        plot_scale: f64,
    ) -> Self {
        Self {
            size_mm,
            margins_mm,
            orientation,
            plot_scale,
        }
    }

    /// Sheet size in millimeters.
    #[must_use]
    pub fn size_mm(&self) -> (f64, f64) {
        self.size_mm
    }

    /// Margins in millimeters.
    #[must_use]
    pub fn margins_mm(&self) -> (f64, f64, f64, f64) {
        self.margins_mm
    }

    /// Sheet orientation.
    #[must_use]
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// Plot scale.
    #[must_use]
    pub fn plot_scale(&self) -> f64 {
        self.plot_scale
    }
}

impl Default for PaperSettings {
    /// Portrait A4 sheet with 10 mm margins at 1:1 scale.
    fn default() -> Self {
        Self::new(
            (210.0, 297.0),
            (10.0, 10.0, 10.0, 10.0),
            Orientation::Portrait,
            1.0,
        )
    }
}

/// A sheet with paper space and viewports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    id: LayoutId,
    name: String,
    paper: PaperSettings,
    entities: EntityContainer,
    /// Paper-space entity IDs that represent viewports.
    viewports: Vec<EntityId>,
}

impl Layout {
    /// Creates a layout with empty paper space and no viewports.
    #[must_use]
    pub(crate) fn new(id: LayoutId, name: impl Into<String>, paper: PaperSettings) -> Self {
        Self {
            id,
            name: name.into(),
            paper,
            entities: EntityContainer::new(),
            viewports: Vec::new(),
        }
    }

    /// Stable layout ID.
    #[must_use]
    pub fn id(&self) -> LayoutId {
        self.id
    }

    /// Case-insensitively unique layout name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Paper settings.
    #[must_use]
    pub fn paper(&self) -> &PaperSettings {
        &self.paper
    }

    /// Layout paper-space entities.
    #[must_use]
    pub fn entities(&self) -> &EntityContainer {
        &self.entities
    }

    /// Layout viewport IDs.
    #[must_use]
    pub fn viewports(&self) -> &[EntityId] {
        &self.viewports
    }

    /// Crate-private mutable paper-space access.
    pub(crate) fn entities_mut(&mut self) -> &mut EntityContainer {
        &mut self.entities
    }
}
