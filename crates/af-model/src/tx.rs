//! Transaction system: the document's only mutation path.
//!
//! [`Session::transact`](crate::session::Session::transact) lends a [`TxContext`]
//! that applies each change immediately and records a reversible [`DocOp`] snapshot.
//!
//! Non-empty success commits a [`Transaction`] and change set. Empty success
//! produces neither. Errors apply recorded inverses in reverse for exact rollback.
//!
//! # Operation inverses
//!
//! Add/remove are duals and modifications swap before/after snapshots. Removal
//! snapshots include positions so undo restores exact ordering.
//!
//! # ID identity and rollback
//!
//! A transaction-local cursor allocates IDs. The document allocator advances only
//! on commit, so rollback preserves `nextObjectId`; committed IDs never recycle.
//!
//! # Nested transactions
//!
//! Exclusive borrowing prevents nesting at compile time.
//!
//! # Undo and redo
//!
//! [`apply_forward`] and [`apply_inverse`] apply stored snapshots without command
//! revalidation.

use crate::container::ContainerRef;
use crate::doc::{DocError, Document, Limits};
use crate::entity::{Color, EntityOps, EntityRecord, GeomIssue, LineTypeRef, Lineweight};
use crate::groups::Group;
use crate::id::{EntityId, GroupId, LayerId, ObjectId, StyleId};
use crate::layers::Layer;
use crate::lin::ParsedLinetype;
use crate::styles::LineType;

/// Atomic document operation with snapshots for forward and inverse application.
///
/// Transaction history is session state and is never serialized.
///
/// Exhaustive application matches must be extended with every new variant.
#[derive(Debug, Clone, PartialEq)]
pub enum DocOp {
    /// Entity insertion at draw position `index`.
    AddEntity {
        /// Entity container.
        container: ContainerRef,
        /// Complete inserted record.
        record: EntityRecord,
        /// Draw position; zero is the back.
        index: usize,
    },
    /// Entity removal with record and position for exact inverse ordering.
    RemoveEntity {
        /// Source container.
        container: ContainerRef,
        /// Complete removed record.
        record: EntityRecord,
        /// Former draw position.
        index: usize,
    },
    /// Entity modification with complete before/after records.
    ModifyEntity {
        /// Entity container.
        container: ContainerRef,
        /// Previous state.
        before: EntityRecord,
        /// New state.
        after: EntityRecord,
    },
    /// Layer insertion at creation-order position `index`.
    AddLayer {
        /// Complete inserted layer.
        layer: Layer,
        /// Layer-table position.
        index: usize,
    },
    /// Layer modification with complete before/after records.
    ModifyLayer {
        /// Previous state.
        before: Layer,
        /// New state.
        after: Layer,
    },
    /// Layer removal with record and position for exact inverse ordering.
    RemoveLayer {
        /// Complete removed layer.
        layer: Layer,
        /// Former layer-table position.
        index: usize,
    },
    /// Group insertion at creation-order position `index`.
    AddGroup {
        /// Complete inserted group.
        group: Group,
        /// Group-table position.
        index: usize,
    },
    /// Group modification with complete before/after records.
    ModifyGroup {
        /// Previous state.
        before: Group,
        /// New state.
        after: Group,
    },
    /// Group removal with record and position for exact inverse ordering.
    RemoveGroup {
        /// Complete removed group.
        group: Group,
        /// Former group-table position.
        index: usize,
    },
    /// Line-type insertion at creation-order position `index`.
    AddLineType {
        /// Complete inserted line type.
        line_type: LineType,
        /// Line-type table position.
        index: usize,
    },
    /// Line-type removal with record and position for exact inverse ordering.
    RemoveLineType {
        /// Complete removed line type.
        line_type: LineType,
        /// Former table position.
        index: usize,
    },
    /// Document-property change.
    SetDocProp(DocProp),
}

/// Document property changed by [`DocOp::SetDocProp`].
///
/// These reversible metadata changes allocate no IDs. Uses `PartialEq` because
/// variants contain floating-point values.
#[derive(Debug, Clone, PartialEq)]
pub enum DocProp {
    /// Current-layer change.
    CurrentLayer {
        /// Previous layer.
        before: LayerId,
        /// New layer.
        after: LayerId,
    },
    /// Current-color change.
    CurrentColor {
        /// Previous color.
        before: Color,
        /// New color.
        after: Color,
    },
    /// Linear display-precision change.
    LinearPrecision {
        /// Previous precision.
        before: u8,
        /// New precision.
        after: u8,
    },
    /// Drawing-limits change.
    Limits {
        /// Previous limits.
        before: Limits,
        /// New limits.
        after: Limits,
    },
    /// Current line-type change.
    CurrentLineType {
        /// Previous line type.
        before: LineTypeRef,
        /// New line type.
        after: LineTypeRef,
    },
    /// Current lineweight change.
    CurrentLineweight {
        /// Previous lineweight.
        before: Lineweight,
        /// New lineweight.
        after: Lineweight,
    },
    /// `LTSCALE` change.
    Ltscale {
        /// Previous scale.
        before: f64,
        /// New scale.
        after: f64,
    },
}

/// Committed atomic batch of [`DocOp`] values with a label and sequence number.
///
/// Only [`Session::transact`](crate::session::Session::transact) can construct it.
#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    seq: u64,
    label: String,
    ops: Vec<DocOp>,
}

impl Transaction {
    /// Creates a crate-internal transaction.
    pub(crate) fn new(seq: u64, label: String, ops: Vec<DocOp>) -> Self {
        Self { seq, label, ops }
    }

    /// Monotonic per-session sequence number.
    #[must_use]
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Human-readable transaction label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Operations in application order.
    #[must_use]
    pub fn ops(&self) -> &[DocOp] {
        &self.ops
    }

    /// Number of operations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether the transaction has no operations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// Transaction operation error.
///
/// [`TxContext`] methods return errors instead of panicking; closure errors roll back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TxError {
    /// Entity ID does not exist.
    UnknownEntity(EntityId),
    /// Referenced container does not exist.
    UnknownContainer(ContainerRef),
    /// Entity references an unknown layer.
    UnknownLayer(LayerId),
    /// Entity references an unknown explicit line type.
    UnknownLineType(StyleId),
    /// Entity geometry is invalid.
    InvalidGeometry(GeomIssue),
    /// Layer name is already in use case-insensitively.
    DuplicateLayerName(String),
    /// Layer `"0"` is protected from deletion.
    LayerZeroProtected(LayerId),
    /// Current layer cannot be deleted.
    CurrentLayerRemoval(LayerId),
    /// Layer is still referenced by an entity.
    LayerInUse(LayerId),
    /// Group name is already in use case-insensitively.
    DuplicateGroupName(String),
    /// Group ID does not exist.
    UnknownGroup(GroupId),
    /// Line-type name is already in use case-insensitively.
    DuplicateLineTypeName(String),
    /// `"Continuous"` is protected from deletion.
    LineTypeProtected(StyleId),
    /// Line type is still referenced.
    LineTypeInUse(StyleId),
    /// Internal inconsistency while applying or reversing an operation.
    Internal(&'static str),
}

impl core::fmt::Display for TxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TxError::UnknownEntity(id) => write!(f, "unknown entity id {}", id.raw().0),
            TxError::UnknownContainer(c) => write!(f, "unknown container {c:?}"),
            TxError::UnknownLayer(id) => write!(f, "unknown layer id {}", id.raw().0),
            TxError::UnknownLineType(id) => write!(f, "unknown line type id {}", id.raw().0),
            TxError::InvalidGeometry(g) => write!(f, "invalid geometry: {g}"),
            TxError::DuplicateLayerName(name) => {
                write!(f, "duplicate layer name (case-insensitive): {name:?}")
            }
            TxError::LayerZeroProtected(id) => {
                write!(f, "layer \"0\" is indelible (id {})", id.raw().0)
            }
            TxError::CurrentLayerRemoval(id) => {
                write!(f, "cannot remove the current layer (id {})", id.raw().0)
            }
            TxError::LayerInUse(id) => {
                write!(
                    f,
                    "layer id {} is referenced by at least one entity",
                    id.raw().0
                )
            }
            TxError::DuplicateGroupName(name) => {
                write!(f, "duplicate group name (case-insensitive): {name:?}")
            }
            TxError::UnknownGroup(id) => write!(f, "unknown group id {}", id.raw().0),
            TxError::DuplicateLineTypeName(name) => {
                write!(f, "duplicate line type name (case-insensitive): {name:?}")
            }
            TxError::LineTypeProtected(id) => {
                write!(
                    f,
                    "line type \"Continuous\" is indelible (id {})",
                    id.raw().0
                )
            }
            TxError::LineTypeInUse(id) => {
                write!(
                    f,
                    "line type id {} is still referenced (layer, entity or CELTYPE)",
                    id.raw().0
                )
            }
            TxError::Internal(msg) => write!(f, "internal transaction error: {msg}"),
        }
    }
}

