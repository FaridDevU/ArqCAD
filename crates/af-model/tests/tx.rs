//! Integration tests for the public transaction API: `Session::transact`,
//! `TxContext`, and `ChangeSet`.
//!
//! Tests use only the public surface and never access document internals.

use af_math::{Point2, Transform2, Vec2};
use af_model::entity::{
    Color, EntityGeometry, EntityOps, EntityRecord, LineGeo, LineTypeRef, Lineweight, PointGeo,
};
use af_model::id::{BlockId, EntityId, LayerId, ObjectId, StyleId};
use af_model::units::Units;
use af_model::{ContainerRef, Document, Layer, Session, TxError, TxOutcome};

// --------------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------------

/// Point record with a placeholder ID replaced by `add_entity`.
fn point_rec(layer: LayerId, x: f64, y: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(Point2::new(x, y))),
    )
}

/// Line record with a placeholder ID.
fn line_rec(layer: LayerId, x1: f64, y1: f64, x2: f64, y2: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Line(LineGeo::new(Point2::new(x1, y1), Point2::new(x2, y2))),
    )
}

/// Translates line or point geometry by `(dx, dy)`.
fn translate(r: &mut EntityRecord, dx: f64, dy: f64) {
    let t = Transform2::translate(Vec2::new(dx, dy));
    r.geometry = r
        .geometry
        .transform(&t)
        .expect("una traslación siempre es válida para Line/Point");
}

/// Adds a point in its own transaction and returns its ID.
fn add_point(session: &mut Session, layer: LayerId, x: f64, y: f64) -> EntityId {
    session
        .transact("add", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, x, y))
        })
        .expect("commit")
        .value
}

/// Default document line type, "Continuous".
fn continuous(session: &Session) -> StyleId {
    session.document().line_types().next().unwrap().id()
}

/// Inert layer with a placeholder ID replaced by `add_layer_raw`.
fn layer_val(name: &str, color: Color, lt: StyleId) -> Layer {
    Layer::new(ObjectId::NIL.into(), name, color, lt, Lineweight::ByLayer)
}

/// Adds an ACI-1 layer in its own transaction and returns its ID.
fn add_layer(session: &mut Session, name: &str, lt: StyleId) -> LayerId {
    session
        .transact("add layer", |tx| -> Result<LayerId, TxError> {
            tx.add_layer_raw(layer_val(name, Color::aci(1).unwrap(), lt))
        })
        .expect("commit")
        .value
}

// --------------------------------------------------------------------------
// `add_entity` always allocates an ID and ignores the caller's placeholder.
// --------------------------------------------------------------------------

#[test]
fn add_entity_ignora_el_id_del_llamador_y_usa_el_asignador() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let expected = session.document().next_object_id();

    // Ignore the caller's deliberately invalid placeholder ID.
    let mut rec = point_rec(l0, 0.0, 0.0);
    rec.id = ObjectId(999_999).into();

    let out = session
        .transact("add", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, rec)
        })
        .unwrap();

    assert_eq!(out.value.raw().0, expected, "el id proviene del asignador");
    assert!(
        session
            .document()
            .entity(ObjectId(999_999).into())
            .is_none()
    );
    assert!(session.document().entity(out.value).is_some());
    // Commit advances the allocator exactly once.
    assert_eq!(session.document().next_object_id(), expected + 1);
}

#[test]
fn ids_consecutivos_no_se_reciclan_tras_borrado() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let a = add_point(&mut session, l0, 0.0, 0.0);
    let b = add_point(&mut session, l0, 1.0, 0.0);
    assert_eq!(b.raw().0, a.raw().0 + 1);

    // Removing `a` does not let the next entity recycle its ID.
    session
        .transact("rm", |tx| -> Result<(), TxError> { tx.remove_entity(a) })
        .unwrap();
    let c = add_point(&mut session, l0, 2.0, 0.0);
    assert_eq!(c.raw().0, b.raw().0 + 1, "id monotónico, sin reciclaje");
}

// --------------------------------------------------------------------------
// Successful add, remove, modify, and change-set behavior.
// --------------------------------------------------------------------------

#[test]
fn add_produce_changeset_con_la_entidad_en_added() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let out = session
        .transact("add", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, line_rec(l0, 0.0, 0.0, 1.0, 1.0))
        })
        .unwrap();

    let cs = out.change_set.expect("hubo ops -> changeset");
    assert_eq!(cs.added(), &[out.value]);
    assert!(cs.removed().is_empty() && cs.modified().is_empty());
    assert_eq!(cs.cause(), af_model::Cause::Do);
    assert_eq!(cs.tx_seq(), out.transaction.unwrap().seq());
}

#[test]
fn remove_produce_changeset_con_la_entidad_en_removed() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 0.0, 0.0);

    let out = session
        .transact("rm", |tx| -> Result<(), TxError> { tx.remove_entity(id) })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert_eq!(cs.removed(), &[id]);
    assert!(cs.added().is_empty() && cs.modified().is_empty());
    assert!(session.document().entity(id).is_none());
}

