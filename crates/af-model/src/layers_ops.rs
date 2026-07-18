//! Layer-management policy built on reversible [`TxContext`] operations.
//!
//! It provides create, rename, property update, policy-based deletion, and current
//! layer operations while enforcing valid names, protected layer `"0"`, explicit
//! deletion behavior, and a drawable current layer.
//!
//! # Transaction context
//!
//! Functions borrow the caller's `&mut TxContext`; none opens its own transaction.
//! Multi-step operations therefore roll back and undo as one unit.
//!
//! # Name policy
//!
//! Names must be nonblank, case-insensitively unique, and contain none of
//! [`FORBIDDEN_NAME_CHARS`].
//!
//! # Layer states
//!
//! Off and frozen layers cannot become current. Locked layers remain visible but
//! cannot be edited; `plot` controls printed output.

use crate::doc::Document;
use crate::entity::{Color, Lineweight};
use crate::id::{EntityId, LayerId, ObjectId, StyleId};
use crate::layers::Layer;
use crate::tx::{TxContext, TxError};

/// Characters forbidden in DXF/DWG layer names.
///
/// [`create_layer`] and [`rename_layer`] reject these reserved separators and wildcards.
pub const FORBIDDEN_NAME_CHARS: &[char] = &['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', '='];

/// Properties used by [`create_layer`].
///
/// [`LayerProps::new`] supplies visible, unfrozen, unlocked, printable defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerProps {
    /// Valid, case-insensitively unique layer name.
    pub name: String,
    /// Concrete default layer color.
    pub color: Color,
    /// Existing default line type.
    pub line_type: StyleId,
    /// Default lineweight.
    pub lineweight: Lineweight,
    /// Whether the layer is off.
    pub off: bool,
    /// Whether the layer is frozen.
    pub frozen: bool,
    /// Whether the layer is locked.
    pub locked: bool,
    /// Whether the layer is plotted.
    pub plot: bool,
    /// Optional description.
    pub description: String,
}

impl LayerProps {
    /// Creates default properties for a new layer.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        color: Color,
        line_type: StyleId,
        lineweight: Lineweight,
    ) -> Self {
        Self {
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
}

/// Changes applied by [`set_layer_props`].
///
/// `Some` changes a property; `None` preserves it. An empty patch is a no-op.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerPatch {
    /// New default color.
    pub color: Option<Color>,
    /// New existing default line type.
    pub line_type: Option<StyleId>,
    /// New default lineweight.
    pub lineweight: Option<Lineweight>,
    /// New `off` state.
    pub off: Option<bool>,
    /// New `frozen` state.
    pub frozen: Option<bool>,
    /// New `locked` state.
    pub locked: Option<bool>,
    /// New `plot` state.
    pub plot: Option<bool>,
    /// New description.
    pub description: Option<String>,
}

/// Explicit policy for deleting a layer that still has entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletePolicy {
    /// Rejects deletion when any entity in any container references the layer.
    RejectIfUsed,
    /// Moves all entities to an existing distinct layer, then deletes the source.
    MoveEntitiesTo(LayerId),
    /// Deletes all entities on the layer in every container, then deletes the layer.
    /// The caller is responsible for confirmation before this destructive policy.
    ///
    /// [`MoveEntitiesTo`]: DeletePolicy::MoveEntitiesTo
    DeleteEntities,
}

/// Layer operation error.
///
/// Policy-specific variants supplement errors translated from [`TxError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerOpError {
    /// Name is blank.
    EmptyName,
    /// Name contains a forbidden DXF character.
    InvalidNameChar {
        /// Requested name.
        name: String,
        /// First forbidden character.
        ch: char,
    },
    /// Another layer has the same case-insensitive name.
    DuplicateName(String),
    /// Layer `"0"` cannot be deleted or renamed.
    LayerZeroProtected(LayerId),
    /// Referenced layer does not exist.
    UnknownLayer(LayerId),
    /// Referenced default line type does not exist.
    UnknownLineType(StyleId),
    /// An off or frozen layer cannot become current.
    CurrentLayerNotDrawable(LayerId),
    /// Current layer cannot be deleted.
    CurrentLayerRemoval(LayerId),
    /// The layer is still referenced by `count` entities.
    LayerInUse {
        /// Layer requested for deletion.
        layer: LayerId,
        /// Referencing entity count across all containers.
        count: usize,
    },
    /// Move target does not exist.
    MoveTargetMissing(LayerId),
    /// Move target is the source layer.
    MoveTargetIsSource(LayerId),
    /// Low-level transaction failure not translated by layer policy.
    Tx(TxError),
}

