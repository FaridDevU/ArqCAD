//! Validated command arguments ([`ArgValue`], [`ParsedArgs`]) and JSON argument
//! validation against a parameter schema.
//!
//! [`validate_args`] converts each JSON value to its typed [`ArgValue`] while
//! checking type, range, and referenced entities or layers. Validation happens
//! before execution, so commands never receive malformed arguments.

use std::collections::HashMap;

use af_math::Point2;
use af_model::Document;
use af_model::id::{EntityId, LayerId, ObjectId};
use serde_json::Value;

use crate::spec::{CmdError, ParamSpec, ParamType};

/// A validated, typed command argument.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    /// A 2D point.
    Point(Point2),
    /// A distance greater than zero.
    Distance(f64),
    /// An angle in radians.
    Angle(f64),
    /// A nonnegative count.
    Count(u64),
    /// A set of existing entities with verified IDs.
    EntitySet(Vec<EntityId>),
    /// The canonical `Enum` variant.
    Enum(String),
    /// Free-form text.
    Text(String),
    /// An existing layer with a resolved ID.
    LayerRef(LayerId),
    /// A Boolean value.
    Flag(bool),
    /// A validated polyline path of ordered `(point, bulge)` vertices.
    /// Omitted bulges are zero, and the path always contains at least one vertex.
    Path(Vec<(Point2, f64)>),
}

/// Validated arguments indexed by parameter name.
#[derive(Debug, Clone, Default)]
pub struct ParsedArgs {
    values: HashMap<String, ArgValue>,
}

impl ParsedArgs {
    /// Returns an empty argument set.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Inserts a validated argument.
    pub(crate) fn insert(&mut self, name: String, value: ArgValue) {
        self.values.insert(name, value);
    }

    /// Returns the number of arguments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` when there are no arguments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns an argument by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ArgValue> {
        self.values.get(name)
    }

    /// Returns parameter `name` as a `Point`, if present and correctly typed.
    #[must_use]
    pub fn point(&self, name: &str) -> Option<Point2> {
        match self.get(name) {
            Some(ArgValue::Point(p)) => Some(*p),
            _ => None,
        }
    }

    /// Returns parameter `name` as a `Distance`, if present and correctly typed.
    #[must_use]
    pub fn distance(&self, name: &str) -> Option<f64> {
        match self.get(name) {
            Some(ArgValue::Distance(d)) => Some(*d),
            _ => None,
        }
    }

    /// Returns parameter `name` as an `Angle` in radians, if correctly typed.
    #[must_use]
    pub fn angle(&self, name: &str) -> Option<f64> {
        match self.get(name) {
            Some(ArgValue::Angle(a)) => Some(*a),
            _ => None,
        }
    }

    /// Returns parameter `name` as a `Count`, if present and correctly typed.
    #[must_use]
    pub fn count(&self, name: &str) -> Option<u64> {
        match self.get(name) {
            Some(ArgValue::Count(n)) => Some(*n),
            _ => None,
        }
    }

    /// Returns parameter `name` as an `EntitySet`, if present and correctly typed.
    #[must_use]
    pub fn entity_set(&self, name: &str) -> Option<&[EntityId]> {
        match self.get(name) {
            Some(ArgValue::EntitySet(ids)) => Some(ids),
            _ => None,
        }
    }

    /// Returns parameter `name` as an `Enum`, if present and correctly typed.
    #[must_use]
    pub fn enum_value(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(ArgValue::Enum(s)) => Some(s),
            _ => None,
        }
    }

    /// Returns parameter `name` as `Text`, if present and correctly typed.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(ArgValue::Text(s)) => Some(s),
            _ => None,
        }
    }

    /// Returns parameter `name` as a `LayerRef`, if present and correctly typed.
    #[must_use]
    pub fn layer(&self, name: &str) -> Option<LayerId> {
        match self.get(name) {
            Some(ArgValue::LayerRef(id)) => Some(*id),
            _ => None,
        }
    }

    /// Returns `true` only when parameter `name` is present as `Flag(true)`.
    /// An absent flag is `false`.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.get(name), Some(ArgValue::Flag(true)))
    }

    /// Returns parameter `name` as a `Path` of `(point, bulge)` vertices, if typed.
    #[must_use]
    pub fn path(&self, name: &str) -> Option<&[(Point2, f64)]> {
        match self.get(name) {
            Some(ArgValue::Path(v)) => Some(v),
            _ => None,
        }
    }
}