#[test]
fn modify_produce_changeset_con_la_entidad_en_modified() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 0.0, 0.0);

    let out = session
        .transact("mv", |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |r| translate(r, 5.0, 3.0))
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert_eq!(cs.modified(), &[id]);
    assert!(cs.added().is_empty() && cs.removed().is_empty());

    let (rec, _) = session.document().entity(id).unwrap();
    if let EntityGeometry::Point(g) = &rec.geometry {
        assert_eq!(g.position, Point2::new(5.0, 3.0));
    } else {
        panic!("esperaba punto");
    }
}

#[test]
fn las_ops_se_ven_entre_si_dentro_del_mismo_closure() {
    // Add, read, and then modify the newly created entity.
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let out = session
        .transact("add+mv", |tx| -> Result<EntityId, TxError> {
            let id = tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, 0.0, 0.0))?;
            // The next operation can already read the entity.
            assert!(tx.doc().entity(id).is_some());
            tx.modify_entity(id, |r| translate(r, 1.0, 1.0))?;
            Ok(id)
        })
        .unwrap();
    // Net result is one added entity in final state, not a modification.
    let cs = out.change_set.unwrap();
    assert_eq!(cs.added(), &[out.value]);
    assert!(cs.modified().is_empty());
    let (rec, _) = session.document().entity(out.value).unwrap();
    if let EntityGeometry::Point(g) = &rec.geometry {
        assert_eq!(g.position, Point2::new(1.0, 1.0));
    }
}

// --------------------------------------------------------------------------
// Containers other than model space.
// --------------------------------------------------------------------------

#[test]
fn add_entity_en_paper_space_de_un_layout() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let lid = session.document().layouts().next().unwrap().id();

    let out = session
        .transact("add to layout", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::Layout(lid), point_rec(l0, 3.0, 3.0))
        })
        .unwrap();
    let id = out.value;

    // The entity belongs to layout paper space, not model space.
    let (_, cref) = session.document().entity(id).unwrap();
    assert_eq!(cref, ContainerRef::Layout(lid));
    assert!(session.document().model_space().is_empty());
    assert_eq!(session.document().layout(lid).unwrap().entities().len(), 1);
}

// --------------------------------------------------------------------------
// Empty transactions produce neither a transaction nor a change set.
// --------------------------------------------------------------------------

#[test]
fn transaccion_vacia_no_produce_transaction_ni_changeset() {
    let mut session = Session::new(Units::default());
    let before = serde_json::to_string(session.document()).unwrap();

    let out = session
        .transact("nada", |_tx| -> Result<(), TxError> { Ok(()) })
        .unwrap();
    assert!(out.transaction.is_none());
    assert!(out.change_set.is_none());

    // The document remains unchanged.
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn modify_no_op_no_registra_operacion() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 0.0, 0.0);

    // A no-op edit records no operation or transaction.
    let out = session
        .transact("noop", |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |_r| {})
        })
        .unwrap();
    assert!(out.transaction.is_none());
    assert!(out.change_set.is_none());
}

// --------------------------------------------------------------------------
// Change-set deduplication.
// --------------------------------------------------------------------------

#[test]
fn creada_y_borrada_en_la_misma_tx_se_omite_del_changeset() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();

    let out = session
        .transact("add+rm", |tx| -> Result<EntityId, TxError> {
            let id = tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, 0.0, 0.0))?;
            tx.remove_entity(id)?;
            Ok(id)
        })
        .unwrap();
    let id = out.value;

    // Two reversible operations produce a transaction but no net change set.
    let tx = out.transaction.expect("2 ops -> hay transacción");
    assert_eq!(tx.len(), 2);
    let cs = out.change_set.unwrap();
    assert!(cs.added().is_empty());
    assert!(cs.removed().is_empty());
    assert!(cs.is_empty());
    // The entity is absent from the final document.
    assert!(session.document().entity(id).is_none());
}

#[test]
fn add_luego_modify_aparece_solo_en_added() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let out = session
        .transact("add+mod", |tx| -> Result<EntityId, TxError> {
            let id = tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, 0.0, 0.0))?;
            tx.modify_entity(id, |r| r.visible = false)?;
            Ok(id)
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert_eq!(cs.added(), &[out.value]);
    assert!(cs.modified().is_empty());
    assert!(cs.removed().is_empty());
}

#[test]
fn modify_multiple_aparece_una_sola_vez_en_modified() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 0.0, 0.0);

    let out = session
        .transact("mod x2", |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |r| r.visible = false)?;
            tx.modify_entity(id, |r| translate(r, 5.0, 0.0))?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert_eq!(cs.modified(), &[id], "dedup: una sola entrada");
    assert!(cs.added().is_empty() && cs.removed().is_empty());
    assert_eq!(
        out.transaction.unwrap().len(),
        2,
        "pero la tx guarda ambas ops"
    );
}

#[test]
fn modify_y_volver_al_estado_original_se_omite_del_changeset() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 0.0, 0.0); // Initially visible.

    let out = session
        .transact("mod back", |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |r| r.visible = false)?;
            tx.modify_entity(id, |r| r.visible = true)?; // Return to the original state.
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert!(cs.modified().is_empty(), "neto sin cambios -> no modified");
    assert!(cs.is_empty());
    assert_eq!(out.transaction.unwrap().len(), 2);
}

