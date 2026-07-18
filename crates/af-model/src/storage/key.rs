//! [`GeoKind`] and [`EntityKey`] identify entries in drawing order.
//!
//! Each key combines the geometry kind that selects a pool with the physical
//! handle inside that pool. Keys are runtime storage references, not serialized
//! identities.

use super::pool::Handle;
use crate::entity::EntityGeometry;

/// Fieldless discriminator that selects the typed pool for an entity geometry.
///
/// [`GeoKind::of`] uses an exhaustive match so adding an `EntityGeometry`
/// variant requires adding its corresponding kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum GeoKind {
    /// Line segment.
    Line,
    /// Single point.
    Point,
    /// Circle.
    Circle,
    /// Circular arc.
    Arc,
    /// Ellipse or elliptical arc.
    Ellipse,
    /// Polyline made of line and arc segments.
    Polyline,
    /// Infinite construction line.
    Xline,
    /// Infinite ray.
    Ray,
    /// Cubic spline interpolated through fit points.
    Spline,
    /// Masking polygon.
    Wipeout,
}

impl GeoKind {
    /// Returns the typed pool kind for a geometry value.
    ///
    /// The exhaustive match keeps this mapping synchronized with `EntityGeometry`.
    pub(crate) fn of(geometry: &EntityGeometry) -> Self {
        match geometry {
            EntityGeometry::Line(_) => GeoKind::Line,
            EntityGeometry::Point(_) => GeoKind::Point,
            EntityGeometry::Circle(_) => GeoKind::Circle,
            EntityGeometry::Arc(_) => GeoKind::Arc,
            EntityGeometry::Ellipse(_) => GeoKind::Ellipse,
            EntityGeometry::Polyline(_) => GeoKind::Polyline,
            EntityGeometry::Xline(_) => GeoKind::Xline,
            EntityGeometry::Ray(_) => GeoKind::Ray,
            EntityGeometry::Spline(_) => GeoKind::Spline,
            EntityGeometry::Wipeout(_) => GeoKind::Wipeout,
        }
    }
}

/// Drawing-order reference containing a pool kind and physical slot handle.
///
/// The handle is process-local; [`EntityId`](crate::id::EntityId) provides
/// persistent identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EntityKey {
    /// Geometry kind that selects the pool.
    pub(crate) kind: GeoKind,
    /// Physical slot in that pool.
    pub(crate) handle: Handle,
}

impl EntityKey {
    /// Creates a key from its kind and physical handle.
    pub(crate) fn new(kind: GeoKind, handle: Handle) -> Self {
        Self { kind, handle }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use af_math::Point2;

    use crate::entity::{LineGeo, PointGeo};

    #[test]
    fn of_maps_variants_to_kinds() {
        let line = EntityGeometry::Line(LineGeo::new(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)));
        let point = EntityGeometry::Point(PointGeo::new(Point2::new(2.0, 3.0)));
        assert_eq!(GeoKind::of(&line), GeoKind::Line);
        assert_eq!(GeoKind::of(&point), GeoKind::Point);
    }

    #[test]
    fn entity_key_carries_kind_and_handle() {
        let h = Handle {
            index: 4,
            generation: 2,
        };
        let k = EntityKey::new(GeoKind::Circle, h);
        assert_eq!(k.kind, GeoKind::Circle);
        assert_eq!(k.handle, h);
    }
}
