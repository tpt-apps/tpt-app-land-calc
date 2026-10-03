//! Engine-level validation: the adapter's point parsing, coordinate-mode
//! detection, lat/lon projection, and (pro) buffer/overlay/report paths —
//! every expected number hand-calculated, mirroring tpt-gis's tests but
//! through the engine's own API surface.

use tpt_land_engine::{
    analyze_parcel, parse_points_text, CoordMode, EngineError, NamedParcel,
};
use tpt_gis::{Point, Polygon, Ring};

fn pts(coords: &[(f64, f64)]) -> Vec<Point> {
    coords.iter().map(|(x, y)| Point::new(*x, *y)).collect()
}

fn rect_polygon() -> Polygon {
    Polygon::new(Ring::new(pts(&[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)])))
}

#[test]
fn point_list_parsing_formats() {
    // Comma, whitespace, semicolon and tab separators; comments; trailing
    // labels; repeated closing point.
    let text = "# north-west corner\n-43.53, 172.63\n\
                -43.531 172.631\n\
                -43.532;172.632\n\
                -43.533\t172.633 corner SE\n\
                -43.53,172.63\n";
    let points = parse_points_text(text).expect("parses all formats");
    assert_eq!(points.len(), 5);
    assert_eq!(points[0], (-43.53, 172.63));
    assert_eq!(points[4], (-43.53, 172.63));
}

#[test]
fn point_list_errors_name_the_line() {
    let err = parse_points_text("-43.53, 172.63\nrubbish\n").unwrap_err();
    match err {
        EngineError::BadPointLine { line_no, text } => {
            assert_eq!(line_no, 2);
            assert_eq!(text, "rubbish");
        }
        other => panic!("expected BadPointLine, got {other:?}"),
    }
    match parse_points_text("-43.53, 172.63\n-43.54, 172.64\n").unwrap_err() {
        EngineError::NotEnoughPoints(n) => assert_eq!(n, 2),
        other => panic!("expected NotEnoughPoints, got {other:?}"),
    }
}

#[test]
fn coord_mode_detection() {
    assert_eq!(CoordMode::detect(&[(-43.5, 172.6), (-43.6, 172.7)]), Some(CoordMode::LatLon));
    // Anything outside the ±90/±180 caps reads as planar metres.
    assert_eq!(CoordMode::detect(&[(0.0, 0.0), (100.0, 50.0)]), Some(CoordMode::PlanarMeters));
    assert_eq!(CoordMode::detect(&[(500000.0, 5400000.0)]), Some(CoordMode::PlanarMeters));
    // Small planar coordinates are genuinely ambiguous — they read as
    // lat/lon (documented: use the format selector to override).
    assert_eq!(CoordMode::detect(&[(0.0, 0.0), (30.0, 50.0)]), Some(CoordMode::LatLon));
    assert_eq!(CoordMode::detect(&[]), None);
}

#[test]
fn planar_parcel_hand_calculated() {
    // 100 × 50 m: area 5000 m², perimeter 300 m, centroid (50, 25).
    let analysis = analyze_parcel("Rect", &rect_polygon());
    assert!((analysis.area_m2 - 5000.0).abs() < 1e-9);
    assert!((analysis.perimeter_m - 300.0).abs() < 1e-9);
    let (cx, cy) = analysis.centroid;
    assert!((cx - 50.0).abs() < 1e-9 && (cy - 25.0).abs() < 1e-9);
    assert_eq!(analysis.vertex_count, 4);
}

#[test]
fn latlon_parcel_matches_projected_hand_calc() {
    // A 0.001° × 0.001° square near Christchurch, measured through the
    // engine: 111.195 m tall; ~80.6 m wide; area = w·h.
    let text = "-43.500, 172.600\n-43.500, 172.601\n-43.501, 172.601\n-43.501, 172.600\n";
    let points = parse_points_text(text).unwrap();
    let ring = tpt_land_engine::ring_from_points(&points, CoordMode::LatLon);
    let analysis = analyze_parcel("GPS", &Polygon::new(ring));

    let proj = tpt_gis::LocalProjection::new(-43.5005, 172.6005);
    let (m_lat, m_lon) = proj.scales_m_per_degree();
    let expected_area = m_lat * 0.001 * m_lon * 0.001;
    let rel = (analysis.area_m2 - expected_area).abs() / expected_area;
    assert!(rel < 0.001, "area {} vs expected {expected_area}", analysis.area_m2);
    let h = m_lat * 0.001;
    let rel_p = (analysis.perimeter_m - 2.0 * (h + m_lon * 0.001)).abs() / analysis.perimeter_m;
    assert!(rel_p < 0.001, "perimeter {}", analysis.perimeter_m);
}

