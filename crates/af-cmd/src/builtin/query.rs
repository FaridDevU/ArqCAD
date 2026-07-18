//! Read-only query commands: ID, DIST (`DI`), AREA (`AA`), LIST (`LI`), and
//! MEASUREGEOM (`MEA`).
//!
//! They create no transactions and return formatted core geometry results in
//! [`CommandOutcome::message`].

use std::f64::consts::{PI, TAU};

use af_geom::closed_polyline_signed_area;
use af_model::Document;
use af_model::entity::{EntityGeometry, EntityRecord};
use af_model::id::EntityId;

use crate::args::ParsedArgs;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

use super::report::{color_name, fmt_angle, fmt_len, fmt_pt, vertex_angle};

// ============================ registration ============================

/// Registers ID, DIST, AREA, LIST, and MEASUREGEOM.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(id_spec())?;
    registry.register(dist_spec())?;
    registry.register(area_spec())?;
    registry.register(list_spec())?;
    registry.register(measuregeom_spec())?;
    Ok(())
}

// ============================ specs ============================

/// Returns the ID point-coordinate specification.
#[must_use]
pub fn id_spec() -> CommandSpec {
    CommandSpec::new("ID", "Id", false, id_exec)
        .param(ParamSpec::required("point", ParamType::Point))
}

/// Returns the DIST specification with alias `DI`.
#[must_use]
pub fn dist_spec() -> CommandSpec {
    CommandSpec::new("DIST", "Dist", false, dist_exec)
        .alias("DI")
        .param(ParamSpec::required("p1", ParamType::Point))
        .param(ParamSpec::required("p2", ParamType::Point))
}

/// Returns the AREA specification with alias `AA`.
#[must_use]
pub fn area_spec() -> CommandSpec {
    CommandSpec::new("AREA", "Area", false, area_exec)
        .alias("AA")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Returns the LIST specification with alias `LI`.
#[must_use]
pub fn list_spec() -> CommandSpec {
    CommandSpec::new("LIST", "List", false, list_exec)
        .alias("LI")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
}

/// Returns the mode-driven MEASUREGEOM specification with alias `MEA`.
#[must_use]
pub fn measuregeom_spec() -> CommandSpec {
    CommandSpec::new("MEASUREGEOM", "Measuregeom", false, measuregeom_exec)
        .alias("MEA")
        .param(ParamSpec::required(
            "mode",
            ParamType::Enum(
                ["distance", "radius", "angle", "area", "length", "bounds"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            ),
        ))
        .param(ParamSpec::optional("p1", ParamType::Point))
        .param(ParamSpec::optional("p2", ParamType::Point))
        .param(ParamSpec::optional("p3", ParamType::Point))
        .param(ParamSpec::optional("entities", ParamType::EntitySet))
}

// ============================ exec ============================

fn id_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let p = args
        .point("point")
        .ok_or_else(|| CmdError::MissingParam("point".to_string()))?;
    let doc = ctx.document();
    Ok(CommandOutcome::message(format!(
        "X = {}   Y = {}",
        fmt_len(doc, p.x),
        fmt_len(doc, p.y)
    )))
}

fn dist_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let a = args
        .point("p1")
        .ok_or_else(|| CmdError::MissingParam("p1".to_string()))?;
    let b = args
        .point("p2")
        .ok_or_else(|| CmdError::MissingParam("p2".to_string()))?;
    Ok(CommandOutcome::message(dist_report(ctx.document(), a, b)))
}

fn area_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    Ok(CommandOutcome::message(area_report(ctx.document(), ids)))
}

fn list_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?;
    Ok(CommandOutcome::message(list_report(ctx.document(), ids)))
}

