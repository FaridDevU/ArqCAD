//! Integration tests for `af_model::id`.

use af_model::id::{
    BlockId, EntityId, IdAllocator, LayerId, LayoutId, MaterialId, ObjectId, StyleId, ViewportId,
};
use std::collections::HashSet;

/// `alloc` yields 1, 2, 3 while `peek` consumes nothing.
#[test]
fn alloc_produce_secuencia_y_peek_no_consume() {
    let mut alloc = IdAllocator::new();

    assert_eq!(alloc.peek(), ObjectId(1));
    assert_eq!(alloc.peek(), ObjectId(1)); // Repeated peeks change nothing.

    assert_eq!(alloc.alloc(), Ok(ObjectId(1)));
    assert_eq!(alloc.alloc(), Ok(ObjectId(2)));
    assert_eq!(alloc.alloc(), Ok(ObjectId(3)));

    assert_eq!(alloc.peek(), ObjectId(4));
}

/// `ensure_above(10)` makes 11 next; a later lower bound cannot lower it.
#[test]
fn ensure_above_sube_pero_nunca_baja_next() {
    let mut alloc = IdAllocator::new();

    alloc.ensure_above(10).unwrap();
    assert_eq!(alloc.alloc(), Ok(ObjectId(11)));

    // A lower bound below the current cursor cannot lower it.
    alloc.ensure_above(3).unwrap();
    assert_eq!(alloc.alloc(), Ok(ObjectId(12)));
}

/// `alloc()` never returns NIL or repeats a value.
#[test]
fn alloc_nunca_devuelve_nil_ni_repite() {
    let mut alloc = IdAllocator::new();
    let mut seen = HashSet::new();
    for _ in 0..1000 {
        let id = alloc.alloc().unwrap();
        assert!(!id.is_nil());
        assert!(seen.insert(id), "id repetido: {id:?}");
    }
}

/// `ObjectId(7)` serializes as `7` and round-trips.
#[test]
fn object_id_serializa_como_numero_plano_y_hace_roundtrip() {
    let id = ObjectId(7);

    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "7");

    let back: ObjectId = serde_json::from_str(&json).unwrap();
    assert_eq!(back, id);
}

/// `EntityId(7)` serializes as `7`.
#[test]
fn entity_id_7_serializa_como_7() {
    let id = EntityId::from(ObjectId(7));

    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "7");

    let back: EntityId = serde_json::from_str(&json).unwrap();
    assert_eq!(back, id);
    assert_eq!(back.raw(), ObjectId(7));
}

/// Other typed IDs round-trip as plain numbers.
macro_rules! roundtrip_newtype_test {
    ($test_name:ident, $ty:ident) => {
        #[test]
        fn $test_name() {
            let id = $ty::from(ObjectId(42));

            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, "42");

            let back: $ty = serde_json::from_str(&json).unwrap();
            assert_eq!(back, id);
            assert_eq!(back.raw(), ObjectId(42));
        }
    };
}

roundtrip_newtype_test!(layer_id_roundtrip, LayerId);
roundtrip_newtype_test!(block_id_roundtrip, BlockId);
roundtrip_newtype_test!(style_id_roundtrip, StyleId);
roundtrip_newtype_test!(layout_id_roundtrip, LayoutId);
roundtrip_newtype_test!(viewport_id_roundtrip, ViewportId);
roundtrip_newtype_test!(material_id_roundtrip, MaterialId);

/// `IdAllocator` round-trips as `{"next": n}`.
#[test]
fn id_allocator_serializa_como_objeto_con_next() {
    let mut alloc = IdAllocator::new();
    alloc.alloc().unwrap();
    alloc.alloc().unwrap();

    let json = serde_json::to_string(&alloc).unwrap();
    assert_eq!(json, r#"{"next":3}"#);

    let back: IdAllocator = serde_json::from_str(&json).unwrap();
    assert_eq!(back.peek(), ObjectId(3));
}

/// Deserialization preserves the exact `next` cursor.
#[test]
fn id_allocator_deserializa_next_arbitrario() {
    let alloc: IdAllocator = serde_json::from_str(r#"{"next":100}"#).unwrap();
    assert_eq!(alloc.peek(), ObjectId(100));
}

#[test]
fn id_exhaustion_is_terminal_without_nil_or_wrap() {
    let mut alloc = IdAllocator::new();
    alloc.ensure_above(u64::MAX - 2).unwrap();
    assert_eq!(alloc.alloc(), Ok(ObjectId(u64::MAX - 1)));
    assert_eq!(alloc.peek(), ObjectId(u64::MAX));
    assert!(alloc.alloc().is_err());
    assert_eq!(alloc.peek(), ObjectId(u64::MAX));
    assert!(alloc.ensure_above(u64::MAX).is_err());
    assert_eq!(alloc.peek(), ObjectId(u64::MAX));
    assert!(alloc.alloc().is_err());
}

#[test]
fn id_exhaustion_deserialized_zero_never_allocates_nil() {
    let mut alloc: IdAllocator = serde_json::from_str(r#"{"next":0}"#).unwrap();
    assert_eq!(alloc.peek(), ObjectId::NIL);
    assert!(alloc.alloc().is_err());
    assert_eq!(alloc.peek(), ObjectId::NIL);
}
