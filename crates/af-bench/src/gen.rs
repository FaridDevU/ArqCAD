//! Deterministic synthetic-document generator for performance baselines.
//!
//! [`synth_doc`] creates a [`Session`] with a stable mix of 40% lines, 25%
//! circles, 20% bulged polylines, and 15% points across a centered
//! `10000 × 10000` area. One transaction per entity type avoids measuring
//! thousands of trivial transaction commits.
//!
//! Equal `(n, seed)` inputs produce identical entity IDs, coordinates, and draw
//! order. Determinism compares serialized model space because each new session
//! intentionally receives a fresh document UUID. [`Xorshift64`] supplies the
//! entity randomness without an extra dependency.

use af_math::Point2;
use af_model::entity::{
    CircleGeo, Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
    PolyVertex, PolylineGeo,
};
use af_model::id::ObjectId;
use af_model::units::Units;
use af_model::{ContainerRef, Session, TxError};

/// Side length of the centered square containing synthetic entities.
pub const SYNTH_AREA_SIDE: f64 = 10_000.0;

const HALF_AREA: f64 = SYNTH_AREA_SIDE / 2.0;

/// Deterministic xorshift64 PRNG with a fixed seed.
///
/// Zero is a fixed point, so [`Xorshift64::new`] replaces it with a nonzero constant.
pub struct Xorshift64(u64);

impl Xorshift64 {
    /// Creates a generator, remapping a zero seed to a nonzero value.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    /// Returns the next pseudorandom `u64` using xorshift64 shifts 13/7/17.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Returns a uniform `f64` in `[lo, hi)` from the upper 53 bits.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + u * (hi - lo)
    }

    /// Returns a uniform integer in the inclusive range `[lo, hi]`.
    pub fn range_i(&mut self, lo: usize, hi: usize) -> usize {
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as usize
    }
}

/// Returns a uniform random point in `[-HALF_AREA, HALF_AREA]^2`.
fn random_point(rng: &mut Xorshift64) -> Point2 {
    Point2::new(
        rng.range(-HALF_AREA, HALF_AREA),
        rng.range(-HALF_AREA, HALF_AREA),
    )
}

fn new_record(layer: af_model::id::LayerId, geometry: EntityGeometry) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        geometry,
    )
}

