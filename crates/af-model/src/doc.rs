//! [`DrawingDocument`] (alias [`Document`]) is the data-model root.
//!
//! It contains persistent drawing state only. Selection, history, spatial
//! indexing, and camera state belong to the runtime session.
//!
//! # Transaction-only mutation
//!
//! Fields are private. Public getters and ordered iterators provide reads;
//! crate-private methods support transactions. Collections expose no public
//! mutable references.
//!
//! # `new`
//!
//! [`Document::new`] creates a minimal valid document with layer `"0"`, the
//! `"Continuous"` line type, standard text and dimension styles, an empty
//! `"Layout1"`, and the requested units.

use std::collections::{BTreeMap, HashMap, HashSet};

use af_math::{Point2, Tol};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::container::{ContainerRef, EntityContainer};
use crate::entity::{Color, EntityRecord, LineTypeRef, Lineweight};
use crate::groups::Group;
use crate::id::{
    BlockId, EntityId, GroupId, IdAllocator, IdExhausted, LayerId, LayoutId, ObjectId, StyleId,
};
use crate::layers::Layer;
use crate::layouts::{Layout, PaperSettings};
use crate::styles::{DimStyle, LineType, TextStyle};
use crate::units::Units;
use crate::validate::{
    Issue, IssueCode, Severity, block_dependencies, find_block_cycle, prune_group_members,
    repair_layer_line_types, scan_and_repair_container,
};

/// Drawing limits (`LIMMIN`/`LIMMAX`) used by the grid and limit-based zoom.
/// They are metadata and do not clip geometry.
///
/// Defaults to metric A3 bounds `(0,0)–(420,297)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// Lower-left corner (`LIMMIN`).
    pub min: Point2,
    /// Upper-right corner (`LIMMAX`).
    pub max: Point2,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            min: Point2::ORIGIN,
            max: Point2::new(420.0, 297.0),
        }
    }
}

/// Default linear display precision (`LUPREC`): four decimal places.
fn default_linear_precision() -> u8 {
    4
}

/// Global document identity stored as a UUID v4 string.
///
/// It survives save-as operations and is independent of document [`ObjectId`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(Uuid);

impl DocumentId {
    /// Generates a new UUID v4 identity.
    #[must_use]
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }

    /// Underlying UUID.
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

/// Document title, author, comments, and custom metadata.
///
/// Optional ISO timestamps are managed by the I/O layer; the model does not read
/// the system clock.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    title: String,
    author: String,
    comments: String,
    custom: BTreeMap<String, String>,
    created_utc: Option<String>,
    modified_utc: Option<String>,
}

impl Metadata {
    /// Document title; may be empty.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Author; may be empty.
    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Free-form comments; may be empty.
    #[must_use]
    pub fn comments(&self) -> &str {
        &self.comments
    }

    /// Custom key/value pairs in stable key order.
    #[must_use]
    pub fn custom(&self) -> &BTreeMap<String, String> {
        &self.custom
    }

    /// Recorded creation timestamp in ISO UTC form.
    #[must_use]
    pub fn created_utc(&self) -> Option<&str> {
        self.created_utc.as_deref()
    }

    /// Recorded last-modified timestamp in ISO UTC form.
    #[must_use]
    pub fn modified_utc(&self) -> Option<&str> {
        self.modified_utc.as_deref()
    }
}

/// External reference (Xref).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalReference {
    path: String,
    doc_id_hint: Option<String>,
    last_seen: Option<String>,
}

impl ExternalReference {
    /// External-reference path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Optional external-document identity hint.
    #[must_use]
    pub fn doc_id_hint(&self) -> Option<&str> {
        self.doc_id_hint.as_deref()
    }

    /// Optional last-resolved timestamp.
    #[must_use]
    pub fn last_seen(&self) -> Option<&str> {
        self.last_seen.as_deref()
    }
}

/// Reusable block definition with a unique name, base point, and entities.
///
/// [`Document::validate_full`] rejects cyclic block references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDefinition {
    id: BlockId,
    name: String,
    base_point: Point2,
    entities: EntityContainer,
    description: String,
}

impl BlockDefinition {
    /// Stable block ID.
    #[must_use]
    pub fn id(&self) -> BlockId {
        self.id
    }

    /// Case-insensitively unique block name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Insertion base point.
    #[must_use]
    pub fn base_point(&self) -> Point2 {
        self.base_point
    }

    /// Definition entities.
    #[must_use]
    pub fn entities(&self) -> &EntityContainer {
        &self.entities
    }

    /// Optional free-form description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Crate-private mutable entity access.
    pub(crate) fn entities_mut(&mut self) -> &mut EntityContainer {
        &mut self.entities
    }
}

/// Named-object class used in uniqueness errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    /// Layer.
    Layer,
    /// Line type.
    LineType,
    /// Text style.
    TextStyle,
    /// Dimension style.
    DimStyle,
    /// Block definition.
    Block,
}

