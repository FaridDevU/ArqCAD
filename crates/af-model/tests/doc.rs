//! Integration tests for the public read API of `DrawingDocument`.
//!
//! These cover construction, getters, container and entity lookup, and serde
//! round trips. Internal mutation tests remain unit tests in the crate.

use af_model::container::ContainerRef;
use af_model::doc::Document;
use af_model::id::{LayoutId, ObjectId};
use af_model::units::{LinearUnit, Units};

#[test]
fn documento_nuevo_expone_defaults_por_la_api_publica() {
    let doc = Document::new(Units::default());

    // Units and document identity are present.
    assert_eq!(doc.units().linear, LinearUnit::Mm);
    assert_eq!(doc.id(), doc.id());

    // Layer "0" is current and supports case-insensitive lookup.
    let l0 = doc.layer_by_name("0").expect("capa 0 existe");
    assert_eq!(doc.current_layer(), l0.id());
    assert!(doc.layer_by_name("0").is_some());
    assert!(doc.layer(l0.id()).is_some());

    // Default styles are exposed through the public API.
    assert!(doc.line_types().any(|s| s.name() == "Continuous"));
    assert!(doc.text_styles().any(|s| s.name() == "Standard"));
    assert!(doc.dim_styles().any(|s| s.name() == "Standard"));

    // A new document has an empty "Layout1".
    let layout = doc.layouts().next().expect("Layout1");
    assert_eq!(layout.name(), "Layout1");
    assert!(layout.entities().is_empty());

    // Model space is empty, with no blocks or external references.
    assert!(doc.model_space().is_empty());
    assert_eq!(doc.blocks().count(), 0);
    assert!(doc.external_refs().is_empty());
}

#[test]
fn container_por_referencia_resuelve_model_space_y_layout() {
    let doc = Document::new(Units::default());

    // Model space always exists.
    assert!(doc.container(ContainerRef::ModelSpace).is_some());

    // Layout1 paper space exists.
    let lid = doc.layouts().next().unwrap().id();
    assert!(doc.container(ContainerRef::Layout(lid)).is_some());

    // An unknown layout returns `None`.
    let ghost: LayoutId = ObjectId(9_999).into();
    assert!(doc.container(ContainerRef::Layout(ghost)).is_none());
}

#[test]
fn entity_en_documento_nuevo_no_encuentra_nada() {
    let doc = Document::new(Units::default());
    // A new document contains no entities, so any ID lookup returns `None`.
    use af_model::id::{EntityId, ObjectId};
    let ghost: EntityId = ObjectId(9_999).into();
    assert!(doc.entity(ghost).is_none());
}

#[test]
fn validate_full_de_documento_nuevo_no_reporta_issues() {
    let mut doc = Document::new(Units::default());
    let issues = doc.validate_full();
    assert!(issues.is_empty(), "issues inesperados: {issues:?}");
}

#[test]
fn roundtrip_serde_de_documento_nuevo() {
    let doc = Document::new(Units {
        linear: LinearUnit::M,
    });
    let json = serde_json::to_string(&doc).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, back);
    assert_eq!(back.units().linear, LinearUnit::M);
    assert_eq!(back.id(), doc.id());
}

#[test]
fn serde_usa_camelcase_en_las_claves_del_documento() {
    let doc = Document::new(Units::default());
    let json = serde_json::to_string(&doc).unwrap();
    // Representative keys use camelCase.
    for key in [
        "\"currentLayer\"",
        "\"lineTypes\"",
        "\"textStyles\"",
        "\"dimStyles\"",
        "\"modelSpace\"",
        "\"externalRefs\"",
        "\"nextObjectId\"",
    ] {
        assert!(json.contains(key), "falta la clave {key} en: {json}");
    }
}
