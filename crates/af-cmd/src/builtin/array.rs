//! ARRAY (`AR`) creates copies of an entity set in one transaction.
//!
//! - `rect` creates a signed-spacing `rows` by `cols` grid without duplicating the
//!   original cell.
//! - `polar` distributes `items`, including the original, over `angle` around
//!   `center`. `rotate=false` preserves each copy's orientation.
//!
//! Full rotations use `angle/items`; partial sweeps use `angle/(items-1)` so the
//! final copy lands on the sweep endpoint.
//!
//! Every copy receives a new ID, inherits entity properties, and requires editable
//! model-space sources. Only `rect` and `polar` modes are supported.

use core::f64::consts::TAU;

use af_math::{BBox, Point2, Transform2, Vec2};
use af_model::entity::{EntityOps, EntityRecord};
use af_model::id::EntityId;
use af_model::{ContainerRef, TxContext};
use serde_json::json;

use crate::args::ParsedArgs;
use crate::builtin::edit_common::validate_editable;
use crate::registry::{CommandRegistry, RegisterError};
use crate::spec::{CmdError, CommandCtx, CommandOutcome, CommandSpec, ParamSpec, ParamType};

/// Tolerance for recognizing a full polar revolution.
const FULL_TURN_EPS: f64 = 1e-9;

/// A resolved array plan using radians internally.
pub(crate) enum ArrayMode {
    /// A `rows` by `cols` grid with `(dx, dy)` spacing.
    Rect {
        rows: u64,
        cols: u64,
        dx: f64,
        dy: f64,
    },
    /// `items` distributed over `angle` around `center`.
    Polar {
        center: Point2,
        items: u64,
        angle: f64,
        rotate: bool,
    },
}

/// Returns the ARRAY specification with alias `AR`.
///
/// `rect` uses rows, columns, and spacing; `polar` uses center, items, angle, and rotate.
#[must_use]
pub fn array_spec() -> CommandSpec {
    CommandSpec::new("ARRAY", "Array", true, array_exec)
        .alias("AR")
        .param(ParamSpec::required("entities", ParamType::EntitySet))
        .param(ParamSpec::required(
            "mode",
            ParamType::Enum(vec!["rect".to_string(), "polar".to_string()]),
        ))
        // Rectangular mode.
        .param(ParamSpec::with_default("rows", ParamType::Count, json!(1)))
        .param(ParamSpec::with_default("cols", ParamType::Count, json!(1)))
        .param(ParamSpec::with_default(
            "spacing",
            ParamType::Point,
            json!([1.0, 1.0]),
        ))
        // Polar mode.
        .param(ParamSpec::with_default(
            "center",
            ParamType::Point,
            json!([0.0, 0.0]),
        ))
        .param(ParamSpec::with_default("items", ParamType::Count, json!(1)))
        .param(ParamSpec::with_default(
            "angle",
            ParamType::Angle,
            json!(TAU),
        ))
        .param(ParamSpec::with_default(
            "rotate",
            ParamType::Flag,
            json!(true),
        ))
}

/// Registers ARRAY.
///
/// # Errors
/// Returns [`RegisterError`] on a name or alias collision.
pub fn register(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    registry.register(array_spec())
}

/// Resolves ARRAY mode and creates all copies in one transaction.
fn array_exec(ctx: &mut CommandCtx<'_>, args: ParsedArgs) -> Result<CommandOutcome, CmdError> {
    let ids: Vec<EntityId> = args
        .entity_set("entities")
        .ok_or_else(|| CmdError::MissingParam("entities".to_string()))?
        .to_vec();
    let mode = args
        .enum_value("mode")
        .ok_or_else(|| CmdError::MissingParam("mode".to_string()))?;

    let plan = match mode {
        "rect" => {
            let rows = args.count("rows").unwrap_or(1);
            let cols = args.count("cols").unwrap_or(1);
            let sp = args.point("spacing").unwrap_or(Point2::new(1.0, 1.0));
            if rows.saturating_mul(cols) < 2 {
                return Err(CmdError::Failed(
                    "ARRAY rect: rows·cols debe ser ≥ 2 para crear alguna copia".to_string(),
                ));
            }
            ArrayMode::Rect {
                rows,
                cols,
                dx: sp.x,
                dy: sp.y,
            }
        }
        "polar" => {
            let center = args.point("center").unwrap_or(Point2::ORIGIN);
            let items = args.count("items").unwrap_or(1);
            let angle = args.angle("angle").unwrap_or(TAU);
            let rotate = args.flag("rotate");
            if items < 2 {
                return Err(CmdError::Failed(
                    "ARRAY polar: items debe ser ≥ 2 para crear alguna copia".to_string(),
                ));
            }
            ArrayMode::Polar {
                center,
                items,
                angle,
                rotate,
            }
        }
        // Keep a defensive error even though registry validation restricts `mode`.
        other => {
            return Err(CmdError::Failed(format!(
                "ARRAY: modo desconocido '{other}'"
            )));
        }
    };

    let created = ctx.transact("Array", |tx| apply_array(tx, &ids, &plan))?;
    Ok(CommandOutcome::created(created))
}

