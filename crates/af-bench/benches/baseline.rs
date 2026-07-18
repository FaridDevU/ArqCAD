//! Performance baseline for the current engine, measured before the SoA migration.
//!
//! Benchmarks delegate to `af_bench::workloads`; this file only creates
//! deterministic fixtures and registers them. Small samples and short timing
//! windows keep the suite practical rather than statistically exhaustive.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;

use af_bench::r#gen::{SYNTH_AREA_SIDE, synth_doc};
use af_bench::workloads::{
    MOVE_DELTA, commit_and_undo, commit_move, dxf_export, evenly_spaced_ids, extents_bbox_union,
    grid_points, hit_test, iter_bbox_union, load_roundtrip, save_roundtrip,
};
use af_model::{ContainerRef, Session};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

/// Fixed seed for reproducible benchmarks across runs and machines.
const SEED: u64 = 0x0026_0000;

fn doc_10k() -> Session {
    synth_doc(10_000, SEED)
}

fn doc_100k() -> Session {
    synth_doc(100_000, SEED)
}

/// Cargo-provided scratch directory for `.arcf` benchmark fixtures.
fn bench_tmp_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).expect("bench_tmp_dir: create_dir_all");
    dir
}

fn bench_iter_10k_bbox(c: &mut Criterion) {
    let session = doc_10k();
    // Fast public path: aggregate the bounding-box column.
    c.bench_function("iter_10k_bbox", |b| {
        b.iter(|| {
            black_box(extents_bbox_union(
                black_box(session.document()),
                ContainerRef::ModelSpace,
            ))
        });
    });
}

fn bench_iter_100k_bbox(c: &mut Criterion) {
    let session = doc_100k();
    // Measure 100k global bounds through the fast column path.
    c.bench_function("iter_100k_bbox", |b| {
        b.iter(|| {
            black_box(extents_bbox_union(
                black_box(session.document()),
                ContainerRef::ModelSpace,
            ))
        });
    });
}

/// Comparison variant that materializes records and recomputes every bound.
fn bench_iter_100k_bbox_materializado(c: &mut Criterion) {
    let session = doc_100k();
    c.bench_function("iter_100k_bbox_materializado", |b| {
        b.iter(|| {
            black_box(iter_bbox_union(
                black_box(session.document()),
                ContainerRef::ModelSpace,
            ))
        });
    });
}

fn bench_hit_test_10k(c: &mut Criterion) {
    let session = doc_10k();
    // Run 100 deterministic picks on a regular grid without RNG.
    let points = grid_points(100, SYNTH_AREA_SIDE / 2.0);
    c.bench_function("hit_test_10k", |b| {
        b.iter(|| {
            black_box(hit_test(
                black_box(session.document()),
                ContainerRef::ModelSpace,
                black_box(&points),
                0.5,
            ))
        });
    });
}

fn bench_tx_commit(c: &mut Criterion) {
    let base = doc_10k();
    let ids = evenly_spaced_ids(base.document(), ContainerRef::ModelSpace, 100);
    c.bench_function("tx_commit", |b| {
        b.iter_batched(
            // Use a fresh clone so samples do not accumulate transaction history.
            || Session::from_document(base.document().clone()),
            |mut session| commit_move(black_box(&mut session), &ids, MOVE_DELTA),
            BatchSize::PerIteration,
        );
    });
}

fn bench_undo_simple(c: &mut Criterion) {
    let base = doc_10k();
    let ids = evenly_spaced_ids(base.document(), ContainerRef::ModelSpace, 100);
    c.bench_function("undo_simple", |b| {
        b.iter_batched(
            || Session::from_document(base.document().clone()),
            |mut session| commit_and_undo(black_box(&mut session), &ids, MOVE_DELTA),
            BatchSize::PerIteration,
        );
    });
}

fn bench_save_10k(c: &mut Criterion) {
    let session = doc_10k();
    let path = bench_tmp_dir().join("save_10k.arcf");
    c.bench_function("save_10k", |b| {
        b.iter(|| save_roundtrip(black_box(session.document()), black_box(&path)));
    });
}

fn bench_load_10k(c: &mut Criterion) {
    let session = doc_10k();
    let path = bench_tmp_dir().join("load_10k.arcf");
    save_roundtrip(session.document(), &path); // Prepare the disk fixture outside measurement.
    c.bench_function("load_10k", |b| {
        b.iter(|| black_box(load_roundtrip(black_box(&path))));
    });
}

fn bench_dxf_export_10k(c: &mut Criterion) {
    let session = doc_10k();
    c.bench_function("dxf_export_10k", |b| {
        b.iter(|| black_box(dxf_export(black_box(session.document()))));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_millis(1500))
        .warm_up_time(Duration::from_millis(500));
    targets =
        bench_iter_10k_bbox,
        bench_iter_100k_bbox,
        bench_iter_100k_bbox_materializado,
        bench_hit_test_10k,
        bench_tx_commit,
        bench_undo_simple,
        bench_save_10k,
        bench_load_10k,
        bench_dxf_export_10k,
}
criterion_main!(benches);