impl core::fmt::Display for LayerOpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LayerOpError::EmptyName => write!(f, "layer name is empty"),
            LayerOpError::InvalidNameChar { name, ch } => {
                write!(f, "layer name {name:?} contains forbidden character {ch:?}")
            }
            LayerOpError::DuplicateName(name) => {
                write!(f, "duplicate layer name (case-insensitive): {name:?}")
            }
            LayerOpError::LayerZeroProtected(id) => {
                write!(f, "layer \"0\" is protected (id {})", id.raw().0)
            }
            LayerOpError::UnknownLayer(id) => write!(f, "unknown layer id {}", id.raw().0),
            LayerOpError::UnknownLineType(id) => write!(f, "unknown line type id {}", id.raw().0),
            LayerOpError::CurrentLayerNotDrawable(id) => write!(
                f,
                "cannot make an off/frozen layer current (id {})",
                id.raw().0
            ),
            LayerOpError::CurrentLayerRemoval(id) => {
                write!(f, "cannot remove the current layer (id {})", id.raw().0)
            }
            LayerOpError::LayerInUse { layer, count } => write!(
                f,
                "layer id {} is in use by {count} entit{} and cannot be deleted",
                layer.raw().0,
                if *count == 1 { "y" } else { "ies" }
            ),
            LayerOpError::MoveTargetMissing(id) => {
                write!(f, "move-to target layer id {} does not exist", id.raw().0)
            }
            LayerOpError::MoveTargetIsSource(id) => write!(
                f,
                "move-to target layer id {} is the layer being deleted",
                id.raw().0
            ),
            LayerOpError::Tx(e) => write!(f, "transaction error: {e}"),
        }
    }
}

impl std::error::Error for LayerOpError {}

impl From<TxError> for LayerOpError {
    /// Translates policy-relevant low-level errors and preserves all others.
    fn from(e: TxError) -> Self {
        match e {
            TxError::DuplicateLayerName(name) => LayerOpError::DuplicateName(name),
            TxError::LayerZeroProtected(id) => LayerOpError::LayerZeroProtected(id),
            TxError::CurrentLayerRemoval(id) => LayerOpError::CurrentLayerRemoval(id),
            TxError::UnknownLayer(id) => LayerOpError::UnknownLayer(id),
            TxError::UnknownLineType(id) => LayerOpError::UnknownLineType(id),
            TxError::UnknownEntity(_)
            | TxError::UnknownContainer(_)
            | TxError::InvalidGeometry(_)
            | TxError::LayerInUse(_)
            | TxError::DuplicateGroupName(_)
            | TxError::UnknownGroup(_)
            | TxError::DuplicateLineTypeName(_)
            | TxError::LineTypeProtected(_)
            | TxError::LineTypeInUse(_)
            | TxError::Internal(_) => LayerOpError::Tx(e),
        }
    }
}

/// Creates a layer with the requested properties and returns its ID.
///
/// Validates name policy; the transaction checks uniqueness and line-type existence.
///
/// # Errors
/// Returns the corresponding [`LayerOpError`] for invalid names, duplicates, or
/// unknown line types.
pub fn create_layer(tx: &mut TxContext<'_>, props: LayerProps) -> Result<LayerId, LayerOpError> {
    validate_name(&props.name)?;
    let layer = Layer::new(
        ObjectId::NIL.into(),
        props.name,
        props.color,
        props.line_type,
        props.lineweight,
    )
    .with_off(props.off)
    .with_frozen(props.frozen)
    .with_locked(props.locked)
    .with_plot(props.plot)
    .with_description(props.description);
    let id = tx.add_layer_raw(layer)?;
    Ok(id)
}

/// Renames layer `id`.
///
/// Layer `"0"` is protected. Case-only changes are valid; an exact match is a no-op.
///
/// # Errors
/// Returns the corresponding [`LayerOpError`] for unknown/protected layers,
/// invalid names, or duplicates.
pub fn rename_layer(
    tx: &mut TxContext<'_>,
    id: LayerId,
    new_name: impl Into<String>,
) -> Result<(), LayerOpError> {
    let new_name = new_name.into();
    let current = tx
        .doc()
        .layer(id)
        .ok_or(LayerOpError::UnknownLayer(id))?
        .clone();
    if current.name().eq_ignore_ascii_case("0") {
        return Err(LayerOpError::LayerZeroProtected(id));
    }
    validate_name(&new_name)?;
    let renamed = current.with_name(new_name);
    tx.modify_layer_raw(id, renamed)?;
    Ok(())
}

