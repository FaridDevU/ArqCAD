//! Shared performance-baseline workloads.
//!
//! Each public function is the exact work registered with Criterion. Unit tests
//! run every workload once because `harness = false` benchmark targets do not run
//! under the default `cargo test` invocation.
//!
//! Workloads call the real selection, native I/O, and DXF implementations on
//! documents from `gen::synth_doc`.

use std::path::Path;

use af_math::{BBox, Point2, Transform2, Vec2};
use af_model::entity::EntityOps;
use af_model::extents::{ExtentsFilter, doc_extents};
use af_model::id::EntityId;
use af_model::{ContainerRef, Document, Session, TxError};
use af_select::{SpatialIndex, pick};

// ------------------------------------------------------------- iter_*_bbox

/// Returns global bounds through [`doc_extents`] and the pooled bounding-box column.
/// Returns `None` for a missing or empty container.
#[must_use]
pub fn extents_bbox_union(doc: &Document, container: ContainerRef) -> Option<BBox> {
    doc_extents(doc, container, ExtentsFilter::All)
}

/// Returns global bounds by materializing records and recomputing geometry bounds.
#[must_use]
pub fn iter_bbox_union(doc: &Document, container: ContainerRef) -> Option<BBox> {
    let entities = doc.container(container)?;
    let mut acc: Option<BBox> = None;
    for rec in entities.iter() {
        let bb = rec.geometry.bbox();
        acc = Some(acc.map_or(bb, |a| a.union(bb)));
    }
    acc
}

// ------------------------------------------------------------- hit_test_10k

/// Generates `n` deterministic regular-grid points over `[-half, half]^2`.
#[must_use]
pub fn grid_points(n: usize, half: f64) -> Vec<Point2> {
    if n == 0 {
        return Vec::new();
    }
    let side = (n as f64).sqrt().ceil().max(1.0) as usize;
    let step = if side > 1 {
        (2.0 * half) / (side - 1) as f64
    } else {
        0.0
    };
    let mut pts = Vec::with_capacity(n);
    'outer: for row in 0..side {
        for col in 0..side {
            if pts.len() == n {
                break 'outer;
            }
            pts.push(Point2::new(
                -half + step * col as f64,
                -half + step * row as f64,
            ));
        }
    }
    pts
}

/// Builds a spatial index and returns the total hits across all query points.
#[must_use]
pub fn hit_test(doc: &Document, container: ContainerRef, points: &[Point2], tol: f64) -> usize {
    let index = SpatialIndex::build(doc, container);
    points
        .iter()
        .map(|&p| pick(doc, &index, p, tol).len())
        .sum()
}

// ------------------------------------------------------------- IDs to move

/// Returns up to `n` IDs distributed uniformly through draw order.
#[must_use]
pub fn evenly_spaced_ids(doc: &Document, container: ContainerRef, n: usize) -> Vec<EntityId> {
    let Some(c) = doc.container(container) else {
        return Vec::new();
    };
    let len = c.len();
    if len == 0 || n == 0 {
        return Vec::new();
    }
    let n = n.min(len);
    let ids: Vec<EntityId> = c.iter_records().map(|r| r.id).collect();
    (0..n).map(|i| ids[i * len / n]).collect()
}

// ------------------------------------------------------------- tx_commit / undo_simple

/// Fixed deterministic translation used by transaction workloads.
pub const MOVE_DELTA: Vec2 = Vec2::new(10.0, -5.0);

/// Moves `ids` by `delta` in one committed transaction.
pub fn commit_move(session: &mut Session, ids: &[EntityId], delta: Vec2) {
    let t = Transform2::translate(delta);
    session
        .transact("bench-move", |tx| -> Result<(), TxError> {
            for &id in ids {
                tx.modify_entity(id, |rec| {
                    rec.geometry = rec
                        .geometry
                        .transform(&t)
                        .expect("la traslación pura es representable por cualquier geometría");
                })?;
            }
            Ok(())
        })
        .expect("commit_move: tx_commit no debería fallar");
}

/// Commits a move and immediately undoes the same transaction.
pub fn commit_and_undo(session: &mut Session, ids: &[EntityId], delta: Vec2) {
    commit_move(session, ids, delta);
    session.undo().expect("commit_and_undo: nada que deshacer");
}

// ------------------------------------------------------------- save_10k / load_10k

/// Serializes `doc` to `.arcf` at `path`.
pub fn save_roundtrip(doc: &Document, path: &Path) {
    af_io_native::save(doc, path).expect("save_roundtrip: save");
}

/// Loads and returns the `.arcf` document at `path`.
pub fn load_roundtrip(path: &Path) -> Document {
    af_io_native::load(path).expect("load_roundtrip: load").0
}

// ------------------------------------------------------------- dxf_export_10k

