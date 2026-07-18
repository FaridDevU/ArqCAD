//! Load validation report types and reusable checks.
//!
//! Recover valid data from partially corrupt files, but report every repair.
//!
//! | Corruption | Severity | Action |
//! |---|---|---|
//! | Duplicate object ID | [`Severity::Error`] | Report without repair |
//! | Entity references unknown layer | [`Severity::Repaired`] | Reassign to layer `"0"` |
//! | Entity references unknown style/block | [`Severity::Repaired`] | Discard entity |
//! | Layer references unknown default line type | [`Severity::Repaired`] | Use first line type |
//! | Empty line-type catalog | [`Severity::Error`] | Report without repair |
//! | `nextObjectId` ≤ maximum ID | [`Severity::Repaired`] | Raise with `ensure_above` |
//! | Block-definition cycle | [`Severity::Error`] | Report without repair |
//! | Geometry contains `NaN`/∞ | [`Severity::Repaired`] | Discard entity |
//!
//! [`Document::validate_full`](crate::doc::Document::validate_full) orchestrates
//! document traversal and repair.

use std::collections::{HashMap, HashSet};

use af_math::Tol;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::container::EntityContainer;
use crate::entity::{EntityGeometry, EntityOps, LineTypeRef};
use crate::groups::Group;
use crate::id::{BlockId, EntityId, GroupId, LayerId, ObjectId, StyleId};
use crate::layers::Layer;

/// Validation [`Issue`] severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Notable condition without data changes.
    Warning,
    /// Data was modified to recover it.
    Repaired,
    /// Unrecoverable error reported without repair.
    Error,
}

/// Closed code identifying an [`Issue`] class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueCode {
    /// Two objects share one ID.
    DuplicateId,
    /// Entity references an unknown layer.
    DanglingLayerRef,
    /// Entity references an unknown style.
    DanglingStyleRef,
    /// Entity references an unknown block.
    DanglingBlockRef,
    /// `nextObjectId` is not above all used IDs.
    NextObjectIdTooLow,
    /// Block definitions form a reference cycle.
    BlockCycle,
    /// Geometry contains a nonfinite coordinate.
    NonFiniteGeometry,
    /// Group contains a missing entity member.
    DanglingGroupMember,
}

/// One validation/load report item.
///
/// This report value is not part of serialized document state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Severity and repair status.
    pub severity: Severity,
    /// Problem class.
    pub code: IssueCode,
    /// Human-readable message.
    pub message: String,
    /// Optional affected object ID.
    pub object: Option<ObjectId>,
}

impl Issue {
    /// Creates an [`Issue`].
    pub(crate) fn new(
        severity: Severity,
        code: IssueCode,
        message: impl Into<String>,
        object: Option<ObjectId>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            object,
        }
    }
}

/// Repairs a container and reports each change: discards nonfinite geometry and
/// unknown explicit line types, and reassigns unknown layers to `layer0`.
///
/// Discarding takes priority; duplicate IDs and block cycles are global checks.
pub(crate) fn scan_and_repair_container(
    c: &mut EntityContainer,
    layer0: LayerId,
    valid_layers: &HashSet<LayerId>,
    valid_line_types: &HashSet<StyleId>,
    tol: &Tol,
    issues: &mut Vec<Issue>,
) {
    let mut to_discard: Vec<(EntityId, IssueCode, String)> = Vec::new();
    let mut to_relayer: Vec<EntityId> = Vec::new();

    for rec in c.iter_records() {
        // Discard nonfinite geometry.
        if rec.geometry.validate(tol).is_err() {
            to_discard.push((
                rec.id,
                IssueCode::NonFiniteGeometry,
                format!(
                    "entity {} discarded: geometry contains a non-finite coordinate",
                    rec.id.raw().0
                ),
            ));
            continue;
        }
        // Discard unknown explicit line types.
        if let LineTypeRef::Style(sid) = rec.line_type
            && !valid_line_types.contains(&sid)
        {
            to_discard.push((
                rec.id,
                IssueCode::DanglingStyleRef,
                format!(
                    "entity {} discarded: references missing line type {}",
                    rec.id.raw().0,
                    sid.raw().0
                ),
            ));
            continue;
        }
        // Reassign unknown layers to layer "0".
        if !valid_layers.contains(&rec.layer) {
            to_relayer.push(rec.id);
        }
    }

    for (id, code, message) in to_discard {
        c.remove_by_id(id);
        issues.push(Issue::new(
            Severity::Repaired,
            code,
            message,
            Some(id.raw()),
        ));
    }
    for id in to_relayer {
        if let Some(mut rec) = c.get(id) {
            rec.layer = layer0;
            c.replace(id, rec);
        }
        issues.push(Issue::new(
            Severity::Repaired,
            IssueCode::DanglingLayerRef,
            format!(
                "entity {} reassigned to layer \"0\": referenced layer did not exist",
                id.raw().0
            ),
            Some(id.raw()),
        ));
    }
}

