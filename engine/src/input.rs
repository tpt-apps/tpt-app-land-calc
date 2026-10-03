//! Boundary input: free-text point lists and GeoJSON parcel files, plus the
//! single-parcel measurement. Coordinate handling: GPS lat/lon pairs are
//! projected to planar metres with a local equirectangular projection
//! anchored at the parcel's mean position; planar metres pass straight
//! through. All engine outputs are metres/m².

use tpt_gis::{parse_geojson, GeoGeometry, LocalProjection, Point, Polygon, Ring};

/// How raw coordinates should be interpreted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoordMode {
    /// WGS84 degrees; projected to metres before measuring.
    LatLon,
    /// Already planar metres — measured as-is.
    PlanarMeters,
}

impl CoordMode {
    /// Guesses from the value ranges: lat/lon pairs sit inside ±90/±180.
    /// Returns `None` when the values are out of range for both readings.
    pub fn detect(values: &[(f64, f64)]) -> Option<CoordMode> {
        if values.is_empty() {
            return None;
        }
        let plausible = values
            .iter()
            .all(|(a, b)| a.abs() <= 90.0 && b.abs() <= 180.0);
        Some(if plausible {
            CoordMode::LatLon
        } else {
            CoordMode::PlanarMeters
        })
    }
}

#[derive(Debug)]
pub enum EngineError {
    /// Fewer than 3 usable boundary points.
    NotEnoughPoints(usize),
    /// A line of the pasted point list that could not be read.
    BadPointLine { line_no: usize, text: String },
    /// File-level parse failure with a human-readable message.
    Parse(String),
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EngineError::NotEnoughPoints(n) => {
                write!(f, "a parcel needs at least 3 boundary points, got {n}")
            }
            EngineError::BadPointLine { line_no, text } => {
                write!(f, "line {line_no}: cannot read {text:?} — expected \"lat, lon\" or \"x, y\"")
            }
            EngineError::Parse(message) => write!(f, "{message}"),
        }
    }
}

/// The shared planar frame a set of geometries lives in. Every load in one
/// analysis must map its coordinates through the *same* frame — a frame
/// anchors the lat/lon projection, so mixing frames shifts geometries
/// relative to each other and corrupts overlays.
#[derive(Debug, Clone)]
pub enum Frame {
    /// Coordinates are already planar metres — pass through.
    Planar,
    /// Coordinates are WGS84 degrees; project through this anchor.
    Local(LocalProjection),
}

impl Frame {
    /// Chooses a frame from the UI's format selector (`hint`) or, on auto,
    /// coordinate-range detection.
    pub fn choose(hint: Option<CoordMode>, all_values: &[(f64, f64)]) -> Result<Frame, EngineError> {
        match hint.or_else(|| CoordMode::detect(all_values)) {
            Some(CoordMode::PlanarMeters) => Ok(Frame::Planar),
            Some(CoordMode::LatLon) => Ok(Frame::Local(LocalProjection::for_points(all_values))),
            None => Err(EngineError::Parse(
                "coordinates are outside both lat/lon and planar-metre ranges — set the format selector to planar metres"
                    .to_string(),
            )),
        }
    }

    /// Maps raw coordinate pairs into this frame as a ring (closing
    /// duplicate dropped).
    pub fn project_ring(&self, points: &[(f64, f64)]) -> Ring {
        match self {
            Frame::Planar => ring_from_points(points, CoordMode::PlanarMeters),
            Frame::Local(proj) => {
                let mut pts: Vec<Point> =
                    points.iter().map(|(lat, lon)| proj.project(*lat, *lon)).collect();
                if pts.len() > 1 && pts[0].distance_to(pts[pts.len() - 1]) < 1e-9 {
                    pts.pop();
                }
                Ring::new(pts)
            }
        }
    }
}

/// One named parcel in planar metres, ready to measure or draw.
#[derive(Debug, Clone)]
pub struct NamedParcel {
    pub name: String,
    pub polygon: Polygon,
}

