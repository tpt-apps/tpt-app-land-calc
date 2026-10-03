//! tpt-land-engine — the analysis core of TPT Land Calc.
//!
//! A thin, UI-free adapter over the [tpt-gis] primitives: boundary points
//! or files in (free: pasted GPS/planar point lists and single GeoJSON;
//! pro: multi-feature GeoJSON and shapefile batches), plain result structs
//! out (area, perimeter, centroid — plus, behind the `pro` feature, buffer
//! zones, overlay fragments, and a PDF report).
//!
//! Everything here is plain `std` Rust with no wasm/UI dependencies, so the
//! correctness moat is host-testable: `cargo test -p tpt-land-engine` runs
//! hand-calculable validation cases (shoelace cross-checks, lat/lon
//! projection, buffer/overlay nets) before any browser is involved.
//!
//! [tpt-gis]: https://github.com/tpt-solutions/tpt-gis

pub mod input;
pub mod report;

pub use input::{
    analyze_parcel, load_geojson_parcels, load_geojson_zone, parse_points_text, ring_from_points,
    zone_in_frame, CoordMode, EngineError, Frame, GeoJsonLoad, NamedParcel, ParcelAnalysis,
    RawZone,
};

/// Buffer/overlay analysis — paid desktop edition only.
#[cfg(feature = "pro")]
pub use report::render_report_pdf;

#[cfg(feature = "pro")]
pub mod spatial;

#[cfg(feature = "pro")]
pub use spatial::{
    buffer_parcel, load_shapefile_parcels, overlay_parcel, BufferResult, OverlayResult,
    ShapefileLoad,
};