/// Applies an array atomically after validating every source entity.
pub(crate) fn apply_array(
    tx: &mut TxContext<'_>,
    ids: &[EntityId],
    mode: &ArrayMode,
) -> Result<Vec<EntityId>, CmdError> {
    let records = validate_editable(tx, "ARRAY", ids)?;
    let transforms = build_transforms(mode, &records);

    let mut created = Vec::with_capacity(transforms.len() * records.len());
    for t in &transforms {
        for (id, record) in &records {
            let mut copy = record.clone();
            copy.geometry = copy.geometry.transform(t).map_err(|e| {
                CmdError::Failed(format!(
                    "ARRAY: entity {} cannot be arrayed: {e}",
                    id.raw().0
                ))
            })?;
            created.push(tx.add_entity(ContainerRef::ModelSpace, copy)?);
        }
    }
    Ok(created)
}

/// Returns one transform per copy, excluding the original.
fn build_transforms(mode: &ArrayMode, records: &[(EntityId, EntityRecord)]) -> Vec<Transform2> {
    match mode {
        ArrayMode::Rect { rows, cols, dx, dy } => {
            let mut out = Vec::with_capacity((rows * cols).saturating_sub(1) as usize);
            for r in 0..*rows {
                for c in 0..*cols {
                    if r == 0 && c == 0 {
                        continue; // Original cell.
                    }
                    out.push(Transform2::translate(Vec2::new(
                        c as f64 * *dx,
                        r as f64 * *dy,
                    )));
                }
            }
            out
        }
        ArrayMode::Polar {
            center,
            items,
            angle,
            rotate,
        } => {
            let n = *items;
            // Partial sweeps include both endpoints; full sweeps avoid overlap.
            let step = if angle.abs() >= TAU - FULL_TURN_EPS {
                angle / n as f64
            } else {
                angle / (n - 1) as f64
            };
            // Without rotation, orbit the set's bounds center using translation only.
            let base_ref = union_center(records);
            (1..n)
                .map(|k| {
                    let theta = k as f64 * step;
                    if *rotate {
                        Transform2::rotate_about(theta, *center)
                    } else {
                        let orbited = Transform2::rotate_about(theta, *center).apply(base_ref);
                        Transform2::translate(orbited - base_ref)
                    }
                })
                .collect()
        }
    }
}