/// Error from a crate-private document mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocError {
    /// A case-insensitive name conflict exists.
    DuplicateName {
        /// Conflicting object class.
        kind: NameKind,
        /// Requested name.
        name: String,
    },
    /// Referenced layer does not exist.
    UnknownLayer(LayerId),
    /// Referenced line type does not exist.
    UnknownLineType(StyleId),
    /// Persistent ID space is exhausted.
    IdExhausted,
}

impl core::fmt::Display for DocError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DocError::DuplicateName { kind, name } => {
                write!(f, "duplicate {kind:?} name (case-insensitive): {name:?}")
            }
            DocError::UnknownLayer(id) => write!(f, "unknown layer id {}", id.raw().0),
            DocError::UnknownLineType(id) => write!(f, "unknown line type id {}", id.raw().0),
            DocError::IdExhausted => write!(f, "persistent object id space exhausted"),
        }
    }
}

impl std::error::Error for DocError {}

impl From<IdExhausted> for DocError {
    fn from(_: IdExhausted) -> Self {
        DocError::IdExhausted
    }
}

/// Root of the CAD data model.
///
/// Public alias: [`Document`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawingDocument {
    id: DocumentId,
    units: Units,
    tolerances: Tol,
    metadata: Metadata,
    #[serde(rename = "nextObjectId", with = "id_allocator_serde")]
    id_allocator: IdAllocator,
    layers: IndexMap<LayerId, Layer>,
    current_layer: LayerId,
    /// Default color for new entities (`CECOLOR`). Older files default to `ByLayer`.
    #[serde(default)]
    current_color: Color,
    /// Linear display precision (`LUPREC`); older files default to four places.
    #[serde(default = "default_linear_precision")]
    linear_precision: u8,
    /// Drawing limits; older files use the A3 default.
    #[serde(default)]
    limits: Limits,
    /// Default line type for new entities (`CELTYPE`).
    #[serde(default)]
    current_line_type: LineTypeRef,
    /// Default lineweight for new entities (`CELWEIGHT`).
    #[serde(default)]
    current_lineweight: Lineweight,
    /// Global line-type pattern scale (`LTSCALE`).
    #[serde(default = "default_ltscale")]
    ltscale: f64,
    line_types: IndexMap<StyleId, LineType>,
    text_styles: IndexMap<StyleId, TextStyle>,
    dim_styles: IndexMap<StyleId, DimStyle>,
    blocks: IndexMap<BlockId, BlockDefinition>,
    model_space: EntityContainer,
    layouts: IndexMap<LayoutId, Layout>,
    /// Named groups; older files deserialize to an empty table.
    #[serde(default)]
    groups: IndexMap<GroupId, Group>,
    external_refs: Vec<ExternalReference>,
}

/// Public alias for [`DrawingDocument`].
pub type Document = DrawingDocument;

/// Default `LTSCALE` value for deserialization.
fn default_ltscale() -> f64 {
    1.0
}

/// Serializes the allocator as the next ID scalar and restores its cursor on load.
mod id_allocator_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::id::IdAllocator;

    pub fn serialize<S: Serializer>(alloc: &IdAllocator, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(alloc.peek().0)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<IdAllocator, D::Error> {
        let next = u64::deserialize(d)?;
        let mut alloc = IdAllocator::new();
        alloc
            .ensure_above(next.saturating_sub(1))
            .map_err(serde::de::Error::custom)?;
        Ok(alloc)
    }
}

impl DrawingDocument {
    /// Creates a minimal valid document with the requested units.
    #[must_use]
    pub fn new(units: Units) -> Self {
        let mut id_allocator = IdAllocator::new();

        // Stable allocation order; IDs remain opaque to callers.
        let layer0_id: LayerId = id_allocator.alloc().expect("fresh id allocator").into();
        let continuous_id: StyleId = id_allocator.alloc().expect("fresh id allocator").into();
        let standard_text_id: StyleId = id_allocator.alloc().expect("fresh id allocator").into();
        let standard_dim_id: StyleId = id_allocator.alloc().expect("fresh id allocator").into();
        let layout1_id: LayoutId = id_allocator.alloc().expect("fresh id allocator").into();

        let mut layers = IndexMap::new();
        layers.insert(
            layer0_id,
            Layer::new(
                layer0_id,
                "0",
                Color::aci(7).expect("ACI 7 is in range 1..=255"),
                continuous_id,
                Lineweight::Mm(0.25),
            ),
        );

        let mut line_types = IndexMap::new();
        line_types.insert(
            continuous_id,
            LineType::new(continuous_id, "Continuous", "Solid line"),
        );

        let mut text_styles = IndexMap::new();
        text_styles.insert(
            standard_text_id,
            TextStyle::new(standard_text_id, "Standard"),
        );

        let mut dim_styles = IndexMap::new();
        dim_styles.insert(standard_dim_id, DimStyle::new(standard_dim_id, "Standard"));

        let mut layouts = IndexMap::new();
        layouts.insert(
            layout1_id,
            Layout::new(layout1_id, "Layout1", PaperSettings::default()),
        );

        Self {
            id: DocumentId::new_v4(),
            units,
            tolerances: Tol::default(),
            metadata: Metadata::default(),
            id_allocator,
            layers,
            current_layer: layer0_id,
            current_color: Color::default(),
            linear_precision: default_linear_precision(),
            limits: Limits::default(),
            current_line_type: LineTypeRef::default(),
            current_lineweight: Lineweight::default(),
            ltscale: default_ltscale(),
            line_types,
            text_styles,
            dim_styles,
            blocks: IndexMap::new(),
            model_space: EntityContainer::new(),
            layouts,
            groups: IndexMap::new(),
            external_refs: Vec::new(),
        }
    }

