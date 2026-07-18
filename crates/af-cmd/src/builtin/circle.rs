//! CIRCLE (`C`) creates a circle on the current layer in one transaction.
//!
//! Modes are center-radius, two diameter endpoints, three-point circumcircle, and
//! tangent-tangent-radius (`ttr`). TTR center candidates come from
//! [`af_geom::tangent`] and are selected using the two pick points.
//!
//! TTR accepts an entity set and one pick point for each tangent entity.

use af_geom::circle::circumcircle;
use af_geom::{TangentCurve, tangent_circle_centers};
use af_model::Document;
use af_model::entity::{CircleGeo, EntityGeometry};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_distance, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the CIRCLE specification with alias `C`.
#[must_use]
pub fn circle_spec() -> CommandSpec {
    CommandSpec::new("CIRCLE", "Circle", true, circle_exec)
        .alias("C")
        .param(ParamSpec::with_default(
            "mode",
            ParamType::Enum(vec![
                "center".into(),
                "2p".into(),
                "3p".into(),
                "ttr".into(),
            ]),
            serde_json::json!("center"),
        ))
        .param(ParamSpec::optional("center", ParamType::Point))
        .param(ParamSpec::optional("radius", ParamType::Distance))
        .param(ParamSpec::optional("p1", ParamType::Point))
        .param(ParamSpec::optional("p2", ParamType::Point))
        .param(ParamSpec::optional("p3", ParamType::Point))
        // TTR uses exactly two tangent entities.
        .param(ParamSpec::optional("entities", ParamType::EntitySet))
}

fn circle_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    // Registry validation inserts the canonical default mode.
    let mode = args.enum_value("mode").unwrap_or("center");
    let geo = match mode {
        "center" => CircleGeo::new(req_point(&args, "center")?, req_distance(&args, "radius")?),
        "2p" => {
            let p1 = req_point(&args, "p1")?;
            let p2 = req_point(&args, "p2")?;
            // Diameter endpoints define the midpoint center and half-length radius.
            CircleGeo::new(p1.midpoint(p2), p1.dist(p2) / 2.0)
        }
        "3p" => {
            let p1 = req_point(&args, "p1")?;
            let p2 = req_point(&args, "p2")?;
            let p3 = req_point(&args, "p3")?;
            let (center, radius) = circumcircle(p1, p2, p3).ok_or_else(|| {
                CmdError::Failed("CIRCLE 3P: los tres puntos son colineales".to_string())
            })?;
            CircleGeo::new(center, radius)
        }
        // TTR planning is read-only; entity creation remains the sole mutation.
        "ttr" => ttr_geo(ctx.document(), &args)?,
        // Registry Enum validation restricts `mode` to the branches above.
        other => {
            return Err(CmdError::Failed(format!(
                "CIRCLE: modo no soportado '{other}'"
            )));
        }
    };
    let id = create_entity(ctx, "Circle", EntityGeometry::Circle(geo))?;
    Ok(CommandOutcome::created(vec![id]))
}

/// Computes a radius-`radius` circle tangent to both referenced entities, choosing
/// among candidates using `p1` and `p2`. This function is read-only.
///
/// Arcs contribute their complete supporting circle and line segments contribute
/// their infinite line, allowing tangency to object extensions.
///
/// The chosen center minimizes its summed distance to both pick points. No valid
/// center produces an error before any transaction.
fn ttr_geo(doc: &Document, args: &ParsedArgs) -> Result<CircleGeo, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    if ids.len() != 2 {
        return Err(CmdError::Failed(format!(
            "CIRCLE TTR: se requieren exactamente dos entidades tangentes, se dieron {}",
            ids.len()
        )));
    }
    let radius = req_distance(args, "radius")?;
    let pick1 = req_point(args, "p1")?;
    let pick2 = req_point(args, "p2")?;

    let curve1 = tangent_curve_of(doc, ids[0])?;
    let curve2 = tangent_curve_of(doc, ids[1])?;

    let center = tangent_circle_centers(&curve1, &curve2, radius)
        .into_iter()
        .min_by(|a, b| {
            let da = a.dist(pick1) + a.dist(pick2);
            let db = b.dist(pick1) + b.dist(pick2);
            da.partial_cmp(&db).unwrap_or(core::cmp::Ordering::Equal)
        })
        .ok_or_else(|| {
            CmdError::Failed(
                "CIRCLE TTR: no existe círculo del radio dado tangente a ambas entidades"
                    .to_string(),
            )
        })?;

    Ok(CircleGeo::new(center, radius))
}

/// Converts a line, arc, or circle entity to its [`TangentCurve`] representation.
fn tangent_curve_of(doc: &Document, id: EntityId) -> Result<TangentCurve, CmdError> {
    let (rec, _) = doc.entity(id).ok_or(CmdError::UnknownEntity(id))?;
    match &rec.geometry {
        EntityGeometry::Line(l) => Ok(TangentCurve::Line { a: l.p1, b: l.p2 }),
        EntityGeometry::Circle(c) => Ok(TangentCurve::Circle {
            center: c.center,
            radius: c.radius,
        }),
        // Use the supporting circle so tangency may lie on the arc's extension.
        EntityGeometry::Arc(a) => Ok(TangentCurve::Circle {
            center: a.center,
            radius: a.radius,
        }),
        _ => Err(CmdError::Failed(format!(
            "CIRCLE TTR: la entidad {} no es una tangente válida (usa línea, arco o círculo)",
            id.raw().0
        ))),
    }
}
