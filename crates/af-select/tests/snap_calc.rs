//! Calculated snap scenarios: Intersection, Perpendicular,
//! Nearest, Tangent, Extension, and GeometricCenter.
//!
//! Each test isolates one kind with a minimal mask; the last uses a mixed mask to
//! verify deterministic ranking.

mod common;

use core::f64::consts::PI;

use af_math::Point2;
use af_model::entity::SnapKind;
use af_model::{ContainerRef, Session};
use af_select::{SnapMask, SnapOpts, SpatialIndex, snap};
use common::{add, arc_rec, circle_rec, line_rec, polyline_rec, session};

fn index(s: &Session) -> SpatialIndex {
    SpatialIndex::build(s.document(), ContainerRef::ModelSpace)
}

fn only(kind: SnapKind) -> SnapMask {
    SnapMask::NONE.with(kind)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}
fn close_pt(p: Point2, q: Point2) -> bool {
    close(p.x, q.x) && close(p.y, q.y)
}

// --------------------------------------------------------------------------
// Intersection
// --------------------------------------------------------------------------

/// Two intersecting lines produce exact crossing `(1,1)`.
#[test]
fn intersection_dos_lineas_secantes() {
    let mut s = session();
    let l = s.document().current_layer();
    add(
        &mut s,
        line_rec(l, Point2::new(0.0, 0.0), Point2::new(2.0, 2.0)),
    );
    add(
        &mut s,
        line_rec(l, Point2::new(0.0, 2.0), Point2::new(2.0, 0.0)),
    );

    let opts = SnapOpts {
        kinds: only(SnapKind::Intersection),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &index(&s), Point2::new(1.0, 1.0), 0.5, opts);

    assert_eq!(hits.len(), 1, "un único cruce: {hits:?}");
    assert_eq!(hits[0].kind, SnapKind::Intersection);
    assert!(close_pt(hits[0].point, Point2::new(1.0, 1.0)));
}

/// Supporting lines crossing outside both segments produce no intersection snap.
#[test]
fn intersection_solo_sobre_los_tramos() {
    let mut s = session();
    let l = s.document().current_layer();
    // Short disjoint segments whose supporting lines cross at (2,0).
    add(
        &mut s,
        line_rec(l, Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
    );
    add(
        &mut s,
        line_rec(l, Point2::new(2.0, 1.0), Point2::new(2.0, 2.0)),
    );

    let opts = SnapOpts {
        kinds: only(SnapKind::Intersection),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &index(&s), Point2::new(2.0, 0.0), 3.0, opts);
    assert!(
        hits.is_empty(),
        "el cruce está en la prolongación: {hits:?}"
    );
}

// --------------------------------------------------------------------------
// Perpendicular from last_point
// --------------------------------------------------------------------------

/// Perpendicular foot from `last_point` has an approximately zero dot product.
#[test]
fn perpendicular_cae_en_el_pie() {
    let mut s = session();
    let l = s.document().current_layer();
    let a = Point2::new(0.0, 0.0);
    let b = Point2::new(4.0, 0.0);
    add(&mut s, line_rec(l, a, b));

    let lp = Point2::new(1.0, 3.0);
    let opts = SnapOpts {
        kinds: only(SnapKind::Perpendicular),
        last_point: Some(lp),
        ..SnapOpts::default()
    };
    // The foot is `(1,0)` and the cursor lies nearby.
    let hits = snap(s.document(), &index(&s), Point2::new(1.0, 0.2), 1.0, opts);

    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].kind, SnapKind::Perpendicular);
    assert!(close_pt(hits[0].point, Point2::new(1.0, 0.0)));
    // `(lp → foot)` is perpendicular to the line direction.
    assert!(close((lp - hits[0].point).dot(b - a), 0.0));
}

/// Without `last_point`, no perpendicular snap is produced.
#[test]
fn perpendicular_requiere_last_point() {
    let mut s = session();
    let l = s.document().current_layer();
    add(
        &mut s,
        line_rec(l, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
    );
    let opts = SnapOpts {
        kinds: only(SnapKind::Perpendicular),
        last_point: None,
        ..SnapOpts::default()
    };
    assert!(snap(s.document(), &index(&s), Point2::new(1.0, 0.2), 1.0, opts).is_empty());
}

// --------------------------------------------------------------------------
// Nearest
// --------------------------------------------------------------------------

/// The nearest point lies on line `y=0`.
#[test]
fn nearest_yace_sobre_la_curva() {
    let mut s = session();
    let l = s.document().current_layer();
    add(
        &mut s,
        line_rec(l, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
    );

    let opts = SnapOpts {
        kinds: only(SnapKind::Nearest),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &index(&s), Point2::new(2.0, 3.0), 5.0, opts);

    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].kind, SnapKind::Nearest);
    assert!(close_pt(hits[0].point, Point2::new(2.0, 0.0)));
    assert!(close(hits[0].point.y, 0.0), "sobre la recta y=0");
}

/// A circle's nearest point lies one radius from its center.
#[test]
fn nearest_sobre_circulo() {
    let mut s = session();
    let l = s.document().current_layer();
    let c = Point2::new(0.0, 0.0);
    add(&mut s, circle_rec(l, c, 1.0));

    let opts = SnapOpts {
        kinds: only(SnapKind::Nearest),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &index(&s), Point2::new(3.0, 0.0), 5.0, opts);
    assert_eq!(hits.len(), 1);
    assert!(close_pt(hits[0].point, Point2::new(1.0, 0.0)));
    assert!(close(hits[0].point.dist(c), 1.0));
}

// --------------------------------------------------------------------------
// Tangent from last_point
// --------------------------------------------------------------------------