#[test]
fn modify_luego_remove_aparece_solo_en_removed() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 0.0, 0.0);

    let out = session
        .transact("mod+rm", |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |r| translate(r, 5.0, 0.0))?;
            tx.remove_entity(id)?;
            Ok(())
        })
        .unwrap();
    let cs = out.change_set.unwrap();
    assert_eq!(cs.removed(), &[id]);
    assert!(cs.added().is_empty() && cs.modified().is_empty());
}

// --------------------------------------------------------------------------
// Operations fail fast inside the closure without panicking.
// --------------------------------------------------------------------------

#[test]
fn remove_entidad_inexistente_es_error() {
    let mut session = Session::new(Units::default());
    let ghost: EntityId = ObjectId(555).into();
    let res = session.transact("bad", |tx| -> Result<(), TxError> {
        tx.remove_entity(ghost)
    });
    assert_eq!(res.unwrap_err(), TxError::UnknownEntity(ghost));
}

#[test]
fn add_en_contenedor_inexistente_es_error() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let ghost_block: BlockId = ObjectId(555).into();
    let res = session.transact("bad", |tx| -> Result<EntityId, TxError> {
        tx.add_entity(ContainerRef::Block(ghost_block), point_rec(l0, 0.0, 0.0))
    });
    assert!(matches!(res, Err(TxError::UnknownContainer(_))));
    // Rejection consumes no ID.
    assert_eq!(session.document().next_object_id(), 6);
}

#[test]
fn add_con_capa_inexistente_es_error() {
    let mut session = Session::new(Units::default());
    let ghost_layer: LayerId = ObjectId(777).into();
    let res = session.transact("bad", |tx| -> Result<EntityId, TxError> {
        tx.add_entity(ContainerRef::ModelSpace, point_rec(ghost_layer, 0.0, 0.0))
    });
    assert_eq!(res.unwrap_err(), TxError::UnknownLayer(ghost_layer));
    assert!(session.document().model_space().is_empty());
}

#[test]
fn modify_a_geometria_invalida_es_error_y_no_muta() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let id = add_point(&mut session, l0, 1.0, 2.0);
    let before = serde_json::to_string(session.document()).unwrap();

    let res = session.transact("bad mod", |tx| -> Result<(), TxError> {
        tx.modify_entity(id, |r| {
            r.geometry = EntityGeometry::Point(PointGeo::new(Point2::new(f64::NAN, 0.0)));
        })
    });
    assert!(matches!(res, Err(TxError::InvalidGeometry(_))));
    // Validation occurs before writing, so the entity remains intact.
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

#[test]
fn add_con_geometria_no_finita_es_error() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let res = session.transact("bad", |tx| -> Result<EntityId, TxError> {
        tx.add_entity(
            ContainerRef::ModelSpace,
            line_rec(l0, f64::NAN, 0.0, 1.0, 1.0),
        )
    });
    assert!(matches!(res, Err(TxError::InvalidGeometry(_))));
    assert!(session.document().model_space().is_empty());
    assert_eq!(session.document().next_object_id(), 6, "sin id consumido");
}

// --------------------------------------------------------------------------
// Rollback restores byte-identical state, draw order, and allocator cursor.
// --------------------------------------------------------------------------

#[test]
fn rollback_en_op_k_de_n_es_byte_identico() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let a = add_point(&mut session, l0, 0.0, 0.0);
    let b = add_point(&mut session, l0, 1.0, 0.0);
    let _c = add_point(&mut session, l0, 2.0, 0.0);

    let before = serde_json::to_string(session.document()).unwrap();

    // Fail after insertion, middle removal, and modification.
    let result: Result<TxOutcome<()>, TxError> =
        session.transact("boom", |tx| -> Result<(), TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, 9.0, 9.0))?; // Insert.
            tx.remove_entity(b)?; // Remove the middle entity.
            tx.modify_entity(a, |r| r.visible = false)?; // Modify.
            Err(TxError::Internal("fallo inyectado en la op 4"))
        });
    assert!(result.is_err());

    let after = serde_json::to_string(session.document()).unwrap();
    assert_eq!(
        before, after,
        "rollback debe dejar el documento byte-idéntico (contenido, draw order y nextObjectId)"
    );
}

#[test]
fn rollback_restaura_el_draw_order_exacto() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    let a = add_point(&mut session, l0, 0.0, 0.0);
    let b = add_point(&mut session, l0, 1.0, 0.0);
    let c = add_point(&mut session, l0, 2.0, 0.0);

    let result: Result<TxOutcome<()>, TxError> =
        session.transact("boom", |tx| -> Result<(), TxError> {
            tx.remove_entity(b)?; // Remove the middle entity.
            Err(TxError::Internal("fallo"))
        });
    assert!(result.is_err());

    // Exact order and positions are restored.
    let order: Vec<EntityId> = session
        .document()
        .model_space()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(order, vec![a, b, c]);
    assert_eq!(session.document().model_space().index_of(b), Some(1));
}