fn measuregeom_exec(
    ctx: &mut CommandCtx<'_>,
    args: ParsedArgs,
) -> Result<CommandOutcome, CmdError> {
    let mode = args
        .enum_value("mode")
        .ok_or_else(|| CmdError::MissingParam("mode".to_string()))?;
    let doc = ctx.document();
    let msg = match mode {
        "distance" => {
            let a = args
                .point("p1")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM distance: faltan p1 y p2".into()))?;
            let b = args
                .point("p2")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM distance: falta p2".into()))?;
            dist_report(doc, a, b)
        }
        "area" => {
            let ids = args
                .entity_set("entities")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM area: falta 'entities'".into()))?;
            area_report(doc, ids)
        }
        "length" => {
            let ids = args
                .entity_set("entities")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM length: falta 'entities'".into()))?;
            length_report(doc, ids)
        }
        "bounds" => {
            let ids = args
                .entity_set("entities")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM bounds: falta 'entities'".into()))?;
            let [id] = ids else {
                return Err(CmdError::Failed(
                    "MEASUREGEOM bounds: se requiere exactamente una entidad".into(),
                ));
            };
            let bbox = doc
                .entity(*id)
                .and_then(|(_, container)| doc.container(container)?.bbox(*id))
                .ok_or_else(|| {
                    CmdError::Failed("MEASUREGEOM bounds: entidad inexistente o sin bounds".into())
                })?;
            format!(
                "Min = {}, Max = {}, Width = {}, Height = {}",
                fmt_pt(doc, bbox.min),
                fmt_pt(doc, bbox.max),
                fmt_len(doc, bbox.width()),
                fmt_len(doc, bbox.height()),
            )
        }
        "radius" => {
            let ids = args
                .entity_set("entities")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM radius: falta 'entities'".into()))?;
            radius_report(doc, ids)?
        }
        "angle" => {
            let v = args
                .point("p1")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM angle: falta p1 (vértice)".into()))?;
            let a = args
                .point("p2")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM angle: falta p2".into()))?;
            let b = args
                .point("p3")
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM angle: falta p3".into()))?;
            let ang = vertex_angle(v, a, b)
                .ok_or_else(|| CmdError::Failed("MEASUREGEOM angle: rayo degenerado".into()))?;
            format!("Angle = {}", fmt_angle(doc, ang))
        }
        other => unreachable!("modo fuera del Enum de MEASUREGEOM: {other}"),
    };
    Ok(CommandOutcome::message(msg))
}

// ============================ report helpers ============================

/// Formats distance, XY angle, and deltas between two points.
fn dist_report(doc: &Document, a: af_math::Point2, b: af_math::Point2) -> String {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let dist = a.dist(b);
    format!(
        "Distance = {}, Angle in XY Plane = {}, Delta X = {}, Delta Y = {}",
        fmt_len(doc, dist),
        fmt_angle(doc, dy.atan2(dx)),
        fmt_len(doc, dx),
        fmt_len(doc, dy),
    )
}

/// Formats area and perimeter for measurable entities plus totals and omissions.
fn area_report(doc: &Document, ids: &[EntityId]) -> String {
    let mut out = String::new();
    let mut total_area = 0.0;
    let mut total_peri = 0.0;
    let mut measured = 0u32;
    for &id in ids {
        let Some((rec, _)) = doc.entity(id) else {
            out.push_str(&format!("Entity {}: no existe\n", id.raw().0));
            continue;
        };
        match area_of(&rec.geometry) {
            Some((area, peri, kind)) => {
                total_area += area;
                total_peri += peri;
                measured += 1;
                out.push_str(&format!(
                    "Entity {} ({kind}): Area = {}, Perimeter = {}\n",
                    id.raw().0,
                    fmt_len(doc, area),
                    fmt_len(doc, peri),
                ));
            }
            None => out.push_str(&format!(
                "Entity {}: no es un área cerrada (omitida)\n",
                id.raw().0
            )),
        }
    }
    out.push_str(&format!(
        "Total area = {}, Total perimeter = {} ({measured} medida(s))",
        fmt_len(doc, total_area),
        fmt_len(doc, total_peri),
    ));
    out
}

/// Formats exact finite geometry lengths and their total without renderer tessellation.
fn length_report(doc: &Document, ids: &[EntityId]) -> String {
    let mut out = String::new();
    let mut total = 0.0;
    let mut measured = 0u32;
    for &id in ids {
        let Some((rec, _)) = doc.entity(id) else {
            out.push_str(&format!("Entity {}: no existe\n", id.raw().0));
            continue;
        };
        match length_of(&rec.geometry) {
            Some((length, kind)) => {
                total += length;
                measured += 1;
                out.push_str(&format!(
                    "Entity {} ({kind}): Length = {}\n",
                    id.raw().0,
                    fmt_len(doc, length),
                ));
            }
            None => out.push_str(&format!(
                "Entity {}: no tiene longitud finita soportada (omitida)\n",
                id.raw().0
            )),
        }
    }
    out.push_str(&format!(
        "Total length = {} ({measured} medida(s))",
        fmt_len(doc, total),
    ));
    out
}

fn length_of(g: &EntityGeometry) -> Option<(f64, &'static str)> {
    match g {
        EntityGeometry::Line(line) => Some((line.p1.dist(line.p2), "line")),
        EntityGeometry::Circle(circle) => Some((TAU * circle.radius, "circle")),
        EntityGeometry::Arc(arc) => Some((arc.length(), "arc")),
        EntityGeometry::Ellipse(ellipse) => Some((ellipse.length(), "ellipse")),
        EntityGeometry::Polyline(polyline) => Some((polyline.length(), "polyline")),
        EntityGeometry::Point(_)
        | EntityGeometry::Xline(_)
        | EntityGeometry::Ray(_)
        | EntityGeometry::Spline(_)
        | EntityGeometry::Wipeout(_) => None,
    }
}

