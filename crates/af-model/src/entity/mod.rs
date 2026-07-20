//! Entity base types: common properties plus concrete geometry.
//!
//! Each [`EntityRecord`] combines persistent common properties with one
//! [`EntityGeometry`]. Render and selection state stay outside the model.
//!
//! # Static geometry capabilities
//!
//! [`EntityGeometry`] is a flat serializable enum that dispatches [`EntityOps`]
//! through exhaustive matches without trait objects.
//!
//! # Exhaustive dispatch
//!
//! Adding a variant must break every unhandled match; do not add wildcard arms.
//!
//! # Serialization
//!
//! [`EntityGeometry`] uses internal `type` tagging with camelCase names.

mod arc;
mod circle;
mod ellipse;
mod line;
mod point;
mod polyline;
mod ray;
mod record;
mod spline;
mod style_props;
mod wipeout;
mod xline;

use af_math::{BBox, Point2, Tol, Transform2};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub use af_geom::bulge::ArcSeg;
pub use arc::ArcGeo;
pub use circle::CircleGeo;
pub use ellipse::EllipseGeo;
pub use line::LineGeo;
pub use point::PointGeo;
pub use polyline::{PolyVertex, PolylineGeo, SegKind};
pub use ray::RayGeo;
pub use record::EntityRecord;
pub use spline::SplineGeo;
pub use style_props::{AciColor, Color, ColorError, LineTypeRef, Lineweight};
pub use wipeout::WipeoutGeo;
pub use xline::XlineGeo;

/// Half-length used to materialize infinite entities for bounded consumers.
///
/// Finite proxies can affect zoom extents and should be filtered by that consumer.
pub const INFINITE_HALF_LEN: f64 = 1.0e9;

/// [`SnapPoint`] collection returned by [`EntityOps::snap_points`].
///
/// Four points fit inline; larger collections spill without semantic change.
pub type SnapVec = SmallVec<[SnapPoint; 4]>;

/// Snap-point kind.
///
/// Geometry declares the first kinds; selection algorithms calculate the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    /// Endpoint of an open entity.
    Endpoint,
    /// Midpoint of a segment or arc sweep.
    Midpoint,
    /// Center of a circle or arc.
    Center,
    /// Node of a point entity.
    Node,
    /// Circle or arc quadrant at 0, 90, 180, or 270 degrees.
    Quadrant,
    /// Text or block insertion point.
    Insertion,
    /// Exact intersection between two entities.
    Intersection,
    /// Perpendicular foot from the latest reference point to a curve.
    Perpendicular,
    /// Nearest projected point on a curve.
    Nearest,
    /// Tangency point from the latest reference point to a circle or arc.
    Tangent,
    /// Point on a line or arc extension aligned with the cursor.
    Extension,
    /// Centroid of a closed polyline.
    GeometricCenter,
}

/// Exact entity point used for snapping.
///
/// This is exact geometry derived from the entity definition, not a render
/// tessellation vertex or an arbitrary UI target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapPoint {
    /// Snap-point coordinates.
    pub point: Point2,
    /// Snap-point kind.
    pub kind: SnapKind,
}

impl SnapPoint {
    /// Creates a [`SnapPoint`].
    #[inline]
    #[must_use]
    pub fn new(point: Point2, kind: SnapKind) -> Self {
        Self { point, kind }
    }
}

/// Error returned when a transform cannot be represented by the geometry.
///
/// Lines and points support any affine transform. A circle rejects nonuniform
/// scaling because the result would be an ellipse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformError {
    /// The transform has nonuniform scale unsupported by this geometry.
    NonUniformScaleUnsupported,
}

impl core::fmt::Display for TransformError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            TransformError::NonUniformScaleUnsupported => {
                f.write_str("non-uniform scale is not supported for this geometry")
            }
        }
    }
}

impl std::error::Error for TransformError {}

/// Problem detected by [`EntityOps::validate`].
///
/// Lines and points only use [`GeomIssue::NonFinite`]. Circles may use
/// `DegenerateRadius`, ellipses may use `InvalidAxisRatio`, and polylines may
/// use `TooFewVertices` and `CoincidentVertices`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomIssue {
    /// A coordinate or scalar is `NaN` or infinite.
    NonFinite,
    /// Radius is at or below `tol.point_merge`.
    DegenerateRadius,
    /// An ellipse minor-to-major axis ratio is outside `(0, 1]`.
    InvalidAxisRatio,
    /// Fewer vertices than required; a polyline requires at least two.
    TooFewVertices,
    /// Consecutive vertices coincide within `tol.point_merge`.
    CoincidentVertices,
    /// Direction vector is at or below `tol.linear`.
    ZeroDirection,
}

impl core::fmt::Display for GeomIssue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            GeomIssue::NonFinite => "geometry contains a non-finite coordinate",
            GeomIssue::DegenerateRadius => "radius is below the merge tolerance",
            GeomIssue::InvalidAxisRatio => "ellipse axis ratio must be in (0, 1]",
            GeomIssue::TooFewVertices => "too few vertices for this geometry",
            GeomIssue::CoincidentVertices => "consecutive vertices coincide within tolerance",
            GeomIssue::ZeroDirection => "direction vector is null (line/ray has no direction)",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for GeomIssue {}

/// Geometry operations dispatched by concrete entity geometry.
///
/// Each geometry type and [`EntityGeometry`] implement this trait. Dispatch is
/// static through exhaustive enum matches.
pub trait EntityOps {
    /// Axis-aligned bounding box containing the geometry.
    ///
    /// Invariant: `bbox()` contains every point from `snap_points()`.
    fn bbox(&self) -> BBox;