#[test]
fn el_error_del_closure_se_propaga_tras_el_rollback() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();

    let res = session.transact("boom", |tx| -> Result<(), TxError> {
        tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, 0.0, 0.0))?;
        Err(TxError::Internal("mi error"))
    });
    assert_eq!(res.unwrap_err(), TxError::Internal("mi error"));
    // The added entity was rolled back.
    assert!(session.document().model_space().is_empty());
}

// --------------------------------------------------------------------------
// Serialization round trip after a committed transaction.
// --------------------------------------------------------------------------

#[test]
fn documento_roundtrips_tras_transacciones() {
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();
    add_point(&mut session, l0, 0.0, 0.0);
    let id = add_point(&mut session, l0, 1.0, 0.0);
    session
        .transact("mv", |tx| -> Result<(), TxError> {
            tx.modify_entity(id, |r| translate(r, 3.0, 4.0))
        })
        .unwrap();

    let json = serde_json::to_string(session.document()).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, session.document());
}

// --------------------------------------------------------------------------
// Layer add, remove, modify, and `layers_changed` behavior.
// --------------------------------------------------------------------------

#[test]
fn add_layer_produce_changeset_con_la_capa_en_layers_changed() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let expected = session.document().next_object_id();

    let out = session
        .transact("add", |tx| -> Result<LayerId, TxError> {
            tx.add_layer_raw(layer_val("Muros", Color::aci(1).unwrap(), lt))
        })
        .unwrap();
    let id = out.value;

    // Layers use the shared allocator, which advances once on commit.
    assert_eq!(id.raw().0, expected, "el id proviene del asignador");
    assert_eq!(session.document().next_object_id(), expected + 1);

    let cs = out.change_set.expect("hubo ops -> changeset");
    assert_eq!(cs.layers_changed(), &[id]);
    assert!(cs.added().is_empty() && cs.removed().is_empty() && cs.modified().is_empty());
    assert!(
        !cs.doc_changed(),
        "la tabla de capas no es una propiedad de documento"
    );
    assert_eq!(session.document().layer(id).unwrap().name(), "Muros");
}

#[test]
fn add_layer_ignora_el_id_del_llamador() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let expected = session.document().next_object_id();

    // Ignore the caller's deliberately invalid layer ID.
    let bogus = Layer::new(
        ObjectId(999_999).into(),
        "X",
        Color::aci(2).unwrap(),
        lt,
        Lineweight::ByLayer,
    );
    let out = session
        .transact("add", |tx| -> Result<LayerId, TxError> {
            tx.add_layer_raw(bogus)
        })
        .unwrap();

    assert_eq!(out.value.raw().0, expected);
    assert!(session.document().layer(ObjectId(999_999).into()).is_none());
    assert!(session.document().layer(out.value).is_some());
}

#[test]
fn capa_y_entidad_comparten_el_mismo_cursor_de_ids() {
    // A layer and entity in one transaction receive consecutive deferred IDs.
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let start = session.document().next_object_id();

    let out = session
        .transact("mix", |tx| -> Result<(LayerId, EntityId), TxError> {
            let lid = tx.add_layer_raw(layer_val("Muros", Color::aci(1).unwrap(), lt))?;
            let eid = tx.add_entity(ContainerRef::ModelSpace, point_rec(lid, 0.0, 0.0))?;
            Ok((lid, eid))
        })
        .unwrap();
    let (lid, eid) = out.value;

    assert_eq!(lid.raw().0, start);
    assert_eq!(eid.raw().0, start + 1);
    assert_eq!(session.document().next_object_id(), start + 2);

    let cs = out.change_set.unwrap();
    assert_eq!(cs.added(), &[eid], "la entidad en added");
    assert_eq!(cs.layers_changed(), &[lid], "la capa en layers_changed");
}

#[test]
fn remove_layer_produce_changeset_y_la_capa_desaparece() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let id = add_layer(&mut session, "Muros", lt);

    let out = session
        .transact("rm", |tx| -> Result<(), TxError> {
            tx.remove_layer_raw(id)
        })
        .unwrap();

    let cs = out.change_set.unwrap();
    assert_eq!(cs.layers_changed(), &[id]);
    assert!(session.document().layer(id).is_none());
}

#[test]
fn modify_layer_produce_changeset_y_conserva_el_id() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let id = add_layer(&mut session, "Muros", lt); // ACI color 1.

    // Modification preserves identity despite an invalid replacement ID.
    let out = session
        .transact("mod", |tx| -> Result<(), TxError> {
            let nuevo = Layer::new(
                ObjectId(424_242).into(),
                "Muros",
                Color::aci(3).unwrap(),
                lt,
                Lineweight::ByLayer,
            );
            tx.modify_layer_raw(id, nuevo)
        })
        .unwrap();

    let cs = out.change_set.unwrap();
    assert_eq!(cs.layers_changed(), &[id]);
    assert_eq!(
        session.document().layer(id).unwrap().color(),
        Color::aci(3).unwrap()
    );
    assert!(session.document().layer(ObjectId(424_242).into()).is_none());
}

