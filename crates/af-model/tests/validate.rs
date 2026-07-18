//! Integration tests for public document validation.
//!
//! Corruption setup requires crate-private mutation and remains in unit tests.
//! These tests verify that a healthy document yields no issues and validation is
//! idempotent.

use af_model::doc::Document;
use af_model::units::Units;
use af_model::{IssueCode, Severity};

#[test]
fn documento_sano_no_produce_issues_y_es_idempotente() {
    let mut doc = Document::new(Units::default());
    assert!(doc.validate_full().is_empty());
    // Revalidation finds no pending repair.
    assert!(doc.validate_full().is_empty());
}

#[test]
fn tipos_de_reporte_son_publicos() {
    // Compile-time coverage ensures report vocabulary remains public API.
    let _ = Severity::Repaired;
    let _ = IssueCode::DanglingLayerRef;
    let _ = Severity::Error;
    let _ = IssueCode::BlockCycle;
}
