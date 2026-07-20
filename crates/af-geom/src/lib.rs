#![forbid(unsafe_code)]
//! Two-dimensional geometry kernel for arcs, polyline bulges, distances,
//! intersections, and chord-error flattening. Depends only on af-math.

pub mod arc;
pub mod area;
pub mod bulge;
pub mod circle;
pub mod ellipse;
pub mod flatten;
pub mod intersect;
pub mod nurbs;
pub mod offset;
pub mod polygon;
pub mod project;
pub mod revcloud;
pub mod tangent;

pub use arc::arc_bbox;
pub use area::{bulge_segment_area, closed_polyline_signed_area, polygon_signed_area};
pub use bulge::{
    ArcSeg, BulgeError, arc_to_bulge, bulge_to_arc, seg_angle_fraction, seg_point_at,
    split_bulge_segment,
};
pub use circle::circumcircle;
pub use ellipse::Ellipse;
pub use intersect::{
    Hit, LineX, SegGeom, arc_arc, circle_arc, circle_circle, line_arc, line_circle, line_line,
    resolve_poly_seg, seg_seg,
};
pub use nurbs::FitSpline;
pub use offset::{OffsetError, offset_arc, offset_circle, offset_line, offset_polyline};
pub use polygon::{rectangle_vertices, regular_polygon_vertices};
pub use project::{
    nearest_on_arc, nearest_on_segment, perp_foot_line, polygon_centroid, project_on_circle,
    tangent_points,
};
pub use revcloud::revcloud_vertices;
pub use tangent::{TangentCurve, tangent_circle_centers, tangent_contact_point, tangent_point_on};
