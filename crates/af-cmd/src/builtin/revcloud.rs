//! REVCLOUD converts a closed outline into a revision-cloud polyline on the current
//! layer in one transaction.
//!
//! Input bulges are ignored, and each straight outline edge becomes outward arcs
//! with target chord length `arc_len` through [`af_geom::revcloud_vertices`].

use af_geom::revcloud_vertices;
use af_math::{Point2, Tol};
use af_model::ContainerRef;
use af_model::entity::{EntityGeometry, PolyVertex, PolylineGeo};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_distance};
use crate::builtin::edit_common::validate_editable;
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

const MAX_REVCLOUD_SEGMENTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloudStyle {
    Normal,
    Calligraphy,
}

/// Returns the REVCLOUD specification.
#[must_use]
pub fn revcloud_spec() -> CommandSpec {
    CommandSpec::new("REVCLOUD", "Revision Cloud", true, revcloud_exec)
        .alias("RC")
        .param(ParamSpec::optional("contour", ParamType::Path))
        .param(ParamSpec::optional("source", ParamType::EntitySet))
        .param(ParamSpec::required("arc_len", ParamType::Distance))
        .param(ParamSpec::with_default(
            "style",
            ParamType::Enum(vec!["NORMAL".to_string(), "CALLIGRAPHY".to_string()]),
            serde_json::json!("NORMAL"),
        ))
}

fn revcloud_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let arc_len = req_distance(&args, "arc_len")?; // The registry guarantees a positive value.
    let style = match args
        .enum_value("style")
        .ok_or_else(|| CmdError::MissingParam("style".to_string()))?
    {
        "NORMAL" => CloudStyle::Normal,
        "CALLIGRAPHY" => CloudStyle::Calligraphy,
        other => {
            return Err(CmdError::Failed(format!(
                "REVCLOUD: estilo validado inesperado '{other}'"
            )));
        }
    };
    let contour = args
        .path("contour")
        .map(|path| path.iter().map(|&(pt, _)| pt).collect::<Vec<_>>());
    let source = args.entity_set("source").map(<[EntityId]>::to_vec);

    match (contour, source) {
        (Some(contour), None) => {
            let geo = cloud_geometry(contour, arc_len, style)?;
            let id = create_entity(ctx, "Revision Cloud", geo)?;
            Ok(CommandOutcome::created(vec![id]))
        }
        (None, Some(source)) => convert_source(ctx, &source, arc_len, style),
        _ => Err(CmdError::Failed(
            "REVCLOUD: indique exactamente uno de 'contour' o 'source'".to_string(),
        )),
    }
}

fn convert_source(
    ctx: &mut CommandCtx<'_>,
    source: &[EntityId],
    arc_len: f64,
    style: CloudStyle,
) -> Result<CommandOutcome, CmdError> {
    if source.len() != 1 {
        return Err(CmdError::Failed(
            "REVCLOUD CONVERT: 'source' debe contener exactamente una entidad".to_string(),
        ));
    }
    let id = ctx.transact("Revision Cloud", |tx| -> Result<EntityId, CmdError> {
        let mut records = validate_editable(tx, "REVCLOUD CONVERT", source)?;
        let (source_id, mut replacement) = records.pop().ok_or_else(|| {
            CmdError::Failed("REVCLOUD CONVERT: no se encontró la entidad de origen".to_string())
        })?;
        let EntityGeometry::Polyline(poly) = &replacement.geometry else {
            return Err(CmdError::Failed(format!(
                "REVCLOUD CONVERT: la entidad {} no es una Polyline",
                source_id.raw().0
            )));
        };
        if !poly.is_closed_effective() {
            return Err(CmdError::Failed(format!(
                "REVCLOUD CONVERT: la Polyline {} debe estar cerrada",
                source_id.raw().0
            )));
        }
        let contour = poly.vertices.iter().map(|v| v.pt).collect();
        replacement.geometry = cloud_geometry(contour, arc_len, style)?;
        tx.remove_entity(source_id)?;
        Ok(tx.add_entity(ContainerRef::ModelSpace, replacement)?)
    })?;
    Ok(CommandOutcome::created(vec![id]))
}