#[test]
fn modify_layer_no_op_no_registra_transaccion() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let id = add_layer(&mut session, "Muros", lt);

    // Replacing a layer with identical fields records no operation.
    let out = session
        .transact("noop", |tx| -> Result<(), TxError> {
            tx.modify_layer_raw(id, layer_val("Muros", Color::aci(1).unwrap(), lt))
        })
        .unwrap();
    assert!(out.transaction.is_none());
    assert!(out.change_set.is_none());
}

#[test]
fn capa_creada_y_borrada_en_la_misma_tx_se_omite_de_layers_changed() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);

    let out = session
        .transact("add+rm", |tx| -> Result<LayerId, TxError> {
            let id = tx.add_layer_raw(layer_val("Efimera", Color::aci(1).unwrap(), lt))?;
            tx.remove_layer_raw(id)?;
            Ok(id)
        })
        .unwrap();
    let id = out.value;

    // Two reversible operations produce a transaction but no net change.
    let tx = out.transaction.expect("2 ops -> hay transacción");
    assert_eq!(tx.len(), 2);
    let cs = out.change_set.unwrap();
    assert!(cs.layers_changed().is_empty());
    assert!(cs.is_empty());
    assert!(session.document().layer(id).is_none());
}

// --------------------------------------------------------------------------
// Low-level layer-operation rejections do not mutate or panic.
// --------------------------------------------------------------------------

#[test]
fn add_layer_nombre_duplicado_es_error() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    add_layer(&mut session, "Muros", lt);

    // Case-insensitive duplicate names are rejected.
    let res = session.transact("dup", |tx| -> Result<LayerId, TxError> {
        tx.add_layer_raw(layer_val("muros", Color::aci(1).unwrap(), lt))
    });
    assert!(matches!(res, Err(TxError::DuplicateLayerName(_))));

    // Layer "0" cannot be recreated.
    let res0 = session.transact("dup0", |tx| -> Result<LayerId, TxError> {
        tx.add_layer_raw(layer_val("0", Color::aci(1).unwrap(), lt))
    });
    assert!(matches!(res0, Err(TxError::DuplicateLayerName(_))));
}

#[test]
fn add_layer_con_linetype_inexistente_es_error() {
    let mut session = Session::new(Units::default());
    let ghost_lt: StyleId = ObjectId(888).into();
    let res = session.transact("bad lt", |tx| -> Result<LayerId, TxError> {
        tx.add_layer_raw(layer_val("Muros", Color::aci(1).unwrap(), ghost_lt))
    });
    assert_eq!(res.unwrap_err(), TxError::UnknownLineType(ghost_lt));
    assert!(session.document().layer_by_name("Muros").is_none());
}

#[test]
fn remove_layer_cero_es_error_aunque_no_sea_la_actual() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let l0 = session.document().current_layer();
    let muros = add_layer(&mut session, "Muros", lt);
    // Change current layer to isolate layer "0" protection.
    session
        .transact("cur", |tx| -> Result<(), TxError> {
            tx.set_current_layer(muros)
        })
        .unwrap();

    let res = session.transact("rm0", |tx| -> Result<(), TxError> {
        tx.remove_layer_raw(l0)
    });
    assert_eq!(res.unwrap_err(), TxError::LayerZeroProtected(l0));
    assert!(session.document().layer(l0).is_some());
}

#[test]
fn remove_layer_actual_es_error() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let muros = add_layer(&mut session, "Muros", lt);
    session
        .transact("cur", |tx| -> Result<(), TxError> {
            tx.set_current_layer(muros)
        })
        .unwrap();

    let res = session.transact("rm cur", |tx| -> Result<(), TxError> {
        tx.remove_layer_raw(muros)
    });
    assert_eq!(res.unwrap_err(), TxError::CurrentLayerRemoval(muros));
    assert!(session.document().layer(muros).is_some());
}

#[test]
fn remove_layer_en_uso_en_model_space_es_error() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let muros = add_layer(&mut session, "Muros", lt);
    add_point(&mut session, muros, 0.0, 0.0); // Entity on Muros.

    let res = session.transact("rm used", |tx| -> Result<(), TxError> {
        tx.remove_layer_raw(muros)
    });
    assert_eq!(res.unwrap_err(), TxError::LayerInUse(muros));
    assert!(session.document().layer(muros).is_some());
}

#[test]
fn remove_layer_en_uso_en_un_layout_es_error() {
    // Reference scans cover every container, not only model space.
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let muros = add_layer(&mut session, "Muros", lt);
    let lid = session.document().layouts().next().unwrap().id();
    session
        .transact("add to layout", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::Layout(lid), point_rec(muros, 1.0, 1.0))
        })
        .unwrap();

    let res = session.transact("rm used", |tx| -> Result<(), TxError> {
        tx.remove_layer_raw(muros)
    });
    assert_eq!(res.unwrap_err(), TxError::LayerInUse(muros));
}

#[test]
fn modify_layer_rename_a_nombre_existente_es_error() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    add_layer(&mut session, "Muros", lt);
    let techos = add_layer(&mut session, "Techos", lt);

    // Renaming Techos to "muros" collides case-insensitively with Muros.
    let res = session.transact("rename", |tx| -> Result<(), TxError> {
        tx.modify_layer_raw(techos, layer_val("muros", Color::aci(1).unwrap(), lt))
    });
    assert!(matches!(res, Err(TxError::DuplicateLayerName(_))));
    assert_eq!(session.document().layer(techos).unwrap().name(), "Techos");
}