    // Public reads.

    /// Global document identity.
    #[must_use]
    pub fn id(&self) -> DocumentId {
        self.id
    }

    /// Document units used to interpret geometry.
    #[must_use]
    pub fn units(&self) -> Units {
        self.units
    }

    /// Document tolerances.
    #[must_use]
    pub fn tolerances(&self) -> Tol {
        self.tolerances
    }

    /// Document metadata.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Next [`ObjectId`] the document will allocate.
    #[must_use]
    pub fn next_object_id(&self) -> u64 {
        self.id_allocator.peek().0
    }

    /// Iterates layers in stable creation order.
    pub fn layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.values()
    }

    /// Layer by ID.
    #[must_use]
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(&id)
    }

    /// Layer by case-insensitive name.
    #[must_use]
    pub fn layer_by_name(&self, name: &str) -> Option<&Layer> {
        let needle = name.to_lowercase();
        self.layers
            .values()
            .find(|l| l.name().to_lowercase() == needle)
    }

    /// Current layer ID, which always refers to an existing layer.
    #[must_use]
    pub fn current_layer(&self) -> LayerId {
        self.current_layer
    }

    /// Current color (`CECOLOR`) used for new entities unless overridden.
    #[must_use]
    pub fn current_color(&self) -> Color {
        self.current_color
    }

    /// Linear display precision (`LUPREC`); does not affect stored geometry.
    #[must_use]
    pub fn linear_precision(&self) -> u8 {
        self.linear_precision
    }

    /// Drawing limits (`LIMMIN`/`LIMMAX`).
    #[must_use]
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Current line type (`CELTYPE`) used for new entities unless overridden.
    #[must_use]
    pub fn current_line_type(&self) -> LineTypeRef {
        self.current_line_type
    }

    /// Current lineweight (`CELWEIGHT`) used for new entities unless overridden.
    #[must_use]
    pub fn current_lineweight(&self) -> Lineweight {
        self.current_lineweight
    }

    /// Positive global line-type pattern scale (`LTSCALE`).
    #[must_use]
    pub fn ltscale(&self) -> f64 {
        self.ltscale
    }

    /// Iterates block definitions in stable order.
    pub fn blocks(&self) -> impl Iterator<Item = &BlockDefinition> {
        self.blocks.values()
    }

    /// Block definition by ID.
    #[must_use]
    pub fn block(&self, id: BlockId) -> Option<&BlockDefinition> {
        self.blocks.get(&id)
    }

    /// Iterates line types in stable order.
    pub fn line_types(&self) -> impl Iterator<Item = &LineType> {
        self.line_types.values()
    }

    /// Line type by ID.
    #[must_use]
    pub fn line_type(&self, id: StyleId) -> Option<&LineType> {
        self.line_types.get(&id)
    }

    /// Line type by case-insensitive name.
    #[must_use]
    pub fn line_type_by_name(&self, name: &str) -> Option<&LineType> {
        self.line_types
            .values()
            .find(|lt| lt.name().eq_ignore_ascii_case(name))
    }

    /// Iterates text styles in stable order.
    pub fn text_styles(&self) -> impl Iterator<Item = &TextStyle> {
        self.text_styles.values()
    }

    /// Text style by ID.
    #[must_use]
    pub fn text_style(&self, id: StyleId) -> Option<&TextStyle> {
        self.text_styles.get(&id)
    }

    /// Iterates dimension styles in stable order.
    pub fn dim_styles(&self) -> impl Iterator<Item = &DimStyle> {
        self.dim_styles.values()
    }

    /// Dimension style by ID.
    #[must_use]
    pub fn dim_style(&self, id: StyleId) -> Option<&DimStyle> {
        self.dim_styles.get(&id)
    }

    /// Model-space entity container.
    #[must_use]
    pub fn model_space(&self) -> &EntityContainer {
        &self.model_space
    }

    /// Entity container selected by `container`.
    ///
    /// Returns `None` for an unknown layout or block; model space always exists.
    #[must_use]
    pub fn container(&self, container: ContainerRef) -> Option<&EntityContainer> {
        match container {
            ContainerRef::ModelSpace => Some(&self.model_space),
            ContainerRef::Layout(id) => self.layouts.get(&id).map(Layout::entities),
            ContainerRef::Block(id) => self.blocks.get(&id).map(BlockDefinition::entities),
        }
    }

    /// Finds an entity by ID across all containers and returns its materialized
    /// record with the containing location.
    ///
    /// The returned record is owned because typed pools reconstruct it on read.
    #[must_use]
    pub fn entity(&self, id: crate::id::EntityId) -> Option<(EntityRecord, ContainerRef)> {
        if let Some(rec) = self.model_space.get(id) {
            return Some((rec, ContainerRef::ModelSpace));
        }
        for (lid, layout) in &self.layouts {
            if let Some(rec) = layout.entities().get(id) {
                return Some((rec, ContainerRef::Layout(*lid)));
            }
        }
        for (bid, block) in &self.blocks {
            if let Some(rec) = block.entities().get(id) {
                return Some((rec, ContainerRef::Block(*bid)));
            }
        }
        None
    }

    /// Iterates layouts in stable order.
    pub fn layouts(&self) -> impl Iterator<Item = &Layout> {
        self.layouts.values()
    }

    /// Layout by ID.
    #[must_use]
    pub fn layout(&self, id: LayoutId) -> Option<&Layout> {
        self.layouts.get(&id)
    }

    /// Iterates groups in stable creation order.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.values()
    }

    /// Group by ID.
    #[must_use]
    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.get(&id)
    }

    /// Group by case-insensitive name.
    #[must_use]
    pub fn group_by_name(&self, name: &str) -> Option<&Group> {
        let needle = name.to_lowercase();
        self.groups
            .values()
            .find(|g| g.name().to_lowercase() == needle)
    }

    /// External references.
    #[must_use]
    pub fn external_refs(&self) -> &[ExternalReference] {
        &self.external_refs
    }

    // Load validation.

    /// Validates the document, repairs recoverable problems, and reports every
    /// detected issue or applied repair.
    ///
    /// Unrecoverable corruption is reported as [`Severity::Error`] without
    /// modifying the affected data.
    pub fn validate_full(&mut self) -> Vec<Issue> {
        let mut issues = Vec::new();

        // Inventory duplicate and maximum IDs.
        let mut all_ids: Vec<ObjectId> = Vec::new();
        for rec in self.model_space.iter_records() {
            all_ids.push(rec.id.raw());
        }
        for layout in self.layouts.values() {
            for rec in layout.entities().iter_records() {
                all_ids.push(rec.id.raw());
            }
        }
        for block in self.blocks.values() {
            for rec in block.entities().iter_records() {
                all_ids.push(rec.id.raw());
            }
        }
        for k in self.layers.keys() {
            all_ids.push(k.raw());
        }
        for k in self.line_types.keys() {
            all_ids.push(k.raw());
        }
        for k in self.text_styles.keys() {
            all_ids.push(k.raw());
        }
        for k in self.dim_styles.keys() {
            all_ids.push(k.raw());
        }
        for k in self.blocks.keys() {
            all_ids.push(k.raw());
        }
        for k in self.layouts.keys() {
            all_ids.push(k.raw());
        }
        for k in self.groups.keys() {
            all_ids.push(k.raw());
        }

        let mut counts: HashMap<ObjectId, u32> = HashMap::new();
        let mut max_id: u64 = 0;
        for oid in &all_ids {
            *counts.entry(*oid).or_insert(0) += 1;
            max_id = max_id.max(oid.0);
        }
        let mut dups: Vec<ObjectId> = counts
            .iter()
            .filter(|&(_, &c)| c > 1)
            .map(|(&id, _)| id)
            .collect();
        dups.sort_by_key(|o| o.0); // Keep report order deterministic.
        for oid in dups {
            issues.push(Issue::new(
                Severity::Error,
                IssueCode::DuplicateId,
                format!("duplicate object id {}", oid.0),
                Some(oid),
            ));
        }

        // Repair entity layer, style, and geometry references.
        let valid_layers: HashSet<LayerId> = self.layers.keys().copied().collect();
        let valid_line_types: HashSet<StyleId> = self.line_types.keys().copied().collect();
        let tol = self.tolerances;
        if let Some(layer0) = self.resolve_zero_layer() {
            scan_and_repair_container(
                &mut self.model_space,
                layer0,
                &valid_layers,
                &valid_line_types,
                &tol,
                &mut issues,
            );
            for layout in self.layouts.values_mut() {
                scan_and_repair_container(
                    layout.entities_mut(),
                    layer0,
                    &valid_layers,
                    &valid_line_types,
                    &tol,
                    &mut issues,
                );
            }
            for block in self.blocks.values_mut() {
                scan_and_repair_container(
                    block.entities_mut(),
                    layer0,
                    &valid_layers,
                    &valid_line_types,
                    &tol,
                    &mut issues,
                );
            }
        }

        // Repair unknown default line types using the first catalog entry.
        match self.line_types.keys().next().copied() {
            Some(fallback) => {
                repair_layer_line_types(&mut self.layers, &valid_line_types, fallback, &mut issues);
            }
            // An empty line-type catalog offers no safe repair target.
            None => issues.push(Issue::new(
                Severity::Error,
                IssueCode::DanglingStyleRef,
                "line type catalog is empty (no default line type to resolve ByLayer)".to_string(),
                None,
            )),
        }

        // Raise `nextObjectId` above the maximum existing ID.
        let peek = self.id_allocator.peek().0;
        if peek <= max_id {
            match self.id_allocator.ensure_above(max_id) {
                Ok(()) => {
                    let raised = self.id_allocator.peek().0;
                    issues.push(Issue::new(
                        Severity::Repaired,
                        IssueCode::NextObjectIdTooLow,
                        format!(
                            "nextObjectId ({peek}) was <= max object id ({max_id}); raised to {raised}"
                        ),
                        None,
                    ));
                }
                Err(_) => issues.push(Issue::new(
                    Severity::Error,
                    IssueCode::NextObjectIdTooLow,
                    format!("max object id is {max_id}; nextObjectId cannot be raised above it"),
                    None,
                )),
            }
        }

        // Detect block-definition cycles.
        let mut adj: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        for (bid, block) in &self.blocks {
            adj.insert(*bid, block_dependencies(block.entities()));
        }
        if let Some(cycle) = find_block_cycle(&adj) {
            issues.push(Issue::new(
                Severity::Error,
                IssueCode::BlockCycle,
                format!("block definition cycle involving block {}", cycle.raw().0),
                Some(cycle.raw()),
            ));
        }

        // Prune group members that reference missing entities.
        let valid_entities: HashSet<EntityId> = self
            .model_space
            .iter_records()
            .chain(
                self.layouts
                    .values()
                    .flat_map(|l| l.entities().iter_records()),
            )
            .chain(
                self.blocks
                    .values()
                    .flat_map(|b| b.entities().iter_records()),
            )
            .map(|rec| rec.id)
            .collect();
        prune_group_members(&mut self.groups, &valid_entities, &mut issues);

        issues
    }

    /// ID of layer `"0"`, falling back to the first layer for corrupt documents.
    fn resolve_zero_layer(&self) -> Option<LayerId> {
        self.layers
            .values()
            .find(|l| l.name().eq_ignore_ascii_case("0"))
            .or_else(|| self.layers.values().next())
            .map(Layer::id)
    }
}

