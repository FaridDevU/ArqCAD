//! [`SlotFill`] implementations for geometry stored in typed pools.
//!
//! Fill values are unobservable and cheap to construct. Fixed-size geometry uses
//! zeroed coordinates, while vector-backed geometry uses empty vectors so removing
//! a slot does not retain its previous allocation.

use af_math::{Point2, Vec2};

use super::pool::SlotFill;
use crate::entity::{
    ArcGeo, CircleGeo, EllipseGeo, LineGeo, PointGeo, PolylineGeo, RayGeo, SplineGeo, WipeoutGeo,
    XlineGeo,
};

impl SlotFill for LineGeo {
    fn slot_fill() -> Self {
        LineGeo::new(Point2::default(), Point2::default())
    }
}

impl SlotFill for PointGeo {
    fn slot_fill() -> Self {
        PointGeo::new(Point2::default())
    }
}

impl SlotFill for CircleGeo {
    fn slot_fill() -> Self {
        CircleGeo::new(Point2::default(), 0.0)
    }
}

impl SlotFill for ArcGeo {
    fn slot_fill() -> Self {
        ArcGeo::new(Point2::default(), 0.0, 0.0, 0.0)
    }
}

impl SlotFill for EllipseGeo {
    fn slot_fill() -> Self {
        EllipseGeo::new(Point2::default(), 0.0, 0.0, 0.0, 0.0, 0.0)
    }
}

impl SlotFill for XlineGeo {
    fn slot_fill() -> Self {
        XlineGeo::new(Point2::default(), Vec2::default())
    }
}

impl SlotFill for RayGeo {
    fn slot_fill() -> Self {
        RayGeo::new(Point2::default(), Vec2::default())
    }
}

impl SlotFill for PolylineGeo {
    fn slot_fill() -> Self {
        // An empty vector avoids allocation and drops the previous vertex buffer.
        PolylineGeo::new(Vec::new(), false)
    }
}

impl SlotFill for SplineGeo {
    fn slot_fill() -> Self {
        SplineGeo::new(Vec::new(), false)
    }
}

impl SlotFill for WipeoutGeo {
    fn slot_fill() -> Self {
        WipeoutGeo::new(Vec::new())
    }
}