/// Generates `n` deterministic entities with the module's stable 40/25/20/15 mix
/// in four type-batched transactions.
///
/// Equal `(n, seed)` inputs produce equal IDs, coordinates, and draw order.
#[must_use]
pub fn synth_doc(n: usize, seed: u64) -> Session {
    let mut session = Session::new(Units::default());
    let layer = session.document().current_layer();
    let mut rng = Xorshift64::new(seed);

    let n_lines = n * 40 / 100;
    let n_circles = n * 25 / 100;
    let n_polylines = n * 20 / 100;
    // Assign rounding remainders to points so the total remains exactly `n`.
    let n_points = n - n_lines - n_circles - n_polylines;

    session
        .transact("synth-lines", |tx| -> Result<(), TxError> {
            for _ in 0..n_lines {
                let p1 = random_point(&mut rng);
                let p2 = random_point(&mut rng);
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    new_record(layer, EntityGeometry::Line(LineGeo::new(p1, p2))),
                )?;
            }
            Ok(())
        })
        .expect("synth_doc: batch de líneas");

    session
        .transact("synth-circles", |tx| -> Result<(), TxError> {
            for _ in 0..n_circles {
                let center = random_point(&mut rng);
                let radius = rng.range(1.0, 200.0);
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    new_record(
                        layer,
                        EntityGeometry::Circle(CircleGeo::new(center, radius)),
                    ),
                )?;
            }
            Ok(())
        })
        .expect("synth_doc: batch de círculos");

    session
        .transact("synth-polylines", |tx| -> Result<(), TxError> {
            for _ in 0..n_polylines {
                let vcount = rng.range_i(2, 8);
                let mut vertices = Vec::with_capacity(vcount);
                let mut cur = random_point(&mut rng);
                for i in 0..vcount {
                    if i > 0 {
                        cur = Point2::new(
                            cur.x + rng.range(-100.0, 100.0),
                            cur.y + rng.range(-100.0, 100.0),
                        );
                    }
                    vertices.push(PolyVertex::new(cur, rng.range(-0.5, 0.5)));
                }
                let closed = rng.next_u64().is_multiple_of(2);
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    new_record(
                        layer,
                        EntityGeometry::Polyline(PolylineGeo::new(vertices, closed)),
                    ),
                )?;
            }
            Ok(())
        })
        .expect("synth_doc: batch de polilíneas");

    session
        .transact("synth-points", |tx| -> Result<(), TxError> {
            for _ in 0..n_points {
                let p = random_point(&mut rng);
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    new_record(layer, EntityGeometry::Point(PointGeo::new(p))),
                )?;
            }
            Ok(())
        })
        .expect("synth_doc: batch de puntos");

    session
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes generated model space, excluding the session-specific document UUID.
    fn model_space_json(session: &Session) -> String {
        serde_json::to_string(session.document().model_space()).expect("serialize model_space")
    }

    #[test]
    fn mismo_seed_produce_mismo_documento() {
        let a = synth_doc(500, 0x00C0_FFEE);
        let b = synth_doc(500, 0x00C0_FFEE);
        assert_eq!(
            model_space_json(&a),
            model_space_json(&b),
            "mismo (n, seed) debe producir las mismas entidades"
        );
    }

    #[test]
    fn seeds_distintas_producen_documentos_distintos() {
        let a = synth_doc(500, 1);
        let b = synth_doc(500, 2);
        let ja = model_space_json(&a);
        let jb = model_space_json(&b);
        assert_ne!(
            ja, jb,
            "seeds distintas deberían producir entidades distintas"
        );
    }

    #[test]
    fn mezcla_respeta_los_porcentajes_y_la_suma_total() {
        let session = synth_doc(1000, 42);
        let container = session
            .document()
            .container(ContainerRef::ModelSpace)
            .expect("model space");
        assert_eq!(container.len(), 1000);

        let mut lines = 0usize;
        let mut circles = 0usize;
        let mut polylines = 0usize;
        let mut points = 0usize;
        for rec in container.iter_records() {
            match &rec.geometry {
                EntityGeometry::Line(_) => lines += 1,
                EntityGeometry::Circle(_) => circles += 1,
                EntityGeometry::Polyline(_) => polylines += 1,
                EntityGeometry::Point(_) => points += 1,
                other => panic!("synth_doc no debería generar {other:?}"),
            }
        }
        assert_eq!(lines, 400);
        assert_eq!(circles, 250);
        assert_eq!(polylines, 200);
        assert_eq!(points, 150);
    }

    #[test]
    fn n_no_multiplo_de_20_no_pierde_ni_sobra_entidades() {
        // The remainder must absorb percentage rounding without changing the total.
        let session = synth_doc(1007, 7);
        let container = session
            .document()
            .container(ContainerRef::ModelSpace)
            .expect("model space");
        assert_eq!(container.len(), 1007);
    }

    #[test]
    fn ids_nunca_se_reciclan_entre_batches() {
        let session = synth_doc(37, 9);
        let container = session
            .document()
            .container(ContainerRef::ModelSpace)
            .expect("model space");
        let mut ids: Vec<u64> = container.iter_records().map(|r| r.id.raw().0).collect();
        let original_len = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            original_len,
            "ids duplicados en el documento sintético"
        );
    }

    #[test]
    fn xorshift64_es_determinista_y_no_degenera_en_cero() {
        let mut a = Xorshift64::new(1234);
        let mut b = Xorshift64::new(1234);
        for _ in 0..1000 {
            let (na, nb) = (a.next_u64(), b.next_u64());
            assert_eq!(na, nb);
            assert_ne!(na, 0, "xorshift64 no debería degenerar a 0 con esta seed");
        }
    }

    #[test]
    fn xorshift64_seed_cero_no_degenera() {
        let mut rng = Xorshift64::new(0);
        assert_ne!(rng.next_u64(), 0);
    }
}