#[test]
fn modify_layer_inexistente_es_error() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let ghost: LayerId = ObjectId(777).into();
    let res = session.transact("bad", |tx| -> Result<(), TxError> {
        tx.modify_layer_raw(ghost, layer_val("X", Color::aci(1).unwrap(), lt))
    });
    assert_eq!(res.unwrap_err(), TxError::UnknownLayer(ghost));
}

#[test]
fn modify_layer_con_linetype_inexistente_es_error_y_no_muta() {
    // Reject an unknown line-type reference before mutating the layer.
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let muros = add_layer(&mut session, "Muros", lt);
    let before = serde_json::to_string(session.document()).unwrap();

    let ghost_lt: StyleId = ObjectId(888).into();
    let res = session.transact("bad lt", |tx| -> Result<(), TxError> {
        tx.modify_layer_raw(muros, layer_val("Muros", Color::aci(1).unwrap(), ghost_lt))
    });
    assert_eq!(res.unwrap_err(), TxError::UnknownLineType(ghost_lt));

    // The document remains byte-identical and validates cleanly.
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
    let issues = session.document().clone().validate_full();
    assert!(issues.is_empty(), "validate_full reportó: {issues:?}");
}

#[test]
fn modify_layer_renombrar_la_cero_es_error() {
    // Layer "0" cannot be renamed; rejection leaves the document unchanged.
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let l0 = session.document().current_layer();

    let res = session.transact("rename 0", |tx| -> Result<(), TxError> {
        tx.modify_layer_raw(l0, layer_val("Base", Color::aci(1).unwrap(), lt))
    });
    assert_eq!(res.unwrap_err(), TxError::LayerZeroProtected(l0));
    assert_eq!(session.document().layer(l0).unwrap().name(), "0");
}

#[test]
fn exploit_renombrar_y_borrar_la_cero_en_dos_txs_es_imposible() {
    // Rejecting rename also prevents bypassing deletion protection in a later transaction.
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let l0 = session.document().current_layer();
    // Move the current layer away from "0" to isolate name protection.
    let otra = add_layer(&mut session, "Otra", lt);
    session
        .transact("cur", |tx| -> Result<(), TxError> {
            tx.set_current_layer(otra)
        })
        .unwrap();

    // Renaming "0" to "Base" is rejected.
    let paso1 = session.transact("rename 0", |tx| -> Result<(), TxError> {
        tx.modify_layer_raw(l0, layer_val("Base", Color::aci(1).unwrap(), lt))
    });
    assert_eq!(paso1.unwrap_err(), TxError::LayerZeroProtected(l0));

    // The unchanged name keeps deletion protection effective.
    let paso2 = session.transact("rm 0", |tx| -> Result<(), TxError> {
        tx.remove_layer_raw(l0)
    });
    assert_eq!(paso2.unwrap_err(), TxError::LayerZeroProtected(l0));
    assert!(session.document().layer(l0).is_some());
    assert_eq!(session.document().layer(l0).unwrap().name(), "0");
}

#[test]
fn modify_layer_cero_cambiando_otras_props_se_permite() {
    // Name protection does not prevent changes to other layer "0" properties.
    let mut session = Session::new(Units::default());
    let l0 = session.document().current_layer();

    // Derive only a color change from the actual layer "0" value.
    let recolored = session
        .document()
        .layer(l0)
        .unwrap()
        .clone()
        .with_color(Color::aci(3).unwrap());
    let out = session
        .transact("recolor 0", |tx| -> Result<(), TxError> {
            tx.modify_layer_raw(l0, recolored)
        })
        .expect("commit");

    let l = session.document().layer(l0).unwrap();
    assert_eq!(l.color(), Color::aci(3).unwrap());
    assert_eq!(l.name(), "0", "el nombre no cambió");
    let cs = out.change_set.expect("changeset");
    assert_eq!(cs.layers_changed(), &[l0]);
}

#[test]
fn layer_builder_with_cubre_todas_las_props_via_modify() {
    // Builder methods compose a complete replacement for `modify_layer_raw`.
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let muros = add_layer(&mut session, "Muros", lt);

    let derived = session
        .document()
        .layer(muros)
        .unwrap()
        .clone()
        .with_name("Muros2")
        .with_color(Color::aci(5).unwrap())
        .with_line_type(lt)
        .with_lineweight(Lineweight::Mm(0.5))
        .with_off(true)
        .with_frozen(true)
        .with_locked(true)
        .with_plot(false)
        .with_description("perimetral");

    session
        .transact("props", |tx| -> Result<(), TxError> {
            tx.modify_layer_raw(muros, derived)
        })
        .expect("commit");

    let l = session.document().layer(muros).unwrap();
    assert_eq!(l.name(), "Muros2");
    assert_eq!(l.color(), Color::aci(5).unwrap());
    assert_eq!(l.line_type(), lt);
    assert_eq!(l.lineweight(), Lineweight::Mm(0.5));
    assert!(l.is_off());
    assert!(l.is_frozen());
    assert!(l.is_locked());
    assert!(!l.is_plottable());
    assert_eq!(l.description(), "perimetral");
}