/// Everything a results panel shows for one parcel.
#[derive(Debug, Clone)]
pub struct ParcelAnalysis {
    pub name: String,
    /// Net area in m² (exterior minus holes).
    pub area_m2: f64,
    /// Exterior boundary length in m.
    pub perimeter_m: f64,
    /// Area centroid (planar metres).
    pub centroid: (f64, f64),
    pub vertex_count: usize,
    pub hole_count: usize,
}

/// Reads a pasted point list: one `x, y` (or `lat, lon`) pair per line,
/// separated by comma / semicolon / whitespace. Blank lines and `#`
/// comments are skipped; a trailing non-numeric label on a line is ignored.
pub fn parse_points_text(text: &str) -> Result<Vec<(f64, f64)>, EngineError> {
    let mut points = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts: Vec<&str> = line
            .split([',', ';', '\t', ' '])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect();
        // Drop a trailing text label like "-43.53, 172.63 paddock 2".
        if parts.len() > 2 && parts[..2].iter().all(|p| p.parse::<f64>().is_ok()) {
            parts.truncate(2);
        }
        if parts.len() < 2 {
            return Err(EngineError::BadPointLine {
                line_no: idx + 1,
                text: line.to_string(),
            });
        }
        let x = parts[0]
            .parse::<f64>()
            .map_err(|_| EngineError::BadPointLine { line_no: idx + 1, text: line.to_string() })?;
        let y = parts[1]
            .parse::<f64>()
            .map_err(|_| EngineError::BadPointLine { line_no: idx + 1, text: line.to_string() })?;
        points.push((x, y));
    }
    if points.len() < 3 {
        return Err(EngineError::NotEnoughPoints(points.len()));
    }
    Ok(points)
}

/// Projects lat/lon points to planar metres around their own mean position.
pub fn project_latlon(points: &[(f64, f64)]) -> Vec<Point> {
    let proj = LocalProjection::for_points(points);
    points.iter().map(|(lat, lon)| proj.project(*lat, *lon)).collect()
}

/// Converts raw coordinate pairs into a planar-metres ring per `mode`
/// (`PlanarMeters` passes through; `LatLon` projects). The ring is closed
/// automatically if the last point repeats the first, and left open
/// otherwise (rings are implicitly closed everywhere downstream).
pub fn ring_from_points(points: &[(f64, f64)], mode: CoordMode) -> Ring {
    let planar: Vec<(f64, f64)> = match mode {
        CoordMode::PlanarMeters => points.to_vec(),
        CoordMode::LatLon => {
            let projected = project_latlon(points);
            projected.into_iter().map(|p| (p.x, p.y)).collect()
        }
    };
    let mut pts: Vec<Point> = planar.into_iter().map(|(x, y)| Point::new(x, y)).collect();
    if pts.len() > 1 && pts[0].distance_to(pts[pts.len() - 1]) < 1e-9 {
        pts.pop();
    }
    Ring::new(pts)
}

/// Single-parcel measurement — the free edition's entire engine surface.
pub fn analyze_parcel(name: &str, polygon: &Polygon) -> ParcelAnalysis {
    let centroid = polygon.centroid();
    ParcelAnalysis {
        name: name.to_string(),
        area_m2: polygon.area(),
        perimeter_m: polygon.perimeter(),
        centroid: (centroid.x, centroid.y),
        vertex_count: polygon.total_vertex_count(),
        hole_count: polygon.holes.len(),
    }
}

/// What a GeoJSON load produced: the parcels (planar metres, all in one
/// frame) plus the frame they live in — the zone importer must reuse it.
#[derive(Debug, Clone)]
pub struct GeoJsonLoad {
    pub parcels: Vec<NamedParcel>,
    pub coord_mode: CoordMode,
    pub frame: Frame,
}