/// Crate-private mutation surface used by transactions.
#[allow(dead_code)]
impl DrawingDocument {
    /// Allocates a new document [`ObjectId`].
    pub(crate) fn alloc_id(&mut self) -> Result<ObjectId, IdExhausted> {
        self.id_allocator.alloc()
    }

    /// Commits a deferred transaction allocator cursor.
    pub(crate) fn advance_id_cursor_to(&mut self, next: u64) -> Result<(), IdExhausted> {
        self.id_allocator.ensure_above(next.saturating_sub(1))
    }

    /// Mutable model-space access.
    pub(crate) fn model_space_mut(&mut self) -> &mut EntityContainer {
        &mut self.model_space
    }

    /// Mutable access to a selected container, or `None` if absent.
    pub(crate) fn container_mut(
        &mut self,
        container: ContainerRef,
    ) -> Option<&mut EntityContainer> {
        match container {
            ContainerRef::ModelSpace => Some(&mut self.model_space),
            ContainerRef::Layout(id) => self.layouts.get_mut(&id).map(Layout::entities_mut),
            ContainerRef::Block(id) => self.blocks.get_mut(&id).map(BlockDefinition::entities_mut),
        }
    }

    /// Creates a layer with a case-insensitively unique name and returns its ID.
    ///
    /// # Errors
    /// Returns [`DocError::DuplicateName`] when the name already exists.
    pub(crate) fn add_layer(
        &mut self,
        name: impl Into<String>,
        color: Color,
        line_type: StyleId,
        lineweight: Lineweight,
    ) -> Result<LayerId, DocError> {
        let name = name.into();
        if self.name_taken(self.layers.values().map(Layer::name), &name) {
            return Err(DocError::DuplicateName {
                kind: NameKind::Layer,
                name,
            });
        }
        let id: LayerId = self.id_allocator.alloc()?.into();
        self.layers
            .insert(id, Layer::new(id, name, color, line_type, lineweight));
        Ok(id)
    }

