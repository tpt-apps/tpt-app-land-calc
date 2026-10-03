//! Pro edition spatial analysis: buffer zones and overlays over the
//! tpt-gis primitives, plus the shapefile batch importer. All results are
//! planar metres/m² — the same projection the free edition measures in.

use crate::input::{CoordMode, EngineError, Frame, NamedParcel};
use tpt_gis::{buffer_polygon, intersection_area, intersection_fragments, read_shapefile, Polygon, Ring, ShpShape};

/// What a shapefile load produced: parcels plus the shared frame (mirrors
/// [`crate::input::GeoJsonLoad`]).
#[derive(Debug, Clone)]
pub struct ShapefileLoad {
    pub parcels: Vec<NamedParcel>,
    pub coord_mode: CoordMode,
    pub frame: Frame,
}

/// A buffered parcel: the offset polygon itself plus the area it adds
/// (negative for erosion).
#[derive(Debug, Clone)]
pub struct BufferResult {
    pub polygon: Polygon,
    pub area_m2: f64,
    /// `buffered.area − original.area`.
    pub added_m2: f64,
}

/// Buffers a parcel by `distance_m` (positive grows, negative erodes).
pub fn buffer_parcel(polygon: &Polygon, distance_m: f64) -> Result<BufferResult, EngineError> {
    let buffered = buffer_polygon(polygon, distance_m).ok_or_else(|| {
        EngineError::Parse(format!(
            "buffer of {distance_m} m consumes this parcel entirely — use a smaller distance"
        ))
    })?;
    Ok(BufferResult {
        area_m2: buffered.area(),
        added_m2: buffered.area() - polygon.area(),
        polygon: buffered,
    })
}

/// The overlay of an analysis zone over a parcel: net intersection area,
/// its share of the parcel, and the drawable intersection fragments.
#[derive(Debug, Clone)]
pub struct OverlayResult {
    pub area_m2: f64,
    /// `area / parcel.area`, 0–1 (0 when the parcel has no area).
    pub share: f64,
    /// Fragment outlines (parcel-exterior ∩ zone) in planar metres, for the
    /// map.
    pub fragments: Vec<Vec<(f64, f64)>>,
}

/// Intersects `zone` (e.g. a setback polygon or hazard area) with `parcel`.
pub fn overlay_parcel(parcel: &Polygon, zone: &Polygon) -> OverlayResult {
    let area = intersection_area(parcel, zone);
    let parcel_area = parcel.area();
    let share = if parcel_area > 1e-9 { area / parcel_area } else { 0.0 };
    OverlayResult {
        area_m2: area,
        share,
        fragments: intersection_fragments(&parcel.exterior, zone)
            .into_iter()
            .map(|frag| frag.into_iter().map(|p| (p.x, p.y)).collect())
            .collect(),
    }
}

/// Loads parcels from `.shp` file contents (geometry-only reader: types 1
/// and 5). Coordinates are projected per `mode_hint` (the UI's format
/// selector) or, on auto, if they read as lat/lon — same rule as the
/// GeoJSON loader.
pub fn load_shapefile_parcels(
    bytes: &[u8],
    mode_hint: Option<CoordMode>,
) -> Result<ShapefileLoad, EngineError> {
    let shapes =
        read_shapefile(bytes).map_err(|e| EngineError::Parse(format!("shapefile: {e}")))?;
    let raw: Vec<(String, Polygon)> = shapes
        .into_iter()
        .filter_map(|shape| match shape {
            ShpShape::Polygon { name, polygon } => Some((name, polygon)),
            ShpShape::Point(_) => None,
        })
        .collect();
    if raw.is_empty() {
        return Err(EngineError::Parse(
            "shapefile contains no polygon records to measure".to_string(),
        ));
    }
    let all_values: Vec<(f64, f64)> = raw
        .iter()
        .flat_map(|(_, p)| {
            p.exterior
                .points
                .iter()
                .chain(p.holes.iter().flat_map(|h| h.points.iter()))
                .map(|pt| (pt.x, pt.y))
        })
        .collect();
    let frame = Frame::choose(mode_hint, &all_values)?;
    let coord_mode = match frame {
        Frame::Planar => CoordMode::PlanarMeters,
        Frame::Local(_) => CoordMode::LatLon,
    };
    let parcels = raw
        .into_iter()
        .map(|(name, polygon)| NamedParcel {
            name,
            polygon: in_frame(&polygon, &frame),
        })
        .collect();
    Ok(ShapefileLoad { parcels, coord_mode, frame })
}

/// Maps one polygon into the shared planar frame.
fn in_frame(polygon: &Polygon, frame: &Frame) -> Polygon {
    if matches!(frame, Frame::Planar) {
        return polygon.clone();
    }
    let map_ring = |ring: &Ring| -> Ring {
        let pts: Vec<(f64, f64)> = ring.points.iter().map(|p| (p.x, p.y)).collect();
        frame.project_ring(&pts)
    };
    Polygon {
        exterior: map_ring(&polygon.exterior),
        holes: polygon.holes.iter().map(&map_ring).collect(),
    }
}