/// At a circle tangent point, the radius is perpendicular to the tangent line.
#[test]
fn tangent_verificada() {
    let mut s = session();
    let l = s.document().current_layer();
    let c = Point2::new(0.0, 0.0);
    add(&mut s, circle_rec(l, c, 1.0));

    let lp = Point2::new(2.0, 0.0);
    let s3 = 3.0f64.sqrt() / 2.0; // Upper tangent point `(0.5, √3/2)`.
    let opts = SnapOpts {
        kinds: only(SnapKind::Tangent),
        last_point: Some(lp),
        ..SnapOpts::default()
    };
    // Cursor near the upper tangent point.
    let hits = snap(s.document(), &index(&s), Point2::new(0.5, s3), 0.5, opts);

    assert!(!hits.is_empty(), "esperaba tangente");
    let t = hits[0].point;
    assert_eq!(hits[0].kind, SnapKind::Tangent);
    assert!(close_pt(t, Point2::new(0.5, s3)));
    // `(t − c)` is perpendicular to `(t − lp)`.
    assert!(close((t - c).dot(t - lp), 0.0));
    assert!(close(t.dist(c), 1.0));
}

// --------------------------------------------------------------------------
// Extension
// --------------------------------------------------------------------------

/// A line extension is collinear with its segment and aligned with the cursor.
#[test]
fn extension_colineal() {
    let mut s = session();
    let l = s.document().current_layer();
    let a = Point2::new(0.0, 0.0);
    let b = Point2::new(2.0, 0.0);
    add(&mut s, line_rec(l, a, b));

    let opts = SnapOpts {
        kinds: only(SnapKind::Extension),
        ..SnapOpts::default()
    };
    // Cursor just beyond endpoint `b` within the aperture.
    let hits = snap(s.document(), &index(&s), Point2::new(2.5, 0.2), 1.0, opts);

    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].kind, SnapKind::Extension);
    let p = hits[0].point;
    assert!(close_pt(p, Point2::new(2.5, 0.0)));
    // Collinearity requires `(p − a) × (b − a) ≈ 0`.
    assert!(close((p - a).cross(b - a), 0.0));
}

/// A foot inside the segment does not produce an extension snap.
#[test]
fn extension_no_dentro_del_segmento() {
    let mut s = session();
    let l = s.document().current_layer();
    add(
        &mut s,
        line_rec(l, Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)),
    );
    let opts = SnapOpts {
        kinds: only(SnapKind::Extension),
        ..SnapOpts::default()
    };
    // Cursor over the segment interior has `t≈0.5`, so no extension.
    assert!(snap(s.document(), &index(&s), Point2::new(2.0, 0.2), 1.0, opts).is_empty());
}

// --------------------------------------------------------------------------
// GeometricCenter
// --------------------------------------------------------------------------

/// A closed unit-square polyline has centroid `(0.5, 0.5)`.
#[test]
fn geometric_center_de_polilinea_cerrada() {
    let mut s = session();
    let l = s.document().current_layer();
    let sq = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(1.0, 1.0),
        Point2::new(0.0, 1.0),
    ];
    add(&mut s, polyline_rec(l, &sq, true));

    let opts = SnapOpts {
        kinds: only(SnapKind::GeometricCenter),
        ..SnapOpts::default()
    };
    let hits = snap(s.document(), &index(&s), Point2::new(0.5, 0.5), 1.0, opts);

    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].kind, SnapKind::GeometricCenter);
    assert!(close_pt(hits[0].point, Point2::new(0.5, 0.5)));
}

/// An open polyline produces no geometric-center snap.
#[test]
fn geometric_center_solo_cerradas() {
    let mut s = session();
    let l = s.document().current_layer();
    let pts = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(1.0, 1.0),
    ];
    add(&mut s, polyline_rec(l, &pts, false));
    let opts = SnapOpts {
        kinds: only(SnapKind::GeometricCenter),
        ..SnapOpts::default()
    };
    assert!(snap(s.document(), &index(&s), Point2::new(0.5, 0.4), 2.0, opts).is_empty());
}

// --------------------------------------------------------------------------
// Determinism with a mixed mask
// --------------------------------------------------------------------------

/// Identical all-kind queries with `last_point` produce identical ranking.
#[test]
fn ranking_determinista_mascara_mixta() {
    let mut s = session();
    let l = s.document().current_layer();
    add(
        &mut s,
        line_rec(l, Point2::new(1.0, 0.0), Point2::new(3.0, 0.0)),
    );
    add(&mut s, circle_rec(l, Point2::new(0.0, 2.0), 1.0));
    add(&mut s, arc_rec(l, Point2::new(-1.0, -1.0), 1.5, 0.0, PI));
    let sq = [
        Point2::new(-2.0, -2.0),
        Point2::new(-1.5, -2.0),
        Point2::new(-1.5, -1.5),
        Point2::new(-2.0, -1.5),
    ];
    add(&mut s, polyline_rec(l, &sq, true));

    let idx = index(&s);
    let cursor = Point2::new(0.3, 0.3);
    let opts = SnapOpts {
        kinds: SnapMask::ALL,
        last_point: Some(Point2::new(2.0, 2.0)),
        ..SnapOpts::default()
    };

    let a = snap(s.document(), &idx, cursor, 6.0, opts);
    let b = snap(s.document(), &idx, cursor, 6.0, opts);
    assert_eq!(a, b, "misma consulta, mismo resultado");
    // A mixed mask includes calculated snaps such as Nearest.
    assert!(a.iter().any(|h| h.kind == SnapKind::Nearest));
}
