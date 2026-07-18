//! Built-in history, drawing, editing, layer, property, group, visibility, system,
//! and query commands.
//!
//! [`register_builtins`] registers every command; submodules expose specifications
//! or registration functions for selective use.

mod edit_common;
mod report;

pub mod addselected;
pub mod align;
pub mod arc;
pub mod array;
pub mod audit;
pub mod break_cmd;
pub mod chamfer;
pub mod chprop;
pub mod circle;
pub mod clayer;
pub mod color_cmd;
pub mod copy;
pub mod donut;
mod draw;
pub mod ellipse;
pub mod erase;
pub mod explode;
pub mod group;
pub mod isolate;
pub mod join;
pub mod lay_bulk;
pub mod lay_object;
pub mod layer_cmd;
pub mod layiso;
pub mod lengthen;
pub mod limits_cmd;
pub mod line;
pub mod line_props;
pub mod matchprop;
pub mod mirror;
pub mod modify;
pub mod move_cmd;
pub mod nudge;
pub mod oops;
pub mod overkill;
pub mod pline;
pub mod point;
pub mod polygon;
pub mod purge;
pub mod query;
pub mod rectang;
pub mod rename_cmd;
pub mod revcloud;
pub mod reverse;
pub mod rotate;
pub mod scale;
pub mod setbylayer;
pub mod setvar;
pub mod spline;
pub mod stretch;
pub(crate) mod style_value;
pub mod undo_redo;
pub mod units_cmd;
pub mod wipeout;
pub mod xline;

use crate::registry::{CommandRegistry, RegisterError};

/// Registers every built-in command in `registry`.
///
/// # Errors
/// Returns [`RegisterError`] on any case-insensitive name or alias collision.
pub fn register_builtins(registry: &mut CommandRegistry) -> Result<(), RegisterError> {
    undo_redo::register_builtins(registry)?;
    registry.register(line::line_spec())?;
    registry.register(move_cmd::move_spec())?;
    registry.register(erase::erase_spec())?;
    registry.register(copy::copy_spec())?;
    registry.register(rotate::rotate_spec())?;
    registry.register(scale::scale_spec())?;
    registry.register(mirror::mirror_spec())?;
    registry.register(circle::circle_spec())?;
    registry.register(arc::arc_spec())?;
    registry.register(ellipse::ellipse_spec())?;
    registry.register(pline::pline_spec())?;
    registry.register(spline::spline_spec())?;
    registry.register(rectang::rectang_spec())?;
    registry.register(polygon::polygon_spec())?;
    registry.register(point::point_spec())?;
    registry.register(wipeout::wipeout_spec())?;
    registry.register(xline::xline_spec())?;
    registry.register(xline::ray_spec())?;
    registry.register(donut::donut_spec())?;
    registry.register(revcloud::revcloud_spec())?;
    registry.register(layer_cmd::layer_spec())?;
    registry.register(clayer::clayer_spec())?;
    registry.register(color_cmd::color_spec())?;
    line_props::register(registry)?;
    registry.register(matchprop::matchprop_spec())?;
    registry.register(chprop::chprop_spec())?;
    registry.register(stretch::stretch_spec())?;
    registry.register(array::array_spec())?;
    registry.register(explode::explode_spec())?;
    modify::register(registry)?;
    // Geometric editing.
    chamfer::register(registry)?;
    break_cmd::register(registry)?;
    join::register(registry)?;
    lengthen::register(registry)?;
    align::register(registry)?;
    // Editing utilities.
    registry.register(addselected::addselected_spec())?;
    registry.register(oops::oops_spec())?;
    registry.register(overkill::overkill_spec())?;
    registry.register(setbylayer::setbylayer_spec())?;
    registry.register(reverse::reverse_spec())?;
    registry.register(nudge::nudge_spec())?;
    // Object-driven layer tools.
    lay_object::register(registry)?;
    layiso::register(registry)?;
    lay_bulk::register(registry)?;
    rename_cmd::register(registry)?;
    // Groups and visibility.
    group::register(registry)?;
    isolate::register(registry)?;
    // System and query commands.
    setvar::register(registry)?;
    units_cmd::register(registry)?;
    limits_cmd::register(registry)?;
    purge::register(registry)?;
    audit::register(registry)?;
    query::register(registry)?;
    Ok(())
}