#[test]
fn geojson_parcel_loader() {
    let geojson = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","properties":{"name":"Paddock A"},
         "geometry":{"type":"Polygon","coordinates":[[[0,0],[100,0],[100,50],[0,50],[0,0]]]}},
        {"type":"Feature","properties":{"name":"Block","block":"B"},
         "geometry":{"type":"MultiPolygon","coordinates":[
            [[[200,0],[300,0],[300,50],[200,50],[200,0]]],
            [[[400,0],[500,0],[500,50],[400,50],[400,0]]]]}}]}"#;
    let parcels = tpt_land_engine::load_geojson_parcels(geojson, None).expect("loads");
    assert_eq!(parcels.coord_mode, CoordMode::PlanarMeters, "coords up to 500 exceed the lat/lon cap");
    let parcels = parcels.parcels;
    assert_eq!(parcels.len(), 3, "multipolygon flattens into separate parcels");
    assert_eq!(parcels[0].name, "Paddock A");
    assert!((parcels[0].polygon.area() - 5000.0).abs() < 1e-9);
    assert_eq!(parcels[1].name, "Block 1");
    assert_eq!(parcels[2].name, "Block 2");
    assert!((parcels[2].polygon.area() - 5000.0).abs() < 1e-9);
}

#[test]
fn geojson_loader_rejects_empty() {
    let err = tpt_land_engine::load_geojson_parcels(
        r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[1,2]}}]}"#,
        None,
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::Parse(_)));
}

#[test]
fn closing_duplicate_point_is_dropped() {
    let text = "0, 0\n100, 0\n100, 50\n0, 50\n0, 0\n";
    let points = parse_points_text(text).unwrap();
    let ring = tpt_land_engine::ring_from_points(&points, CoordMode::PlanarMeters);
    assert_eq!(ring.points.len(), 4, "closing repeat dropped");
    assert!((ring.area() - 5000.0).abs() < 1e-9);
}

// ---- Pro engine surface (compiled only with --features pro) ----

#[cfg(feature = "pro")]
mod pro_tests {
    use super::*;
    use tpt_land_engine::{buffer_parcel, load_shapefile_parcels, overlay_parcel};
    use tpt_gis::{Polygon, Ring};

    #[test]
    fn buffer_and_overlay_through_engine() {
        // Buffer +5: 110×60 = 6600 (adds 1600).
        let buffered = buffer_parcel(&rect_polygon(), 5.0).unwrap();
        assert!((buffered.area_m2 - 6600.0).abs() < 1e-6);
        assert!((buffered.added_m2 - 1600.0).abs() < 1e-6);
        assert!(buffer_parcel(&rect_polygon(), -30.0).is_err());

        // Overlay half-overlap: 2500 m² = 50% of the parcel.
        let zone = Polygon::new(Ring::new(pts(&[(50.0, 0.0), (150.0, 0.0), (150.0, 50.0), (50.0, 50.0)])));
        let overlay = overlay_parcel(&rect_polygon(), &zone);
        assert!((overlay.area_m2 - 2500.0).abs() < 1e-6);
        assert!((overlay.share - 0.5).abs() < 1e-9);
        assert!(!overlay.fragments.is_empty());
    }