fn cloud_geometry(
    contour: Vec<Point2>,
    arc_len: f64,
    style: CloudStyle,
) -> Result<EntityGeometry, CmdError> {
    let contour = validate_contour(contour, arc_len)?;
    let mut verts = revcloud_vertices(&contour, arc_len);
    if verts.len() < 3 {
        return Err(CmdError::Failed(
            "REVCLOUD: no se pudo generar la nube".to_string(),
        ));
    }
    if style == CloudStyle::Calligraphy {
        // A closed alternating cloud needs an even arc count. Split the first chord
        // deterministically while preserving the 4096-segment limit.
        if !verts.len().is_multiple_of(2) {
            let midpoint = verts[0].0.lerp(verts[1].0, 0.5);
            verts.insert(1, (midpoint, verts[0].1));
        }
        for (index, (_, bulge)) in verts.iter_mut().enumerate() {
            let magnitude = if index % 2 == 0 { 0.25 } else { 0.75 };
            *bulge = bulge.signum() * magnitude;
        }
    }
    let vertices: Vec<PolyVertex> = verts
        .into_iter()
        .map(|(pt, bulge)| PolyVertex::new(pt, bulge))
        .collect();
    Ok(EntityGeometry::Polyline(PolylineGeo::new(vertices, true)))
}

fn validate_contour(mut contour: Vec<Point2>, arc_len: f64) -> Result<Vec<Point2>, CmdError> {
    let tol = Tol::default();
    if contour.len() >= 2 && tol.points_coincide(contour[0], contour[contour.len() - 1]) {
        contour.pop();
    }
    if contour.len() < 3 {
        return Err(CmdError::Failed(
            "REVCLOUD: el contorno cerrado requiere al menos 3 vértices".to_string(),
        ));
    }

    let mut total = 0usize;
    for index in 0..contour.len() {
        let a = contour[index];
        let b = contour[(index + 1) % contour.len()];
        let len = a.dist(b);
        if !len.is_finite() {
            return Err(CmdError::Failed(
                "REVCLOUD: el contorno produce una longitud no finita".to_string(),
            ));
        }
        if len <= tol.point_merge {
            return Err(CmdError::Failed(format!(
                "REVCLOUD: el contorno contiene un tramo degenerado en el vértice {index}"
            )));
        }
        let segs = (len / arc_len).round().max(1.0);
        if !segs.is_finite() || segs > MAX_REVCLOUD_SEGMENTS as f64 {
            return Err(too_many_segments());
        }
        total = total
            .checked_add(segs as usize)
            .filter(|&count| count <= MAX_REVCLOUD_SEGMENTS)
            .ok_or_else(too_many_segments)?;
    }
    Ok(contour)
}

