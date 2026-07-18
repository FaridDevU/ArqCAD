//! `pick_10k` and `bulk_build_100k` benchmarks.
//!
//! Fixtures use an inline fixed-seed LCG for deterministic runs without `rand`.

use af_math::Point2;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
use af_model::units::Units;
use af_model::{ContainerRef, Document, Session, TxError};
use af_select::{SpatialIndex, pick};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

/// Deterministic inline LCG using Knuth/PCG constants.
struct Lcg(u64);
impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    /// Uniform `f64` in `[lo, hi)`.
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + u * (hi - lo)
    }
}

/// Builds a document with `n` short lines scattered across a large area.
fn doc_with_lines(n: usize, seed: u64) -> Document {
    let mut session = Session::new(Units::default());
    let layer = session.document().current_layer();
    let mut rng = Lcg(seed);
    session
        .transact("bench-fill", |tx| -> Result<(), TxError> {
            for _ in 0..n {
                let x = rng.range(-5_000.0, 5_000.0);
                let y = rng.range(-5_000.0, 5_000.0);
                let dx = rng.range(-2.0, 2.0);
                let dy = rng.range(-2.0, 2.0);
                let rec = EntityRecord::new(
                    af_model::id::ObjectId::NIL.into(),
                    layer,
                    Color::ByLayer,
                    LineTypeRef::ByLayer,
                    Lineweight::ByLayer,
                    EntityGeometry::Line(LineGeo::new(
                        Point2::new(x, y),
                        Point2::new(x + dx, y + dy),
                    )),
                );
                tx.add_entity(ContainerRef::ModelSpace, rec)?;
            }
            Ok(())
        })
        .expect("fill tx");
    session.document().clone()
}

fn bench_pick_10k(c: &mut Criterion) {
    let doc = doc_with_lines(10_000, 0x00A1_1CE5);
    let index = SpatialIndex::build(&doc, ContainerRef::ModelSpace);
    // Sweep query points across the area to include both misses and hits.
    let mut rng = Lcg(0x0000_BEEF);
    let pts: Vec<Point2> = (0..256)
        .map(|_| Point2::new(rng.range(-5_000.0, 5_000.0), rng.range(-5_000.0, 5_000.0)))
        .collect();
    let mut i = 0usize;
    c.bench_function("pick_10k", |b| {
        b.iter(|| {
            let p = pts[i % pts.len()];
            i += 1;
            let hits = pick(
                black_box(&doc),
                black_box(&index),
                black_box(p),
                black_box(0.5),
            );
            black_box(hits.len())
        });
    });
}

fn bench_bulk_build_100k(c: &mut Criterion) {
    let doc = doc_with_lines(100_000, 0x0010_0000);
    c.bench_function("bulk_build_100k", |b| {
        b.iter(|| {
            let index = SpatialIndex::build(black_box(&doc), ContainerRef::ModelSpace);
            black_box(index.len())
        });
    });
}

criterion_group!(benches, bench_pick_10k, bench_bulk_build_100k);
criterion_main!(benches);
