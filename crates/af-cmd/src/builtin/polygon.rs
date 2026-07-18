//! POLYGON (`POL`) creates a closed regular polygon with 3 to 1024 sides on the
//! current layer in one transaction.
//!
//! [`af_geom::polygon::regular_polygon_vertices`] provides counterclockwise
//! vertices. `inscribed` treats `radius` as circumradius; `circumscribed` treats it
//! as apothem and derives `radius / cos(π/n)`.
//!
//! `angle` positions the first vertex counterclockwise from positive X.

use core::f64::consts::PI;

use af_geom::polygon::regular_polygon_vertices;
use af_model::entity::{EntityGeometry, PolyVertex, PolylineGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_distance, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the POLYGON specification with alias `POL`.
#[must_use]
pub fn polygon_spec() -> CommandSpec {
    CommandSpec::new("POLYGON", "Polygon", true, polygon_exec)
        .alias("POL")
        .param(ParamSpec::required("sides", ParamType::Count))
        .param(ParamSpec::required("center", ParamType::Point))
        .param(ParamSpec::required("radius", ParamType::Distance))
        .param(ParamSpec::with_default(
            "mode",
            ParamType::Enum(vec!["inscribed".into(), "circumscribed".into()]),
            serde_json::json!("inscribed"),
        ))
        .param(ParamSpec::with_default(
            "angle",
            ParamType::Angle,
            serde_json::json!(0.0),
        ))
}

fn polygon_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // `Count` guarantees nonnegativity; enforce the polygon-specific range here.
    let sides = args
        .count("sides")
        .ok_or_else(|| CmdError::MissingParam("sides".to_string()))?;
    if !(3..=1024).contains(&sides) {
        return Err(CmdError::OutOfRange {
            param: "sides".to_string(),
            message: "el número de lados debe estar en 3..=1024".to_string(),
        });
    }
    let n = sides as usize;
    let center = req_point(&args, "center")?;
    let radius = req_distance(&args, "radius")?;
    let angle = args.angle("angle").unwrap_or(0.0);
    let mode = args.enum_value("mode").unwrap_or("inscribed");
    let circumradius = match mode {
        "inscribed" => radius,
        // Convert apothem to circumradius.
        "circumscribed" => radius / (PI / n as f64).cos(),
        other => {
            return Err(CmdError::Failed(format!(
                "POLYGON: modo no soportado '{other}'"
            )));
        }
    };

    let vertices: Vec<PolyVertex> = regular_polygon_vertices(center, circumradius, n, angle)
        .into_iter()
        .map(|pt| PolyVertex::new(pt, 0.0))
        .collect();
    // Retain a defensive empty-result check after validating the side count.
    if vertices.len() < 3 {
        return Err(CmdError::Failed(
            "POLYGON: no se pudieron generar los vértices".to_string(),
        ));
    }
    let geo = EntityGeometry::Polyline(PolylineGeo::new(vertices, true));
    let id = create_entity(ctx, "Polygon", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}
