//! Property-based selection filters for QSELECT and SELECTSIMILAR.
//!
//! These read-only operations produce ID lists without touching the spatial index.

use af_model::Document;
use af_model::entity::{Color, EntityGeometry};
use af_model::id::{EntityId, LayerId};

/// Named entity type used by QSELECT and SELECTSIMILAR filters.
///
/// [`EntityKind::of`] matches every geometry variant explicitly so additions fail
/// compilation until handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    /// [`EntityGeometry::Line`].
    Line,
    /// [`EntityGeometry::Point`].
    Point,
    /// [`EntityGeometry::Circle`].
    Circle,
    /// [`EntityGeometry::Arc`].
    Arc,
    /// [`EntityGeometry::Ellipse`].
    Ellipse,
    /// [`EntityGeometry::Polyline`].
    Polyline,
    /// [`EntityGeometry::Xline`].
    Xline,
    /// [`EntityGeometry::Ray`].
    Ray,
    /// [`EntityGeometry::Spline`].
    Spline,
    /// [`EntityGeometry::Wipeout`].
    Wipeout,
}

impl EntityKind {
    /// Returns the kind of concrete geometry.
    #[must_use]
    pub fn of(geom: &EntityGeometry) -> Self {
        match geom {
            EntityGeometry::Line(_) => EntityKind::Line,
            EntityGeometry::Point(_) => EntityKind::Point,
            EntityGeometry::Circle(_) => EntityKind::Circle,
            EntityGeometry::Arc(_) => EntityKind::Arc,
            EntityGeometry::Ellipse(_) => EntityKind::Ellipse,
            EntityGeometry::Polyline(_) => EntityKind::Polyline,
            EntityGeometry::Xline(_) => EntityKind::Xline,
            EntityGeometry::Ray(_) => EntityKind::Ray,
            EntityGeometry::Spline(_) => EntityKind::Spline,
            EntityGeometry::Wipeout(_) => EntityKind::Wipeout,
        }
    }
}

/// QSELECT property criteria. Values within a field use OR semantics, while
/// populated fields combine with AND semantics. `None` means unrestricted.
#[derive(Debug, Clone, Default)]
pub struct SelectionFilter {
    /// Accepted entity kinds.
    pub kinds: Option<Vec<EntityKind>>,
    /// Accepted layers.
    pub layers: Option<Vec<LayerId>>,
    /// Accepted exact colors.
    pub colors: Option<Vec<Color>>,
}

impl SelectionFilter {
    /// Returns whether `id` exists and satisfies all active criteria.
    #[must_use]
    pub fn matches(&self, doc: &Document, id: EntityId) -> bool {
        let Some((rec, _)) = doc.entity(id) else {
            return false;
        };
        if let Some(kinds) = &self.kinds
            && !kinds.contains(&EntityKind::of(&rec.geometry))
        {
            return false;
        }
        if let Some(layers) = &self.layers
            && !layers.contains(&rec.layer)
        {
            return false;
        }
        if let Some(colors) = &self.colors
            && !colors.contains(&rec.color)
        {
            return false;
        }
        true
    }
}

/// Retains existing IDs that satisfy `filter`, preserving input order.
#[must_use]
pub fn apply_filter(
    ids: impl IntoIterator<Item = EntityId>,
    doc: &Document,
    filter: &SelectionFilter,
) -> Vec<EntityId> {
    ids.into_iter()
        .filter(|&id| filter.matches(doc, id))
        .collect()
}

/// Returns entities similar to `id`: same kind and layer, plus the same explicit
/// color when the reference color is not `ByLayer`.
///
/// Results include `id` and follow draw order. Unknown IDs return an empty list.
#[must_use]
pub fn select_similar(doc: &Document, id: EntityId) -> Vec<EntityId> {
    let Some((rec, cref)) = doc.entity(id) else {
        return Vec::new();
    };
    let filter = SelectionFilter {
        kinds: Some(vec![EntityKind::of(&rec.geometry)]),
        layers: Some(vec![rec.layer]),
        colors: match rec.color {
            Color::ByLayer => None,
            c => Some(vec![c]),
        },
    };
    let Some(container) = doc.container(cref) else {
        return Vec::new();
    };
    apply_filter(container.iter_records().map(|r| r.id), doc, &filter)
}