/// Returns area and perimeter for circles, complete ellipses, and closed polylines.
fn area_of(g: &EntityGeometry) -> Option<(f64, f64, &'static str)> {
    match g {
        EntityGeometry::Circle(c) => {
            Some((PI * c.radius * c.radius, 2.0 * PI * c.radius, "circle"))
        }
        EntityGeometry::Polyline(p) if p.is_closed_effective() => {
            let verts: Vec<_> = p.vertices.iter().map(|v| (v.pt, v.bulge)).collect();
            let area = closed_polyline_signed_area(&verts).abs();
            Some((area, p.length(), "polyline"))
        }
        EntityGeometry::Ellipse(e) if (e.sweep() - TAU).abs() <= 1e-9 => {
            let semi_minor = e.semi_minor();
            Some((PI * e.semi_major * semi_minor, e.length(), "ellipse"))
        }
        EntityGeometry::Line(_)
        | EntityGeometry::Point(_)
        | EntityGeometry::Arc(_)
        | EntityGeometry::Ellipse(_)
        | EntityGeometry::Polyline(_)
        | EntityGeometry::Xline(_)
        | EntityGeometry::Ray(_)
        | EntityGeometry::Spline(_)
        | EntityGeometry::Wipeout(_) => None,
    }
}

/// Formats radius and diameter for the first circular entity, plus circle metrics.
fn radius_report(doc: &Document, ids: &[EntityId]) -> Result<String, CmdError> {
    for &id in ids {
        let Some((rec, _)) = doc.entity(id) else {
            continue;
        };
        match &rec.geometry {
            EntityGeometry::Circle(c) => {
                return Ok(format!(
                    "Radius = {}, Diameter = {}, Circumference = {}, Area = {}",
                    fmt_len(doc, c.radius),
                    fmt_len(doc, 2.0 * c.radius),
                    fmt_len(doc, 2.0 * PI * c.radius),
                    fmt_len(doc, PI * c.radius * c.radius),
                ));
            }
            EntityGeometry::Arc(a) => {
                return Ok(format!(
                    "Radius = {}, Diameter = {}, Arc length = {}",
                    fmt_len(doc, a.radius),
                    fmt_len(doc, 2.0 * a.radius),
                    fmt_len(doc, a.length()),
                ));
            }
            _ => continue,
        }
    }
    Err(CmdError::Failed(
        "MEASUREGEOM radius: ninguna entidad circular en el conjunto".into(),
    ))
}

/// Formats readable properties for each entity in LIST-style blocks.
fn list_report(doc: &Document, ids: &[EntityId]) -> String {
    let mut out = String::new();
    for &id in ids {
        let Some((rec, _)) = doc.entity(id) else {
            out.push_str(&format!("Entity {}: no existe\n", id.raw().0));
            continue;
        };
        out.push_str(&entity_header(doc, id.raw().0, &rec));
        out.push_str(&geometry_report(doc, &rec.geometry));
        out.push('\n');
    }
    // Avoid a trailing newline.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Formats the common LIST type, layer, and color header.
fn entity_header(doc: &Document, id: u64, rec: &EntityRecord) -> String {
    let layer = doc
        .layer(rec.layer)
        .map_or("<desconocida>", af_model::Layer::name);
    format!(
        "{} #{}  Layer: {}  Color: {}\n",
        geom_kind(&rec.geometry),
        id,
        layer,
        color_name(rec.color),
    )
}

/// Returns the geometry class name used by LIST.
fn geom_kind(g: &EntityGeometry) -> &'static str {
    match g {
        EntityGeometry::Line(_) => "LINE",
        EntityGeometry::Point(_) => "POINT",
        EntityGeometry::Circle(_) => "CIRCLE",
        EntityGeometry::Arc(_) => "ARC",
        EntityGeometry::Ellipse(_) => "ELLIPSE",
        EntityGeometry::Polyline(_) => "LWPOLYLINE",
        EntityGeometry::Xline(_) => "XLINE",
        EntityGeometry::Ray(_) => "RAY",
        EntityGeometry::Spline(_) => "SPLINE",
        EntityGeometry::Wipeout(_) => "WIPEOUT",
    }
}