// --------------------------------------------------------------------------
// Byte-identical rollback with interleaved layer and entity operations.
// --------------------------------------------------------------------------

#[test]
fn rollback_con_capas_y_entidades_entrelazadas_es_byte_identico() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let l0 = session.document().current_layer();
    // Committed baseline: one extra layer and one entity on layer "0".
    let techos = add_layer(&mut session, "Techos", lt);
    add_point(&mut session, l0, 0.0, 0.0);

    let before = serde_json::to_string(session.document()).unwrap();

    // Fail after interleaving a layer insertion, entity insertion, and layer edit.
    // Rollback must restore content, table order, draw order, and allocator state.
    let res: Result<TxOutcome<()>, TxError> =
        session.transact("boom", |tx| -> Result<(), TxError> {
            let nueva = tx.add_layer_raw(layer_val("Muros", Color::aci(2).unwrap(), lt))?;
            tx.add_entity(ContainerRef::ModelSpace, point_rec(nueva, 9.0, 9.0))?;
            tx.modify_layer_raw(techos, layer_val("Techos", Color::aci(4).unwrap(), lt))?;
            Err(TxError::Internal("fallo inyectado"))
        });
    assert!(res.is_err());

    let after = serde_json::to_string(session.document()).unwrap();
    assert_eq!(
        before, after,
        "rollback byte-idéntico con capas y entidades"
    );
}

#[test]
fn rollback_de_remove_layer_restaura_la_tabla_exacta() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    add_layer(&mut session, "A", lt);
    let b = add_layer(&mut session, "B", lt);
    add_layer(&mut session, "C", lt);

    let before = serde_json::to_string(session.document()).unwrap();

    let res: Result<TxOutcome<()>, TxError> =
        session.transact("boom", |tx| -> Result<(), TxError> {
            tx.remove_layer_raw(b)?; // Remove the middle layer.
            Err(TxError::Internal("fallo"))
        });
    assert!(res.is_err());

    // Table order and serialization are restored exactly.
    let names: Vec<String> = session
        .document()
        .layers()
        .map(|l| l.name().to_string())
        .collect();
    assert_eq!(names, vec!["0", "A", "B", "C"]);
    assert_eq!(before, serde_json::to_string(session.document()).unwrap());
}

// --------------------------------------------------------------------------
// Property tests.
// --------------------------------------------------------------------------

use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Cmd {
    AddLine(f64, f64, f64, f64),
    AddPoint(f64, f64),
    Remove(usize),
    Modify(usize, f64, f64),
    AddLayer(u8),
    ModifyLayer(usize, u8),
    RemoveLayer(usize),
}

fn coord() -> impl Strategy<Value = f64> {
    -1.0e5f64..1.0e5
}

fn delta() -> impl Strategy<Value = f64> {
    -1.0e3f64..1.0e3
}

fn cmd() -> impl Strategy<Value = Cmd> {
    prop_oneof![
        (coord(), coord(), coord(), coord()).prop_map(|(a, b, c, d)| Cmd::AddLine(a, b, c, d)),
        (coord(), coord()).prop_map(|(x, y)| Cmd::AddPoint(x, y)),
        any::<usize>().prop_map(Cmd::Remove),
        (any::<usize>(), delta(), delta()).prop_map(|(i, dx, dy)| Cmd::Modify(i, dx, dy)),
        any::<u8>().prop_map(Cmd::AddLayer),
        (any::<usize>(), any::<u8>()).prop_map(|(i, c)| Cmd::ModifyLayer(i, c)),
        any::<usize>().prop_map(Cmd::RemoveLayer),
    ]
}

/// Valid ACI color in `1..=255` from an arbitrary byte.
fn aci_color(c: u8) -> Color {
    Color::aci((c % 255) + 1).unwrap()
}

