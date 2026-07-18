//! RECTANG (`REC`) creates a counterclockwise closed polyline between opposite
//! axis-aligned corners. It supports chamfer distances, fillet radius, and width.

use af_geom::polygon::rectangle_vertices;
use af_math::Point2;
use af_model::entity::{EntityGeometry, PolyVertex, PolylineGeo};

use crate::args::ParsedArgs;
use crate::builtin::draw::{create_entity, req_point};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Returns the RECTANG specification with alias `REC`.
#[must_use]
pub fn rectang_spec() -> CommandSpec {
    CommandSpec::new("RECTANG", "Rectangle", true, rectang_exec)
        .alias("REC")
        .param(ParamSpec::required("p1", ParamType::Point))
        .param(ParamSpec::required("p2", ParamType::Point))
        .param(ParamSpec::optional("chamfer1", ParamType::Distance))
        .param(ParamSpec::optional("chamfer2", ParamType::Distance))
        .param(ParamSpec::optional("fillet", ParamType::Distance))
        .param(ParamSpec::optional("width", ParamType::Distance))
}

fn rectang_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let p1 = req_point(&args, "p1")?;
    let p2 = req_point(&args, "p2")?;
    let chamfer = match (args.distance("chamfer1"), args.distance("chamfer2")) {
        (None, None) => None,
        (Some(first), Some(second)) => Some((first, second)),
        _ => {
            return Err(CmdError::Failed(
                "RECTANG: chamfer1 y chamfer2 deben aparecer juntos".to_string(),
            ));
        }
    };
    let fillet = args.distance("fillet");
    if chamfer.is_some() && fillet.is_some() {
        return Err(CmdError::Failed(
            "RECTANG: chamfer y fillet son incompatibles".to_string(),
        ));
    }
    // Complete geometry validation before opening the creation transaction.
    let polyline = rectangle_polyline(p1, p2, chamfer, fillet, args.distance("width"))?;
    let geo = EntityGeometry::Polyline(polyline);
    let id = create_entity(ctx, "Rectangle", geo)?;
    Ok(CommandOutcome::created(vec![id]))
}