/// Reassigns unknown default layer line types to `fallback` and reports each repair.
///
/// Layers cannot be discarded safely, so reassignment preserves `ByLayer` resolution.
pub(crate) fn repair_layer_line_types(
    layers: &mut IndexMap<LayerId, Layer>,
    valid_line_types: &HashSet<StyleId>,
    fallback: StyleId,
    issues: &mut Vec<Issue>,
) {
    for (id, layer) in layers.iter_mut() {
        let current = layer.line_type();
        if !valid_line_types.contains(&current) {
            *layer = layer.clone().with_line_type(fallback);
            issues.push(Issue::new(
                Severity::Repaired,
                IssueCode::DanglingStyleRef,
                format!(
                    "layer {} default line type {} did not exist; reassigned to {}",
                    id.raw().0,
                    current.raw().0,
                    fallback.raw().0
                ),
                Some(id.raw()),
            ));
        }
    }
}

/// Prunes missing entity members and reports each repaired group. Empty named
/// groups remain valid.
pub(crate) fn prune_group_members(
    groups: &mut IndexMap<GroupId, Group>,
    valid_entities: &HashSet<EntityId>,
    issues: &mut Vec<Issue>,
) {
    for (id, group) in groups.iter_mut() {
        let kept: Vec<EntityId> = group
            .members()
            .iter()
            .copied()
            .filter(|m| valid_entities.contains(m))
            .collect();
        let dropped = group.members().len() - kept.len();
        if dropped > 0 {
            *group = group.clone().with_members(kept);
            issues.push(Issue::new(
                Severity::Repaired,
                IssueCode::DanglingGroupMember,
                format!(
                    "group {} dropped {dropped} member(s) referencing non-existent entities",
                    id.raw().0
                ),
                Some(id.raw()),
            ));
        }
    }
}

/// Block ID referenced by geometry, if any.
///
/// The exhaustive match forces new referencing geometry variants to declare edges.
pub(crate) fn referenced_block(geom: &EntityGeometry) -> Option<BlockId> {
    match geom {
        EntityGeometry::Line(_)
        | EntityGeometry::Point(_)
        | EntityGeometry::Circle(_)
        | EntityGeometry::Arc(_)
        | EntityGeometry::Ellipse(_)
        | EntityGeometry::Polyline(_)
        | EntityGeometry::Xline(_)
        | EntityGeometry::Ray(_)
        | EntityGeometry::Spline(_)
        | EntityGeometry::Wipeout(_) => None,
    }
}

/// Block IDs referenced by entities in a container.
pub(crate) fn block_dependencies(c: &EntityContainer) -> Vec<BlockId> {
    c.iter_records()
        .filter_map(|rec| referenced_block(&rec.geometry))
        .collect()
}

/// Detects a block-definition dependency cycle using tri-color DFS.
pub(crate) fn find_block_cycle(adj: &HashMap<BlockId, Vec<BlockId>>) -> Option<BlockId> {
    // 0 = unvisited, 1 = active stack, 2 = closed.
    let mut color: HashMap<BlockId, u8> = HashMap::new();
    for &start in adj.keys() {
        if color.get(&start).copied().unwrap_or(0) == 0
            && let Some(found) = dfs_cycle(start, adj, &mut color)
        {
            return Some(found);
        }
    }
    None
}

