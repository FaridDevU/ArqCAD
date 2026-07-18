//! Integration tests for public layer-management operations composed through
//! `Session::transact`.
//!
//! Each operation covers success, policy errors, and undo. Operations that allocate
//! no ID restore byte-identical state; creation differs only in `nextObjectId`.
//!
//! Block-definition coverage remains in the crate's unit tests because block setup
//! requires internal document mutation.

use af_math::Point2;
use af_model::entity::{Color, EntityGeometry, EntityRecord, LineTypeRef, Lineweight, PointGeo};
use af_model::id::{EntityId, LayerId, ObjectId, StyleId};
use af_model::layers_ops::{
    DeletePolicy, LayerOpError, LayerPatch, LayerProps, create_layer, delete_layer, rename_layer,
    set_current_layer, set_layer_props,
};
use af_model::units::Units;
use af_model::{ContainerRef, Session, TxError};

// --------------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------------

/// ID of the default "Continuous" line type.
fn continuous(session: &Session) -> StyleId {
    session.document().line_types().next().unwrap().id()
}

/// ID of permanent layer "0", also current in a new document.
fn layer_zero(session: &Session) -> LayerId {
    session.document().layer_by_name("0").unwrap().id()
}

fn point_rec(layer: LayerId, x: f64) -> EntityRecord {
    EntityRecord::new(
        ObjectId::NIL.into(),
        layer,
        Color::ByLayer,
        LineTypeRef::ByLayer,
        Lineweight::ByLayer,
        EntityGeometry::Point(PointGeo::new(Point2::new(x, 0.0))),
    )
}

/// Creates a layer in its own transaction and returns its ID.
fn make_layer(session: &mut Session, name: &str) -> LayerId {
    let lt = continuous(session);
    session
        .transact("mk layer", |tx| -> Result<LayerId, LayerOpError> {
            create_layer(
                tx,
                LayerProps::new(name, Color::aci(1).unwrap(), lt, Lineweight::ByLayer),
            )
        })
        .expect("commits")
        .value
}

/// Adds a model-space point and returns its ID.
fn add_point(session: &mut Session, layer: LayerId, x: f64) -> EntityId {
    session
        .transact("seed", |tx| -> Result<EntityId, TxError> {
            tx.add_entity(ContainerRef::ModelSpace, point_rec(layer, x))
        })
        .expect("commits")
        .value
}

/// Document JSON used for byte-identical comparison.
fn dump(session: &Session) -> String {
    serde_json::to_string(session.document()).unwrap()
}

/// Serialization with `nextObjectId` normalized for operations that allocate IDs.
fn dump_modulo_id(session: &Session) -> serde_json::Value {
    let mut v = serde_json::to_value(session.document()).unwrap();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("nextObjectId".to_string(), serde_json::Value::from(0u64));
    }
    v
}

// --------------------------------------------------------------------------
// create_layer
// --------------------------------------------------------------------------

#[test]
fn create_layer_caso_feliz_con_defaults() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let out = session
        .transact("create", |tx| -> Result<LayerId, LayerOpError> {
            create_layer(
                tx,
                LayerProps::new("Muros", Color::aci(1).unwrap(), lt, Lineweight::ByLayer),
            )
        })
        .expect("commits");
    let id = out.value;

    let layer = session.document().layer(id).expect("capa creada");
    assert_eq!(layer.name(), "Muros");
    assert_eq!(layer.color(), Color::aci(1).unwrap());
    assert_eq!(layer.line_type(), lt);
    assert!(!layer.is_off() && !layer.is_frozen() && !layer.is_locked());
    assert!(layer.is_plottable());
    assert_eq!(layer.description(), "");

    // The change set reports the new catalog layer.
    assert_eq!(out.change_set.unwrap().layers_changed(), &[id]);
}

#[test]
fn create_layer_respeta_estados_y_descripcion() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let mut props = LayerProps::new("Oculta", Color::aci(3).unwrap(), lt, Lineweight::Mm(0.5));
    props.off = true;
    props.frozen = true;
    props.locked = true;
    props.plot = false;
    props.description = "no se dibuja".to_string();

    let id = session
        .transact("create", |tx| -> Result<LayerId, LayerOpError> {
            create_layer(tx, props)
        })
        .expect("commits")
        .value;

    let layer = session.document().layer(id).unwrap();
    assert!(layer.is_off() && layer.is_frozen() && layer.is_locked());
    assert!(!layer.is_plottable());
    assert_eq!(layer.lineweight(), Lineweight::Mm(0.5));
    assert_eq!(layer.description(), "no se dibuja");
}