    /// Creates a uniquely named line type and returns its ID.
    ///
    /// # Errors
    /// Returns [`DocError::DuplicateName`] when the name already exists.
    pub(crate) fn add_line_type(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<StyleId, DocError> {
        let name = name.into();
        if self.name_taken(self.line_types.values().map(LineType::name), &name) {
            return Err(DocError::DuplicateName {
                kind: NameKind::LineType,
                name,
            });
        }
        let id: StyleId = self.id_allocator.alloc()?.into();
        self.line_types
            .insert(id, LineType::new(id, name, description));
        Ok(id)
    }

    /// Creates a uniquely named text style and returns its ID.
    ///
    /// # Errors
    /// Returns [`DocError::DuplicateName`] when the name already exists.
    pub(crate) fn add_text_style(&mut self, name: impl Into<String>) -> Result<StyleId, DocError> {
        let name = name.into();
        if self.name_taken(self.text_styles.values().map(TextStyle::name), &name) {
            return Err(DocError::DuplicateName {
                kind: NameKind::TextStyle,
                name,
            });
        }
        let id: StyleId = self.id_allocator.alloc()?.into();
        self.text_styles.insert(id, TextStyle::new(id, name));
        Ok(id)
    }

    /// Creates a uniquely named dimension style and returns its ID.
    ///
    /// # Errors
    /// Returns [`DocError::DuplicateName`] when the name already exists.
    pub(crate) fn add_dim_style(&mut self, name: impl Into<String>) -> Result<StyleId, DocError> {
        let name = name.into();
        if self.name_taken(self.dim_styles.values().map(DimStyle::name), &name) {
            return Err(DocError::DuplicateName {
                kind: NameKind::DimStyle,
                name,
            });
        }
        let id: StyleId = self.id_allocator.alloc()?.into();
        self.dim_styles.insert(id, DimStyle::new(id, name));
        Ok(id)
    }

    /// Creates an empty, uniquely named block definition and returns its ID.
    ///
    /// # Errors
    /// Returns [`DocError::DuplicateName`] when the name already exists.
    pub(crate) fn add_block(
        &mut self,
        name: impl Into<String>,
        base_point: Point2,
    ) -> Result<BlockId, DocError> {
        let name = name.into();
        if self.name_taken(self.blocks.values().map(BlockDefinition::name), &name) {
            return Err(DocError::DuplicateName {
                kind: NameKind::Block,
                name,
            });
        }
        let id: BlockId = self.id_allocator.alloc()?.into();
        self.blocks.insert(
            id,
            BlockDefinition {
                id,
                name,
                base_point,
                entities: EntityContainer::new(),
                description: String::new(),
            },
        );
        Ok(id)
    }

    /// Sets the current layer while preserving its existence invariant.
    ///
    /// # Errors
    /// Returns [`DocError::UnknownLayer`] for an unknown ID.
    pub(crate) fn set_current_layer(&mut self, id: LayerId) -> Result<(), DocError> {
        if self.layers.contains_key(&id) {
            self.current_layer = id;
            Ok(())
        } else {
            Err(DocError::UnknownLayer(id))
        }
    }

    /// Sets the current color (`CECOLOR`).
    pub(crate) fn set_current_color(&mut self, color: Color) {
        self.current_color = color;
    }

    /// Sets linear display precision without changing geometry.
    pub(crate) fn set_linear_precision(&mut self, precision: u8) {
        self.linear_precision = precision;
    }

    /// Sets drawing-limit metadata.
    pub(crate) fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// Sets the current line type; an explicit style ID must exist.
    ///
    /// # Errors
    /// Returns [`DocError::UnknownLineType`] for an unknown explicit style ID.
    pub(crate) fn set_current_line_type(&mut self, lt: LineTypeRef) -> Result<(), DocError> {
        if let LineTypeRef::Style(id) = lt
            && !self.line_types.contains_key(&id)
        {
            return Err(DocError::UnknownLineType(id));
        }
        self.current_line_type = lt;
        Ok(())
    }

    /// Sets the current lineweight (`CELWEIGHT`).
    pub(crate) fn set_current_lineweight(&mut self, lw: Lineweight) {
        self.current_lineweight = lw;
    }

    /// Sets `LTSCALE`; the caller guarantees a positive finite value.
    pub(crate) fn set_ltscale(&mut self, scale: f64) {
        self.ltscale = scale;
    }

    /// Whether any existing name matches case-insensitively.
    fn name_taken<'a>(&self, existing: impl Iterator<Item = &'a str>, name: &str) -> bool {
        let needle = name.to_lowercase();
        existing.map(str::to_lowercase).any(|n| n == needle)
    }
}