/// Exports `doc` to in-memory DXF R2000 ASCII and returns its byte length.
#[must_use]
pub fn dxf_export(doc: &Document) -> usize {
    let mut buf = Vec::new();
    af_io_dxf::export_dxf(doc, &mut buf, af_io_dxf::ExportOptions::default())
        .expect("dxf_export: export_dxf");
    buf.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::synth_doc;

    /// Small shared document for one-iteration smoke checks.
    fn small_session() -> Session {
        synth_doc(200, 0x5EED)
    }

    #[test]
    fn smoke_iter_bbox_union() {
        let session = small_session();
        let bbox = iter_bbox_union(session.document(), ContainerRef::ModelSpace)
            .expect("200 entidades deberían dar un bbox");
        assert!(bbox.width().is_finite() && bbox.height().is_finite());
    }

    #[test]
    fn extents_column_equivale_a_materializado() {
        // Column and materialized paths must produce exactly equal global bounds.
        let session = small_session();
        let by_column = extents_bbox_union(session.document(), ContainerRef::ModelSpace);
        let by_materialized = iter_bbox_union(session.document(), ContainerRef::ModelSpace);
        assert_eq!(by_column, by_materialized);
        assert!(by_column.is_some());
    }

    #[test]
    fn smoke_grid_points_produce_n_puntos() {
        let pts = grid_points(100, 5_000.0);
        assert_eq!(pts.len(), 100);
        let pts9 = grid_points(9, 1.0);
        assert_eq!(pts9.len(), 9);
    }

    #[test]
    fn smoke_hit_test() {
        let session = small_session();
        let pts = grid_points(100, crate::r#gen::SYNTH_AREA_SIDE / 2.0);
        // Hit count depends on the seed; this only verifies the workload completes.
        let _hits = hit_test(session.document(), ContainerRef::ModelSpace, &pts, 0.5);
    }

    #[test]
    fn smoke_evenly_spaced_ids_son_unicos_y_existen() {
        let session = small_session();
        let ids = evenly_spaced_ids(session.document(), ContainerRef::ModelSpace, 50);
        assert_eq!(ids.len(), 50);
        for &id in &ids {
            assert!(session.document().entity(id).is_some());
        }
        let mut sorted = ids.clone();
        sorted.sort_by_key(|id| id.raw().0);
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            ids.len(),
            "evenly_spaced_ids no debería repetir ids"
        );
    }

    #[test]
    fn smoke_tx_commit() {
        let mut session = small_session();
        let ids = evenly_spaced_ids(session.document(), ContainerRef::ModelSpace, 20);
        let before: Vec<_> = ids
            .iter()
            .map(|&id| session.document().entity(id).unwrap().0.geometry.bbox())
            .collect();
        commit_move(&mut session, &ids, MOVE_DELTA);
        for (i, &id) in ids.iter().enumerate() {
            let after_bbox = session.document().entity(id).unwrap().0.geometry.bbox();
            assert_ne!(after_bbox, before[i], "la entidad debería haberse movido");
        }
        // Assert the latest transaction is the benchmark move, not generator history.
        assert_eq!(session.undo_label(), Some("bench-move"));
    }

    #[test]
    fn smoke_undo_simple_restaura_el_estado_previo() {
        let mut session = small_session();
        let ids = evenly_spaced_ids(session.document(), ContainerRef::ModelSpace, 20);
        let before: Vec<_> = ids
            .iter()
            .map(|&id| session.document().entity(id).unwrap().0.clone())
            .collect();
        let label_before_move = session.undo_label().map(str::to_string);

        commit_and_undo(&mut session, &ids, MOVE_DELTA);

        for (i, &id) in ids.iter().enumerate() {
            let after = session.document().entity(id).unwrap().0.clone();
            assert_eq!(
                after, before[i],
                "undo_simple debería restaurar el registro"
            );
        }
        // Undo removes only `bench-move` and moves it to the redo stack.
        assert_eq!(session.undo_label().map(str::to_string), label_before_move);
        assert_eq!(session.redo_label(), Some("bench-move"));
    }

    #[test]
    fn smoke_save_load_roundtrip() {
        let session = small_session();
        let dir = std::env::temp_dir().join(format!("af-bench-smoke-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let path = dir.join("smoke.arcf");
        save_roundtrip(session.document(), &path);
        let loaded = load_roundtrip(&path);
        assert_eq!(
            loaded.container(ContainerRef::ModelSpace).map(|c| c.len()),
            session
                .document()
                .container(ContainerRef::ModelSpace)
                .map(|c| c.len()),
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn smoke_dxf_export() {
        let session = small_session();
        let len = dxf_export(session.document());
        assert!(len > 0, "el export DXF debería producir bytes");
    }
}
