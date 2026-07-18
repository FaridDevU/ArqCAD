//! DONUT (`DO`) creates a filled ring as a wide, two-semicircle polyline on the
//! current layer in one transaction.
//!
//! The closed polyline has two `bulge = 1` vertices on the mean circle:
//! - `r_mean = (d_int + d_ext) / 4`
//! - `width = (d_ext - d_int) / 2`
//!
//! This places the outer and inner edges at their requested radii. Width is stored
//! in [`PolylineGeo::width`](af_model::entity::PolylineGeo).
//!
//! Omitting `diam_int` represents a filled disk with inner diameter zero because
//! `Distance` parameters accept only positive values.

use af_math::Point2;
use af_model::entity::{EntityGeometry, PolyVertex, PolylineGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_distance, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the DONUT specification with alias `DO`.
#[must_use]
pub fn donut_spec() -> CommandSpec {
    CommandSpec::new("DONUT", "Donut", true, donut_exec)
        .alias("DO")
        .param(ParamSpec::required("center", ParamType::Point))
        .param(ParamSpec::required("diam_ext", ParamType::Distance))
        // Omission represents the zero inner diameter accepted by this command.
        .param(ParamSpec::optional("diam_int", ParamType::Distance))
}

fn donut_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let center = req_point(&args, "center")?;
    let d_ext = req_distance(&args, "diam_ext")?; // The registry guarantees a positive value.
    let d_int = args.distance("diam_int").unwrap_or(0.0); // Omitted means 0, a filled disk.
    if d_int >= d_ext {
        return Err(CmdError::OutOfRange {
            param: "diam_int".to_string(),
            message: "el diámetro interior debe ser menor que el exterior".to_string(),
        });
    }
    let r_mean = (d_int + d_ext) / 4.0;
    let width = (d_ext - d_int) / 2.0;
    // Two opposing bulge-one vertices form a complete counterclockwise circle.
    let vertices = vec![
        PolyVertex::new(Point2::new(center.x - r_mean, center.y), 1.0),
        PolyVertex::new(Point2::new(center.x + r_mean, center.y), 1.0),
    ];
    let geo = EntityGeometry::Polyline(PolylineGeo::new(vertices, true).with_width(width));
    let id = create_entity(ctx, "Donut", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}

#[cfg(test)]
mod tests {
    use crate::builtin::register_builtins;
    use crate::{CommandOutcome, CommandRegistry};
    use af_model::Session;
    use af_model::entity::{EntityGeometry, PolylineGeo};
    use af_model::units::Units;
    use serde_json::json;

    fn setup() -> (CommandRegistry, Session) {
        let mut reg = CommandRegistry::new();
        register_builtins(&mut reg).expect("builtins register");
        (reg, Session::new(Units::default()))
    }

    fn donut_geo(session: &Session, out: &CommandOutcome) -> PolylineGeo {
        assert_eq!(out.created.len(), 1, "DONUT crea 1 entidad");
        assert!(out.tx_seq.is_some(), "exactamente 1 tx");
        let (rec, _) = session.document().entity(out.created[0]).expect("entity");
        match rec.geometry.clone() {
            EntityGeometry::Polyline(p) => p,
            other => panic!("se esperaba Polyline, fue {other:?}"),
        }
    }

    #[test]
    fn donut_anillo_dos_semicirculos_con_grosor() {
        let (reg, mut session) = setup();
        let out = reg
            .execute(
                &mut session,
                "DO", // alias
                &json!({ "center": [0, 0], "diam_ext": 10, "diam_int": 6 }),
            )
            .expect("DONUT");
        assert_eq!(session.history().undo_depth(), 1, "1 tx");
        let p = donut_geo(&session, &out);
        assert!(p.closed);
        assert_eq!(p.vertices.len(), 2);
        assert_eq!(p.vertices[0].pt.x, -4.0);
        assert_eq!(p.vertices[1].pt.x, 4.0);
        assert_eq!(p.vertices[0].bulge, 1.0);
        assert_eq!(p.vertices[1].bulge, 1.0);
        assert_eq!(p.width, 2.0);
    }

    #[test]
    fn donut_diam_int_omitido_es_disco_relleno() {
        let (reg, mut session) = setup();
        let out = reg
            .execute(
                &mut session,
                "DONUT",
                &json!({ "center": [1, 2], "diam_ext": 8 }),
            )
            .expect("DONUT disco");
        let p = donut_geo(&session, &out);
        assert_eq!(p.width, 4.0);
        assert_eq!(p.vertices[0].pt.x, 1.0 - 2.0);
        assert_eq!(p.vertices[1].pt.x, 1.0 + 2.0);
    }

    #[test]
    fn donut_interior_no_menor_que_exterior_es_error() {
        let (reg, mut session) = setup();
        let err = reg
            .execute(
                &mut session,
                "DONUT",
                &json!({ "center": [0, 0], "diam_ext": 4, "diam_int": 4 }),
            )
            .unwrap_err();
        assert!(matches!(err, crate::CmdError::OutOfRange { .. }));
        assert_eq!(session.history().undo_depth(), 0, "0 tx en fallo");
    }
}