fn rectangle_polyline(
    p1: Point2,
    p2: Point2,
    chamfer: Option<(f64, f64)>,
    fillet: Option<f64>,
    width: Option<f64>,
) -> Result<PolylineGeo, CmdError> {
    const MIN_DIMENSION: f64 = 0.000_001;

    let [bottom_left, bottom_right, top_right, top_left] = rectangle_vertices(p1, p2);
    let rectangle_width = bottom_right.x - bottom_left.x;
    let rectangle_height = top_left.y - bottom_left.y;
    if !rectangle_width.is_finite()
        || !rectangle_height.is_finite()
        || rectangle_width <= MIN_DIMENSION
        || rectangle_height <= MIN_DIMENSION
    {
        if chamfer.is_none() && fillet.is_none() {
            let vertices = [bottom_left, bottom_right, top_right, top_left]
                .into_iter()
                .map(|point| PolyVertex::new(point, 0.0))
                .collect();
            return Ok(PolylineGeo::new(vertices, true).with_width(width.unwrap_or(0.0)));
        }
        return Err(CmdError::OutOfRange {
            param: "p2".to_string(),
            message: "RECTANG requiere ancho y alto finitos mayores que 0.000001".to_string(),
        });
    }

    let stroke_width = width.unwrap_or(0.0);
    if !stroke_width.is_finite() || stroke_width < 0.0 {
        return Err(CmdError::OutOfRange {
            param: "width".to_string(),
            message: "RECTANG width debe ser finito y no negativo".to_string(),
        });
    }

    let vertices = if let Some((incoming, outgoing)) = chamfer {
        if !incoming.is_finite() || incoming <= 0.0 || !outgoing.is_finite() || outgoing <= 0.0 {
            return Err(CmdError::OutOfRange {
                param: "chamfer1".to_string(),
                message: "RECTANG chamfer requiere dos distancias positivas".to_string(),
            });
        }
        let trim = incoming + outgoing;
        if !trim.is_finite() || trim >= rectangle_width.min(rectangle_height) {
            return Err(CmdError::OutOfRange {
                param: "chamfer1".to_string(),
                message: "RECTANG chamfer se solapa con el lado opuesto".to_string(),
            });
        }
        vec![
            PolyVertex::new(Point2::new(bottom_left.x + outgoing, bottom_left.y), 0.0),
            PolyVertex::new(Point2::new(bottom_right.x - incoming, bottom_right.y), 0.0),
            PolyVertex::new(Point2::new(bottom_right.x, bottom_right.y + outgoing), 0.0),
            PolyVertex::new(Point2::new(top_right.x, top_right.y - incoming), 0.0),
            PolyVertex::new(Point2::new(top_right.x - outgoing, top_right.y), 0.0),
            PolyVertex::new(Point2::new(top_left.x + incoming, top_left.y), 0.0),
            PolyVertex::new(Point2::new(top_left.x, top_left.y - outgoing), 0.0),
            PolyVertex::new(Point2::new(bottom_left.x, bottom_left.y + incoming), 0.0),
        ]
    } else if let Some(radius) = fillet {
        if !radius.is_finite() || radius <= 0.0 {
            return Err(CmdError::OutOfRange {
                param: "fillet".to_string(),
                message: "RECTANG fillet requiere un radio positivo".to_string(),
            });
        }
        if radius >= rectangle_width.min(rectangle_height) / 2.0 {
            return Err(CmdError::OutOfRange {
                param: "fillet".to_string(),
                message: "RECTANG fillet se solapa con el lado opuesto".to_string(),
            });
        }
        let quarter_bulge = std::f64::consts::FRAC_PI_8.tan();
        vec![
            PolyVertex::new(Point2::new(bottom_left.x + radius, bottom_left.y), 0.0),
            PolyVertex::new(
                Point2::new(bottom_right.x - radius, bottom_right.y),
                quarter_bulge,
            ),
            PolyVertex::new(Point2::new(bottom_right.x, bottom_right.y + radius), 0.0),
            PolyVertex::new(
                Point2::new(top_right.x, top_right.y - radius),
                quarter_bulge,
            ),
            PolyVertex::new(Point2::new(top_right.x - radius, top_right.y), 0.0),
            PolyVertex::new(Point2::new(top_left.x + radius, top_left.y), quarter_bulge),
            PolyVertex::new(Point2::new(top_left.x, top_left.y - radius), 0.0),
            PolyVertex::new(
                Point2::new(bottom_left.x, bottom_left.y + radius),
                quarter_bulge,
            ),
        ]
    } else {
        [bottom_left, bottom_right, top_right, top_left]
            .into_iter()
            .map(|point| PolyVertex::new(point, 0.0))
            .collect()
    };

    Ok(PolylineGeo::new(vertices, true).with_width(stroke_width))
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_geom::closed_polyline_signed_area;

    const TOL: f64 = 1e-9;

    fn area(polyline: &PolylineGeo) -> f64 {
        let vertices: Vec<_> = polyline
            .vertices
            .iter()
            .map(|vertex| (vertex.pt, vertex.bulge))
            .collect();
        closed_polyline_signed_area(&vertices).abs()
    }

    #[test]
    fn spec_conserva_alias_y_declara_modificadores() {
        let spec = rectang_spec();
        assert_eq!(spec.name(), "RECTANG");
        assert_eq!(spec.aliases(), ["REC"]);
        assert_eq!(
            spec.params()
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>(),
            ["p1", "p2", "chamfer1", "chamfer2", "fillet", "width"]
        );
    }

    #[test]
    fn rectangulo_basico_es_ccw_cerrado_y_fino() {
        let polyline = rectangle_polyline(
            Point2::new(10.0, 8.0),
            Point2::new(0.0, 0.0),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(polyline.closed);
        assert_eq!(polyline.width, 0.0);
        assert_eq!(polyline.vertices.len(), 4);
        assert!((area(&polyline) - 80.0).abs() < TOL);
        assert!((polyline.length() - 36.0).abs() < TOL);
    }

    #[test]
    fn chamfer_asimetrico_tiene_ocho_vertices_y_area_exacta() {
        let polyline = rectangle_polyline(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 8.0),
            Some((1.0, 2.0)),
            None,
            Some(0.75),
        )
        .unwrap();
        let points: Vec<_> = polyline.vertices.iter().map(|vertex| vertex.pt).collect();
        assert_eq!(
            points,
            [
                Point2::new(2.0, 0.0),
                Point2::new(9.0, 0.0),
                Point2::new(10.0, 2.0),
                Point2::new(10.0, 7.0),
                Point2::new(8.0, 8.0),
                Point2::new(1.0, 8.0),
                Point2::new(0.0, 6.0),
                Point2::new(0.0, 1.0),
            ]
        );
        assert!(polyline.vertices.iter().all(|vertex| vertex.bulge == 0.0));
        assert_eq!(polyline.width, 0.75);
        assert!((area(&polyline) - 76.0).abs() < TOL);
        let expected_perimeter = 24.0 + 4.0 * 5.0_f64.sqrt();
        assert!((polyline.length() - expected_perimeter).abs() < TOL);
    }

    #[test]
    fn fillet_usa_cuatro_bulges_y_area_analitica() {
        let radius = 2.0;
        let polyline = rectangle_polyline(
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 8.0),
            None,
            Some(radius),
            None,
        )
        .unwrap();
        let quarter_bulge = std::f64::consts::FRAC_PI_8.tan();
        assert_eq!(polyline.vertices.len(), 8);
        assert_eq!(
            polyline
                .vertices
                .iter()
                .filter(|vertex| (vertex.bulge - quarter_bulge).abs() < TOL)
                .count(),
            4
        );
        let expected_area = 80.0 - (4.0 - std::f64::consts::PI) * radius * radius;
        let expected_perimeter = 36.0 - 8.0 * radius + 2.0 * std::f64::consts::PI * radius;
        assert!((area(&polyline) - expected_area).abs() < TOL);
        assert!((polyline.length() - expected_perimeter).abs() < TOL);
    }

    #[test]
    fn rechaza_dimensiones_solapes_y_width_no_finito() {
        assert!(
            rectangle_polyline(Point2::ORIGIN, Point2::new(0.0, 8.0), None, Some(1.0), None,)
                .is_err()
        );
        assert!(
            rectangle_polyline(
                Point2::ORIGIN,
                Point2::new(10.0, 8.0),
                Some((4.0, 4.0)),
                None,
                None,
            )
            .is_err()
        );
        assert!(
            rectangle_polyline(
                Point2::ORIGIN,
                Point2::new(10.0, 8.0),
                None,
                Some(4.0),
                None,
            )
            .is_err()
        );
        assert!(
            rectangle_polyline(
                Point2::ORIGIN,
                Point2::new(10.0, 8.0),
                None,
                None,
                Some(f64::INFINITY),
            )
            .is_err()
        );
    }
}