/// Validates a JSON argument object against `params` and resolves references in `doc`.
///
/// - `args` must be a JSON object, or `null` for no arguments.
/// - Unknown keys produce [`CmdError::UnknownParam`].
/// - Missing parameters use their default; required parameters without one
///   produce [`CmdError::MissingParam`].
///
/// # Errors
/// Returns the first validation problem as a contextual [`CmdError`].
pub(crate) fn validate_args(
    params: &[ParamSpec],
    args: &Value,
    doc: &Document,
) -> Result<ParsedArgs, CmdError> {
    let empty = serde_json::Map::new();
    let map = if args.is_null() {
        &empty
    } else {
        args.as_object().ok_or(CmdError::NotAnObject)?
    };

    // Reject unknown arguments, including misspelled parameter names.
    for key in map.keys() {
        if !params.iter().any(|p| &p.name == key) {
            return Err(CmdError::UnknownParam(key.clone()));
        }
    }

    let mut parsed = ParsedArgs::new();
    for param in params {
        match map.get(&param.name) {
            Some(v) => {
                let value = validate_value(param, v, doc)?;
                parsed.insert(param.name.clone(), value);
            }
            None => {
                if let Some(default) = &param.default {
                    let value = validate_value(param, default, doc)?;
                    parsed.insert(param.name.clone(), value);
                } else if !param.optional {
                    return Err(CmdError::MissingParam(param.name.clone()));
                }
            }
        }
    }
    Ok(parsed)
}

