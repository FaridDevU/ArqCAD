//! Document extents from the union of entity bounding boxes.
//!
//! [`doc_extents`] aggregates boxes from a [`ContainerRef`] using an
//! [`ExtentsFilter`]. `Visible` excludes hidden entities and entities on off or
//! frozen layers; `All` includes every entity.
//!
//! Missing, empty, or fully hidden containers return `None`, never a fabricated
//! degenerate box at the origin.

use af_math::BBox;

use crate::container::ContainerRef;
use crate::doc::Document;
use crate::id::LayerId;

/// Visibility filter for [`doc_extents`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtentsFilter {
    /// Visible entities on active, unfrozen layers.
    Visible,
    /// Every entity in the container.
    All,
}

/// Aggregated bounding box for `container` under `filter`.
///
/// Returns `None` when the container is absent, empty, or has no visible entity.
#[must_use]
pub fn doc_extents(doc: &Document, container: ContainerRef, filter: ExtentsFilter) -> Option<BBox> {
    let entities = doc.container(container)?;
    let mut extents: Option<BBox> = None;
    // Read cached pool boxes and common visibility data without materializing records.
    entities.visit_bboxes(|_id, common, bb| {
        if filter == ExtentsFilter::Visible && !is_visible(doc, common.visible(), common.layer()) {
            return;
        }
        extents = Some(extents.map_or(*bb, |acc| acc.union(*bb)));
    });
    extents
}

/// Whether an entity contributes to visible extents.
///
/// Unknown layers are treated as visible so corrupt references do not lose extents.
fn is_visible(doc: &Document, visible: bool, layer: LayerId) -> bool {
    if !visible {
        return false;
    }
    match doc.layer(layer) {
        Some(l) => !(l.is_off() || l.is_frozen()),
        None => true,
    }
}