impl std::error::Error for TxError {}

impl From<DocError> for TxError {
    fn from(e: DocError) -> Self {
        match e {
            DocError::UnknownLayer(id) => TxError::UnknownLayer(id),
            DocError::UnknownLineType(id) => TxError::UnknownLineType(id),
            DocError::IdExhausted => TxError::Internal("persistent object id space exhausted"),
            // Preserve this unreachable mapping to keep conversion exhaustive.
            DocError::DuplicateName { .. } => TxError::Internal("unexpected duplicate-name error"),
        }
    }
}

/// Result of [`TxContext::load_linetypes`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadLinetypesReport {
    /// Newly assigned IDs in input order.
    pub loaded: Vec<StyleId>,
    /// Names skipped because a case-insensitive match already existed.
    pub skipped_existing: Vec<String>,
}

/// Document mutation surface borrowed by a transaction closure.
///
/// Applies each change immediately and records its [`DocOp`].
pub struct TxContext<'a> {
    doc: &'a mut Document,
    ops: Vec<DocOp>,
    /// Transaction-local next-ID cursor committed only on success.
    id_cursor: u64,
}

impl<'a> TxContext<'a> {
    /// Creates a context with an initial local ID cursor.
    pub(crate) fn new(doc: &'a mut Document, start_next: u64) -> Self {
        Self {
            doc,
            ops: Vec::new(),
            id_cursor: start_next,
        }
    }

    /// Consumes the context and returns recorded operations and the final ID cursor.
    pub(crate) fn into_parts(self) -> (Vec<DocOp>, u64) {
        (self.ops, self.id_cursor)
    }

    /// Reads the document including operations already applied by this closure.
    #[must_use]
    pub fn doc(&self) -> &Document {
        &*self.doc
    }

    /// Consumes a valid ID from the deferred cursor.
    fn alloc_id(&mut self) -> Result<ObjectId, TxError> {
        if self.id_cursor == 0 || self.id_cursor == u64::MAX {
            return Err(TxError::Internal("persistent object id space exhausted"));
        }
        let id = ObjectId(self.id_cursor);
        self.id_cursor += 1;
        Ok(id)
    }

    /// Inserts an entity into `container` and returns its newly assigned ID.
    ///
    /// The incoming record ID is ignored. The record is appended in draw order.
    ///
    /// # Errors
    /// Returns [`TxError`] for invalid geometry or unknown references.
    ///
    /// Errors consume no ID and do not mutate the document.
    pub fn add_entity(
        &mut self,
        container: ContainerRef,
        mut record: EntityRecord,
    ) -> Result<EntityId, TxError> {
        // Validate before mutation so rejection consumes no ID.
        self.validate_record(&record)?;
        if self.doc.container(container).is_none() {
            return Err(TxError::UnknownContainer(container));
        }
        let id: EntityId = self.alloc_id()?.into();
        record.id = id;
        let c = self
            .doc
            .container_mut(container)
            .ok_or(TxError::Internal("add: container vanished"))?;
        let index = c.push(record.clone());
        self.ops.push(DocOp::AddEntity {
            container,
            record,
            index,
        });
        Ok(id)
    }

    /// Removes an entity from any container and records its draw position.
    ///
    /// # Errors
    /// Returns [`TxError::UnknownEntity`] for an unknown ID.
    pub fn remove_entity(&mut self, id: EntityId) -> Result<(), TxError> {
        let container = self
            .doc
            .entity(id)
            .map(|(_, c)| c)
            .ok_or(TxError::UnknownEntity(id))?;
        let c = self
            .doc
            .container_mut(container)
            .ok_or(TxError::Internal("remove: container vanished"))?;
        let (record, index) = c
            .remove_by_id(id)
            .ok_or(TxError::Internal("remove: entity vanished"))?;
        self.ops.push(DocOp::RemoveEntity {
            container,
            record,
            index,
        });
        Ok(())
    }

    /// Applies `edit` to a record copy while preserving immutable identity.
    ///
    /// An unchanged result records no operation.
    ///
    /// # Errors
    /// Returns [`TxError`] for unknown entities or invalid edited records.
    pub fn modify_entity(
        &mut self,
        id: EntityId,
        edit: impl FnOnce(&mut EntityRecord),
    ) -> Result<(), TxError> {
        let (before, container) = match self.doc.entity(id) {
            Some((rec, c)) => (rec, c),
            None => return Err(TxError::UnknownEntity(id)),
        };
        let mut after = before.clone();
        edit(&mut after);
        after.id = before.id; // Identity is immutable.
        self.validate_record(&after)?;
        if after == before {
            return Ok(()); // No change to record.
        }
        let c = self
            .doc
            .container_mut(container)
            .ok_or(TxError::Internal("modify: container vanished"))?;
        if !c.replace(id, after.clone()) {
            return Err(TxError::Internal("modify: entity vanished"));
        }
        self.ops.push(DocOp::ModifyEntity {
            container,
            before,
            after,
        });
        Ok(())
    }

    /// Sets the current document layer.
    ///
    /// Setting the existing layer is a no-op.
    ///
    /// # Errors
    /// Returns [`TxError::UnknownLayer`] for an unknown layer.
    pub fn set_current_layer(&mut self, layer: LayerId) -> Result<(), TxError> {
        let before = self.doc.current_layer();
        if before == layer {
            return Ok(());
        }
        self.doc.set_current_layer(layer)?; // Validates existence.
        self.ops.push(DocOp::SetDocProp(DocProp::CurrentLayer {
            before,
            after: layer,
        }));
        Ok(())
    }

    /// Sets the current document color for new entities.
    ///
    /// Setting the existing color is a no-op.
    pub fn set_current_color(&mut self, color: Color) {
        let before = self.doc.current_color();
        if before == color {
            return;
        }
        self.doc.set_current_color(color);
        self.ops.push(DocOp::SetDocProp(DocProp::CurrentColor {
            before,
            after: color,
        }));
    }

    /// Sets linear display precision.
    ///
    /// Setting the existing precision is a no-op; callers clamp the range.
    pub fn set_linear_precision(&mut self, precision: u8) {
        let before = self.doc.linear_precision();
        if before == precision {
            return;
        }
        self.doc.set_linear_precision(precision);
        self.ops.push(DocOp::SetDocProp(DocProp::LinearPrecision {
            before,
            after: precision,
        }));
    }

    /// Sets drawing limits.
    ///
    /// Setting the existing limits is a no-op.
    pub fn set_limits(&mut self, limits: Limits) {
        let before = self.doc.limits();
        if before == limits {
            return;
        }
        self.doc.set_limits(limits);
        self.ops.push(DocOp::SetDocProp(DocProp::Limits {
            before,
            after: limits,
        }));
    }

