//! [`EntityRecord`]: common properties and concrete entity geometry.

use serde::{Deserialize, Serialize};

use crate::entity::EntityGeometry;
use crate::entity::style_props::{Color, LineTypeRef, Lineweight};
use crate::id::{EntityId, LayerId};

/// A model entity with stable identity, common properties, and geometry.
///
/// # No session state
///
/// The record contains persistent data only. Render and selection state live
/// outside the model.
///
/// # Transaction-only mutation
///
/// Fields are public because transaction snapshots pass records by value. The
/// document still exposes no in-place setters; document changes go through a
/// transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityRecord {
    /// Stable entity identity that is never recycled.
    pub id: EntityId,
    /// Layer containing the entity.
    pub layer: LayerId,
    /// Color, possibly inherited from a layer or block.
    pub color: Color,
    /// Line type, possibly inherited.
    pub line_type: LineTypeRef,
    /// Line weight, possibly inherited.
    pub lineweight: Lineweight,
    /// Per-entity visibility in addition to layer visibility.
    pub visible: bool,
    /// Concrete entity geometry.
    pub geometry: EntityGeometry,
}

impl EntityRecord {
    /// Creates a visible entity record.
    ///
    /// This convenience constructor does not mutate a document.
    #[must_use]
    pub fn new(
        id: EntityId,
        layer: LayerId,
        color: Color,
        line_type: LineTypeRef,
        lineweight: Lineweight,
        geometry: EntityGeometry,
    ) -> Self {
        Self {
            id,
            layer,
            color,
            line_type,
            lineweight,
            visible: true,
            geometry,
        }
    }
}