/// Reversible crate-private layer-table mutations used by transactions.
///
/// They implement mechanics only; transactions validate policy before mutation.
///
/// Creation order is serialized, so operations retain positions for exact undo.
impl DrawingDocument {
    /// Appends a layer and returns its creation-order position.
    pub(crate) fn push_layer(&mut self, layer: Layer) -> usize {
        let index = self.layers.len();
        self.layers.insert(layer.id(), layer);
        index
    }

    /// Inserts a layer at a clamped position to restore creation order.
    pub(crate) fn insert_layer_at(&mut self, index: usize, layer: Layer) {
        let i = index.min(self.layers.len());
        self.layers.shift_insert(i, layer.id(), layer);
    }

    /// Removes a layer while preserving order and returns its position and record.
    pub(crate) fn remove_layer(&mut self, id: LayerId) -> Option<(usize, Layer)> {
        self.layers
            .shift_remove_full(&id)
            .map(|(index, _id, layer)| (index, layer))
    }

    /// Replaces a layer in place and returns its previous value.
    pub(crate) fn replace_layer(&mut self, id: LayerId, layer: Layer) -> Option<Layer> {
        self.layers
            .get_mut(&id)
            .map(|slot| std::mem::replace(slot, layer))
    }

    /// Appends a line type and returns its creation-order position.
    pub(crate) fn push_line_type(&mut self, lt: LineType) -> usize {
        let index = self.line_types.len();
        self.line_types.insert(lt.id(), lt);
        index
    }