    /// Sets the current document line type.
    ///
    /// Setting the existing value is a no-op; explicit style IDs must exist.
    ///
    /// # Errors
    /// Returns [`TxError::UnknownLineType`] for an unknown explicit style ID.
    pub fn set_current_line_type(&mut self, lt: LineTypeRef) -> Result<(), TxError> {
        let before = self.doc.current_line_type();
        if before == lt {
            return Ok(());
        }
        self.doc.set_current_line_type(lt)?; // Validates existence.
        self.ops.push(DocOp::SetDocProp(DocProp::CurrentLineType {
            before,
            after: lt,
        }));
        Ok(())
    }

    /// Sets the current document lineweight.
    ///
    /// Setting the existing value is a no-op.
    pub fn set_current_lineweight(&mut self, lw: Lineweight) {
        let before = self.doc.current_lineweight();
        if before == lw {
            return;
        }
        self.doc.set_current_lineweight(lw);
        self.ops.push(DocOp::SetDocProp(DocProp::CurrentLineweight {
            before,
            after: lw,
        }));
    }

    /// Sets `LTSCALE`, rejecting nonpositive or nonfinite values.
    ///
    /// Setting the existing value is a no-op.
    ///
    /// # Errors
    /// Returns [`TxError::InvalidGeometry`] for invalid values.
    pub fn set_ltscale(&mut self, scale: f64) -> Result<(), TxError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(TxError::InvalidGeometry(GeomIssue::NonFinite));
        }
        let before = self.doc.ltscale();
        if before == scale {
            return Ok(());
        }
        self.doc.set_ltscale(scale);
        self.ops.push(DocOp::SetDocProp(DocProp::Ltscale {
            before,
            after: scale,
        }));
        Ok(())
    }

    /// Inserts a line type and returns its newly assigned ID.
    ///
    /// # Errors
    /// Returns [`TxError::DuplicateLineTypeName`] without consuming an ID.
    pub fn add_line_type_raw(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        pattern: Vec<f64>,
    ) -> Result<StyleId, TxError> {
        let name = name.into();
        if self.doc.line_type_by_name(&name).is_some() {
            return Err(TxError::DuplicateLineTypeName(name));
        }
        let id: StyleId = self.alloc_id()?.into();
        let lt = LineType::with_pattern(id, name, description, pattern);
        let index = self.doc.push_line_type(lt.clone());
        self.ops.push(DocOp::AddLineType {
            line_type: lt,
            index,
        });
        Ok(id)
    }

    /// Removes line type `id` while recording its position for inverse restore.
    ///
    /// `"Continuous"` and referenced line types cannot be removed.
    ///
    /// # Errors
    /// Returns the corresponding [`TxError`] for unknown or protected line types.
    pub fn remove_line_type_raw(&mut self, id: StyleId) -> Result<(), TxError> {
        let is_continuous = {
            let lt = self.doc.line_type(id).ok_or(TxError::UnknownLineType(id))?;
            lt.name().eq_ignore_ascii_case("Continuous")
        };
        if is_continuous {
            return Err(TxError::LineTypeProtected(id));
        }
        if self.doc.current_line_type() == LineTypeRef::Style(id)
            || Self::line_type_in_use(self.doc(), id)
        {
            return Err(TxError::LineTypeInUse(id));
        }
        let (index, line_type) = self
            .doc
            .remove_line_type(id)
            .ok_or(TxError::Internal("remove_line_type: line type vanished"))?;
        self.ops.push(DocOp::RemoveLineType { line_type, index });
        Ok(())
    }

    /// Loads parsed `.lin` definitions through [`add_line_type_raw`](Self::add_line_type_raw).
    ///
    /// Case-insensitive duplicates are skipped; new IDs retain input order.
    ///
    /// # Errors
    /// Propagates insertion errors so the outer transaction can roll back the batch.
    pub fn load_linetypes(
        &mut self,
        defs: impl IntoIterator<Item = ParsedLinetype>,
    ) -> Result<LoadLinetypesReport, TxError> {
        let mut report = LoadLinetypesReport::default();
        for def in defs {
            match self.add_line_type_raw(&def.name, def.description, def.pattern) {
                Ok(id) => report.loaded.push(id),
                Err(TxError::DuplicateLineTypeName(_)) => {
                    report.skipped_existing.push(def.name);
                }
                Err(other) => return Err(other),
            }
        }
        Ok(report)
    }

    /// Whether any layer or entity explicitly references `lt`.
    fn line_type_in_use(doc: &Document, lt: StyleId) -> bool {
        if doc.layers().any(|l| l.line_type() == lt) {
            return true;
        }
        let refs_lt = |r: &EntityRecord| r.line_type == LineTypeRef::Style(lt);
        doc.model_space().iter_records().any(|r| refs_lt(&r))
            || doc
                .layouts()
                .any(|l| l.entities().iter_records().any(|r| refs_lt(&r)))
            || doc
                .blocks()
                .any(|b| b.entities().iter_records().any(|r| refs_lt(&r)))
    }

    /// Inserts a layer and returns its newly assigned ID.
    ///
    /// The incoming ID is ignored; the shared deferred cursor assigns it. The layer
    /// is appended in creation order.
    ///
    /// Validates case-insensitive uniqueness and default line-type existence.
    ///
    /// # Errors
    /// Returns [`TxError::DuplicateLayerName`] or [`TxError::UnknownLineType`].
    ///
    /// Errors consume no ID and do not mutate the document.
    pub fn add_layer_raw(&mut self, layer: Layer) -> Result<LayerId, TxError> {
        // Validate before mutation so rejection consumes no ID.
        if self.doc.layer_by_name(layer.name()).is_some() {
            return Err(TxError::DuplicateLayerName(layer.name().to_string()));
        }
        if self.doc.line_type(layer.line_type()).is_none() {
            return Err(TxError::UnknownLineType(layer.line_type()));
        }
        let id: LayerId = self.alloc_id()?.into();
        let layer = layer.with_id(id);
        let index = self.doc.push_layer(layer.clone());
        self.ops.push(DocOp::AddLayer { layer, index });
        Ok(id)
    }

    /// Replaces layer `id` while preserving position and immutable identity.
    ///
    /// Unchanged replacements are no-ops. Renames must remain unique, line types
    /// must exist, and layer `"0"` cannot be renamed.
    ///
    /// # Errors
    /// Returns the corresponding [`TxError`] for unknown/protected layers,
    /// duplicate names, or unknown line types.
    ///
    /// Validation errors do not mutate the document.
    pub fn modify_layer_raw(&mut self, id: LayerId, layer: Layer) -> Result<(), TxError> {
        let before = match self.doc.layer(id) {
            Some(l) => l.clone(),
            None => return Err(TxError::UnknownLayer(id)),
        };
        let after = layer.with_id(id); // Identity is immutable.
        if after == before {
            return Ok(()); // No change to record.
        }
        // Layer "0" keeps a stable name so deletion protection cannot be bypassed.
        if before.name().eq_ignore_ascii_case("0") && !after.name().eq_ignore_ascii_case("0") {
            return Err(TxError::LayerZeroProtected(id));
        }
        // A rename cannot collide with another layer.
        if let Some(other) = self.doc.layer_by_name(after.name())
            && other.id() != id
        {
            return Err(TxError::DuplicateLayerName(after.name().to_string()));
        }
        // The resulting default line type must exist.
        if self.doc.line_type(after.line_type()).is_none() {
            return Err(TxError::UnknownLineType(after.line_type()));
        }
        self.doc
            .replace_layer(id, after.clone())
            .ok_or(TxError::Internal("modify_layer: layer vanished"))?;
        self.ops.push(DocOp::ModifyLayer { before, after });
        Ok(())
    }

    /// Removes layer `id` while recording its table position for inverse restore.
    ///
    /// Layer `"0"`, the current layer, and referenced layers cannot be removed.
    ///
    /// # Errors
    /// Returns the corresponding [`TxError`] for unknown or protected layers.
    pub fn remove_layer_raw(&mut self, id: LayerId) -> Result<(), TxError> {
        let is_zero = {
            let layer = self.doc.layer(id).ok_or(TxError::UnknownLayer(id))?;
            layer.name().eq_ignore_ascii_case("0")
        };
        if is_zero {
            return Err(TxError::LayerZeroProtected(id));
        }
        if self.doc.current_layer() == id {
            return Err(TxError::CurrentLayerRemoval(id));
        }
        if Self::layer_in_use(self.doc(), id) {
            return Err(TxError::LayerInUse(id));
        }
        let (index, layer) = self
            .doc
            .remove_layer(id)
            .ok_or(TxError::Internal("remove_layer: layer vanished"))?;
        self.ops.push(DocOp::RemoveLayer { layer, index });
        Ok(())
    }

    /// Whether any entity in any container references `layer`.
    fn layer_in_use(doc: &Document, layer: LayerId) -> bool {
        if doc.model_space().iter_records().any(|r| r.layer == layer) {
            return true;
        }
        if doc
            .layouts()
            .any(|l| l.entities().iter_records().any(|r| r.layer == layer))
        {
            return true;
        }
        doc.blocks()
            .any(|b| b.entities().iter_records().any(|r| r.layer == layer))
    }

    /// Inserts a group and returns its newly assigned ID.
    ///
    /// The deferred cursor assigns its ID. Names must be unique and every member
    /// must reference an existing entity.
    ///
    /// # Errors
    /// Returns [`TxError::DuplicateGroupName`] or [`TxError::UnknownEntity`].
    ///
    /// Errors consume no ID and do not mutate the document.
    pub fn add_group_raw(&mut self, group: Group) -> Result<GroupId, TxError> {
        // Validate before mutation so rejection consumes no ID.
        if self.doc.group_by_name(group.name()).is_some() {
            return Err(TxError::DuplicateGroupName(group.name().to_string()));
        }
        self.validate_members(group.members())?;
        let id: GroupId = self.alloc_id()?.into();
        let group = group.with_id(id);
        let index = self.doc.push_group(group.clone());
        self.ops.push(DocOp::AddGroup { group, index });
        Ok(id)
    }

    /// Replaces group `id` while preserving table position and identity.
    ///
    /// Unchanged replacements are no-ops. Names remain unique and members must exist.
    ///
    /// # Errors
    /// Returns the corresponding [`TxError`] for unknown groups, duplicates, or members.
    ///
    /// Validation errors do not mutate the document.
    pub fn modify_group_raw(&mut self, id: GroupId, group: Group) -> Result<(), TxError> {
        let before = match self.doc.group(id) {
            Some(g) => g.clone(),
            None => return Err(TxError::UnknownGroup(id)),
        };
        let after = group.with_id(id); // Identity is immutable.
        if after == before {
            return Ok(()); // No change to record.
        }
        if let Some(other) = self.doc.group_by_name(after.name())
            && other.id() != id
        {
            return Err(TxError::DuplicateGroupName(after.name().to_string()));
        }
        self.validate_members(after.members())?;
        self.doc
            .replace_group(id, after.clone())
            .ok_or(TxError::Internal("modify_group: group vanished"))?;
        self.ops.push(DocOp::ModifyGroup { before, after });
        Ok(())
    }

    /// Removes group `id` while preserving its position; entities remain intact.
    ///
    /// # Errors
    /// Returns [`TxError::UnknownGroup`] for an unknown ID.
    pub fn remove_group_raw(&mut self, id: GroupId) -> Result<(), TxError> {
        if self.doc.group(id).is_none() {
            return Err(TxError::UnknownGroup(id));
        }
        let (index, group) = self
            .doc
            .remove_group(id)
            .ok_or(TxError::Internal("remove_group: group vanished"))?;
        self.ops.push(DocOp::RemoveGroup { group, index });
        Ok(())
    }

    /// Validates that every member ID references an existing entity.
    fn validate_members(&self, members: &[EntityId]) -> Result<(), TxError> {
        for &m in members {
            if self.doc.entity(m).is_none() {
                return Err(TxError::UnknownEntity(m));
            }
        }
        Ok(())
    }

    /// Validates geometry plus layer and explicit line-type references.
    fn validate_record(&self, record: &EntityRecord) -> Result<(), TxError> {
        record
            .geometry
            .validate(&self.doc.tolerances())
            .map_err(TxError::InvalidGeometry)?;
        if self.doc.layer(record.layer).is_none() {
            return Err(TxError::UnknownLayer(record.layer));
        }
        if let LineTypeRef::Style(sid) = record.line_type
            && self.doc.line_type(sid).is_none()
        {
            return Err(TxError::UnknownLineType(sid));
        }
        Ok(())
    }
}