proptest! {
    /// Every valid operation sequence leaves the document valid after each transaction.
    #[test]
    fn secuencias_validas_dejan_validate_full_limpio(cmds in prop::collection::vec(cmd(), 0..40)) {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let lt = continuous(&session);
        let mut live: Vec<EntityId> = Vec::new();
        // Track live created layers and a unique-name counter that never yields "0".
        let mut layers_live: Vec<LayerId> = Vec::new();
        let mut layer_seq: u32 = 0;

        for c in cmds {
            match c {
                Cmd::AddLine(x1, y1, x2, y2) => {
                    let out = session
                        .transact("add line", |tx| -> Result<EntityId, TxError> {
                            tx.add_entity(ContainerRef::ModelSpace, line_rec(l0, x1, y1, x2, y2))
                        })
                        .unwrap();
                    live.push(out.value);
                }
                Cmd::AddPoint(x, y) => {
                    let out = session
                        .transact("add pt", |tx| -> Result<EntityId, TxError> {
                            tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, x, y))
                        })
                        .unwrap();
                    live.push(out.value);
                }
                Cmd::Remove(i) => {
                    if !live.is_empty() {
                        let id = live.remove(i % live.len());
                        session
                            .transact("rm", |tx| -> Result<(), TxError> { tx.remove_entity(id) })
                            .unwrap();
                    }
                }
                Cmd::Modify(i, dx, dy) => {
                    if !live.is_empty() {
                        let id = live[i % live.len()];
                        session
                            .transact("mv", |tx| -> Result<(), TxError> {
                                tx.modify_entity(id, |r| translate(r, dx, dy))
                            })
                            .unwrap();
                    }
                }
                Cmd::AddLayer(col) => {
                    // `L{n}` guarantees a unique name distinct from "0".
                    let name = format!("L{layer_seq}");
                    layer_seq += 1;
                    let color = aci_color(col);
                    let out = session
                        .transact("add layer", |tx| -> Result<LayerId, TxError> {
                            tx.add_layer_raw(layer_val(&name, color, lt))
                        })
                        .unwrap();
                    layers_live.push(out.value);
                }
                Cmd::ModifyLayer(i, col) => {
                    if !layers_live.is_empty() {
                        let id = layers_live[i % layers_live.len()];
                        // Preserve the name while changing color.
                        let name = session.document().layer(id).unwrap().name().to_string();
                        let color = aci_color(col);
                        session
                            .transact("mod layer", |tx| -> Result<(), TxError> {
                                tx.modify_layer_raw(id, layer_val(&name, color, lt))
                            })
                            .unwrap();
                    }
                }
                Cmd::RemoveLayer(i) => {
                    // Created layers are noncurrent, unused, and therefore removable.
                    if !layers_live.is_empty() {
                        let id = layers_live.remove(i % layers_live.len());
                        session
                            .transact("rm layer", |tx| -> Result<(), TxError> {
                                tx.remove_layer_raw(id)
                            })
                            .unwrap();
                    }
                }
            }

            // A cloned document validates cleanly after every transaction.
            let issues = session.document().clone().validate_full();
            prop_assert!(issues.is_empty(), "validate_full reportó: {issues:?}");
        }

        // The document also round-trips bit for bit.
        let json = serde_json::to_string(session.document()).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&back, session.document());
    }

    /// Any operation prefix in a failed transaction rolls back byte for byte.
    #[test]
    fn rollback_arbitrario_es_byte_identico(
        seed in prop::collection::vec(cmd(), 0..24),
    ) {
        let mut session = Session::new(Units::default());
        let l0 = session.document().current_layer();
        let lt = continuous(&session);
        // Seed committed entities for removal and modification.
        let mut live: Vec<EntityId> = Vec::new();
        for k in 0..5 {
            live.push(add_point(&mut session, l0, k as f64, 0.0));
        }
        // Seed committed layers for modification and removal.
        let mut seed_layers: Vec<LayerId> = Vec::new();
        for k in 0..3 {
            seed_layers.push(add_layer(&mut session, &format!("S{k}"), lt));
        }

        let before = serde_json::to_string(session.document()).unwrap();

        let live_snapshot = live.clone();
        let layers_snapshot = seed_layers.clone();
        let result: Result<TxOutcome<()>, TxError> =
            session.transact("boom", |tx| -> Result<(), TxError> {
                for (k, c) in seed.iter().enumerate() {
                    // Ignore individual no-ops or rejections; the final failure must
                    // still restore exact state.
                    match c {
                        Cmd::AddLine(x1, y1, x2, y2) => {
                            let _ = tx.add_entity(
                                ContainerRef::ModelSpace,
                                line_rec(l0, *x1, *y1, *x2, *y2),
                            );
                        }
                        Cmd::AddPoint(x, y) => {
                            let _ = tx.add_entity(ContainerRef::ModelSpace, point_rec(l0, *x, *y));
                        }
                        Cmd::Remove(i) => {
                            if !live_snapshot.is_empty() {
                                let _ = tx.remove_entity(live_snapshot[i % live_snapshot.len()]);
                            }
                        }
                        Cmd::Modify(i, dx, dy) => {
                            if !live_snapshot.is_empty() {
                                let id = live_snapshot[i % live_snapshot.len()];
                                let _ = tx.modify_entity(id, |r| translate(r, *dx, *dy));
                            }
                        }
                        Cmd::AddLayer(col) => {
                            // Per-iteration name cannot collide with `S{n}` or "0".
                            let _ = tx.add_layer_raw(layer_val(&format!("f{k}"), aci_color(*col), lt));
                        }
                        Cmd::ModifyLayer(i, col) => {
                            if !layers_snapshot.is_empty() {
                                let j = i % layers_snapshot.len();
                                let _ = tx.modify_layer_raw(
                                    layers_snapshot[j],
                                    layer_val(&format!("S{j}"), aci_color(*col), lt),
                                );
                            }
                        }
                        Cmd::RemoveLayer(i) => {
                            if !layers_snapshot.is_empty() {
                                let _ =
                                    tx.remove_layer_raw(layers_snapshot[i % layers_snapshot.len()]);
                            }
                        }
                    }
                }
                Err(TxError::Internal("fallo inyectado"))
            });

        prop_assert!(result.is_err());
        let after = serde_json::to_string(session.document()).unwrap();
        prop_assert_eq!(before, after);
    }
}