    /// Inserts a line type at a clamped position.
    pub(crate) fn insert_line_type_at(&mut self, index: usize, lt: LineType) {
        let i = index.min(self.line_types.len());
        self.line_types.shift_insert(i, lt.id(), lt);
    }

    /// Removes a line type while preserving order and returns its position and record.
    pub(crate) fn remove_line_type(&mut self, id: StyleId) -> Option<(usize, LineType)> {
        self.line_types
            .shift_remove_full(&id)
            .map(|(index, _id, lt)| (index, lt))
    }
}

/// Reversible group-table mechanics that preserve serialized creation order.
impl DrawingDocument {
    /// Appends a group and returns its position.
    pub(crate) fn push_group(&mut self, group: Group) -> usize {
        let index = self.groups.len();
        self.groups.insert(group.id(), group);
        index
    }

    /// Inserts a group at a clamped position.
    pub(crate) fn insert_group_at(&mut self, index: usize, group: Group) {
        let i = index.min(self.groups.len());
        self.groups.shift_insert(i, group.id(), group);
    }

    /// Removes a group while preserving order and returns its position and record.
    pub(crate) fn remove_group(&mut self, id: GroupId) -> Option<(usize, Group)> {
        self.groups
            .shift_remove_full(&id)
            .map(|(index, _id, group)| (index, group))
    }

    /// Replaces a group in place and returns its previous value.
    pub(crate) fn replace_group(&mut self, id: GroupId, group: Group) -> Option<Group> {
        self.groups
            .get_mut(&id)
            .map(|slot| std::mem::replace(slot, group))
    }
}

impl PartialEq for DrawingDocument {
    /// Structural equality compares the allocator by its next ID.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.units == other.units
            && self.tolerances == other.tolerances
            && self.metadata == other.metadata
            && self.id_allocator.peek() == other.id_allocator.peek()
            && self.layers == other.layers
            && self.current_layer == other.current_layer
            && self.current_color == other.current_color
            && self.linear_precision == other.linear_precision
            && self.limits == other.limits
            && self.current_line_type == other.current_line_type
            && self.current_lineweight == other.current_lineweight
            && self.ltscale == other.ltscale
            && self.line_types == other.line_types
            && self.text_styles == other.text_styles
            && self.dim_styles == other.dim_styles
            && self.blocks == other.blocks
            && self.model_space == other.model_space
            && self.layouts == other.layouts
            && self.groups == other.groups
            && self.external_refs == other.external_refs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{LinearUnit, Units};

    #[test]
    fn new_crea_defaults_completos() {
        let doc = Document::new(Units::default());

        // Layer "0" is permanent and current.
        let l0 = doc.layer_by_name("0").expect("capa 0");
        assert_eq!(doc.current_layer(), l0.id());
        assert_eq!(doc.layers().count(), 1);

        // Default styles.
        assert_eq!(doc.line_types().count(), 1);
        assert!(doc.line_types().any(|s| s.name() == "Continuous"));
        assert_eq!(doc.text_styles().count(), 1);
        assert!(doc.text_styles().any(|s| s.name() == "Standard"));
        assert_eq!(doc.dim_styles().count(), 1);
        assert!(doc.dim_styles().any(|s| s.name() == "Standard"));

        // Empty Layout1.
        assert_eq!(doc.layouts().count(), 1);
        let layout = doc.layouts().next().unwrap();
        assert_eq!(layout.name(), "Layout1");
        assert!(layout.entities().is_empty());

        // Empty model space, blocks, and external references.
        assert!(doc.model_space().is_empty());
        assert_eq!(doc.blocks().count(), 0);
        assert!(doc.external_refs().is_empty());

        // Default units are millimeters.
        assert_eq!(doc.units().linear, LinearUnit::Mm);
    }