/// Applies a transaction forward.
///
/// Applies operations in order without command-rule revalidation.
///
/// # Errors
/// Returns [`TxError`] when the document is not in the expected state.
pub fn apply_forward(doc: &mut Document, transaction: &Transaction) -> Result<(), TxError> {
    for op in transaction.ops() {
        apply_op_forward(doc, op)?;
    }
    Ok(())
}

/// Applies a transaction inverse.
///
/// Applies inverses in reverse order and does not change the ID allocator.
///
/// # Errors
/// Returns [`TxError`] when the document is not in the expected state.
pub fn apply_inverse(doc: &mut Document, transaction: &Transaction) -> Result<(), TxError> {
    for op in transaction.ops().iter().rev() {
        apply_op_inverse(doc, op)?;
    }
    Ok(())
}

/// Applies one operation forward.
pub(crate) fn apply_op_forward(doc: &mut Document, op: &DocOp) -> Result<(), TxError> {
    match op {
        DocOp::AddEntity {
            container,
            record,
            index,
        } => {
            let c = doc
                .container_mut(*container)
                .ok_or(TxError::UnknownContainer(*container))?;
            c.insert_at(*index, record.clone());
            Ok(())
        }
        DocOp::RemoveEntity {
            container, record, ..
        } => {
            let c = doc
                .container_mut(*container)
                .ok_or(TxError::UnknownContainer(*container))?;
            c.remove_by_id(record.id)
                .ok_or(TxError::Internal("forward remove: entity absent"))?;
            Ok(())
        }
        DocOp::ModifyEntity {
            container, after, ..
        } => {
            let c = doc
                .container_mut(*container)
                .ok_or(TxError::UnknownContainer(*container))?;
            if !c.replace(after.id, after.clone()) {
                return Err(TxError::Internal("forward modify: entity absent"));
            }
            Ok(())
        }
        DocOp::AddLayer { layer, index } => {
            doc.insert_layer_at(*index, layer.clone());
            Ok(())
        }
        DocOp::RemoveLayer { layer, .. } => {
            doc.remove_layer(layer.id())
                .ok_or(TxError::Internal("forward remove layer: layer absent"))?;
            Ok(())
        }
        DocOp::ModifyLayer { after, .. } => {
            doc.replace_layer(after.id(), after.clone())
                .ok_or(TxError::Internal("forward modify layer: layer absent"))?;
            Ok(())
        }
        DocOp::AddGroup { group, index } => {
            doc.insert_group_at(*index, group.clone());
            Ok(())
        }
        DocOp::RemoveGroup { group, .. } => {
            doc.remove_group(group.id())
                .ok_or(TxError::Internal("forward remove group: group absent"))?;
            Ok(())
        }
        DocOp::ModifyGroup { after, .. } => {
            doc.replace_group(after.id(), after.clone())
                .ok_or(TxError::Internal("forward modify group: group absent"))?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentLayer { after, .. }) => {
            doc.set_current_layer(*after)?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentColor { after, .. }) => {
            doc.set_current_color(*after);
            Ok(())
        }
        DocOp::SetDocProp(DocProp::LinearPrecision { after, .. }) => {
            doc.set_linear_precision(*after);
            Ok(())
        }
        DocOp::SetDocProp(DocProp::Limits { after, .. }) => {
            doc.set_limits(*after);
            Ok(())
        }
        DocOp::AddLineType { line_type, index } => {
            doc.insert_line_type_at(*index, line_type.clone());
            Ok(())
        }
        DocOp::RemoveLineType { line_type, .. } => {
            doc.remove_line_type(line_type.id())
                .ok_or(TxError::Internal("forward remove line type: absent"))?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentLineType { after, .. }) => {
            doc.set_current_line_type(*after)?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentLineweight { after, .. }) => {
            doc.set_current_lineweight(*after);
            Ok(())
        }
        DocOp::SetDocProp(DocProp::Ltscale { after, .. }) => {
            doc.set_ltscale(*after);
            Ok(())
        }
    }
}