#[test]
fn create_layer_undo_byte_identico_modulo_next_id() {
    let mut session = Session::new(Units::default());
    let before = dump_modulo_id(&session);
    let next_before = session.document().next_object_id();

    let id = make_layer(&mut session, "Muros");
    assert!(session.document().layer(id).is_some());
    // Commit consumes exactly one ID.
    assert_eq!(session.document().next_object_id(), next_before + 1);

    session.undo().expect("undo ok");
    assert!(session.document().layer(id).is_none());
    // Undo restores byte-identical state apart from `nextObjectId`.
    assert_eq!(before, dump_modulo_id(&session));
    assert_eq!(session.document().next_object_id(), next_before + 1);
}

#[test]
fn create_layer_nombre_duplicado_case_insensitive() {
    let mut session = Session::new(Units::default());
    make_layer(&mut session, "Muros");
    let lt = continuous(&session);
    let before = dump(&session);

    let err = session
        .transact("dup", |tx| -> Result<LayerId, LayerOpError> {
            create_layer(
                tx,
                LayerProps::new("muros", Color::aci(1).unwrap(), lt, Lineweight::ByLayer),
            )
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::DuplicateName("muros".to_string()));
    // Rejection rolls back without changing the document.
    assert_eq!(before, dump(&session));
}

#[test]
fn create_layer_nombre_vacio_o_espacios() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    for name in ["", "   "] {
        let err = session
            .transact("empty", |tx| -> Result<LayerId, LayerOpError> {
                create_layer(
                    tx,
                    LayerProps::new(name, Color::aci(1).unwrap(), lt, Lineweight::ByLayer),
                )
            })
            .unwrap_err();
        assert_eq!(err, LayerOpError::EmptyName);
    }
}

#[test]
fn create_layer_caracter_prohibido_dxf() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);
    let err = session
        .transact("bad char", |tx| -> Result<LayerId, LayerOpError> {
            create_layer(
                tx,
                LayerProps::new(
                    "Eje/Central",
                    Color::aci(1).unwrap(),
                    lt,
                    Lineweight::ByLayer,
                ),
            )
        })
        .unwrap_err();
    assert_eq!(
        err,
        LayerOpError::InvalidNameChar {
            name: "Eje/Central".to_string(),
            ch: '/'
        }
    );
}

// --------------------------------------------------------------------------
// rename_layer
// --------------------------------------------------------------------------

#[test]
fn rename_layer_caso_feliz_y_undo() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let before = dump(&session);

    session
        .transact("rename", |tx| -> Result<(), LayerOpError> {
            rename_layer(tx, muros, "Tabiques")
        })
        .expect("commits");
    assert_eq!(session.document().layer(muros).unwrap().name(), "Tabiques");

    // Rename allocates no ID, so undo is byte-identical.
    session.undo().expect("undo ok");
    assert_eq!(session.document().layer(muros).unwrap().name(), "Muros");
    assert_eq!(before, dump(&session));
}

#[test]
fn rename_layer_cero_protegida() {
    let mut session = Session::new(Units::default());
    let l0 = layer_zero(&session);
    let err = session
        .transact("rename 0", |tx| -> Result<(), LayerOpError> {
            rename_layer(tx, l0, "Base")
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::LayerZeroProtected(l0));
}

#[test]
fn rename_layer_a_nombre_de_otra_capa() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    make_layer(&mut session, "Tabiques");

    let err = session
        .transact("collide", |tx| -> Result<(), LayerOpError> {
            rename_layer(tx, muros, "tabiques")
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::DuplicateName("tabiques".to_string()));
}

#[test]
fn rename_layer_solo_cambio_de_mayusculas_es_valido() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "muros");
    // A case-only self-rename does not collide with itself.
    session
        .transact("recase", |tx| -> Result<(), LayerOpError> {
            rename_layer(tx, muros, "MUROS")
        })
        .expect("commits");
    assert_eq!(session.document().layer(muros).unwrap().name(), "MUROS");
}

