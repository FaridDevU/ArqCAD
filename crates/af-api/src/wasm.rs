//! wasm-bindgen facade bindings enabled by the `wasm` feature.
//!
//! This is a thin [`ApiSession`] wrapper. Control data crosses as JSON strings;
//! geometry and binary data cross as typed arrays. Mutating paths use
//! `catch_unwind`, converting panics into structured errors at the boundary.

use std::panic::AssertUnwindSafe;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::error::ApiError;
use crate::session::{ApiSession, error_json, result_json, units_from_str};

/// Plugin and scripting API semantic version.
#[wasm_bindgen(js_name = apiVersion)]
#[must_use]
pub fn api_version() -> String {
    crate::API_VERSION.to_string()
}

/// Converts a panic from a mutating operation into a structured [`ApiError`].
fn guard<T>(f: impl FnOnce() -> Result<T, ApiError>) -> Result<T, ApiError> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(ApiError::new(
            "panic",
            "internal panic caught at API boundary; reload the document",
        ))
    })
}

/// Serializes an infallible read-only query as plain JSON.
fn plain_json<T: Serialize>(t: &T) -> String {
    serde_json::to_string(t)
        .unwrap_or_else(|_| error_json(&ApiError::new("serialize", "failed to serialize response")))
}

/// Converts successful bytes to `Uint8Array` and throws structured JSON on error.
fn bytes_result(r: Result<Vec<u8>, ApiError>) -> Result<Vec<u8>, JsValue> {
    r.map_err(|e| JsValue::from_str(&error_json(&e)))
}

/// JavaScript-facing wrapper around a native [`ApiSession`].
#[wasm_bindgen]
pub struct WasmSession {
    inner: ApiSession,
}

