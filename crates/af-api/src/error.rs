//! [`ApiError`], the structured error type that crosses the facade boundary.
//!
//! Every failure serializes as `{ code, message, detail }`; errors cross as values,
//! never panics. `code` is stable and machine-readable, while `detail` carries
//! optional structured context.

use serde::Serialize;
use serde_json::Value;

use af_cmd::{CmdError, ParseError, RegisterError};
use af_io_dxf::DxfError;
use af_model::{SysvarError, TxError};

/// Structured facade error serialized as `{ code, message, detail? }`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ApiError {
    /// Stable machine-readable error class.
    pub code: String,
    /// Human-readable message suitable for the UI or console.
    pub message: String,
    /// Optional structured context such as IDs, counts, or warnings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl ApiError {
    /// Creates an error without detail.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
        }
    }

    /// Adds JSON detail.
    #[must_use]
    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// Maps each [`CmdError`] variant to a stable code so callers never parse messages.
impl From<CmdError> for ApiError {
    fn from(e: CmdError) -> Self {
        let code = match &e {
            CmdError::UnknownCommand(_) => "unknown_command",
            CmdError::NotAnObject => "args_not_object",
            CmdError::UnknownParam(_) => "unknown_param",
            CmdError::MissingParam(_) => "missing_param",
            CmdError::TypeMismatch { .. } => "type_mismatch",
            CmdError::OutOfRange { .. } => "out_of_range",
            CmdError::InvalidEnum { .. } => "invalid_enum",
            CmdError::UnknownEntity(_) => "unknown_entity",
            CmdError::UnknownLayer(_) => "unknown_layer",
            CmdError::Tx(_) => "tx_error",
            CmdError::NothingToUndo => "nothing_to_undo",
            CmdError::NothingToRedo => "nothing_to_redo",
            CmdError::ContractViolation(_) => "contract_violation",
            CmdError::NotPreviewable(_) => "not_previewable",
            CmdError::Failed(_) => "command_failed",
        };
        // Keep typed context in `detail` so the UI does not parse the message.
        let detail = match &e {
            CmdError::UnknownEntity(id) => Some(serde_json::json!({ "entity": id.raw().0 })),
            CmdError::UnknownLayer(r) => Some(serde_json::json!({ "layer": r })),
            CmdError::InvalidEnum { allowed, .. } => {
                Some(serde_json::json!({ "allowed": allowed }))
            }
            _ => None,
        };
        ApiError {
            code: code.to_string(),
            message: e.to_string(),
            detail,
        }
    }
}

impl From<RegisterError> for ApiError {
    fn from(e: RegisterError) -> Self {
        ApiError::new("register_error", e.to_string())
    }
}

/// Maps a coordinate [`ParseError`] to `parse_error` with its byte position.
impl From<ParseError> for ApiError {
    fn from(e: ParseError) -> Self {
        ApiError {
            code: "parse_error".to_string(),
            message: e.msg,
            detail: Some(serde_json::json!({ "pos": e.pos })),
        }
    }
}

impl From<af_io_native::Error> for ApiError {
    fn from(e: af_io_native::Error) -> Self {
        // Preserve validation issues for unrecoverable on-disk documents.
        let detail = match &e {
            af_io_native::Error::InvalidDocument { count, .. } => {
                Some(serde_json::json!({ "issue_count": count }))
            }
            af_io_native::Error::NewerVersion { found, supported } => {
                Some(serde_json::json!({ "found": found, "supported": supported }))
            }
            _ => None,
        };
        ApiError {
            code: "io_error".to_string(),
            message: e.to_string(),
            detail,
        }
    }
}

impl From<DxfError> for ApiError {
    fn from(e: DxfError) -> Self {
        ApiError::new("dxf_error", e.to_string())
    }
}

/// Maps direct-property-update [`TxError`] variants to the same stable vocabulary
/// used for command errors.
impl From<TxError> for ApiError {
    fn from(e: TxError) -> Self {
        let code = match &e {
            TxError::UnknownEntity(_) => "unknown_entity",
            TxError::UnknownLayer(_) => "unknown_layer",
            TxError::UnknownLineType(_) => "unknown_line_type",
            TxError::InvalidGeometry(_) => "invalid_geometry",
            TxError::DuplicateLayerName(_) => "duplicate_layer_name",
            TxError::LayerZeroProtected(_)
            | TxError::CurrentLayerRemoval(_)
            | TxError::LayerInUse(_) => "layer_rule",
            TxError::DuplicateGroupName(_) => "duplicate_group_name",
            TxError::UnknownGroup(_) => "unknown_group",
            TxError::DuplicateLineTypeName(_)
            | TxError::LineTypeProtected(_)
            | TxError::LineTypeInUse(_) => "line_type_rule",
            TxError::UnknownContainer(_) | TxError::Internal(_) => "tx_error",
        };
        let detail = match &e {
            TxError::UnknownEntity(id) => Some(serde_json::json!({ "entity": id.raw().0 })),
            TxError::UnknownLayer(id) => Some(serde_json::json!({ "layer": id.raw().0 })),
            TxError::UnknownLineType(id) => Some(serde_json::json!({ "lineType": id.raw().0 })),
            _ => None,
        };
        ApiError {
            code: code.to_string(),
            message: e.to_string(),
            detail,
        }
    }
}

/// Maps [`SysvarError`] variants to stable codes shared with command value errors.
impl From<SysvarError> for ApiError {
    fn from(e: SysvarError) -> Self {
        let (code, detail) = match &e {
            SysvarError::Unknown(name) => {
                ("unknown_sysvar", Some(serde_json::json!({ "name": name })))
            }
            SysvarError::TypeMismatch { expected, got, .. } => (
                "type_mismatch",
                Some(serde_json::json!({ "expected": expected, "got": got })),
            ),
            SysvarError::OutOfRange { allowed, .. } => (
                "out_of_range",
                Some(serde_json::json!({ "allowed": allowed })),
            ),
        };
        ApiError {
            code: code.to_string(),
            message: e.to_string(),
            detail,
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::new("malformed_json", e.to_string())
    }
}