fn dfs_cycle(
    node: BlockId,
    adj: &HashMap<BlockId, Vec<BlockId>>,
    color: &mut HashMap<BlockId, u8>,
) -> Option<BlockId> {
    color.insert(node, 1);
    if let Some(children) = adj.get(&node) {
        for &child in children {
            match color.get(&child).copied().unwrap_or(0) {
                1 => return Some(child), // Back edge to an active node.
                0 => {
                    if let Some(found) = dfs_cycle(child, adj, color) {
                        return Some(found);
                    }
                }
                _ => {} // Closed subtree already proved acyclic.
            }
        }
    }
    color.insert(node, 2);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;

    use crate::doc::Document;
    use crate::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
    use crate::id::{BlockId, EntityId, LayerId, ObjectId, StyleId};
    use crate::units::Units;

    // Test setup helpers.

    fn line_geo(x: f64) -> EntityGeometry {
        EntityGeometry::Line(LineGeo::new(Point2::new(x, 0.0), Point2::new(x + 1.0, 0.0)))
    }

    fn record(
        id: EntityId,
        layer: LayerId,
        line_type: LineTypeRef,
        geom: EntityGeometry,
    ) -> EntityRecord {
        EntityRecord::new(
            id,
            layer,
            Color::ByLayer,
            line_type,
            Lineweight::ByLayer,
            geom,
        )
    }

    fn find(issues: &[Issue], code: IssueCode) -> Option<&Issue> {
        issues.iter().find(|i| i.code == code)
    }

    // Duplicate ID: unrepaired error.

    #[test]
    fn corrupcion_id_duplicado() {
        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        // Use an allocated ID to avoid a second allocator-cursor issue.
        let dup: EntityId = doc.alloc_id().unwrap().into();
        doc.model_space_mut()
            .push(record(dup, l0, LineTypeRef::ByLayer, line_geo(0.0)));
        doc.model_space_mut()
            .push(record(dup, l0, LineTypeRef::ByLayer, line_geo(5.0)));

        let issues = doc.validate_full();
        let issue = find(&issues, IssueCode::DuplicateId).expect("esperaba DuplicateId");
        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(issue.object, Some(dup.raw()));
        // Both entities remain because duplicate IDs are unrepaired.
        assert_eq!(doc.model_space().len(), 2);
    }

    // Unknown layer reference: repair to layer "0".

    #[test]
    fn corrupcion_capa_inexistente_se_reasigna_a_cero() {
        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        let id: EntityId = doc.alloc_id().unwrap().into();
        let ghost_layer: LayerId = ObjectId(9999).into(); // Reference, not an object.
        doc.model_space_mut()
            .push(record(id, ghost_layer, LineTypeRef::ByLayer, line_geo(0.0)));

        let issues = doc.validate_full();
        let issue = find(&issues, IssueCode::DanglingLayerRef).expect("esperaba DanglingLayerRef");
        assert_eq!(issue.severity, Severity::Repaired);
        assert_eq!(issue.object, Some(id.raw()));
        // The repaired entity now references layer "0".
        let (rec, _) = doc.entity(id).expect("la entidad sigue presente");
        assert_eq!(rec.layer, l0);
    }

    // Unknown style reference: discard entity and report repair.

    #[test]
    fn corrupcion_estilo_inexistente_descarta_entidad() {
        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        let id: EntityId = doc.alloc_id().unwrap().into();
        let ghost_style: StyleId = ObjectId(8888).into(); // Reference, not an object.
        doc.model_space_mut().push(record(
            id,
            l0,
            LineTypeRef::Style(ghost_style),
            line_geo(0.0),
        ));

        let issues = doc.validate_full();
        let issue = find(&issues, IssueCode::DanglingStyleRef).expect("esperaba DanglingStyleRef");
        assert_eq!(issue.severity, Severity::Repaired);
        assert_eq!(issue.object, Some(id.raw()));
        // Entity was discarded.
        assert!(doc.entity(id).is_none());
        assert_eq!(doc.model_space().len(), 0);
    }

    // Low `nextObjectId`: raise above maximum ID.

    #[test]
    fn corrupcion_next_object_id_bajo_se_sube() {
        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        // Insert a high manual ID without advancing the allocator.
        let high: EntityId = ObjectId(1000).into();
        doc.model_space_mut()
            .push(record(high, l0, LineTypeRef::ByLayer, line_geo(0.0)));
        assert!(
            doc.next_object_id() <= 1000,
            "precondición: allocator por debajo"
        );

        let issues = doc.validate_full();
        let issue =
            find(&issues, IssueCode::NextObjectIdTooLow).expect("esperaba NextObjectIdTooLow");
        assert_eq!(issue.severity, Severity::Repaired);
        assert_eq!(issue.object, None);
        // The repaired allocator is above the maximum ID.
        assert_eq!(doc.next_object_id(), 1001);
    }

    #[test]
    fn id_exhaustion_max_object_id_is_not_reported_as_repaired() {
        let mut doc = Document::new(Units::default());
        let max: EntityId = ObjectId(u64::MAX).into();
        let layer = doc.current_layer();
        doc.model_space_mut()
            .push(record(max, layer, LineTypeRef::ByLayer, line_geo(0.0)));

        let issues = doc.validate_full();
        let issue = find(&issues, IssueCode::NextObjectIdTooLow)
            .expect("expected terminal nextObjectId issue");
        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(doc.next_object_id(), u64::MAX);
    }

    // Block cycle: unrepaired error.

    #[test]
    fn corrupcion_ciclo_de_bloques_detectado_por_el_algoritmo() {
        let a: BlockId = ObjectId(1).into();
        let b: BlockId = ObjectId(2).into();
        let c: BlockId = ObjectId(3).into();

        // A → B → C → A is cyclic.
        let mut cyclic: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        cyclic.insert(a, vec![b]);
        cyclic.insert(b, vec![c]);
        cyclic.insert(c, vec![a]);
        assert!(find_block_cycle(&cyclic).is_some());

        // A → B → C is acyclic.
        let mut acyclic: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
        acyclic.insert(a, vec![b]);
        acyclic.insert(b, vec![c]);
        acyclic.insert(c, vec![]);
        assert!(find_block_cycle(&acyclic).is_none());
    }

    #[test]
    fn documento_con_bloques_sin_referencias_no_reporta_ciclo() {
        let mut doc = Document::new(Units::default());
        doc.add_block("Puerta", Point2::ORIGIN).unwrap();
        doc.add_block("Ventana", Point2::ORIGIN).unwrap();
        let issues = doc.validate_full();
        assert!(find(&issues, IssueCode::BlockCycle).is_none());
    }

    // Unknown default layer line type: repair to a valid style.

    #[test]
    fn corrupcion_capa_con_line_type_inexistente_se_repara() {
        let mut doc = Document::new(Units::default());
        let ghost_style: StyleId = ObjectId(7777).into(); // Reference, not an object.
        let muros = doc
            .add_layer("Muros", Color::ByLayer, ghost_style, Lineweight::ByLayer)
            .unwrap();

        let issues = doc.validate_full();
        let issue = find(&issues, IssueCode::DanglingStyleRef)
            .expect("esperaba DanglingStyleRef para la capa");
        assert_eq!(issue.severity, Severity::Repaired);
        assert_eq!(issue.object, Some(muros.raw()));

        // The repaired layer references an existing style.
        let layer = doc.layer(muros).expect("la capa sigue presente");
        assert!(
            doc.line_type(layer.line_type()).is_some(),
            "el line_type reasignado debe existir en el catálogo"
        );
    }

    // Empty line-type catalog: unrecoverable because no fallback exists.
    #[test]
    fn corrupcion_catalogo_line_types_vacio_es_error_irrecuperable() {
        let doc = Document::new(Units::default());
        let mut val = serde_json::to_value(&doc).unwrap();
        val["lineTypes"] = serde_json::json!({}); // Empty the catalog.
        let mut corrupt: Document = serde_json::from_value(val).unwrap();

        let issues = corrupt.validate_full();
        let issue = find(&issues, IssueCode::DanglingStyleRef)
            .expect("esperaba DanglingStyleRef por catálogo de line_types vacío");
        assert_eq!(
            issue.severity,
            Severity::Error,
            "catálogo vacío es irrecuperable, no reparable"
        );
    }

    // Nonfinite geometry: discard entity and report repair.

    #[test]
    fn corrupcion_geometria_no_finita_descarta_entidad() {
        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        let id: EntityId = doc.alloc_id().unwrap().into();
        let nan_geom = EntityGeometry::Line(LineGeo::new(
            Point2::new(f64::NAN, 0.0),
            Point2::new(1.0, 1.0),
        ));
        doc.model_space_mut()
            .push(record(id, l0, LineTypeRef::ByLayer, nan_geom));

        let issues = doc.validate_full();
        let issue =
            find(&issues, IssueCode::NonFiniteGeometry).expect("esperaba NonFiniteGeometry");
        assert_eq!(issue.severity, Severity::Repaired);
        assert_eq!(issue.object, Some(id.raw()));
        assert!(doc.entity(id).is_none());
    }

    // Missing group member: prune and report repair.

    #[test]
    fn corrupcion_miembro_de_grupo_colgante_se_poda() {
        use crate::groups::Group;
        use crate::id::GroupId;

        let mut doc = Document::new(Units::default());
        let l0 = doc.current_layer();
        let real: EntityId = doc.alloc_id().unwrap().into();
        doc.model_space_mut()
            .push(record(real, l0, LineTypeRef::ByLayer, line_geo(0.0)));
        let ghost: EntityId = ObjectId(9998).into(); // Reference, not an object.
        let gid: GroupId = doc.alloc_id().unwrap().into();
        doc.push_group(Group::new(gid, "G").with_members(vec![real, ghost]));

        let issues = doc.validate_full();
        let issue =
            find(&issues, IssueCode::DanglingGroupMember).expect("esperaba DanglingGroupMember");
        assert_eq!(issue.severity, Severity::Repaired);
        assert_eq!(issue.object, Some(gid.raw()));
        // The valid member survives and the named group remains.
        assert_eq!(doc.group(gid).unwrap().members(), &[real]);
    }

    // Clean document produces no issues.

    #[test]
    fn documento_nuevo_no_produce_issues() {
        let mut doc = Document::new(Units::default());
        assert!(doc.validate_full().is_empty());
    }
}