/// Applies the `Some` properties of a [`LayerPatch`] to layer `id`.
///
/// Names use [`rename_layer`]. Empty or unchanged patches are no-ops.
///
/// # Errors
/// Returns [`LayerOpError::UnknownLayer`] or [`LayerOpError::UnknownLineType`].
pub fn set_layer_props(
    tx: &mut TxContext<'_>,
    id: LayerId,
    patch: LayerPatch,
) -> Result<(), LayerOpError> {
    let mut layer = tx
        .doc()
        .layer(id)
        .ok_or(LayerOpError::UnknownLayer(id))?
        .clone();
    if let Some(color) = patch.color {
        layer = layer.with_color(color);
    }
    if let Some(line_type) = patch.line_type {
        layer = layer.with_line_type(line_type);
    }
    if let Some(lineweight) = patch.lineweight {
        layer = layer.with_lineweight(lineweight);
    }
    if let Some(off) = patch.off {
        layer = layer.with_off(off);
    }
    if let Some(frozen) = patch.frozen {
        layer = layer.with_frozen(frozen);
    }
    if let Some(locked) = patch.locked {
        layer = layer.with_locked(locked);
    }
    if let Some(plot) = patch.plot {
        layer = layer.with_plot(plot);
    }
    if let Some(description) = patch.description {
        layer = layer.with_description(description);
    }
    tx.modify_layer_raw(id, layer)?;
    Ok(())
}

/// Deletes layer `id` according to [`DeletePolicy`].
///
/// Layer `"0"` and the current layer are protected. Policy decides how references
/// are handled within the same transaction.
///
/// # Errors
/// Returns the corresponding [`LayerOpError`] for protected, referenced, or
/// invalid source/target layers.
pub fn delete_layer(
    tx: &mut TxContext<'_>,
    id: LayerId,
    policy: DeletePolicy,
) -> Result<(), LayerOpError> {
    // Reject protected layers before scanning or moving entities.
    let is_zero = tx
        .doc()
        .layer(id)
        .ok_or(LayerOpError::UnknownLayer(id))?
        .name()
        .eq_ignore_ascii_case("0");
    if is_zero {
        return Err(LayerOpError::LayerZeroProtected(id));
    }
    if tx.doc().current_layer() == id {
        return Err(LayerOpError::CurrentLayerRemoval(id));
    }

    match policy {
        DeletePolicy::RejectIfUsed => {
            let count = entities_on_layer(tx.doc(), id).len();
            if count > 0 {
                return Err(LayerOpError::LayerInUse { layer: id, count });
            }
            tx.remove_layer_raw(id)?;
        }
        DeletePolicy::MoveEntitiesTo(dst) => {
            if dst == id {
                return Err(LayerOpError::MoveTargetIsSource(id));
            }
            if tx.doc().layer(dst).is_none() {
                return Err(LayerOpError::MoveTargetMissing(dst));
            }
            // Collect IDs before mutating every container in this transaction.
            let to_move = entities_on_layer(tx.doc(), id);
            for eid in to_move {
                tx.modify_entity(eid, |rec| rec.layer = dst)?;
            }
            tx.remove_layer_raw(id)?;
        }
        DeletePolicy::DeleteEntities => {
            // Collect IDs before deleting across every container.
            let to_delete = entities_on_layer(tx.doc(), id);
            for eid in to_delete {
                tx.remove_entity(eid)?;
            }
            tx.remove_layer_raw(id)?;
        }
    }
    Ok(())
}

/// Number of entities across all containers that reference `layer`.
///
/// Supports destructive-operation confirmation without opening a transaction.
#[must_use]
pub fn layer_entity_count(doc: &Document, layer: LayerId) -> usize {
    entities_on_layer(doc, layer).len()
}

/// Sets the document's current layer.
///
/// Rejects off or frozen layers. Setting the existing current layer is a no-op.
///
/// # Errors
/// Returns [`LayerOpError::UnknownLayer`] or
/// [`LayerOpError::CurrentLayerNotDrawable`].
pub fn set_current_layer(tx: &mut TxContext<'_>, id: LayerId) -> Result<(), LayerOpError> {
    let layer = tx.doc().layer(id).ok_or(LayerOpError::UnknownLayer(id))?;
    if layer.is_off() || layer.is_frozen() {
        return Err(LayerOpError::CurrentLayerNotDrawable(id));
    }
    tx.set_current_layer(id)?;
    Ok(())
}

/// Validates a nonblank layer name without forbidden DXF characters.
fn validate_name(name: &str) -> Result<(), LayerOpError> {
    if name.trim().is_empty() {
        return Err(LayerOpError::EmptyName);
    }
    if let Some(ch) = name.chars().find(|c| FORBIDDEN_NAME_CHARS.contains(c)) {
        return Err(LayerOpError::InvalidNameChar {
            name: name.to_string(),
            ch,
        });
    }
    Ok(())
}

