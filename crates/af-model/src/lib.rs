#![forbid(unsafe_code)]
//! CAD data model for documents, entities, layers, blocks, styles, layouts,
//! transactions, and undo/redo history. All mutations use transactions.

pub mod changeset;
pub mod container;
pub mod doc;
pub mod entity;
pub mod extents;
pub mod groups;
pub mod history;
pub mod id;
pub mod layers;
pub mod layers_ops;
pub mod layouts;
pub mod lin;
pub mod pat;
pub mod session;
pub(crate) mod storage;
pub mod styles;
pub mod sysvar;
pub mod tx;
pub mod units;
pub mod validate;

// Re-export the model root and commonly used types.
pub use changeset::{Cause, ChangeSet};
pub use container::{CommonRef, ContainerRef, EntityContainer, GeoRef};
pub use doc::{
    BlockDefinition, DocError, Document, DocumentId, DrawingDocument, ExternalReference, Limits,
    Metadata, NameKind,
};
pub use groups::Group;
pub use history::{DEFAULT_UNDO_LIMIT, History};
pub use layers::Layer;
pub use layouts::{Layout, Orientation, PaperSettings};
pub use lin::{LinParse, ParsedLinetype, parse_lin};
pub use pat::{HatchPattern, PatFamily, PatParse, parse_pat, standard_patterns};
pub use session::{RedoError, Session, TxOutcome, UndoError};
pub use styles::{DimStyle, LineType, LineTypeDef, TextStyle, linetype_def, linetype_library};
pub use sysvar::{SysvarDef, SysvarError, SysvarScope, SysvarTable, SysvarValue};
pub use tx::{
    DocOp, DocProp, LoadLinetypesReport, Transaction, TxContext, TxError, apply_forward,
    apply_inverse,
};
pub use validate::{Issue, IssueCode, Severity};