fn too_many_segments() -> CmdError {
    CmdError::OutOfRange {
        param: "arc_len".to_string(),
        message: format!(
            "la nube superaría el máximo de {MAX_REVCLOUD_SEGMENTS} arcos; aumente arc_len"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::MAX_REVCLOUD_SEGMENTS;
    use crate::builtin::register_builtins;
    use crate::{CmdError, CommandOutcome, CommandRegistry};
    use af_geom::bulge::bulge_to_arc;
    use af_math::Point2;
    use af_model::entity::{
        Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight, PolyVertex,
        PolylineGeo,
    };
    use af_model::id::{EntityId, LayerId, ObjectId};
    use af_model::layers::Layer;
    use af_model::units::Units;
    use af_model::{ContainerRef, Session, TxError};
    use serde_json::{Value, json};

    fn setup() -> (CommandRegistry, Session) {
        let mut reg = CommandRegistry::new();
        register_builtins(&mut reg).expect("builtins register");
        (reg, Session::new(Units::default()))
    }

    fn cloud_geo(session: &Session, out: &CommandOutcome) -> PolylineGeo {
        assert_eq!(out.created.len(), 1, "REVCLOUD crea 1 entidad");
        assert!(out.tx_seq.is_some(), "exactamente 1 tx");
        let (rec, _) = session.document().entity(out.created[0]).expect("entity");
        match rec.geometry.clone() {
            EntityGeometry::Polyline(p) => p,
            other => panic!("se esperaba Polyline, fue {other:?}"),
        }
    }

    fn square(closed: bool) -> PolylineGeo {
        PolylineGeo::new(
            vec![
                PolyVertex::new(Point2::new(0.0, 0.0), 0.0),
                PolyVertex::new(Point2::new(8.0, 0.0), 0.0),
                PolyVertex::new(Point2::new(8.0, 8.0), 0.0),
                PolyVertex::new(Point2::new(0.0, 8.0), 0.0),
            ],
            closed,
        )
    }

    fn seed(session: &mut Session, geometry: EntityGeometry) -> EntityId {
        let layer = session.document().current_layer();
        session
            .transact("seed", |tx| -> Result<EntityId, TxError> {
                tx.add_entity(
                    ContainerRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        layer,
                        Color::Rgb(12, 34, 56),
                        LineTypeRef::ByBlock,
                        Lineweight::Mm(0.35),
                        geometry,
                    ),
                )
            })
            .expect("seed commits")
            .value
    }

    fn configure_non_default_current(session: &mut Session) -> LayerId {
        let base_layer = session.document().current_layer();
        let continuous = session
            .document()
            .layer(base_layer)
            .expect("layer 0")
            .line_type();
        let layer = Layer::new(
            ObjectId::NIL.into(),
            "REVCLOUD_TEST",
            Color::aci(6).expect("ACI 6"),
            continuous,
            Lineweight::Mm(0.25),
        );
        session
            .transact("configure REVCLOUD defaults", |tx| -> Result<_, TxError> {
                let layer_id = tx.add_layer_raw(layer)?;
                tx.set_current_layer(layer_id)?;
                tx.set_current_color(Color::Rgb(90, 80, 70));
                tx.set_current_line_type(LineTypeRef::ByBlock)?;
                tx.set_current_lineweight(Lineweight::Mm(0.70));
                Ok(layer_id)
            })
            .expect("non-default current state commits")
            .value
    }

    fn snapshot(session: &Session) -> String {
        serde_json::to_string(session.document()).expect("document serializes")
    }

    fn lock_current_layer(session: &mut Session) {
        let layer_id = session.document().current_layer();
        let locked = session
            .document()
            .layer(layer_id)
            .expect("current layer")
            .clone()
            .with_locked(true);
        session
            .transact("lock layer", |tx| -> Result<(), TxError> {
                tx.modify_layer_raw(layer_id, locked)
            })
            .expect("lock commits");
    }

    #[test]
    fn rc_normaliza_cierre_crea_normal_en_current_defaults_y_una_tx() {
        let (reg, mut session) = setup();
        let default_layer = session.document().current_layer();
        let layer = configure_non_default_current(&mut session);
        assert_ne!(layer, default_layer);
        let depth = session.history().undo_depth();
        let out = reg
            .execute(
                &mut session,
                "RC",
                &json!({
                    "contour": [
                        { "pt": [0, 0] }, { "pt": [10, 0] },
                        { "pt": [10, 6] }, { "pt": [0, 6] },
                        { "pt": [0, 0] }
                    ],
                    "arc_len": 2
                }),
            )
            .expect("RC");
        assert_eq!(session.history().undo_depth(), depth + 1, "1 tx");
        let (record, _) = session.document().entity(out.created[0]).expect("entity");
        assert_eq!(record.layer, layer);
        assert_eq!(record.color, session.document().current_color());
        assert_eq!(record.color, Color::Rgb(90, 80, 70));
        assert_eq!(record.line_type, session.document().current_line_type());
        assert_eq!(record.line_type, LineTypeRef::ByBlock);
        assert_eq!(record.lineweight, session.document().current_lineweight());
        assert_eq!(record.lineweight, Lineweight::Mm(0.70));

        let p = cloud_geo(&session, &out);
        assert!(p.closed);
        assert_eq!(
            p.vertices.len(),
            16,
            "el A final redundante no crea un tramo"
        );
        let signo = p.vertices[0].bulge.signum();
        let n = p.vertices.len();
        for i in 0..n {
            let a = p.vertices[i];
            let b = p.vertices[(i + 1) % n];
            assert!(
                bulge_to_arc(a.pt, b.pt, a.bulge).is_ok(),
                "tramo {i} debe ser arco"
            );
            assert_eq!(a.bulge.signum(), signo, "orientación consistente en {i}");
            assert_eq!(a.bulge.abs(), 0.5, "NORMAL en {i}");
        }
    }

    #[test]
    fn convert_calligraphy_reemplaza_con_id_nuevo_props_y_undo_redo() {
        let (reg, mut session) = setup();
        let layer0 = session.document().current_layer();
        let source_layer = configure_non_default_current(&mut session);
        let old_id = seed(&mut session, EntityGeometry::Polyline(square(true)));
        session
            .transact(
                "switch away from source layer",
                |tx| -> Result<(), TxError> { tx.set_current_layer(layer0) },
            )
            .expect("current layer changes");
        let before = session.document().entity(old_id).expect("source").0.clone();
        assert_eq!(before.layer, source_layer);
        assert_ne!(before.layer, session.document().current_layer());
        let depth = session.history().undo_depth();

        let out = reg
            .execute(
                &mut session,
                "REVCLOUD",
                &json!({
                    "source": [old_id.raw().0],
                    "arc_len": 2,
                    "style": "calligraphy"
                }),
            )
            .expect("CONVERT");
        assert_eq!(session.history().undo_depth(), depth + 1, "una sola tx");
        assert_eq!(out.created.len(), 1);
        let new_id = out.created[0];
        assert_ne!(new_id, old_id, "remove+add asigna un ID nuevo");
        assert!(session.document().entity(old_id).is_none());

        {
            let after = session.document().entity(new_id).expect("replacement").0;
            assert_eq!(after.layer, before.layer);
            assert_eq!(after.color, before.color);
            assert_eq!(after.line_type, before.line_type);
            assert_eq!(after.lineweight, before.lineweight);
            assert_eq!(after.visible, before.visible);
            let EntityGeometry::Polyline(poly) = &after.geometry else {
                panic!("replacement must be Polyline");
            };
            assert!(poly.closed);
            let sign = poly.vertices[0].bulge.signum();
            for (index, vertex) in poly.vertices.iter().enumerate() {
                assert_eq!(vertex.bulge.signum(), sign, "outward sign at {index}");
                assert_eq!(
                    vertex.bulge.abs(),
                    if index % 2 == 0 { 0.25 } else { 0.75 },
                    "CALLIGRAPHY at {index}"
                );
            }
        }

        reg.execute(&mut session, "UNDO", &Value::Null)
            .expect("UNDO");
        assert!(session.document().entity(old_id).is_some());
        assert!(session.document().entity(new_id).is_none());
        reg.execute(&mut session, "REDO", &Value::Null)
            .expect("REDO");
        assert!(session.document().entity(old_id).is_none());
        assert!(session.document().entity(new_id).is_some());
    }

    #[test]
    fn calligraphy_alterna_tambien_en_la_costura_de_un_total_impar() {
        let (reg, mut session) = setup();
        let out = reg
            .execute(
                &mut session,
                "RC",
                &json!({
                    "contour": [
                        { "pt": [0, 0] }, { "pt": [8, 0] }, { "pt": [0, 8] }
                    ],
                    "arc_len": 100,
                    "style": "CALLIGRAPHY"
                }),
            )
            .expect("CALLIGRAPHY triangular");
        let poly = cloud_geo(&session, &out);
        assert_eq!(
            poly.vertices.len(),
            4,
            "3 arcos se normalizan a un total par"
        );
        assert!(poly.vertices.len() <= MAX_REVCLOUD_SEGMENTS);
        for index in 0..poly.vertices.len() {
            let current = poly.vertices[index].bulge.abs();
            let next = poly.vertices[(index + 1) % poly.vertices.len()].bulge.abs();
            assert_ne!(current, next, "alternancia ciclica en la costura {index}");
        }
    }

    #[test]
    fn invalidos_y_exceso_no_mutan_y_una_orden_posterior_recupera() {
        let (reg, mut session) = setup();

        let err = reg
            .execute(&mut session, "REVCLOUD", &json!({ "arc_len": 2 }))
            .unwrap_err();
        assert!(matches!(err, CmdError::Failed(_)));

        let err = reg
            .execute(
                &mut session,
                "REVCLOUD",
                &json!({
                    "contour": [
                        { "pt": [0, 0] }, { "pt": [4, 0] }, { "pt": [0, 4] }
                    ],
                    "arc_len": 1,
                    "style": "ETCHED"
                }),
            )
            .unwrap_err();
        assert!(matches!(err, CmdError::InvalidEnum { .. }));

        let err = reg
            .execute(
                &mut session,
                "REVCLOUD",
                &json!({
                    "contour": [
                        { "pt": [0, 0] }, { "pt": [0, 0] },
                        { "pt": [4, 0] }, { "pt": [0, 4] }
                    ],
                    "arc_len": 1
                }),
            )
            .unwrap_err();
        assert!(matches!(err, CmdError::Failed(ref message) if message.contains("degenerado")));

        let err = reg
            .execute(
                &mut session,
                "REVCLOUD",
                &json!({
                    "contour": [
                        { "pt": [0, 0] }, { "pt": [1025, 0] },
                        { "pt": [1025, 1024] }, { "pt": [0, 1024] }
                    ],
                    "arc_len": 1
                }),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            CmdError::OutOfRange { ref param, ref message }
                if param == "arc_len" && message.contains("4096")
        ));
        assert_eq!(
            session.history().undo_depth(),
            0,
            "todos los fallos son 0 tx"
        );
        assert_eq!(
            session.document().model_space().len(),
            0,
            "documento intacto"
        );

        let out = reg
            .execute(
                &mut session,
                "RC",
                &json!({
                    "contour": [
                        { "pt": [0, 0] }, { "pt": [4, 0] },
                        { "pt": [4, 4] }, { "pt": [0, 4] }
                    ],
                    "arc_len": 1
                }),
            )
            .expect("recovery command");
        assert_eq!(out.created.len(), 1);
        assert_eq!(session.history().undo_depth(), 1);
    }

    #[test]
    fn convert_rechaza_cardinalidad_abierta_tipo_y_doble_modo_sin_mutar() {
        let (reg, mut session) = setup();
        let open = seed(&mut session, EntityGeometry::Polyline(square(false)));
        let line = seed(
            &mut session,
            EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(1.0, 1.0))),
        );
        let before = snapshot(&session);
        let depth = session.history().undo_depth();

        let cases = [
            json!({ "source": [], "arc_len": 1 }),
            json!({ "source": [open.raw().0, line.raw().0], "arc_len": 1 }),
            json!({ "source": [open.raw().0], "arc_len": 1 }),
            json!({ "source": [line.raw().0], "arc_len": 1 }),
            json!({
                "source": [open.raw().0],
                "contour": [
                    { "pt": [0, 0] }, { "pt": [4, 0] }, { "pt": [0, 4] }
                ],
                "arc_len": 1
            }),
        ];
        for args in cases {
            assert!(reg.execute(&mut session, "REVCLOUD", &args).is_err());
            assert_eq!(snapshot(&session), before);
            assert_eq!(session.history().undo_depth(), depth);
        }

        let err = reg
            .execute(
                &mut session,
                "REVCLOUD",
                &json!({ "source": [9_999_999u64], "arc_len": 1 }),
            )
            .unwrap_err();
        assert!(matches!(err, CmdError::UnknownEntity(_)));
        assert_eq!(snapshot(&session), before);
        assert_eq!(session.history().undo_depth(), depth);
    }

    #[test]
    fn convert_rechaza_source_en_layer_locked_sin_mutar() {
        let (reg, mut session) = setup();
        let source = seed(&mut session, EntityGeometry::Polyline(square(true)));
        lock_current_layer(&mut session);
        let before = snapshot(&session);
        let depth = session.history().undo_depth();

        let err = reg
            .execute(
                &mut session,
                "REVCLOUD",
                &json!({ "source": [source.raw().0], "arc_len": 1 }),
            )
            .unwrap_err();
        assert!(matches!(err, CmdError::Failed(ref message) if message.contains("locked")));
        assert_eq!(snapshot(&session), before);
        assert_eq!(
            session.history().undo_depth(),
            depth,
            "locked rejection is 0 tx"
        );
    }
}