/// IDs of all entities in every container that reference `layer`.
///
/// Shared by reference counting, moving, and cascading deletion policies.
fn entities_on_layer(doc: &Document, layer: LayerId) -> Vec<EntityId> {
    let mut ids = Vec::new();
    for rec in doc.model_space().iter_records() {
        if rec.layer == layer {
            ids.push(rec.id);
        }
    }
    for layout in doc.layouts() {
        for rec in layout.entities().iter_records() {
            if rec.layer == layer {
                ids.push(rec.id);
            }
        }
    }
    for block in doc.blocks() {
        for rec in block.entities().iter_records() {
            if rec.layer == layer {
                ids.push(rec.id);
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    //! Tests that layer deletion scans block-definition containers.

    use super::*;
    use af_math::Point2;

    use crate::container::ContainerRef;
    use crate::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
    use crate::id::EntityId;
    use crate::session::Session;
    use crate::units::Units;

    fn point_rec(layer: LayerId, x: f64) -> EntityRecord {
        EntityRecord::new(
            ObjectId::NIL.into(),
            layer,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(PointGeo::new(Point2::new(x, 0.0))),
        )
    }

    /// Creates a document with source/target layers and an empty block.
    fn doc_with_block() -> (Session, LayerId, LayerId, crate::id::BlockId) {
        let mut doc = Document::new(Units::default());
        let lt = doc.line_types().next().unwrap().id();
        let muros = doc
            .add_layer("Muros", Color::aci(1).unwrap(), lt, Lineweight::ByLayer)
            .unwrap();
        let destino = doc
            .add_layer("Destino", Color::aci(2).unwrap(), lt, Lineweight::ByLayer)
            .unwrap();
        let bid = doc.add_block("Puerta", Point2::ORIGIN).unwrap();
        (Session::from_document(doc), muros, destino, bid)
    }

    #[test]
    fn delete_reject_cuenta_entidades_en_bloque_y_model_space() {
        let (mut session, muros, _destino, bid) = doc_with_block();
        // Count one source-layer entity in a block and one in model space.
        session
            .transact("seed block", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::Block(bid), point_rec(muros, 0.0))
            })
            .expect("commits");
        session
            .transact("seed model", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, point_rec(muros, 1.0))
            })
            .expect("commits");

        let before = serde_json::to_string(session.document()).unwrap();
        let err = session
            .transact("del reject", |tx| -> Result<(), LayerOpError> {
                delete_layer(tx, muros, DeletePolicy::RejectIfUsed)
            })
            .unwrap_err();
        assert_eq!(
            err,
            LayerOpError::LayerInUse {
                layer: muros,
                count: 2
            }
        );
        // Rejection rolls back and preserves the layer.
        assert!(session.document().layer(muros).is_some());
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    #[test]
    fn delete_move_reubica_entidades_en_bloque_y_undo_byte_identico() {
        let (mut session, muros, destino, bid) = doc_with_block();
        let e_block = session
            .transact("seed block", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::Block(bid), point_rec(muros, 0.0))
            })
            .expect("commits")
            .value;
        let e_model = session
            .transact("seed model", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, point_rec(muros, 1.0))
            })
            .expect("commits")
            .value;

        let before = serde_json::to_string(session.document()).unwrap();
        session
            .transact("del move", |tx| -> Result<(), LayerOpError> {
                delete_layer(tx, muros, DeletePolicy::MoveEntitiesTo(destino))
            })
            .expect("commits");

        // The source disappears and both entities move to the target.
        assert!(session.document().layer(muros).is_none());
        assert_eq!(session.document().entity(e_block).unwrap().0.layer, destino);
        assert_eq!(session.document().entity(e_model).unwrap().0.layer, destino);
        // No entity retains the deleted layer.
        assert!(entities_on_layer(session.document(), muros).is_empty());

        // Undo restores byte-identical state, including `nextObjectId`.
        session.undo().expect("undo ok");
        assert_eq!(session.document().entity(e_block).unwrap().0.layer, muros);
        assert_eq!(session.document().entity(e_model).unwrap().0.layer, muros);
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    #[test]
    fn delete_entities_borra_capa_y_sus_entidades_en_bloque_y_model_space() {
        let (mut session, muros, _destino, bid) = doc_with_block();
        let e_block = session
            .transact("seed block", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::Block(bid), point_rec(muros, 0.0))
            })
            .expect("commits")
            .value;
        let e_model = session
            .transact("seed model", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, point_rec(muros, 1.0))
            })
            .expect("commits")
            .value;
        assert_eq!(layer_entity_count(session.document(), muros), 2);

        session
            .transact("del entities", |tx| -> Result<(), LayerOpError> {
                delete_layer(tx, muros, DeletePolicy::DeleteEntities)
            })
            .expect("commits");

        // The layer and both of its entities disappear.
        assert!(session.document().layer(muros).is_none());
        assert!(session.document().entity(e_block).is_none());
        assert!(session.document().entity(e_model).is_none());
    }
}
