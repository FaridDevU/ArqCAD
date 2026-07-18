#![forbid(unsafe_code)]
//! Synthetic document generation and performance baselines for the current engine.
//!
//! This crate is internal development tooling and is not published.
//!
//! - [`r#gen`] generates deterministic synthetic documents.
//! - [`workloads`] defines shared benchmark and smoke-test operations.

pub mod r#gen;
pub mod workloads;