/// Returns the combined bounds center used by nonrotating polar arrays.
fn union_center(records: &[(EntityId, EntityRecord)]) -> Point2 {
    records
        .iter()
        .map(|(_, r)| r.geometry.bbox())
        .reduce(BBox::union)
        .map_or(Point2::ORIGIN, |b| b.center())
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_model::entity::{Color, EntityGeometry, EntityRecord, LineGeo, LineTypeRef, Lineweight};
    use af_model::id::ObjectId;
    use af_model::units::Units;
    use af_model::{Session, TxError};
    use core::f64::consts::{FRAC_PI_2, TAU};

    fn seed_unit_line(session: &mut Session) -> Vec<EntityId> {
        let layer = session.document().current_layer();
        session
            .transact("seed", |tx| -> Result<Vec<EntityId>, TxError> {
                Ok(vec![tx.add_entity(
                    ContainerRef::ModelSpace,
                    EntityRecord::new(
                        ObjectId::NIL.into(),
                        layer,
                        Color::ByLayer,
                        LineTypeRef::ByLayer,
                        Lineweight::ByLayer,
                        EntityGeometry::Line(LineGeo::new(Point2::ORIGIN, Point2::new(1.0, 0.0))),
                    ),
                )?])
            })
            .expect("seed")
            .value
    }

    fn line_of(session: &Session, id: EntityId) -> LineGeo {
        match &session.document().entity(id).unwrap().0.geometry {
            EntityGeometry::Line(g) => *g,
            other => panic!("esperaba línea, fue {other:?}"),
        }
    }

    #[test]
    fn rect_creates_grid_minus_origin() {
        let mut session = Session::new(Units::default());
        let ids = seed_unit_line(&mut session);
        let plan = ArrayMode::Rect {
            rows: 2,
            cols: 3,
            dx: 10.0,
            dy: 20.0,
        };
        let created = session
            .transact("Array", |tx| apply_array(tx, &ids, &plan))
            .expect("commits")
            .value;
        assert_eq!(created.len(), 5);
        let found = created.iter().any(|&id| {
            let l = line_of(&session, id);
            l.p1 == Point2::new(20.0, 20.0) && l.p2 == Point2::new(21.0, 20.0)
        });
        assert!(found, "falta la copia de la celda (1,2)");
    }

    #[test]
    fn rect_negative_spacing_goes_other_way() {
        let mut session = Session::new(Units::default());
        let ids = seed_unit_line(&mut session);
        let plan = ArrayMode::Rect {
            rows: 1,
            cols: 2,
            dx: -5.0,
            dy: 0.0,
        };
        let created = session
            .transact("Array", |tx| apply_array(tx, &ids, &plan))
            .expect("commits")
            .value;
        assert_eq!(created.len(), 1);
        assert_eq!(line_of(&session, created[0]).p1, Point2::new(-5.0, 0.0));
    }

    #[test]
    fn polar_full_turn_rotates_items_evenly() {
        let mut session = Session::new(Units::default());
        let ids = seed_unit_line(&mut session);
        let plan = ArrayMode::Polar {
            center: Point2::ORIGIN,
            items: 4,
            angle: TAU,
            rotate: true,
        };
        let created = session
            .transact("Array", |tx| apply_array(tx, &ids, &plan))
            .expect("commits")
            .value;
        assert_eq!(created.len(), 3, "4 items => 3 copias");
        let l = line_of(&session, created[0]);
        let tol = 1e-9;
        assert!(l.p1.x.abs() < tol && l.p1.y.abs() < tol);
        assert!(l.p2.x.abs() < tol && (l.p2.y - 1.0).abs() < tol);
    }

    #[test]
    fn polar_partial_uses_items_minus_one_step() {
        let mut session = Session::new(Units::default());
        let ids = seed_unit_line(&mut session);
        let plan = ArrayMode::Polar {
            center: Point2::ORIGIN,
            items: 3,
            angle: FRAC_PI_2,
            rotate: true,
        };
        let created = session
            .transact("Array", |tx| apply_array(tx, &ids, &plan))
            .expect("commits")
            .value;
        assert_eq!(created.len(), 2);
        let l = line_of(&session, created[1]);
        let tol = 1e-9;
        assert!(l.p2.x.abs() < tol && (l.p2.y - 1.0).abs() < tol);
    }

    #[test]
    fn polar_no_rotate_translates_without_turning() {
        let mut session = Session::new(Units::default());
        let ids = seed_unit_line(&mut session);
        let plan = ArrayMode::Polar {
            center: Point2::new(0.5, 5.0),
            items: 2,
            angle: TAU,
            rotate: false,
        };
        let created = session
            .transact("Array", |tx| apply_array(tx, &ids, &plan))
            .expect("commits")
            .value;
        assert_eq!(created.len(), 1);
        let l = line_of(&session, created[0]);
        let tol = 1e-9;
        assert!(
            (l.p1 - Point2::new(0.0, 10.0)).norm() < tol,
            "p1 trasladado"
        );
        assert!(
            (l.p2 - Point2::new(1.0, 10.0)).norm() < tol,
            "p2 trasladado, sin girar"
        );
    }
}
