//! Exact snapping scenarios with hand-calculated distances.
//!
//! Tests use `px_per_unit = 1.0`, making `score = distance − bonus` directly
//! readable in world units. Ties use kind priority, entity ID, then coordinates.

mod common;

use af_math::Point2;
use af_model::entity::{Color, Lineweight, SnapKind};
use af_model::id::{LayerId, ObjectId};
use af_model::{ContainerRef, Layer, Session, TxError};
use af_select::{SnapMask, SnapOpts, SpatialIndex, snap};
use common::{add, circle_rec, line_rec, point_rec, session};

/// Builds the session's model-space index.
fn index(s: &Session) -> SpatialIndex {
    SpatialIndex::build(s.document(), ContainerRef::ModelSpace)
}

/// Creates a layer, applies `modifier`, and returns its ID.
fn add_layer(session: &mut Session, name: &str, modifier: impl FnOnce(Layer) -> Layer) -> LayerId {
    let continuous = session.document().line_types().next().unwrap().id();
    let base = Layer::new(
        ObjectId::NIL.into(),
        name,
        Color::aci(1).unwrap(),
        continuous,
        Lineweight::ByLayer,
    );
    session
        .transact("mklayer", |tx| -> Result<_, TxError> {
            tx.add_layer_raw(modifier(base))
        })
        .expect("mklayer")
        .value
}

// ---------------------------------------------------------------------------
// Priority and distance
// ---------------------------------------------------------------------------

/// At equal distance, kind priority makes Endpoint beat Node.
#[test]
fn gana_prioridad_a_igual_distancia() {
    let mut s = session();
    let layer = s.document().current_layer();
    // Endpoint `(1,0)` is one unit from cursor `(0,0)`.
    add(
        &mut s,
        line_rec(layer, Point2::new(1.0, 0.0), Point2::new(3.0, 0.0)),
    );
    // Node `(0,1)` is one unit away.
    add(&mut s, point_rec(layer, Point2::new(0.0, 1.0)));

    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(0.0, 0.0),
        5.0,
        SnapOpts::default(),
    );

    assert_eq!(hits[0].kind, SnapKind::Endpoint);
    assert_eq!(hits[0].point, Point2::new(1.0, 0.0));
    assert!((hits[0].dist - 1.0).abs() < 1e-12);
    // Endpoint score -2 beats node score -1.
    assert!(hits.iter().any(|h| h.kind == SnapKind::Node));
}

/// Within one kind, the closer node wins.
#[test]
fn mismo_kind_gana_distancia() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(&mut s, point_rec(layer, Point2::new(2.0, 0.0))); // dist 2
    add(&mut s, point_rec(layer, Point2::new(4.0, 0.0))); // dist 4

    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(0.0, 0.0),
        5.0,
        SnapOpts::default(),
    );

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].point, Point2::new(2.0, 0.0));
    assert_eq!(hits[1].point, Point2::new(4.0, 0.0));
}

/// A nearby center beats a farther endpoint when its score is lower.
#[test]
fn bonus_no_roba_snap_de_lejos() {
    let mut s = session();
    let layer = s.document().current_layer();
    // A large radius keeps circle quadrants outside the aperture.
    add(&mut s, circle_rec(layer, Point2::new(0.5, 0.0), 10.0));
    // Endpoint `(2,0)` has distance 2.
    add(
        &mut s,
        line_rec(layer, Point2::new(2.0, 0.0), Point2::new(4.0, 0.0)),
    );

    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(0.0, 0.0),
        5.0,
        SnapOpts::default(),
    );

    assert_eq!(hits[0].kind, SnapKind::Center);
    assert_eq!(hits[0].point, Point2::new(0.5, 0.0));
}

/// Kind bonus can make a slightly farther endpoint beat a nearby center.
#[test]
fn bonus_voltea_empate_cercano() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(&mut s, circle_rec(layer, Point2::new(1.0, 0.0), 10.0)); // center d1
    add(
        &mut s,
        line_rec(layer, Point2::new(1.5, 0.0), Point2::new(3.0, 0.0)),
    ); // endpoint d1.5

    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(0.0, 0.0),
        5.0,
        SnapOpts::default(),
    );

    assert_eq!(hits[0].kind, SnapKind::Endpoint);
    assert_eq!(hits[0].point, Point2::new(1.5, 0.0));
}

/// Exact ties use ascending entity ID.
#[test]
fn empate_exacto_desempata_por_id() {
    let mut s = session();
    let layer = s.document().current_layer();
    let a = add(&mut s, point_rec(layer, Point2::new(1.0, 0.0))); // node d1
    let b = add(&mut s, point_rec(layer, Point2::new(0.0, 1.0))); // node d1

    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(0.0, 0.0),
        5.0,
        SnapOpts::default(),
    );

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].entity, a, "id ascendente: la primera añadida gana");
    assert_eq!(hits[1].entity, b);
}

// ---------------------------------------------------------------------------
// Aperture
// ---------------------------------------------------------------------------