    #[test]
    fn shapefile_loader() {
        // Hand-assembled .shp with two polygons — reuse the byte layout
        // proven in tpt-gis's tests: one 10×10 at origin, one at (100,0).
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&9994u32.to_be_bytes()); // magic
        bytes.extend_from_slice(&[0u8; 20]); // unused header bytes
        let mut rec1: Vec<u8> = Vec::new();
        rec1.extend_from_slice(&5u32.to_le_bytes());
        for v in [0.0f64, 0.0, 10.0, 10.0] { rec1.extend_from_slice(&v.to_le_bytes()); }
        rec1.extend_from_slice(&1i32.to_le_bytes());
        rec1.extend_from_slice(&4i32.to_le_bytes());
        rec1.extend_from_slice(&0i32.to_le_bytes());
        for (x, y) in [(0.0f64, 0.0f64), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            rec1.extend_from_slice(&x.to_le_bytes());
            rec1.extend_from_slice(&y.to_le_bytes());
        }
        let mut rec2: Vec<u8> = Vec::new();
        rec2.extend_from_slice(&5u32.to_le_bytes());
        for v in [100.0f64, 0.0, 110.0, 10.0] { rec2.extend_from_slice(&v.to_le_bytes()); }
        rec2.extend_from_slice(&1i32.to_le_bytes());
        rec2.extend_from_slice(&4i32.to_le_bytes());
        rec2.extend_from_slice(&0i32.to_le_bytes());
        for (x, y) in [(100.0f64, 0.0f64), (110.0, 0.0), (110.0, 10.0), (100.0, 10.0)] {
            rec2.extend_from_slice(&x.to_le_bytes());
            rec2.extend_from_slice(&y.to_le_bytes());
        }
        let total = 100 + 8 + rec1.len() + 8 + rec2.len();
        bytes.extend_from_slice(&((total as u32) / 2).to_be_bytes());
        bytes.extend_from_slice(&1000u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes()); // file shape type
        for v in [0.0f64, 0.0, 110.0, 10.0] { bytes.extend_from_slice(&v.to_le_bytes()); }
        for v in [0.0f64, 0.0, 0.0, 0.0] { bytes.extend_from_slice(&v.to_le_bytes()); }
        assert_eq!(bytes.len(), 100);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&((rec1.len() / 2) as u32).to_be_bytes());
        bytes.extend_from_slice(&rec1);
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&((rec2.len() / 2) as u32).to_be_bytes());
        bytes.extend_from_slice(&rec2);

        // No hint: 0..110 spans the lat/lon cap (110 > 90) so it reads as
        // planar metres and areas stay 100 m².
        let load = load_shapefile_parcels(&bytes, None).unwrap();
        assert_eq!(load.coord_mode, CoordMode::PlanarMeters);
        assert_eq!(load.parcels.len(), 2);
        assert_eq!(load.parcels[0].name, "Parcel 1");
        assert!((load.parcels[0].polygon.area() - 100.0).abs() < 1e-9);
        assert!((load.parcels[1].polygon.area() - 100.0).abs() < 1e-9);
        // A hint overrides detection either way: forcing LatLon on
        // metre-scale shapes blows the area up by degree-scale factors.
        let forced = load_shapefile_parcels(&bytes, Some(CoordMode::LatLon)).unwrap();
        assert!(forced.parcels[0].polygon.area() > 1_000_000.0);
    }

    #[test]
    fn overlay_zone_shares_the_parcel_frame() {
        // A zone must land in the SAME planar frame as the parcels it is
        // intersected with — per-load anchors would shift geometries by the
        // distance between their mean positions and corrupt the overlay.
        let zone_json = r#"{"type":"Feature","properties":{"name":"Zone"},
            "geometry":{"type":"Polygon","coordinates":[[[10,10],[30,10],[30,20],[10,20],[10,10]]]}}"#;
        let raw = tpt_land_engine::load_geojson_zone(zone_json).unwrap();
        assert_eq!(raw.name, "Zone");

        let frame = tpt_land_engine::Frame::Local(tpt_gis::LocalProjection::new(-43.5, 172.6));
        let zone = tpt_land_engine::zone_in_frame(&raw, &frame);
        // 20° × 10° in metres, scaled by cos(lat0) on the east–west axis.
        let m_per_deg = tpt_gis::EARTH_RADIUS_M * (std::f64::consts::PI / 180.0);
        let expected = 20.0f64 * 10.0 * m_per_deg * m_per_deg * (-43.5f64).to_radians().cos();
        assert!(
            (zone.polygon.area() - expected).abs() / expected < 0.001,
            "framed zone area = {}",
            zone.polygon.area()
        );
    }

    #[test]
    fn report_pdf_is_valid_envelope() {
        use tpt_land_engine::report::{render_report_pdf, Row};
        let pdf = render_report_pdf(
            "Test report",
            &[
                Row::Title("TPT Land Calc — analysis".to_string()),
                Row::Head("Parcels".to_string()),
                Row::Table {
                    columns: vec!["Parcel".to_string(), "Area".to_string()],
                    rows: vec![vec!["Rect".to_string(), "5,000 m²".to_string()]],
                    weights: None,
                },
                Row::Note("Generated locally — no data leaves this machine.".to_string()),
            ],
        );
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.windows(5).any(|w| w == b"%%EOF"));
        // One xref table, correct object count in its header.
        let xref_at = pdf.windows(4).position(|w| w == b"xref").unwrap();
        let header = &pdf[xref_at..xref_at + 16];
        let text = String::from_utf8_lossy(header);
        // 2 base + 2 per page (1 page) + 2 fonts + root size line.
        assert!(text.starts_with("xref\n0 7\n"), "xref header: {text:?}");
    }
}

// NamedParcel is otherwise unused without pro in this test file.
#[allow(unused)]
fn _keep_named_parcel_in_scope(_p: NamedParcel) {}