    #[test]
    fn unicidad_de_nombres_case_insensitive() {
        let mut doc = Document::new(Units::default());
        let continuous = doc.line_types().next().unwrap().id();

        // Layer names are case-insensitively unique.
        assert!(
            doc.add_layer("Muros", Color::ByLayer, continuous, Lineweight::ByLayer)
                .is_ok()
        );
        let err = doc
            .add_layer("muros", Color::ByLayer, continuous, Lineweight::ByLayer)
            .unwrap_err();
        assert!(matches!(
            err,
            DocError::DuplicateName {
                kind: NameKind::Layer,
                ..
            }
        ));

        // Layer "0" cannot be recreated.
        assert!(
            doc.add_layer("0", Color::ByLayer, continuous, Lineweight::ByLayer)
                .is_err()
        );

        // Styles and blocks follow the same rule.
        assert!(doc.add_text_style("standard").is_err());
        assert!(doc.add_dim_style("STANDARD").is_err());
        assert!(doc.add_line_type("continuous", "").is_err());
        assert!(doc.add_block("Puerta", Point2::ORIGIN).is_ok());
        assert!(doc.add_block("puerta", Point2::ORIGIN).is_err());
    }

    #[test]
    fn current_layer_solo_apunta_a_capa_existente() {
        let mut doc = Document::new(Units::default());
        let continuous = doc.line_types().next().unwrap().id();
        let muros = doc
            .add_layer("Muros", Color::ByLayer, continuous, Lineweight::ByLayer)
            .unwrap();
        assert!(doc.set_current_layer(muros).is_ok());
        assert_eq!(doc.current_layer(), muros);

        // An unknown ID does not change the current layer.
        let ghost: LayerId = ObjectId(123_456).into();
        assert!(doc.set_current_layer(ghost).is_err());
        assert_eq!(doc.current_layer(), muros);
    }

    #[test]
    fn roundtrip_serde_documento_con_contenido() {
        use crate::entity::{EntityGeometry, EntityRecord, LineGeo, LineTypeRef};
        use crate::id::EntityId;
        use af_math::Point2;

        let mut doc = Document::new(Units::default());
        let continuous = doc.line_types().next().unwrap().id();
        let muros = doc
            .add_layer(
                "Muros",
                Color::aci(1).unwrap(),
                continuous,
                Lineweight::Mm(0.5),
            )
            .unwrap();
        doc.add_block("Puerta", Point2::new(1.0, 2.0)).unwrap();

        let eid: EntityId = doc.alloc_id().unwrap().into();
        doc.model_space_mut().push(EntityRecord::new(
            eid,
            muros,
            Color::ByLayer,
            LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(3.0, 4.0))),
        ));

        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        // IDs survive and container indexes rebuild.
        assert_eq!(back.id(), doc.id());
        assert!(back.entity(eid).is_some());
    }

    #[test]
    fn grupos_roundtrip_y_backcompat_arcf_viejo() {
        use crate::entity::EntityGeometry;
        use crate::groups::Group;

        // Create a document with one grouped entity.
        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        let eid: crate::id::EntityId = doc.alloc_id().unwrap().into();
        doc.model_space_mut().push(EntityRecord::new(
            eid,
            l0,
            Color::ByLayer,
            crate::entity::LineTypeRef::ByLayer,
            Lineweight::ByLayer,
            EntityGeometry::Point(crate::entity::PointGeo::new(Point2::new(1.0, 2.0))),
        ));
        let gid: GroupId = doc.alloc_id().unwrap().into();
        doc.push_group(Group::new(gid, "G1").with_members(vec![eid]));

        // Serialization round trip stays stable.
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        assert_eq!(back.group(gid).unwrap().members(), &[eid]);

        // Files without `groups` deserialize to an empty table.
        let mut val = serde_json::to_value(&doc).unwrap();
        val.as_object_mut().unwrap().remove("groups");
        let old: Document = serde_json::from_value(val).unwrap();
        assert_eq!(old.groups().count(), 0);
    }

    #[test]
    fn nextobjectid_serializa_como_escalar() {
        let doc = Document::new(Units::default());
        let json = serde_json::to_string(&doc).unwrap();
        // Persist `nextObjectId` as a scalar.
        assert!(
            json.contains(&format!("\"nextObjectId\":{}", doc.next_object_id())),
            "json: {json}"
        );
    }
}