#[test]
fn rename_layer_nombre_invalido() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let err = session
        .transact("bad", |tx| -> Result<(), LayerOpError> {
            rename_layer(tx, muros, "")
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::EmptyName);
}

// --------------------------------------------------------------------------
// set_layer_props
// --------------------------------------------------------------------------

#[test]
fn set_layer_props_caso_feliz_y_undo() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let before = dump(&session);

    session
        .transact("props", |tx| -> Result<(), LayerOpError> {
            set_layer_props(
                tx,
                muros,
                LayerPatch {
                    color: Some(Color::aci(5).unwrap()),
                    off: Some(true),
                    description: Some("planta baja".to_string()),
                    ..Default::default()
                },
            )
        })
        .expect("commits");
    let layer = session.document().layer(muros).unwrap();
    assert_eq!(layer.color(), Color::aci(5).unwrap());
    assert!(layer.is_off());
    assert_eq!(layer.description(), "planta baja");

    // Property edits allocate no ID, so undo is byte-identical.
    session.undo().expect("undo ok");
    assert_eq!(before, dump(&session));
}

#[test]
fn set_layer_props_en_capa_cero_permitido_y_undo() {
    // Layer "0" protects its name, not its color or state properties.
    let mut session = Session::new(Units::default());
    let l0 = layer_zero(&session);
    let before = dump(&session);

    session
        .transact("props 0", |tx| -> Result<(), LayerOpError> {
            set_layer_props(
                tx,
                l0,
                LayerPatch {
                    color: Some(Color::aci(4).unwrap()),
                    locked: Some(true),
                    ..Default::default()
                },
            )
        })
        .expect("commits");
    let layer = session.document().layer(l0).unwrap();
    assert_eq!(layer.color(), Color::aci(4).unwrap());
    assert!(layer.is_locked());

    session.undo().expect("undo ok");
    assert_eq!(before, dump(&session));
}

#[test]
fn set_layer_props_patch_vacio_es_noop() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let out = session
        .transact("noop", |tx| -> Result<(), LayerOpError> {
            set_layer_props(tx, muros, LayerPatch::default())
        })
        .expect("commits");
    // No net change produces no transaction or undo entry.
    assert!(out.transaction.is_none());
}

#[test]
fn set_layer_props_capa_inexistente() {
    let mut session = Session::new(Units::default());
    let ghost: LayerId = ObjectId(999_999).into();
    let err = session
        .transact("ghost", |tx| -> Result<(), LayerOpError> {
            set_layer_props(
                tx,
                ghost,
                LayerPatch {
                    off: Some(true),
                    ..Default::default()
                },
            )
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::UnknownLayer(ghost));
}

// --------------------------------------------------------------------------
// delete_layer
// --------------------------------------------------------------------------

#[test]
fn delete_layer_reject_con_entidades_en_model_space() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    add_point(&mut session, muros, 0.0);
    add_point(&mut session, muros, 1.0);
    let before = dump(&session);

    let err = session
        .transact("del", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, muros, DeletePolicy::RejectIfUsed)
        })
        .unwrap_err();
    assert_eq!(
        err,
        LayerOpError::LayerInUse {
            layer: muros,
            count: 2
        }
    );
    // Rollback preserves the layer and its entities.
    assert!(session.document().layer(muros).is_some());
    assert_eq!(before, dump(&session));
}

#[test]
fn delete_layer_reject_sin_uso_y_undo() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let before = dump(&session);

    session
        .transact("del", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, muros, DeletePolicy::RejectIfUsed)
        })
        .expect("commits");
    assert!(session.document().layer(muros).is_none());

    // Deletion allocates no ID; undo restores exact bytes and table position.
    session.undo().expect("undo ok");
    assert!(session.document().layer(muros).is_some());
    assert_eq!(before, dump(&session));
}

#[test]
fn delete_layer_move_reubica_en_model_space_y_undo() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let destino = make_layer(&mut session, "Destino");
    let e1 = add_point(&mut session, muros, 0.0);
    let e2 = add_point(&mut session, muros, 1.0);
    let before = dump(&session);

    session
        .transact("del move", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, muros, DeletePolicy::MoveEntitiesTo(destino))
        })
        .expect("commits");
    assert!(session.document().layer(muros).is_none());
    assert_eq!(session.document().entity(e1).unwrap().0.layer, destino);
    assert_eq!(session.document().entity(e2).unwrap().0.layer, destino);

    // Moving and deleting allocate no IDs, so undo is byte-identical.
    session.undo().expect("undo ok");
    assert!(session.document().layer(muros).is_some());
    assert_eq!(session.document().entity(e1).unwrap().0.layer, muros);
    assert_eq!(before, dump(&session));
}