/// Formats geometry-specific LIST properties.
fn geometry_report(doc: &Document, g: &EntityGeometry) -> String {
    match g {
        EntityGeometry::Line(l) => format!(
            "   from {} to {}   length {}   angle {}\n",
            fmt_pt(doc, l.p1),
            fmt_pt(doc, l.p2),
            fmt_len(doc, l.p1.dist(l.p2)),
            fmt_angle(doc, (l.p2.y - l.p1.y).atan2(l.p2.x - l.p1.x)),
        ),
        EntityGeometry::Point(p) => format!("   at {}\n", fmt_pt(doc, p.position)),
        EntityGeometry::Circle(c) => format!(
            "   center {}   radius {}   area {}\n",
            fmt_pt(doc, c.center),
            fmt_len(doc, c.radius),
            fmt_len(doc, PI * c.radius * c.radius),
        ),
        EntityGeometry::Arc(a) => format!(
            "   center {}   radius {}   start {}   end {}   length {}\n",
            fmt_pt(doc, a.center),
            fmt_len(doc, a.radius),
            fmt_angle(doc, a.start_angle),
            fmt_angle(doc, a.end_angle),
            fmt_len(doc, a.length()),
        ),
        EntityGeometry::Ellipse(e) => format!(
            "   center {}   semi-major {}   semi-minor {}   ratio {}   rotation {}   start {}   end {}\n",
            fmt_pt(doc, e.center),
            fmt_len(doc, e.semi_major),
            fmt_len(doc, e.semi_minor()),
            e.ratio,
            fmt_angle(doc, e.rotation),
            fmt_angle(doc, e.start_param),
            fmt_angle(doc, e.end_param),
        ),
        EntityGeometry::Polyline(p) => {
            let mut s = format!(
                "   vertices {}   closed {}   length {}",
                p.vertices.len(),
                p.is_closed_effective(),
                fmt_len(doc, p.length()),
            );
            if p.is_closed_effective() {
                let verts: Vec<_> = p.vertices.iter().map(|v| (v.pt, v.bulge)).collect();
                s.push_str(&format!(
                    "   area {}",
                    fmt_len(doc, closed_polyline_signed_area(&verts).abs())
                ));
            }
            s.push('\n');
            s
        }
        EntityGeometry::Xline(x) => format!(
            "   base {}   angle {}   (infinite)\n",
            fmt_pt(doc, x.point),
            fmt_angle(doc, x.direction.y.atan2(x.direction.x)),
        ),
        EntityGeometry::Ray(r) => format!(
            "   from {}   angle {}   (infinite)\n",
            fmt_pt(doc, r.origin),
            fmt_angle(doc, r.direction.y.atan2(r.direction.x)),
        ),
        EntityGeometry::Spline(sp) => format!(
            "   fit points {}   closed {}\n",
            sp.fit_points.len(),
            sp.closed,
        ),
        EntityGeometry::Wipeout(w) => {
            format!("   points {}   closed true\n", w.points.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_math::Point2;
    use af_model::entity::EllipseGeo;

    #[test]
    fn area_ellipse_completa_es_exacta_y_perimetro_numerico() {
        let geometry =
            EntityGeometry::Ellipse(EllipseGeo::new(Point2::ORIGIN, 60.0, 0.5, 0.0, 0.0, TAU));
        let (area, perimeter, kind) = area_of(&geometry).unwrap();
        assert_eq!(kind, "ellipse");
        assert!((area - 5_654.866_776_461_628).abs() < 1e-9);
        assert!((perimeter - 290.653_446_616_445).abs() < 1e-6);
    }

    #[test]
    fn area_ellipse_limite_circular_y_arco_omitido() {
        let circle =
            EntityGeometry::Ellipse(EllipseGeo::new(Point2::ORIGIN, 10.0, 1.0, 0.0, 0.0, TAU));
        let (area, perimeter, _) = area_of(&circle).unwrap();
        assert!((area - 100.0 * PI).abs() < 1e-12);
        assert!((perimeter - 20.0 * PI).abs() < 1e-12);

        let arc = EntityGeometry::Ellipse(EllipseGeo::new(Point2::ORIGIN, 10.0, 0.5, 0.0, 0.0, PI));
        assert!(area_of(&arc).is_none());
    }

    #[test]
    fn longitud_elipse_completa_y_cuarto_de_arco() {
        let full = EllipseGeo::new(Point2::ORIGIN, 40.0, 0.5, 0.0, 0.0, TAU);
        let quarter = EllipseGeo::new(
            Point2::ORIGIN,
            40.0,
            0.5,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        );
        assert!((full.length() - 193.768_964_410_963).abs() < 1e-9);
        assert!((quarter.length() - 48.442_241_102_741).abs() < 1e-9);
    }
}