/// Loads parcels from a GeoJSON document. MultiPolygons flatten into one
/// parcel per polygon; point features are ignored (they carry no area).
/// Coordinates are expected in lat/lon or planar metres: `mode_hint` (the
/// UI's format selector) wins when set, otherwise detection falls back to
/// the value ranges — which cannot distinguish small planar coordinates
/// from lat/lon, hence the override.
#[allow(clippy::type_complexity)]
pub fn load_geojson_parcels(
    text: &str,
    mode_hint: Option<CoordMode>,
) -> Result<GeoJsonLoad, EngineError> {
    let features =
        parse_geojson(text).map_err(|e| EngineError::Parse(format!("GeoJSON: {e}")))?;
    let mut raw: Vec<(String, Vec<Vec<(f64, f64)>>)> = Vec::new();
    for feature in features {
        let name = feature.name.unwrap_or_else(|| "Parcel".to_string());
        match feature.geometry {
            GeoGeometry::Polygon(polygon) => {
                raw.push((name, polygon_rings(&polygon)));
            }
            GeoGeometry::MultiPolygon(polygons) => {
                let count = polygons.len();
                for (i, polygon) in polygons.into_iter().enumerate() {
                    let parcel_name = if count > 1 {
                        format!("{name} {}", i + 1)
                    } else {
                        name.clone()
                    };
                    raw.push((parcel_name, polygon_rings(&polygon)));
                }
            }
            GeoGeometry::Points(_) => {}
        }
    }
    if raw.is_empty() {
        return Err(EngineError::Parse(
            "GeoJSON contains no polygon features to measure".to_string(),
        ));
    }
    let all_values: Vec<(f64, f64)> =
        raw.iter().flat_map(|(_, rings)| rings.iter().flatten().copied()).collect();
    let frame = Frame::choose(mode_hint, &all_values)?;
    let coord_mode = match frame {
        Frame::Planar => CoordMode::PlanarMeters,
        Frame::Local(_) => CoordMode::LatLon,
    };
    let parcels = raw
        .into_iter()
        .map(|(name, rings)| {
            let mut rings = rings.into_iter().map(|r| frame.project_ring(&r));
            let exterior = rings.next().unwrap_or_else(|| Ring::new(Vec::new()));
            NamedParcel {
                name,
                polygon: Polygon {
                    exterior,
                    holes: rings.collect(),
                },
            }
        })
        .collect();
    Ok(GeoJsonLoad { parcels, coord_mode, frame })
}

/// A raw, un-projected overlay zone: the first polygon feature of a
/// GeoJSON document, kept in its source coordinates until it is mapped
/// into the parcel set's frame.
#[derive(Debug, Clone)]
pub struct RawZone {
    pub name: String,
    /// Exterior ring first, then holes.
    pub rings: Vec<Vec<(f64, f64)>>,
}

/// Loads the first polygon feature of a GeoJSON document as a raw zone.
pub fn load_geojson_zone(text: &str) -> Result<RawZone, EngineError> {
    let features =
        parse_geojson(text).map_err(|e| EngineError::Parse(format!("GeoJSON: {e}")))?;
    for feature in features {
        let polygon = match feature.geometry {
            GeoGeometry::Polygon(p) => p,
            GeoGeometry::MultiPolygon(mut ps) => match ps.pop() {
                Some(p) => p,
                None => continue,
            },
            GeoGeometry::Points(_) => continue,
        };
        let rings = polygon_rings(&polygon);
        return Ok(RawZone {
            name: feature.name.unwrap_or_else(|| "Overlay zone".to_string()),
            rings,
        });
    }
    Err(EngineError::Parse(
        "GeoJSON contains no polygon feature to use as the overlay zone".to_string(),
    ))
}

/// Maps a raw zone into the parcel set's planar frame.
pub fn zone_in_frame(zone: &RawZone, frame: &Frame) -> NamedParcel {
    let mut rings = zone.rings.iter().map(|r| frame.project_ring(r));
    let exterior = rings.next().unwrap_or_else(|| Ring::new(Vec::new()));
    NamedParcel {
        name: zone.name.clone(),
        polygon: Polygon {
            exterior,
            holes: rings.collect(),
        },
    }
}

fn polygon_rings(polygon: &Polygon) -> Vec<Vec<(f64, f64)>> {
    let ring = |r: &Ring| r.points.iter().map(|p| (p.x, p.y)).collect();
    let mut rings = vec![ring(&polygon.exterior)];
    rings.extend(polygon.holes.iter().map(&ring));
    rings
}
