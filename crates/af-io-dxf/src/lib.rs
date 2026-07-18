#![forbid(unsafe_code)]
//! DXF import and export adapter.
//!
//! [`export_dxf`] writes structurally valid DXF R2000 (AC1015) ASCII with layers,
//! units, and supported LINE, POINT, CIRCLE, and bulged LWPOLYLINE entities.
//!
//! [`import_dxf`] tolerates the supported R12 through R2018 subset and applies it
//! to a [`Session`] in one transaction. Unsupported content is skipped with an
//! [`ImportReport`] warning instead of being silently discarded.
//!
//! # Format behavior
//! - Internal radians convert to file degrees; group-42 bulges remain dimensionless.
//! - Export always uses CRLF, full-precision floats, and hexadecimal handles.
//! - Color export writes nearest ACI plus true color when needed; true color has
//!   precedence on import.
//! - Off layers use negative color; frozen and locked states use flag-70 bits.
//! - The adapter translates data without changing the model to fit DXF.
//!
//! [`Session`]: af_model::Session

mod aci;
mod export;
mod import;
mod report;

pub use export::export_dxf;
pub use import::import_dxf;
pub use report::{DxfError, ExportOptions, ExportReport, ImportOptions, ImportReport};