    /// Applies an affine transform and returns the transformed geometry.
    ///
    /// # Errors
    ///
    /// Returns [`TransformError`] when the geometry cannot represent the
    /// transform. Lines and points never fail.
    fn transform(&self, t: &Transform2) -> Result<Self, TransformError>
    where
        Self: Sized;

    /// Distance from `p` to the geometry, or `None` when it exceeds `tol`.
    fn hit(&self, p: Point2, tol: f64) -> Option<f64>;

    /// Exact snap points for the geometry.
    fn snap_points(&self) -> SnapVec;

    /// Validates the geometry.
    ///
    /// # Errors
    ///
    /// Returns the first [`GeomIssue`] found.
    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue>;
}

/// Concrete entity geometry stored as a flat, closed enum.
///
/// Adding a variant requires extending every exhaustive match in this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntityGeometry {
    /// Straight segment between two points.
    Line(LineGeo),
    /// Single point or node.
    Point(PointGeo),
    /// Circle defined by a center and radius.
    Circle(CircleGeo),
    /// Circular arc with counterclockwise DXF ARC semantics.
    Arc(ArcGeo),
    /// Ellipse or elliptical arc with DXF ELLIPSE semantics.
    Ellipse(EllipseGeo),
    /// Polyline with straight and bulged segments.
    Polyline(PolylineGeo),
    /// Infinite construction line with DXF XLINE semantics.
    Xline(XlineGeo),
    /// Forward infinite ray with DXF RAY semantics.
    Ray(RayGeo),
    /// Cubic interpolating spline defined by fit points.
    Spline(SplineGeo),
    /// Closed masking polygon that hides geometry beneath it in draw order.
    Wipeout(WipeoutGeo),
}

impl EntityOps for EntityGeometry {
    fn bbox(&self) -> BBox {
        // Exhaustive by design: adding a variant must break this match.
        match self {
            EntityGeometry::Line(g) => g.bbox(),
            EntityGeometry::Point(g) => g.bbox(),
            EntityGeometry::Circle(g) => g.bbox(),
            EntityGeometry::Arc(g) => g.bbox(),
            EntityGeometry::Ellipse(g) => g.bbox(),
            EntityGeometry::Polyline(g) => g.bbox(),
            EntityGeometry::Xline(g) => g.bbox(),
            EntityGeometry::Ray(g) => g.bbox(),
            EntityGeometry::Spline(g) => g.bbox(),
            EntityGeometry::Wipeout(g) => g.bbox(),
        }
    }

    fn transform(&self, t: &Transform2) -> Result<Self, TransformError> {
        Ok(match self {
            EntityGeometry::Line(g) => EntityGeometry::Line(g.transform(t)?),
            EntityGeometry::Point(g) => EntityGeometry::Point(g.transform(t)?),
            EntityGeometry::Circle(g) => EntityGeometry::Circle(g.transform(t)?),
            EntityGeometry::Arc(g) => EntityGeometry::Arc(g.transform(t)?),
            EntityGeometry::Ellipse(g) => EntityGeometry::Ellipse(g.transform(t)?),
            EntityGeometry::Polyline(g) => EntityGeometry::Polyline(g.transform(t)?),
            EntityGeometry::Xline(g) => EntityGeometry::Xline(g.transform(t)?),
            EntityGeometry::Ray(g) => EntityGeometry::Ray(g.transform(t)?),
            EntityGeometry::Spline(g) => EntityGeometry::Spline(g.transform(t)?),
            EntityGeometry::Wipeout(g) => EntityGeometry::Wipeout(g.transform(t)?),
        })
    }

    fn hit(&self, p: Point2, tol: f64) -> Option<f64> {
        match self {
            EntityGeometry::Line(g) => g.hit(p, tol),
            EntityGeometry::Point(g) => g.hit(p, tol),
            EntityGeometry::Circle(g) => g.hit(p, tol),
            EntityGeometry::Arc(g) => g.hit(p, tol),
            EntityGeometry::Ellipse(g) => g.hit(p, tol),
            EntityGeometry::Polyline(g) => g.hit(p, tol),
            EntityGeometry::Xline(g) => g.hit(p, tol),
            EntityGeometry::Ray(g) => g.hit(p, tol),
            EntityGeometry::Spline(g) => g.hit(p, tol),
            EntityGeometry::Wipeout(g) => g.hit(p, tol),
        }
    }

    fn snap_points(&self) -> SnapVec {
        match self {
            EntityGeometry::Line(g) => g.snap_points(),
            EntityGeometry::Point(g) => g.snap_points(),
            EntityGeometry::Circle(g) => g.snap_points(),
            EntityGeometry::Arc(g) => g.snap_points(),
            EntityGeometry::Ellipse(g) => g.snap_points(),
            EntityGeometry::Polyline(g) => g.snap_points(),
            EntityGeometry::Xline(g) => g.snap_points(),
            EntityGeometry::Ray(g) => g.snap_points(),
            EntityGeometry::Spline(g) => g.snap_points(),
            EntityGeometry::Wipeout(g) => g.snap_points(),
        }
    }

    fn validate(&self, tol: &Tol) -> Result<(), GeomIssue> {
        match self {
            EntityGeometry::Line(g) => g.validate(tol),
            EntityGeometry::Point(g) => g.validate(tol),
            EntityGeometry::Circle(g) => g.validate(tol),
            EntityGeometry::Arc(g) => g.validate(tol),
            EntityGeometry::Ellipse(g) => g.validate(tol),
            EntityGeometry::Polyline(g) => g.validate(tol),
            EntityGeometry::Xline(g) => g.validate(tol),
            EntityGeometry::Ray(g) => g.validate(tol),
            EntityGeometry::Spline(g) => g.validate(tol),
            EntityGeometry::Wipeout(g) => g.validate(tol),
        }
    }
}