#[test]
fn delete_layer_move_a_si_misma() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let err = session
        .transact("self", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, muros, DeletePolicy::MoveEntitiesTo(muros))
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::MoveTargetIsSource(muros));
}

#[test]
fn delete_layer_move_a_destino_inexistente() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    let ghost: LayerId = ObjectId(999_999).into();
    let err = session
        .transact("ghost dst", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, muros, DeletePolicy::MoveEntitiesTo(ghost))
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::MoveTargetMissing(ghost));
}

#[test]
fn delete_layer_cero_protegida() {
    let mut session = Session::new(Units::default());
    let l0 = layer_zero(&session);
    let err = session
        .transact("del 0", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, l0, DeletePolicy::RejectIfUsed)
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::LayerZeroProtected(l0));
}

#[test]
fn delete_layer_actual_rechazada() {
    let mut session = Session::new(Units::default());
    let muros = make_layer(&mut session, "Muros");
    // Make the layer current, then attempt deletion.
    session
        .transact("set current", |tx| -> Result<(), LayerOpError> {
            set_current_layer(tx, muros)
        })
        .expect("commits");
    let err = session
        .transact("del current", |tx| -> Result<(), LayerOpError> {
            delete_layer(tx, muros, DeletePolicy::RejectIfUsed)
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::CurrentLayerRemoval(muros));
}

// --------------------------------------------------------------------------
// set_current_layer
// --------------------------------------------------------------------------

#[test]
fn set_current_layer_caso_feliz_y_undo() {
    let mut session = Session::new(Units::default());
    let l0 = layer_zero(&session);
    let muros = make_layer(&mut session, "Muros");
    let before = dump(&session);

    session
        .transact("current", |tx| -> Result<(), LayerOpError> {
            set_current_layer(tx, muros)
        })
        .expect("commits");
    assert_eq!(session.document().current_layer(), muros);

    // Changing the current layer allocates no ID; undo restores the previous state.
    session.undo().expect("undo ok");
    assert_eq!(session.document().current_layer(), l0);
    assert_eq!(before, dump(&session));
}

#[test]
fn set_current_layer_a_congelada_o_apagada() {
    let mut session = Session::new(Units::default());
    let lt = continuous(&session);

    let mut congelada =
        LayerProps::new("Congelada", Color::aci(1).unwrap(), lt, Lineweight::ByLayer);
    congelada.frozen = true;
    let mut apagada = LayerProps::new("Apagada", Color::aci(1).unwrap(), lt, Lineweight::ByLayer);
    apagada.off = true;

    let frozen_id = session
        .transact("mk frozen", |tx| create_layer(tx, congelada))
        .expect("commits")
        .value;
    let off_id = session
        .transact("mk off", |tx| create_layer(tx, apagada))
        .expect("commits")
        .value;

    let err_frozen = session
        .transact("cur frozen", |tx| -> Result<(), LayerOpError> {
            set_current_layer(tx, frozen_id)
        })
        .unwrap_err();
    assert_eq!(err_frozen, LayerOpError::CurrentLayerNotDrawable(frozen_id));

    let err_off = session
        .transact("cur off", |tx| -> Result<(), LayerOpError> {
            set_current_layer(tx, off_id)
        })
        .unwrap_err();
    assert_eq!(err_off, LayerOpError::CurrentLayerNotDrawable(off_id));

    // The current layer remains unchanged.
    assert_eq!(session.document().current_layer(), layer_zero(&session));
}

#[test]
fn set_current_layer_inexistente() {
    let mut session = Session::new(Units::default());
    let ghost: LayerId = ObjectId(999_999).into();
    let err = session
        .transact("ghost", |tx| -> Result<(), LayerOpError> {
            set_current_layer(tx, ghost)
        })
        .unwrap_err();
    assert_eq!(err, LayerOpError::UnknownLayer(ghost));
}