/// Circular aperture rejects a candidate inside its AABB but outside the radius.
#[test]
fn fuera_de_apertura_circular_none() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(&mut s, point_rec(layer, Point2::new(4.0, 4.0)));
    let idx = index(&s);
    let cursor = Point2::new(0.0, 0.0);

    assert!(
        snap(s.document(), &idx, cursor, 5.0, SnapOpts::default()).is_empty(),
        "√32 > 5 ⇒ fuera de la apertura circular"
    );
    let hits = snap(s.document(), &idx, cursor, 6.0, SnapOpts::default());
    assert_eq!(hits.len(), 1);
    assert!((hits[0].dist - (32.0f64).sqrt()).abs() < 1e-12);
}

/// No nearby candidates produces an empty result.
#[test]
fn sin_candidatos_none() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(
        &mut s,
        line_rec(layer, Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
    );
    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(1000.0, 1000.0),
        5.0,
        SnapOpts::default(),
    );
    assert!(hits.is_empty());
}

// ---------------------------------------------------------------------------
// Kind mask
// ---------------------------------------------------------------------------

/// Disabled kinds are excluded; enabling one leaves only that kind.
#[test]
fn mascara_activa_desactiva_clases() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(&mut s, point_rec(layer, Point2::new(1.0, 0.0))); // node d1
    // A large circle radius keeps quadrants outside the aperture.
    add(&mut s, circle_rec(layer, Point2::new(0.0, 2.0), 10.0)); // center d2
    let idx = index(&s);
    let cursor = Point2::new(0.0, 0.0);

    // With all kinds, node score -1 beats center score 0.
    let all = snap(s.document(), &idx, cursor, 5.0, SnapOpts::default());
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kind, SnapKind::Node);

    // Disabling Node leaves only Center.
    let no_node = SnapOpts {
        kinds: SnapMask::ALL.without(SnapKind::Node),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &idx, cursor, 5.0, no_node);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].kind, SnapKind::Center);

    // Node only.
    let only_node = SnapOpts {
        kinds: SnapMask::NONE.with(SnapKind::Node),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &idx, cursor, 5.0, only_node);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].kind, SnapKind::Node);
}

// ---------------------------------------------------------------------------
// Layer filters
// ---------------------------------------------------------------------------

/// Off and frozen layers do not snap; locked layers do.
#[test]
fn capas_off_frozen_fuera_locked_snappea() {
    let mut s = session();
    let locked = add_layer(&mut s, "locked", |l| l.with_locked(true));
    let off = add_layer(&mut s, "off", |l| l.with_off(true));
    let frozen = add_layer(&mut s, "frozen", |l| l.with_frozen(true));
    let normal = s.document().current_layer();

    let n = add(&mut s, point_rec(normal, Point2::new(1.0, 0.0)));
    let lk = add(&mut s, point_rec(locked, Point2::new(0.0, 1.0)));
    let _off = add(&mut s, point_rec(off, Point2::new(2.0, 0.0)));
    let _fr = add(&mut s, point_rec(frozen, Point2::new(0.0, 2.0)));

    let hits = snap(
        s.document(),
        &index(&s),
        Point2::new(0.0, 0.0),
        5.0,
        SnapOpts::default(),
    );

    let ids: Vec<_> = hits.iter().map(|h| h.entity).collect();
    assert_eq!(hits.len(), 2, "solo normal + locked: {hits:?}");
    assert!(ids.contains(&n));
    assert!(ids.contains(&lk));
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// Identical queries produce identical results.
#[test]
fn determinismo_repetible() {
    let mut s = session();
    let layer = s.document().current_layer();
    add(
        &mut s,
        line_rec(layer, Point2::new(1.0, 0.0), Point2::new(3.0, 2.0)),
    );
    add(&mut s, circle_rec(layer, Point2::new(0.0, 1.0), 4.0));
    add(&mut s, point_rec(layer, Point2::new(2.0, 2.0)));
    let idx = index(&s);
    let cursor = Point2::new(0.5, 0.5);

    let a = snap(s.document(), &idx, cursor, 6.0, SnapOpts::default());
    let b = snap(s.document(), &idx, cursor, 6.0, SnapOpts::default());
    assert_eq!(a, b);
}

/// Ranking with distinct scores is independent of insertion and candidate order.
#[test]
fn determinismo_independiente_del_orden() {
    let cursor = Point2::new(0.0, 0.0);
    let seq = |s: &Session| -> Vec<(SnapKind, Point2)> {
        snap(s.document(), &index(s), cursor, 5.0, SnapOpts::default())
            .into_iter()
            .map(|h| (h.kind, h.point))
            .collect()
    };

    // Node score -1 and center score 1 require no tie-break.
    let mut s1 = session();
    let l1 = s1.document().current_layer();
    add(&mut s1, point_rec(l1, Point2::new(1.0, 0.0)));
    add(&mut s1, circle_rec(l1, Point2::new(0.0, 3.0), 10.0));

    let mut s2 = session();
    let l2 = s2.document().current_layer();
    add(&mut s2, circle_rec(l2, Point2::new(0.0, 3.0), 10.0));
    add(&mut s2, point_rec(l2, Point2::new(1.0, 0.0)));

    assert_eq!(
        seq(&s1),
        vec![
            (SnapKind::Node, Point2::new(1.0, 0.0)),
            (SnapKind::Center, Point2::new(0.0, 3.0)),
        ]
    );
    assert_eq!(seq(&s1), seq(&s2));
}