#[wasm_bindgen]
impl WasmSession {
    /// Creates a session. Supported units include `"mm"`, `"cm"`, `"m"`, `"in"`,
    /// `"ft"`, and `"unitless"`; the default is `"mm"`.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(units: &str) -> WasmSession {
        WasmSession {
            inner: ApiSession::new(units_from_str(units)),
        }
    }

    /// Executes a command with JSON object arguments and returns `ResultJson`.
    #[must_use]
    pub fn execute(&mut self, name: &str, args_json: &str) -> String {
        self.inner.execute_json(name, args_json)
    }

    /// Previews a command without changing the document or creating a transaction.
    #[wasm_bindgen(js_name = preview)]
    #[must_use]
    pub fn preview(&self, name: &str, args_json: &str) -> String {
        result_json(&guard(|| {
            let args: serde_json::Value = if args_json.trim().is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_str(args_json)?
            };
            self.inner.preview(name, &args)
        }))
    }

    /// Opens an `.arcf` document from bytes and returns recovery warnings.
    #[must_use]
    pub fn open(&mut self, bytes: &[u8]) -> String {
        result_json(&guard(|| self.inner.open(bytes)))
    }

    /// Serializes the document to `.arcf` bytes.
    ///
    /// # Errors
    /// Throws structured `ResultJson` when serialization fails.
    pub fn save(&self) -> Result<Vec<u8>, JsValue> {
        bytes_result(guard(|| self.inner.save()))
    }

    /// Imports DXF bytes into the document and returns a report.
    #[wasm_bindgen(js_name = importDxf)]
    #[must_use]
    pub fn import_dxf(&mut self, bytes: &[u8]) -> String {
        result_json(&guard(|| self.inner.import_dxf(bytes)))
    }

    /// Exports DXF R2000 bytes.
    ///
    /// # Errors
    /// Throws structured `ResultJson` when export fails.
    #[wasm_bindgen(js_name = exportDxf)]
    pub fn export_dxf(&self) -> Result<Vec<u8>, JsValue> {
        bytes_result(guard(|| {
            self.inner.export_dxf().map(|(bytes, _report)| bytes)
        }))
    }

    /// Returns document and session state as JSON.
    #[wasm_bindgen(js_name = docInfo)]
    #[must_use]
    pub fn doc_info(&self) -> String {
        plain_json(&self.inner.doc_info())
    }

    /// Returns document layers as a JSON array of `LayerInfo` values.
    #[wasm_bindgen(js_name = layers)]
    #[must_use]
    pub fn layers(&self) -> String {
        plain_json(&self.inner.layers())
    }

    /// Atomically changes common properties for a set of entities in one transaction.
    #[wasm_bindgen(js_name = setEntityProps)]
    #[must_use]
    pub fn set_entity_props(&mut self, ids_json: &str, props_json: &str) -> String {
        result_json(&guard(|| {
            let ids: Vec<u64> = serde_json::from_str(ids_json)?;
            let props = serde_json::from_str(props_json)?;
            self.inner.set_entity_props(&ids, &props)
        }))
    }

    /// Returns the current system-variable value.
    #[wasm_bindgen(js_name = getSysvar)]
    #[must_use]
    pub fn get_sysvar(&self, name: &str) -> String {
        result_json(&self.inner.get_sysvar(name))
    }

    /// Sets a system variable and queues `SysvarChanged` on success.
    #[wasm_bindgen(js_name = setSysvar)]
    #[must_use]
    pub fn set_sysvar(&mut self, name: &str, value_json: &str) -> String {
        result_json(&guard(|| {
            let value = serde_json::from_str(value_json)?;
            self.inner.set_sysvar(name, value)
        }))
    }

    /// Parses coordinate input with an optional base point and returns an absolute point.
    #[wasm_bindgen(js_name = parseInput)]
    #[must_use]
    pub fn parse_input(&self, input: &str, base_json: &str) -> String {
        let base: Option<[f64; 2]> = if base_json.trim().is_empty() {
            None
        } else {
            serde_json::from_str(base_json).unwrap_or(None)
        };
        result_json(&self.inner.parse_input(input, base))
    }

    /// Returns registered commands as JSON.
    #[wasm_bindgen(js_name = listCommands)]
    #[must_use]
    pub fn list_commands(&self) -> String {
        plain_json(&self.inner.list_commands())
    }

    /// Hit-tests `(x, y)` with tolerance `tol`.
    #[must_use]
    pub fn pick(&self, x: f64, y: f64, tol: f64) -> String {
        plain_json(&self.inner.pick(x, y, tol))
    }

    /// Selects entities by rectangle.
    #[wasm_bindgen(js_name = selectWindow)]
    #[must_use]
    pub fn select_window(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        crossing: bool,
    ) -> String {
        plain_json(
            &self
                .inner
                .select_window(min_x, min_y, max_x, max_y, crossing),
        )
    }

    /// Finds snap candidates near `(x, y)` within `radius`.
    #[must_use]
    pub fn snap(&self, x: f64, y: f64, radius: f64, opts_json: &str) -> String {
        let opts: serde_json::Value = if opts_json.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(opts_json).unwrap_or(serde_json::Value::Null)
        };
        plain_json(&self.inner.snap(x, y, radius, &opts))
    }

    /// Returns the current selection.
    #[must_use]
    pub fn selection(&self) -> String {
        plain_json(&self.inner.selection())
    }

    /// Replaces the selection from a JSON array of IDs.
    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&mut self, ids_json: &str) {
        let ids: Vec<u64> = serde_json::from_str(ids_json).unwrap_or_default();
        self.inner.set_selection(&ids);
    }

    /// Clears the selection.
    #[wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&mut self) {
        self.inner.clear_selection();
    }

    /// Selects by arbitrary window or crossing polygon.
    #[wasm_bindgen(js_name = selectPolygon)]
    #[must_use]
    pub fn select_polygon(&mut self, points_json: &str, crossing: bool) -> String {
        let points: Vec<[f64; 2]> = serde_json::from_str(points_json).unwrap_or_default();
        plain_json(&self.inner.select_polygon(&points, crossing))
    }

    /// Selects by fence polyline.
    #[wasm_bindgen(js_name = selectFence)]
    #[must_use]
    pub fn select_fence(&mut self, points_json: &str) -> String {
        let points: Vec<[f64; 2]> = serde_json::from_str(points_json).unwrap_or_default();
        plain_json(&self.inner.select_fence(&points))
    }

    /// Selects by property filter (`QSELECT`).
    #[wasm_bindgen(js_name = selectFilter)]
    #[must_use]
    pub fn select_filter(&mut self, filter_json: &str) -> String {
        result_json(&guard(|| {
            let filter = serde_json::from_str(filter_json)?;
            self.inner.select_filter(&filter)
        }))
    }

    /// Selects entities similar to `id` (`SELECTSIMILAR`).
    #[wasm_bindgen(js_name = selectSimilar)]
    #[must_use]
    pub fn select_similar(&mut self, id: f64) -> String {
        plain_json(&self.inner.select_similar(id as u64))
    }

    /// Restores the previous selection.
    #[wasm_bindgen(js_name = selectPrevious)]
    #[must_use]
    pub fn select_previous(&mut self) -> String {
        plain_json(&self.inner.select_previous())
    }

    /// Returns all IDs under `(x, y)` for selection cycling without mutating selection.
    #[wasm_bindgen(js_name = pickAll)]
    #[must_use]
    pub fn pick_all(&self, x: f64, y: f64, tol: f64) -> String {
        plain_json(&self.inner.pick_all(x, y, tol))
    }

    /// Returns document groups as JSON.
    #[wasm_bindgen(js_name = groups)]
    #[must_use]
    pub fn groups(&self) -> String {
        plain_json(&self.inner.groups())
    }

    /// Returns full render control data; vertices come from [`render_vertices`](Self::render_vertices).
    #[wasm_bindgen(js_name = renderFull)]
    #[must_use]
    pub fn render_full(&mut self) -> String {
        plain_json(&self.inner.render_full().batches)
    }

    /// Returns packed render vertices as `Float32Array`.
    #[wasm_bindgen(js_name = renderVertices)]
    #[must_use]
    pub fn render_vertices(&self) -> Vec<f32> {
        self.inner.render_vertices()
    }

    /// Returns the render delta since the previous full or delta query.
    #[wasm_bindgen(js_name = renderDelta)]
    #[must_use]
    pub fn render_delta(&mut self) -> String {
        plain_json(&self.inner.render_delta())
    }

    /// Sets flattening chord tolerance in world units and rebuilds render data.
    #[wasm_bindgen(js_name = setChordErr)]
    pub fn set_chord_err(&mut self, chord_err: f64) {
        self.inner.set_chord_err(chord_err);
    }

    /// Drains the event queue as JSON.
    #[wasm_bindgen(js_name = pollEvents)]
    #[must_use]
    pub fn poll_events(&mut self) -> String {
        plain_json(&self.inner.poll_events())
    }
}