/// Validates one JSON value against the type of `param`.
fn validate_value(param: &ParamSpec, v: &Value, doc: &Document) -> Result<ArgValue, CmdError> {
    match &param.ty {
        ParamType::Point => {
            let arr = v
                .as_array()
                .ok_or_else(|| type_mismatch(param, "Point [x, y]", v))?;
            if arr.len() != 2 {
                return Err(type_mismatch(param, "Point [x, y]", v));
            }
            let x = arr[0]
                .as_f64()
                .ok_or_else(|| type_mismatch(param, "Point [x, y]", v))?;
            let y = arr[1]
                .as_f64()
                .ok_or_else(|| type_mismatch(param, "Point [x, y]", v))?;
            if !x.is_finite() || !y.is_finite() {
                return Err(out_of_range(param, "las coordenadas deben ser finitas"));
            }
            Ok(ArgValue::Point(Point2::new(x, y)))
        }
        ParamType::Distance => {
            let d = v
                .as_f64()
                .ok_or_else(|| type_mismatch(param, "Distance (número)", v))?;
            if !d.is_finite() {
                return Err(out_of_range(param, "la distancia debe ser finita"));
            }
            if d <= 0.0 {
                return Err(out_of_range(param, "la distancia debe ser > 0"));
            }
            Ok(ArgValue::Distance(d))
        }
        ParamType::Angle => {
            let a = v
                .as_f64()
                .ok_or_else(|| type_mismatch(param, "Angle (número, radianes)", v))?;
            if !a.is_finite() {
                return Err(out_of_range(param, "el ángulo debe ser finito"));
            }
            Ok(ArgValue::Angle(a))
        }
        ParamType::Count => {
            // Accept only nonnegative JSON integers; `2.0` is not an integer token.
            let n = v
                .as_u64()
                .ok_or_else(|| type_mismatch(param, "Count (entero >= 0)", v))?;
            Ok(ArgValue::Count(n))
        }
        ParamType::EntitySet => {
            let arr = v
                .as_array()
                .ok_or_else(|| type_mismatch(param, "EntitySet ([ids...])", v))?;
            let mut ids = Vec::with_capacity(arr.len());
            for item in arr {
                let raw = item
                    .as_u64()
                    .ok_or_else(|| type_mismatch(param, "EntitySet ([ids...])", v))?;
                let id: EntityId = ObjectId(raw).into();
                if doc.entity(id).is_none() {
                    return Err(CmdError::UnknownEntity(id));
                }
                ids.push(id);
            }
            Ok(ArgValue::EntitySet(ids))
        }
        ParamType::Enum(variants) => {
            let s = v
                .as_str()
                .ok_or_else(|| type_mismatch(param, "Enum (palabra clave)", v))?;
            match variants.iter().find(|var| var.eq_ignore_ascii_case(s)) {
                Some(canon) => Ok(ArgValue::Enum(canon.clone())),
                None => Err(CmdError::InvalidEnum {
                    param: param.name.clone(),
                    value: s.to_string(),
                    allowed: variants.clone(),
                }),
            }
        }
        ParamType::Text => {
            let s = v
                .as_str()
                .ok_or_else(|| type_mismatch(param, "Text (string)", v))?;
            Ok(ArgValue::Text(s.to_string()))
        }
        ParamType::LayerRef => {
            if let Some(s) = v.as_str() {
                match doc.layer_by_name(s) {
                    Some(layer) => Ok(ArgValue::LayerRef(layer.id())),
                    None => Err(CmdError::UnknownLayer(s.to_string())),
                }
            } else if let Some(raw) = v.as_u64() {
                let id: LayerId = ObjectId(raw).into();
                if doc.layer(id).is_some() {
                    Ok(ArgValue::LayerRef(id))
                } else {
                    Err(CmdError::UnknownLayer(raw.to_string()))
                }
            } else {
                Err(type_mismatch(param, "LayerRef (nombre o id)", v))
            }
        }
        ParamType::Flag => {
            let b = v
                .as_bool()
                .ok_or_else(|| type_mismatch(param, "Flag (bool)", v))?;
            Ok(ArgValue::Flag(b))
        }
        ParamType::Path => {
            const SHAPE: &str = "Path ([{pt:[x,y], bulge?}, ...])";
            let arr = v.as_array().ok_or_else(|| type_mismatch(param, SHAPE, v))?;
            if arr.is_empty() {
                return Err(out_of_range(param, "la trayectoria no puede estar vacía"));
            }
            let mut verts = Vec::with_capacity(arr.len());
            for item in arr {
                let obj = item
                    .as_object()
                    .ok_or_else(|| type_mismatch(param, SHAPE, v))?;
                let pt = obj
                    .get("pt")
                    .and_then(Value::as_array)
                    .filter(|a| a.len() == 2)
                    .ok_or_else(|| type_mismatch(param, SHAPE, v))?;
                let x = pt[0]
                    .as_f64()
                    .ok_or_else(|| type_mismatch(param, SHAPE, v))?;
                let y = pt[1]
                    .as_f64()
                    .ok_or_else(|| type_mismatch(param, SHAPE, v))?;
                // A missing `bulge` denotes a straight segment.
                let bulge = match obj.get("bulge") {
                    None => 0.0,
                    Some(b) => b.as_f64().ok_or_else(|| type_mismatch(param, SHAPE, v))?,
                };
                if !x.is_finite() || !y.is_finite() || !bulge.is_finite() {
                    return Err(out_of_range(
                        param,
                        "los vértices y bulges deben ser finitos",
                    ));
                }
                verts.push((Point2::new(x, y), bulge));
            }
            Ok(ArgValue::Path(verts))
        }
    }
}

/// Builds a [`CmdError::TypeMismatch`] describing the received JSON type.
fn type_mismatch(param: &ParamSpec, expected: &'static str, v: &Value) -> CmdError {
    CmdError::TypeMismatch {
        param: param.name.clone(),
        expected,
        found: json_kind(v).to_string(),
    }
}

/// Builds a [`CmdError::OutOfRange`].
fn out_of_range(param: &ParamSpec, message: &str) -> CmdError {
    CmdError::OutOfRange {
        param: param.name.clone(),
        message: message.to_string(),
    }
}

/// Returns a short JSON type name for error messages.
fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