/// Applies one operation inverse.
pub(crate) fn apply_op_inverse(doc: &mut Document, op: &DocOp) -> Result<(), TxError> {
    match op {
        DocOp::AddEntity {
            container, record, ..
        } => {
            let c = doc
                .container_mut(*container)
                .ok_or(TxError::UnknownContainer(*container))?;
            c.remove_by_id(record.id)
                .ok_or(TxError::Internal("inverse add: entity absent"))?;
            Ok(())
        }
        DocOp::RemoveEntity {
            container,
            record,
            index,
        } => {
            let c = doc
                .container_mut(*container)
                .ok_or(TxError::UnknownContainer(*container))?;
            c.insert_at(*index, record.clone());
            Ok(())
        }
        DocOp::ModifyEntity {
            container, before, ..
        } => {
            let c = doc
                .container_mut(*container)
                .ok_or(TxError::UnknownContainer(*container))?;
            if !c.replace(before.id, before.clone()) {
                return Err(TxError::Internal("inverse modify: entity absent"));
            }
            Ok(())
        }
        DocOp::AddLayer { layer, .. } => {
            doc.remove_layer(layer.id())
                .ok_or(TxError::Internal("inverse add layer: layer absent"))?;
            Ok(())
        }
        DocOp::RemoveLayer { layer, index } => {
            doc.insert_layer_at(*index, layer.clone());
            Ok(())
        }
        DocOp::ModifyLayer { before, .. } => {
            doc.replace_layer(before.id(), before.clone())
                .ok_or(TxError::Internal("inverse modify layer: layer absent"))?;
            Ok(())
        }
        DocOp::AddGroup { group, .. } => {
            doc.remove_group(group.id())
                .ok_or(TxError::Internal("inverse add group: group absent"))?;
            Ok(())
        }
        DocOp::RemoveGroup { group, index } => {
            doc.insert_group_at(*index, group.clone());
            Ok(())
        }
        DocOp::ModifyGroup { before, .. } => {
            doc.replace_group(before.id(), before.clone())
                .ok_or(TxError::Internal("inverse modify group: group absent"))?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentLayer { before, .. }) => {
            doc.set_current_layer(*before)?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentColor { before, .. }) => {
            doc.set_current_color(*before);
            Ok(())
        }
        DocOp::SetDocProp(DocProp::LinearPrecision { before, .. }) => {
            doc.set_linear_precision(*before);
            Ok(())
        }
        DocOp::SetDocProp(DocProp::Limits { before, .. }) => {
            doc.set_limits(*before);
            Ok(())
        }
        DocOp::AddLineType { line_type, .. } => {
            doc.remove_line_type(line_type.id())
                .ok_or(TxError::Internal("inverse add line type: absent"))?;
            Ok(())
        }
        DocOp::RemoveLineType { line_type, index } => {
            doc.insert_line_type_at(*index, line_type.clone());
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentLineType { before, .. }) => {
            doc.set_current_line_type(*before)?;
            Ok(())
        }
        DocOp::SetDocProp(DocProp::CurrentLineweight { before, .. }) => {
            doc.set_current_lineweight(*before);
            Ok(())
        }
        DocOp::SetDocProp(DocProp::Ltscale { before, .. }) => {
            doc.set_ltscale(*before);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::{Point2, Vec2};

    use crate::entity::{Color, EntityGeometry, LineGeo, Lineweight, PointGeo};
    use crate::session::Session;
    use crate::units::Units;

    fn line_rec(layer: LayerId, x: f64) -> EntityRecord {
        EntityRecord::new(
            ObjectId::NIL.into(), // Ignored: `add_entity` assigns the ID.
            layer,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Line(LineGeo::new(Point2::new(x, 0.0), Point2::new(x + 1.0, 0.0))),
        )
    }

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

    /// Inserts three model-space entities and returns IDs in draw order.
    fn seed_three(session: &mut Session, layer: LayerId) -> Vec<EntityId> {
        session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![
                    tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 0.0))?,
                    tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 1.0))?,
                    tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, 2.0))?,
                ])
            })
            .expect("seed commits")
            .value
    }

    // Context-free forward/inverse application.

    #[test]
    fn apply_inverse_restaura_draw_order_tras_remove_del_medio() {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let ids = seed_three(&mut session, l0);

        // Remove the middle entity at position 1.
        let out = session
            .transact("erase middle", |tx| -> Result<(), TxError> {
                tx.remove_entity(ids[1])
            })
            .expect("erase commits");
        let tx = out.transaction.expect("una transacción");

        // Remaining order is [0, 2].
        let order: Vec<EntityId> = session
            .document()
            .model_space()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(order, vec![ids[0], ids[2]]);

        // Inverse restores the middle entity at its exact position.
        apply_inverse(session.document_mut(), &tx).expect("inverse ok");
        let restored: Vec<EntityId> = session
            .document()
            .model_space()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(restored, vec![ids[0], ids[1], ids[2]]);
        assert_eq!(session.document().model_space().index_of(ids[1]), Some(1));
    }

    #[test]
    fn apply_forward_es_inverso_de_apply_inverse() {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let ids = seed_three(&mut session, l0);

        let out = session
            .transact("erase middle", |tx| -> Result<(), TxError> {
                tx.remove_entity(ids[1])
            })
            .expect("commits");
        let tx = out.transaction.unwrap();
        let after_remove = serde_json::to_string(session.document()).unwrap();

        // Inverse then forward returns to byte-identical post-removal state.
        apply_inverse(session.document_mut(), &tx).unwrap();
        apply_forward(session.document_mut(), &tx).unwrap();
        let redone = serde_json::to_string(session.document()).unwrap();
        assert_eq!(after_remove, redone);
    }

    // Current-layer property changes.

    #[test]
    fn set_current_layer_transaccional_y_reversible() {
        // Create a document with a second layer.
        let mut doc = Document::new(Units::default());
        let continuous = doc.line_types().next().unwrap().id();
        let muros = doc
            .add_layer("Muros", Color::ByLayer, continuous, Lineweight::ByLayer)
            .unwrap();
        let l0 = doc.current_layer();
        let mut session = Session::from_document(doc);

        let out = session
            .transact("set current", |tx| -> Result<(), TxError> {
                tx.set_current_layer(muros)
            })
            .expect("commits");
        assert_eq!(session.document().current_layer(), muros);
        let cs = out.change_set.expect("changeset");
        assert!(cs.doc_changed());
        assert!(cs.added().is_empty() && cs.removed().is_empty() && cs.modified().is_empty());

        // Inverse restores the previous layer.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().current_layer(), l0);

        // Setting the same layer is a no-op.
        let noop = session
            .transact("noop", |tx| -> Result<(), TxError> {
                let cur = tx.doc().current_layer();
                tx.set_current_layer(cur)
            })
            .expect("commits");
        assert!(noop.transaction.is_none());
    }

    #[test]
    fn set_current_layer_capa_inexistente_es_error() {
        let mut session = Session::new(Units::default());
        let ghost: LayerId = ObjectId(987_654).into();
        let res = session.transact("bad", |tx| -> Result<(), TxError> {
            tx.set_current_layer(ghost)
        });
        assert_eq!(res.unwrap_err(), TxError::UnknownLayer(ghost));
    }

    #[test]
    fn rollback_restaura_la_capa_actual() {
        let mut doc = Document::new(Units::default());
        let continuous = doc.line_types().next().unwrap().id();
        let muros = doc
            .add_layer("Muros", Color::ByLayer, continuous, Lineweight::ByLayer)
            .unwrap();
        let l0 = doc.current_layer();
        let mut session = Session::from_document(doc);
        let before = serde_json::to_string(session.document()).unwrap();

        let res = session.transact("boom", |tx| -> Result<(), TxError> {
            tx.set_current_layer(muros)?;
            Err(TxError::Internal("fallo"))
        });
        assert!(res.is_err());
        assert_eq!(session.document().current_layer(), l0);
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    // Current-color property changes.

    #[test]
    fn set_current_color_transaccional_y_reversible() {
        let mut session = Session::new(Units::default());
        assert_eq!(session.document().current_color(), Color::ByLayer);

        let out = session
            .transact("set current color", |tx: &mut TxContext<'_>| {
                tx.set_current_color(Color::aci(3).unwrap());
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert_eq!(session.document().current_color(), Color::aci(3).unwrap());
        let cs = out.change_set.expect("changeset");
        assert!(cs.doc_changed());
        assert!(cs.added().is_empty() && cs.removed().is_empty() && cs.modified().is_empty());

        // Inverse restores the previous color.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().current_color(), Color::ByLayer);

        // Setting the same color is a no-op.
        let noop = session
            .transact("noop", |tx: &mut TxContext<'_>| {
                let cur = tx.doc().current_color();
                tx.set_current_color(cur);
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert!(noop.transaction.is_none());
    }

    #[test]
    fn rollback_restaura_el_color_actual() {
        let mut session = Session::new(Units::default());
        let before = serde_json::to_string(session.document()).unwrap();

        let res = session.transact("boom", |tx| -> Result<(), TxError> {
            tx.set_current_color(Color::aci(9).unwrap());
            Err(TxError::Internal("fallo"))
        });
        assert!(res.is_err());
        assert_eq!(session.document().current_color(), Color::ByLayer);
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    // Display precision and drawing-limit changes.

    #[test]
    fn set_linear_precision_transaccional_y_reversible() {
        let mut session = Session::new(Units::default());
        assert_eq!(session.document().linear_precision(), 4);

        let out = session
            .transact("units", |tx: &mut TxContext<'_>| {
                tx.set_linear_precision(2);
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert_eq!(session.document().linear_precision(), 2);
        let cs = out.change_set.expect("changeset");
        assert!(cs.doc_changed());

        // Inverse restores the previous precision.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().linear_precision(), 4);
        apply_forward(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().linear_precision(), 2);

        // Setting the same precision is a no-op.
        let noop = session
            .transact("noop", |tx: &mut TxContext<'_>| {
                tx.set_linear_precision(2);
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert!(noop.transaction.is_none());
    }

    #[test]
    fn set_limits_transaccional_y_reversible() {
        use crate::doc::Limits;
        let mut session = Session::new(Units::default());
        let before = session.document().limits();
        let nuevo = Limits {
            min: Point2::new(-5.0, -5.0),
            max: Point2::new(100.0, 80.0),
        };

        let out = session
            .transact("limits", |tx: &mut TxContext<'_>| {
                tx.set_limits(nuevo);
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert_eq!(session.document().limits(), nuevo);

        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().limits(), before);
    }

    // Current line type, lineweight, and scale changes.

    #[test]
    fn set_celtype_celweight_ltscale_transaccional_y_reversible() {
        let mut session = Session::new(Units::default());
        assert_eq!(session.document().current_line_type(), LineTypeRef::ByLayer);
        assert_eq!(session.document().current_lineweight(), Lineweight::ByLayer);
        assert_eq!(session.document().ltscale(), 1.0);

        let out = session
            .transact("props", |tx: &mut TxContext<'_>| {
                tx.set_current_line_type(LineTypeRef::ByBlock)?;
                tx.set_current_lineweight(Lineweight::Mm(0.5));
                tx.set_ltscale(3.0)?;
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert_eq!(session.document().current_line_type(), LineTypeRef::ByBlock);
        assert_eq!(session.document().current_lineweight(), Lineweight::Mm(0.5));
        assert_eq!(session.document().ltscale(), 3.0);
        assert!(out.change_set.unwrap().doc_changed());

        // Inverse restores all three factory values.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().current_line_type(), LineTypeRef::ByLayer);
        assert_eq!(session.document().current_lineweight(), Lineweight::ByLayer);
        assert_eq!(session.document().ltscale(), 1.0);
    }

    #[test]
    fn set_celtype_a_estilo_inexistente_es_error() {
        let mut session = Session::new(Units::default());
        let ghost: StyleId = ObjectId(999_999).into();
        let res = session.transact("bad", |tx| -> Result<(), TxError> {
            tx.set_current_line_type(LineTypeRef::Style(ghost))
        });
        assert!(matches!(res, Err(TxError::UnknownLineType(_))));
        assert_eq!(session.document().current_line_type(), LineTypeRef::ByLayer);
    }

    #[test]
    fn set_ltscale_no_positivo_es_error() {
        let mut session = Session::new(Units::default());
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let res = session.transact("bad", |tx| -> Result<(), TxError> { tx.set_ltscale(bad) });
            assert!(matches!(res, Err(TxError::InvalidGeometry(_))), "bad={bad}");
        }
        assert_eq!(session.document().ltscale(), 1.0);
    }

    // Line-type table insertions and removals.

    #[test]
    fn add_line_type_reversible_y_rechaza_duplicado() {
        let mut session = Session::new(Units::default());
        assert_eq!(session.document().line_types().count(), 1);

        let out = session
            .transact("load dashed", |tx: &mut TxContext<'_>| {
                tx.add_line_type_raw("DASHED", "d", vec![0.5, -0.25])
            })
            .expect("commits");
        let dashed = out.value;
        assert_eq!(session.document().line_types().count(), 2);
        assert_eq!(
            session.document().line_type(dashed).unwrap().pattern(),
            &[0.5, -0.25]
        );

        // Reject case-insensitive duplicates.
        let dup = session.transact("dup", |tx| -> Result<StyleId, TxError> {
            tx.add_line_type_raw("dashed", "", vec![])
        });
        assert!(matches!(dup, Err(TxError::DuplicateLineTypeName(_))));

        // Undo removes the inserted type without rewinding `nextObjectId`.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(session.document().line_types().count(), 1);
        assert!(session.document().line_type(dashed).is_none());
    }

    #[test]
    fn load_linetypes_carga_nuevos_y_omite_existentes() {
        let mut session = Session::new(Units::default());
        assert_eq!(session.document().line_types().count(), 1);

        let defs = vec![
            ParsedLinetype {
                name: "DASHED2".to_string(),
                description: "d2".to_string(),
                pattern: vec![0.6, -0.3],
            },
            ParsedLinetype {
                // Skip existing `"Continuous"` case-insensitively.
                name: "continuous".to_string(),
                description: "dup de fábrica".to_string(),
                pattern: vec![],
            },
            ParsedLinetype {
                name: "CENTER2".to_string(),
                description: "c2".to_string(),
                pattern: vec![1.5, -0.3, 0.3, -0.3],
            },
            ParsedLinetype {
                // Skip a duplicate within the same batch.
                name: "dashed2".to_string(),
                description: "otra vez".to_string(),
                pattern: vec![9.9],
            },
        ];

        let out = session
            .transact("load .lin", |tx: &mut TxContext<'_>| {
                tx.load_linetypes(defs)
            })
            .expect("commits")
            .value;

        assert_eq!(out.loaded.len(), 2);
        assert_eq!(out.skipped_existing, vec!["continuous", "dashed2"]);
        assert_eq!(session.document().line_types().count(), 3);
        assert_eq!(
            session
                .document()
                .line_type_by_name("DASHED2")
                .unwrap()
                .pattern(),
            &[0.6, -0.3]
        );
        assert_eq!(
            session
                .document()
                .line_type_by_name("CENTER2")
                .unwrap()
                .pattern(),
            &[1.5, -0.3, 0.3, -0.3]
        );
    }

    #[test]
    fn remove_line_type_protege_continuous_y_los_en_uso() {
        let mut session = Session::new(Units::default());
        let continuous = session.document().line_types().next().unwrap().id();

        // `"Continuous"` is protected.
        let res = session.transact("rm cont", |tx| -> Result<(), TxError> {
            tx.remove_line_type_raw(continuous)
        });
        assert!(matches!(res, Err(TxError::LineTypeProtected(_))));

        // A current line type cannot be removed.
        let dashed = session
            .transact("load", |tx| -> Result<StyleId, TxError> {
                let id = tx.add_line_type_raw("DASHED", "d", vec![0.5, -0.25])?;
                tx.set_current_line_type(LineTypeRef::Style(id))?;
                Ok(id)
            })
            .unwrap()
            .value;
        let res = session.transact("rm inuse", |tx| -> Result<(), TxError> {
            tx.remove_line_type_raw(dashed)
        });
        assert!(matches!(res, Err(TxError::LineTypeInUse(_))));

        // Release the current reference before reversible removal.
        let out = session
            .transact("free+rm", |tx: &mut TxContext<'_>| {
                tx.set_current_line_type(LineTypeRef::ByLayer)?;
                tx.remove_line_type_raw(dashed)?;
                Ok::<(), TxError>(())
            })
            .expect("commits");
        assert!(session.document().line_type(dashed).is_none());
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert!(session.document().line_type(dashed).is_some());
    }

    // Entity IDs are immutable during modification.

    #[test]
    fn modify_entity_ignora_cambios_al_id() {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let ids = seed_three(&mut session, l0);

        session
            .transact("modify", |tx| -> Result<(), TxError> {
                tx.modify_entity(ids[0], |r| {
                    r.id = ObjectId(424_242).into(); // Must be ignored.
                    r.visible = false;
                })
            })
            .expect("commits");

        // Identity stays fixed while properties change.
        let (rec, _) = session.document().entity(ids[0]).expect("sigue presente");
        assert_eq!(rec.id, ids[0]);
        assert!(!rec.visible);
        assert!(
            session
                .document()
                .entity(ObjectId(424_242).into())
                .is_none()
        );
    }

    #[test]
    fn transform_via_modify_entity() {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let id = session
            .transact("add", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::ModelSpace, line_rec(l0, 0.0))
            })
            .unwrap()
            .value;

        session
            .transact("move", |tx| -> Result<(), TxError> {
                tx.modify_entity(id, |r| {
                    if let EntityGeometry::Line(g) = &r.geometry {
                        let t = af_math::Transform2::translate(Vec2::new(10.0, 5.0));
                        r.geometry = EntityGeometry::Line(g.transform(&t).unwrap());
                    }
                })
            })
            .unwrap();

        let (rec, _) = session.document().entity(id).unwrap();
        if let EntityGeometry::Line(g) = &rec.geometry {
            assert_eq!(g.p1, Point2::new(10.0, 5.0));
            assert_eq!(g.p2, Point2::new(11.0, 5.0));
        } else {
            panic!("esperaba línea");
        }
    }

    // Context-free forward/inverse layer operations.

    fn continuous_id(doc: &Document) -> StyleId {
        doc.line_types().next().unwrap().id()
    }

    fn layer_val(name: &str, color: Color, lt: StyleId) -> Layer {
        Layer::new(ObjectId::NIL.into(), name, color, lt, Lineweight::ByLayer)
    }

    #[test]
    fn apply_inverse_forward_de_add_layer_es_byte_identico() {
        let mut session = Session::new(Units::default());
        let lt = continuous_id(session.document());
        let out = session
            .transact("add layer", |tx| -> Result<LayerId, TxError> {
                tx.add_layer_raw(layer_val("Muros", Color::aci(1).unwrap(), lt))
            })
            .expect("commits");
        let id = out.value;
        let after_commit = serde_json::to_string(session.document()).unwrap();
        let tx = out.transaction.unwrap();

        // Inverse removes the layer; forward restores byte-identical state.
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert!(session.document().layer(id).is_none());
        apply_forward(session.document_mut(), &tx).unwrap();
        assert_eq!(
            after_commit,
            serde_json::to_string(session.document()).unwrap()
        );
    }

    #[test]
    fn apply_inverse_de_remove_layer_restaura_posicion_exacta() {
        // Removing and undoing middle layer B restores exact table order.
        let mut doc = Document::new(Units::default());
        let lt = continuous_id(&doc);
        doc.add_layer("A", Color::ByLayer, lt, Lineweight::ByLayer)
            .unwrap();
        let b = doc
            .add_layer("B", Color::ByLayer, lt, Lineweight::ByLayer)
            .unwrap();
        doc.add_layer("C", Color::ByLayer, lt, Lineweight::ByLayer)
            .unwrap();
        let mut session = Session::from_document(doc);
        let before = serde_json::to_string(session.document()).unwrap();

        let out = session
            .transact("rm B", |tx| -> Result<(), TxError> {
                tx.remove_layer_raw(b)
            })
            .expect("commits");
        let names_after: Vec<String> = session
            .document()
            .layers()
            .map(|l| l.name().to_string())
            .collect();
        assert_eq!(names_after, vec!["0", "A", "C"]);
        let cs = out.change_set.expect("changeset");
        assert_eq!(cs.layers_changed(), &[b]);

        // Inverse restores B at original index 2.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        let names_restored: Vec<String> = session
            .document()
            .layers()
            .map(|l| l.name().to_string())
            .collect();
        assert_eq!(names_restored, vec!["0", "A", "B", "C"]);
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    #[test]
    fn apply_inverse_forward_de_modify_layer_es_byte_identico() {
        let mut doc = Document::new(Units::default());
        let lt = continuous_id(&doc);
        let a = doc
            .add_layer("A", Color::aci(1).unwrap(), lt, Lineweight::ByLayer)
            .unwrap();
        let mut session = Session::from_document(doc);
        let before = serde_json::to_string(session.document()).unwrap();

        let out = session
            .transact("mod A", |tx| -> Result<(), TxError> {
                tx.modify_layer_raw(a, layer_val("A", Color::aci(5).unwrap(), lt))
            })
            .expect("commits");
        let after_commit = serde_json::to_string(session.document()).unwrap();
        assert_ne!(before, after_commit, "el color cambió");
        assert_eq!(
            session.document().layer(a).unwrap().color(),
            Color::aci(5).unwrap()
        );

        // Inverse restores prior bytes; forward reapplies the change.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
        apply_forward(session.document_mut(), &tx).unwrap();
        assert_eq!(
            after_commit,
            serde_json::to_string(session.document()).unwrap()
        );
    }

    #[test]
    fn remove_layer_raw_rechaza_capa_usada_dentro_de_un_bloque() {
        // In-use scanning includes block definitions.
        let mut doc = Document::new(Units::default());
        let lt = continuous_id(&doc);
        let muros = doc
            .add_layer("Muros", Color::aci(1).unwrap(), lt, Lineweight::ByLayer)
            .unwrap();
        let bid = doc.add_block("Puerta", Point2::ORIGIN).unwrap();
        let mut session = Session::from_document(doc);

        // Put a source-layer entity inside the block definition.
        session
            .transact("add in block", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(ContainerRef::Block(bid), point_rec(muros, 0.0))
            })
            .expect("commits");

        let res = session.transact("rm used-in-block", |tx| -> Result<(), TxError> {
            tx.remove_layer_raw(muros)
        });
        assert_eq!(res.unwrap_err(), TxError::LayerInUse(muros));
        assert!(session.document().layer(muros).is_some());
    }

    // Reversible validated group operations.

    use crate::groups::Group;
    use crate::id::GroupId;

    /// Inserts two entities and returns the session and IDs.
    fn seed_two(session: &mut Session) -> (EntityId, EntityId) {
        let l0 = session.document().current_layer();
        let ids = seed_three(session, l0);
        (ids[0], ids[1])
    }

    #[test]
    fn add_group_raw_asigna_id_y_changeset_es_doc_changed() {
        let mut session = Session::new(Units::default());
        let (a, b) = seed_two(&mut session);

        let out = session
            .transact("Group", |tx| -> Result<GroupId, TxError> {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G1").with_members(vec![a, b]))
            })
            .expect("commits");
        let gid = out.value;
        let g = session.document().group(gid).expect("grupo creado");
        assert_eq!(g.name(), "G1");
        assert_eq!(g.members(), &[a, b]);
        assert!(g.is_selectable());
        assert_eq!(
            session.document().group_by_name("g1").map(Group::id),
            Some(gid)
        );

        // Group changes are document changes, not entity/layer deltas.
        let cs = out.change_set.expect("changeset");
        assert!(cs.doc_changed());
        assert!(cs.added().is_empty() && cs.removed().is_empty() && cs.modified().is_empty());
        assert!(cs.layers_changed().is_empty());
    }

    #[test]
    fn add_group_raw_rechaza_nombre_duplicado_y_miembro_inexistente() {
        let mut session = Session::new(Units::default());
        let (a, _b) = seed_two(&mut session);
        session
            .transact("g", |tx| {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G"))
            })
            .expect("commits");

        // Reject a case-insensitive duplicate name.
        let dup = session.transact("dup", |tx| -> Result<GroupId, TxError> {
            tx.add_group_raw(Group::new(ObjectId::NIL.into(), "g"))
        });
        assert_eq!(
            dup.unwrap_err(),
            TxError::DuplicateGroupName("g".to_string())
        );

        // Reject an unknown member.
        let ghost: EntityId = ObjectId(999_999).into();
        let bad = session.transact("bad", |tx| -> Result<GroupId, TxError> {
            tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G2").with_members(vec![a, ghost]))
        });
        assert_eq!(bad.unwrap_err(), TxError::UnknownEntity(ghost));
    }

    #[test]
    fn add_group_undo_forward_es_byte_identico() {
        let mut session = Session::new(Units::default());
        let (a, b) = seed_two(&mut session);
        let out = session
            .transact("Group", |tx| -> Result<GroupId, TxError> {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G").with_members(vec![a, b]))
            })
            .expect("commits");
        let gid = out.value;
        let after_commit = serde_json::to_string(session.document()).unwrap();
        let tx = out.transaction.unwrap();

        // Inverse removes the group; forward restores exact bytes.
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert!(session.document().group(gid).is_none());
        apply_forward(session.document_mut(), &tx).unwrap();
        assert_eq!(
            after_commit,
            serde_json::to_string(session.document()).unwrap()
        );
    }

    #[test]
    fn modify_group_raw_renombra_y_cambia_miembros_reversible() {
        let mut session = Session::new(Units::default());
        let (a, b) = seed_two(&mut session);
        let gid = session
            .transact("g", |tx| -> Result<GroupId, TxError> {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G").with_members(vec![a]))
            })
            .expect("commits")
            .value;
        let before = serde_json::to_string(session.document()).unwrap();

        let out = session
            .transact("edit", |tx| -> Result<(), TxError> {
                let g = tx.doc().group(gid).unwrap().clone();
                tx.modify_group_raw(gid, g.with_name("G-renamed").with_members(vec![a, b]))
            })
            .expect("commits");
        let g = session.document().group(gid).unwrap();
        assert_eq!(g.name(), "G-renamed");
        assert_eq!(g.members(), &[a, b]);
        assert!(out.change_set.unwrap().doc_changed());

        // Inverse restores byte-identical prior state.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    #[test]
    fn modify_group_raw_a_igual_es_noop() {
        let mut session = Session::new(Units::default());
        let (a, _b) = seed_two(&mut session);
        let gid = session
            .transact("g", |tx| -> Result<GroupId, TxError> {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G").with_members(vec![a]))
            })
            .expect("commits")
            .value;

        let noop = session
            .transact("noop", |tx| -> Result<(), TxError> {
                let g = tx.doc().group(gid).unwrap().clone();
                tx.modify_group_raw(gid, g)
            })
            .expect("commits");
        assert!(noop.transaction.is_none());
    }

    #[test]
    fn remove_group_raw_restaura_posicion_exacta_al_invertir() {
        // Removing and undoing middle group B restores exact order.
        let mut session = Session::new(Units::default());
        let (a, _b) = seed_two(&mut session);
        let make = |name: &'static str| {
            move |tx: &mut TxContext<'_>| -> Result<GroupId, TxError> {
                tx.add_group_raw(Group::new(ObjectId::NIL.into(), name).with_members(vec![a]))
            }
        };
        session.transact("A", make("A")).expect("commits");
        let gb = session.transact("B", make("B")).expect("commits").value;
        session.transact("C", make("C")).expect("commits");
        let before = serde_json::to_string(session.document()).unwrap();

        let out = session
            .transact("rm B", |tx| -> Result<(), TxError> {
                tx.remove_group_raw(gb)
            })
            .expect("commits");
        let names: Vec<String> = session
            .document()
            .groups()
            .map(|g| g.name().to_string())
            .collect();
        assert_eq!(names, vec!["A", "C"]);

        // Inverse restores B at original index 1.
        let tx = out.transaction.unwrap();
        apply_inverse(session.document_mut(), &tx).unwrap();
        let restored: Vec<String> = session
            .document()
            .groups()
            .map(|g| g.name().to_string())
            .collect();
        assert_eq!(restored, vec!["A", "B", "C"]);
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }

    #[test]
    fn remove_group_raw_grupo_inexistente_es_error() {
        let mut session = Session::new(Units::default());
        let ghost: GroupId = ObjectId(555).into();
        let res = session.transact("rm", |tx| -> Result<(), TxError> {
            tx.remove_group_raw(ghost)
        });
        assert_eq!(res.unwrap_err(), TxError::UnknownGroup(ghost));
    }

    #[test]
    fn rollback_de_grupo_deja_documento_byte_identico() {
        let mut session = Session::new(Units::default());
        let (a, _b) = seed_two(&mut session);
        let before = serde_json::to_string(session.document()).unwrap();
        let res = session.transact("boom", |tx| -> Result<(), TxError> {
            tx.add_group_raw(Group::new(ObjectId::NIL.into(), "G").with_members(vec![a]))?;
            Err(TxError::Internal("fallo"))
        });
        assert!(res.is_err());
        assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    }
}
