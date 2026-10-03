//! Free WASM edition UI (tpt-appfront-dom), single-parcel area analysis.
//!
//! Rendering architecture (mirrors the FEA Lite proven split):
//! - the **input panel is mounted once** (`tpt_appfront_dom::mount`) — typing
//!   never loses focus; field values are read straight from the DOM when
//!   Calculate runs;
//! - the **results section re-renders fine-grained** via
//!   `tpt_appfront_dom::render` driven by the `outcome` Signal;
//! - the **map** is a raw web-sys `<canvas>` (the DOM backend has no canvas
//!   node kind), redrawn by an effect that watches the outcome.
//!
//! Pro edition (`--features pro`): multi-parcel GeoJSON/shapefile batch
//! import, buffer-zone and overlay analysis on the selected parcel, and
//! PNG/PDF export. The free build compiles none of it — the shipped hub
//! bundle must never carry paid features.

use std::cell::RefCell;
use std::rc::Rc;

use tpt_appfront_core::{create_effect, Signal, UITree};
use tpt_land_engine::{
    analyze_parcel, load_geojson_parcels, parse_points_text, ring_from_points, CoordMode, Frame,
    NamedParcel, ParcelAnalysis,
};
#[cfg(feature = "pro")]
use tpt_land_engine::{
    buffer_parcel, load_geojson_zone, load_shapefile_parcels, overlay_parcel, zone_in_frame,
};
use tpt_gis::Polygon;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::viewer::{self, ParcelDraw, Scene};

/// The results section's current content.
#[derive(Clone)]
enum Outcome {
    Idle,
    Invalid(Vec<String>),
    Solved(Rc<SolvedModel>),
}

/// An imported parcel set plus the planar frame its coordinates live in.
#[derive(Clone)]
struct ImportedSet {
    parcels: Vec<NamedParcel>,
    #[cfg_attr(not(feature = "pro"), allow(dead_code))]
    frame: Frame,
}

/// The UI's coordinate-format selector as an engine hint (`None` = auto).
fn mode_hint(container: &web_sys::Element) -> Option<CoordMode> {
    match read_value(container, ".land-mode").unwrap_or_else(|| "auto".into()).as_str() {
        "latlon" => Some(CoordMode::LatLon),
        "meters" => Some(CoordMode::PlanarMeters),
        _ => None,
    }
}

struct ParcelRow {
    name: String,
    polygon: Polygon,
    analysis: ParcelAnalysis,
}

struct SolvedModel {
    parcels: Vec<ParcelRow>,
    total_m2: f64,
    /// Input read as lat/lon and projected to local metres.
    projected: bool,
    /// Where the boundary came from — shown under the headline.
    source: &'static str,
    /// Free edition only: parcels in the imported file beyond the first
    /// (the Pro upsell hook).
    skipped: Option<usize>,
    /// (pro) Analysis target index into `parcels` (buffer/overlay owner).
    #[cfg(feature = "pro")]
    target: usize,
    /// (pro) Buffer applied to the target parcel.
    #[cfg(feature = "pro")]
    buffer: Option<BufferInfo>,
    /// (pro) Overlay zone intersected with the target parcel.
    #[cfg(feature = "pro")]
    overlay: Option<OverlayInfo>,
    /// (pro) Non-fatal notes (e.g. a buffer distance that consumed the
    /// parcel) — shown under the grid without invalidating the solve.
    #[cfg(feature = "pro")]
    notes: Vec<String>,
}

#[cfg(feature = "pro")]
struct BufferInfo {
    distance_m: f64,
    result: tpt_land_engine::BufferResult,
}

#[cfg(feature = "pro")]
struct OverlayInfo {
    zone_name: String,
    result: tpt_land_engine::OverlayResult,
}

#[derive(Debug, Clone)]
enum Msg {
    Calculate,
    Sample(usize),
    Clear,
    /// Open a TPT Solutions URL — new tab in the browser, system browser
    /// via the desktop IPC bridge in the webview shell (protocol-sniffed,
    /// same as FEA Lite).
    VisitSite(String),
    /// Click an upload button → forward to its hidden file input.
    UploadGeoJson,
    #[cfg(feature = "pro")]
    UploadShapefile,
    #[cfg(feature = "pro")]
    UploadZone,
    /// (pro) Export buttons.
    #[cfg(feature = "pro")]
    ExportPng,
    #[cfg(feature = "pro")]
    ExportPdf,
}

const SAMPLES: [(&str, &str, &str); 3] = [
    (
        "Paddock (metric)",
        "0, 0\n100, 0\n100, 50\n0, 50",
        "meters",
    ),
    (
        "L-shaped block",
        "0, 0\n40, 0\n40, 20\n20, 20\n20, 50\n0, 50",
        "meters",
    ),
    (
        "GPS paddock (Christchurch)",
        "-43.5000, 172.6000\n-43.5000, 172.6010\n-43.5010, 172.6010\n-43.5010, 172.6000",
        "latlon",
    ),
];

const HUB_URL: &str = "https://tptsolutions.co.nz/tools/land-calc";
const M2_PER_HA: f64 = 10_000.0;
const M2_PER_ACRE: f64 = 4_046.8564224;
const M2_PER_FT2: f64 = 0.092_903_04;

const BASE_CSS: &str = r#"
.land-app{--land-bg:#f7f8fa;--land-surface:#ffffff;--land-panel:#f2f4f7;--land-fg:#181d27;--land-muted:#667085;--land-border:#e3e7ee;--land-accent:#0e7c6b;--land-accent-2:#0a5f52;--land-accent-soft:#e6f5f2;--land-accent-fg:#ffffff;--land-err:#b3261e;--land-err-bg:#fbeeec;--land-map-bg:#f4f6f8;--land-map-grid:rgba(20,30,45,.07);--land-overlay:#b7641e;--land-map-label-bg:rgba(255,255,255,.82);--land-shadow:0 1px 2px rgba(16,24,40,.04),0 1px 3px rgba(16,24,40,.06);max-width:1360px;margin:0 auto;background:var(--land-bg);color:var(--land-fg);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:14.5px;line-height:1.55;padding:0 4px 32px}
@media(prefers-color-scheme:dark){.land-app{--land-bg:#0b0f16;--land-surface:#141a24;--land-panel:#161d2a;--land-fg:#e8ebf0;--land-muted:#98a2b3;--land-border:#262f3d;--land-accent:#34b39a;--land-accent-2:#4fd6ba;--land-accent-soft:#122824;--land-accent-fg:#04140f;--land-err:#f2b8b5;--land-err-bg:#2c1a1c;--land-map-bg:#10151d;--land-map-grid:rgba(255,255,255,.06);--land-overlay:#e08a44;--land-map-label-bg:rgba(10,14,20,.85);--land-shadow:0 1px 2px rgba(0,0,0,.3)}}
.land-app *{box-sizing:border-box}
.land-app h2,.land-app h3{margin:0;font-weight:650}
.land-header{display:flex;align-items:center;gap:14px;padding:18px 2px 14px}
.land-brand{width:42px;height:42px;border-radius:10px;background:var(--land-accent);color:var(--land-accent-fg);display:flex;align-items:center;justify-content:center;font-weight:800;font-size:17px;letter-spacing:.5px;flex:none}
.land-title-group{display:flex;flex-direction:column;gap:2px}
.land-title-row{display:flex;align-items:center;gap:10px}
.land-sub{color:var(--land-muted);font-size:13.5px}
.land-chip{background:var(--land-accent-soft);color:var(--land-accent-2);border-radius:999px;padding:2px 10px;font-size:12px;font-weight:700;letter-spacing:.4px;text-transform:uppercase}
.land-main{display:grid;grid-template-columns:minmax(330px,400px) 1fr;gap:14px;align-items:start}
@media(max-width:920px){.land-main{grid-template-columns:1fr}}
.land-sidebar{display:flex;flex-direction:column;gap:14px}
.land-content{display:flex;flex-direction:column;gap:14px}
.land-card{background:var(--land-surface);border:1px solid var(--land-border);border-radius:12px;padding:16px;box-shadow:var(--land-shadow)}
.land-field{display:flex;flex-direction:column;gap:5px;margin-bottom:11px}
.land-field:last-child{margin-bottom:0}
.land-label{font-size:12.5px;font-weight:600;color:var(--land-muted);letter-spacing:.2px}
.land-input{width:100%;padding:8px 10px;border:1px solid var(--land-border);border-radius:8px;background:var(--land-panel);color:var(--land-fg);font:inherit}
.land-input:focus{outline:2px solid var(--land-accent);outline-offset:1px}
textarea.land-points{min-height:132px;resize:vertical;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:13px}
.land-row{display:flex;gap:10px;flex-wrap:wrap}
.land-row>.land-field{flex:1;min-width:130px}
.land-btnrow{display:flex;gap:8px;flex-wrap:wrap;margin-top:12px}
.land-btn{padding:8px 14px;border-radius:8px;border:1px solid var(--land-border);background:var(--land-panel);color:var(--land-fg);font:inherit;font-weight:600;cursor:pointer}
.land-btn:hover{border-color:var(--land-accent)}
.land-btn-primary{background:var(--land-accent);border-color:var(--land-accent);color:var(--land-accent-fg)}
.land-btn-primary:hover{background:var(--land-accent-2)}
.land-hint{color:var(--land-muted);font-size:12.5px;margin-top:8px}
.land-canvas{width:100%;height:auto;display:block;border:1px solid var(--land-border);border-radius:8px}
.land-summary{margin-bottom:10px}
.land-headline{font-size:17px;font-weight:700}
.land-detail{color:var(--land-muted);font-size:13px;margin-top:3px}
.land-issue{background:var(--land-err-bg);color:var(--land-err);border-radius:8px;padding:8px 12px;margin-bottom:6px;font-size:13.5px}
.land-note{background:var(--land-accent-soft);color:var(--land-accent-2);border-radius:8px;padding:8px 12px;margin-top:8px;font-size:13.5px}
.land-upsell{border-color:var(--land-accent)}
.land-upsell ul{margin:8px 0;padding-left:20px;color:var(--land-fg)}
.land-upsell li{margin-bottom:4px}
.land-upsell .land-price{font-weight:700;color:var(--land-accent-2)}
.land-footer-link{background:none;border:none;color:var(--land-accent-2);font:inherit;font-weight:700;cursor:pointer;padding:0;margin-top:10px}
.land-footer-link:hover{text-decoration:underline}
"#;

pub fn mount_app(container: &web_sys::Element) -> Result<(), JsValue> {
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");

    upsert_style(&document, "land-calc-styles", BASE_CSS);
    let existing_class = container.get_attribute("class").unwrap_or_default();
    if !existing_class.split_whitespace().any(|c| c == "land-app") {
        let combined = if existing_class.is_empty() {
            "land-app".to_string()
        } else {
            format!("{existing_class} land-app")
        };
        let _ = container.set_attribute("class", &combined);
    }

    let outcome: Signal<Outcome> = Signal::new(Outcome::Idle);

    // Imported parcel sets and the overlay zone live outside the DOM: file
    // readers stash them here, and Calculate prefers them over the textarea.
    // (The zone store is only ever populated by the Pro edition's import.)
    // The set carries the frame its coordinates were projected into — a
    // later zone import must land in the same frame or overlays shift.
    let parcels_store: Rc<RefCell<Option<ImportedSet>>> = Rc::new(RefCell::new(None));
    let zone_store: Rc<RefCell<Option<NamedParcel>>> = Rc::new(RefCell::new(None));

    // Hidden file inputs backing the upload buttons.
    let make_file_input = |accept: &str| -> web_sys::HtmlInputElement {
        let el: web_sys::HtmlInputElement = document
            .create_element("input")
            .unwrap()
            .dyn_into()
            .unwrap();
        el.set_attribute("type", "file").unwrap();
        el.set_attribute("accept", accept).unwrap();
        el.set_attribute("style", "display:none").unwrap();
        container.append_child(&el).unwrap();
        el
    };
    let geo_input = make_file_input(".geojson,.json");
    #[cfg(feature = "pro")]
    let shp_input = make_file_input(".shp");
    #[cfg(feature = "pro")]
    let zone_input = make_file_input(".geojson,.json");

    let dispatch: Rc<dyn Fn(Msg)> = {
        let container = container.clone();
        let outcome = outcome.clone();
        let parcels_store = parcels_store.clone();
        let zone_store = zone_store.clone();
        let geo_input = geo_input.clone();
        #[cfg(feature = "pro")]
        let shp_input = shp_input.clone();
        #[cfg(feature = "pro")]
        let zone_input = zone_input.clone();
        Rc::new(move |msg| match msg {
            Msg::Calculate => run_solve(&container, &outcome, &parcels_store, &zone_store),
            Msg::Sample(index) => {
                if let Some((_, text, mode)) = SAMPLES.get(index) {
                    write_value(&container, ".land-points", text);
                    write_value(&container, ".land-mode", mode);
                    *parcels_store.borrow_mut() = None;
                    run_solve(&container, &outcome, &parcels_store, &zone_store);
                }
            }
            Msg::Clear => {
                write_value(&container, ".land-points", "");
                write_value(&container, ".land-mode", "auto");
                *parcels_store.borrow_mut() = None;
                outcome.set(Outcome::Idle);
            }
            Msg::VisitSite(url) => visit_site(&url),
            Msg::UploadGeoJson => geo_input.click(),
            #[cfg(feature = "pro")]
            Msg::UploadShapefile => shp_input.click(),
            #[cfg(feature = "pro")]
            Msg::UploadZone => zone_input.click(),
            #[cfg(feature = "pro")]
            Msg::ExportPng => export_png(&container),
            #[cfg(feature = "pro")]
            Msg::ExportPdf => export_pdf(&container, &outcome),
        })
    };

    // 1. Static shell — mounted exactly once.
    let shell = shell_tree();
    let root = tpt_appfront_dom::mount(container, &shell, dispatch.clone())?;
    std::mem::forget(root);

    // 2. Reactive results section into its placeholder.
    let results_el = container
        .query_selector(".land-results-body")?
        .ok_or_else(|| JsValue::from_str("results placeholder missing"))?;
    let outcome_for_render = outcome.clone();
    let handle = tpt_appfront_dom::render(
        &results_el,
        Rc::new(move || results_view(outcome_for_render.clone())),
        dispatch.clone(),
    )?;
    std::mem::forget(handle);

    // 3. Map canvas (raw DOM) + redraw effect.
    let canvas_holder = container
        .query_selector(".land-canvas-holder")?
        .ok_or_else(|| JsValue::from_str("canvas placeholder missing"))?;
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("canvas cast failed"))?;
    canvas.set_width(viewer::W as u32);
    canvas.set_height(viewer::H as u32);
    canvas.set_class_name("land-canvas");
    canvas_holder.append_child(&canvas)?;

    {
        let outcome = outcome.clone();
        let container = container.clone();
        let handle = create_effect(move || {
            redraw(&container, &canvas, outcome.get());
        });
        std::mem::forget(handle);
    }

    // 4. File readers. Each hidden input's change listener reads the picked
    // file and routes it into the stores, then re-solves. Pattern: the
    // 'static outer FnMut owns its own clones; per event it clones them
    // again for the one-shot inner closure (an FnMut body may not move its
    // own captures out).
    {
        let geo_input = geo_input.clone();
        let geo_for_listener = geo_input.clone();
        let container_owned = container.clone();
        let outcome = outcome.clone();
        let parcels_store = parcels_store.clone();
        let zone_store = zone_store.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let container = container_owned.clone();
            let outcome = outcome.clone();
            let parcels_store = parcels_store.clone();
            let zone_store = zone_store.clone();
            read_file_text(&geo_input, move |text| {
                let hint = mode_hint(&container);
                match load_geojson_parcels(&text, hint) {
                    Ok(load) => {
                        if load.parcels.is_empty() {
                            outcome.set(Outcome::Invalid(vec![
                                "GeoJSON file contains no polygon features".to_string(),
                            ]));
                            return;
                        }
                        // Keep every parcel in the store; the free edition
                        // measures the first and says how many more Pro
                        // would import.
                        *parcels_store.borrow_mut() = Some(ImportedSet {
                            parcels: load.parcels,
                            frame: load.frame,
                        });
                        run_solve_imported(
                            &container,
                            &outcome,
                            &parcels_store,
                            &zone_store,
                            load.coord_mode,
                            "GeoJSON file",
                        );
                    }
                    Err(e) => outcome.set(Outcome::Invalid(vec![e.to_string()])),
                }
            });
        });
        geo_for_listener.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    #[cfg(feature = "pro")]
    {
        let shp_input = shp_input.clone();
        let shp_input_for_listener = shp_input.clone();
        let container_owned = container.clone();
        let outcome = outcome.clone();
        let parcels_store = parcels_store.clone();
        let zone_store = zone_store.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let container = container_owned.clone();
            let outcome = outcome.clone();
            let parcels_store = parcels_store.clone();
            let zone_store = zone_store.clone();
            read_file_bytes(&shp_input, move |bytes| {
                let hint = mode_hint(&container);
                match load_shapefile_parcels(&bytes, hint) {
                    Ok(load) if load.parcels.is_empty() => {
                        outcome.set(Outcome::Invalid(vec![
                            "shapefile contains no polygon records".to_string(),
                        ]));
                    }
                    Ok(load) => {
                        *parcels_store.borrow_mut() = Some(ImportedSet {
                            parcels: load.parcels,
                            frame: load.frame,
                        });
                        run_solve_imported(
                            &container,
                            &outcome,
                            &parcels_store,
                            &zone_store,
                            load.coord_mode,
                            "shapefile",
                        );
                    }
                    Err(e) => outcome.set(Outcome::Invalid(vec![e.to_string()])),
                }
            });
        });
        shp_input_for_listener.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    #[cfg(feature = "pro")]
    {
        let zone_input = zone_input.clone();
        let zone_input_for_listener = zone_input.clone();
        let container_owned = container.clone();
        let outcome = outcome.clone();
        let parcels_store = parcels_store.clone();
        let zone_store = zone_store.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let container = container_owned.clone();
            let outcome = outcome.clone();
            let parcels_store = parcels_store.clone();
            let zone_store = zone_store.clone();
            read_file_text(&zone_input, move |text| {
                match load_geojson_zone(&text) {
                    Ok(raw) => {
                        // In the parcel set's frame when one is loaded;
                        // otherwise the selector hint or the zone's own
                        // reading anchors it.
                        let fallback_values: Vec<(f64, f64)> =
                            raw.rings.iter().flatten().copied().collect();
                        let fallback = Frame::choose(mode_hint(&container), &fallback_values);
                        let frame = match parcels_store.borrow().as_ref() {
                            Some(set) => set.frame.clone(),
                            None => match fallback {
                                Ok(frame) => frame,
                                Err(e) => {
                                    outcome.set(Outcome::Invalid(vec![e.to_string()]));
                                    return;
                                }
                            },
                        };
                        *zone_store.borrow_mut() = Some(zone_in_frame(&raw, &frame));
                        run_solve(&container, &outcome, &parcels_store, &zone_store);
                    }
                    Err(e) => outcome.set(Outcome::Invalid(vec![e.to_string()])),
                }
            });
        });
        zone_input_for_listener.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    Ok(())
}

// ---- solve paths ---------------------------------------------------------

/// Solves from the DOM inputs (textarea or an imported parcel set).
fn run_solve(
    container: &web_sys::Element,
    outcome: &Signal<Outcome>,
    parcels_store: &RefCell<Option<ImportedSet>>,
    zone_store: &RefCell<Option<NamedParcel>>,
) {
    let store = parcels_store.borrow();
    if let Some(set) = store.as_ref() {
        solve_parcels(container, outcome, &set.parcels, "imported file", None, zone_store);
        return;
    }
    drop(store);

    let text = read_value(container, ".land-points").unwrap_or_default();
    let points = match parse_points_text(&text) {
        Ok(points) => points,
        Err(e) => {
            outcome.set(Outcome::Invalid(vec![e.to_string()]));
            return;
        }
    };
    let mode = match read_value(container, ".land-mode").unwrap_or_else(|| "auto".into()).as_str() {
        "latlon" => CoordMode::LatLon,
        "meters" => CoordMode::PlanarMeters,
        _ => match CoordMode::detect(&points) {
            Some(mode) => mode,
            None => {
                outcome.set(Outcome::Invalid(vec![
                    "coordinates don't look like lat/lon — set the format to planar metres".to_string(),
                ]));
                return;
            }
        },
    };
    let ring = ring_from_points(&points, mode);
    if !ring.is_usable() {
        outcome.set(Outcome::Invalid(vec![
            "need at least 3 distinct boundary points".to_string(),
        ]));
        return;
    }
    let polygon = Polygon::new(ring);
    let analysis = analyze_parcel("Parcel", &polygon);
    let rows = vec![ParcelRow { name: "Parcel".to_string(), polygon, analysis }];
    solve_rows(
        container,
        outcome,
        rows,
        matches!(mode, CoordMode::LatLon),
        "pasted points",
        None,
        zone_store,
    );
}

/// Solves from an imported parcel set (file-reader path).
#[allow(clippy::too_many_arguments)]
fn run_solve_imported(
    container: &web_sys::Element,
    outcome: &Signal<Outcome>,
    parcels_store: &RefCell<Option<ImportedSet>>,
    zone_store: &RefCell<Option<NamedParcel>>,
    mode: CoordMode,
    source: &'static str,
) {
    let store = parcels_store.borrow();
    let Some(set) = store.as_ref() else { return };
    solve_parcels(container, outcome, &set.parcels, source, Some(mode), zone_store);
}

fn solve_parcels(
    container: &web_sys::Element,
    outcome: &Signal<Outcome>,
    parcels: &[NamedParcel],
    source: &'static str,
    mode: Option<CoordMode>,
    zone_store: &RefCell<Option<NamedParcel>>,
) {
    let rows: Vec<ParcelRow> = parcels
        .iter()
        .map(|p| ParcelRow {
            analysis: analyze_parcel(&p.name, &p.polygon),
            name: p.name.clone(),
            polygon: p.polygon.clone(),
        })
        .collect();
    // Free edition keeps every imported parcel in the model (so the map can
    // show them later if Pro unlocks) but reports the ones it won't measure.
    let skipped = if cfg!(not(feature = "pro")) && rows.len() > 1 {
        Some(rows.len() - 1)
    } else {
        None
    };
    solve_rows(
        container,
        outcome,
        rows,
        matches!(mode, Some(CoordMode::LatLon)),
        source,
        skipped,
        zone_store,
    );
}

#[allow(clippy::too_many_arguments)]
fn solve_rows(
    container: &web_sys::Element,
    outcome: &Signal<Outcome>,
    rows: Vec<ParcelRow>,
    projected: bool,
    source: &'static str,
    skipped: Option<usize>,
    zone_store: &RefCell<Option<NamedParcel>>,
) {
    if rows.is_empty() {
        outcome.set(Outcome::Invalid(vec!["nothing to measure".to_string()]));
        return;
    }
    let total_m2: f64 = rows.iter().map(|r| r.analysis.area_m2).sum();

    #[cfg(feature = "pro")]
    {
        let target = read_value(container, ".land-target")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(rows.len() - 1);

        let mut notes: Vec<String> = Vec::new();
        let buffer = match read_value(container, ".land-buffer")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|d| *d != 0.0)
        {
            None => None,
            Some(distance_m) => match buffer_parcel(&rows[target].polygon, distance_m) {
                Ok(result) => Some(BufferInfo { distance_m, result }),
                Err(e) => {
                    notes.push(format!("buffer {distance_m} m: {e}"));
                    None
                }
            },
        };
        let overlay = zone_store.borrow().as_ref().map(|zone| OverlayInfo {
            zone_name: zone.name.clone(),
            result: overlay_parcel(&rows[target].polygon, &zone.polygon),
        });
        outcome.set(Outcome::Solved(Rc::new(SolvedModel {
            total_m2,
            target,
            buffer,
            overlay,
            notes,
            parcels: rows,
            projected,
            source,
            skipped,
        })));
    }

    #[cfg(not(feature = "pro"))]
    {
        let _ = container;
        let _ = zone_store;
        outcome.set(Outcome::Solved(Rc::new(SolvedModel {
            parcels: rows,
            total_m2,
            projected,
            source,
            skipped,
        })));
    }
}

// ---- file reading --------------------------------------------------------

/// Reads the picked file as text and runs `then` on the main thread.
fn read_file_text<T>(input: &web_sys::HtmlInputElement, then: T)
where
    T: FnOnce(String) + 'static,
{
    let Some(files) = input.files() else { return };
    let Some(file) = files.get(0) else { return };
    let Ok(reader) = web_sys::FileReader::new() else { return };
    let mut then = Some(then);
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(f) = then.take() else { return };
        if let Some(reader) = event.target().and_then(|t| t.dyn_into::<web_sys::FileReader>().ok()) {
            if let Ok(result) = reader.result() {
                f(result.as_string().unwrap_or_default());
            }
        }
    });
    let onload = closure.as_ref().unchecked_ref();
    reader.set_onload(Some(onload));
    closure.forget();
    let _ = reader.read_as_text(&file);
}

/// Reads the picked file as bytes and runs `then` on the main thread.
#[cfg(feature = "pro")]
fn read_file_bytes<T>(input: &web_sys::HtmlInputElement, then: T)
where
    T: FnOnce(Vec<u8>) + 'static,
{
    let Some(files) = input.files() else { return };
    let Some(file) = files.get(0) else { return };
    let Ok(reader) = web_sys::FileReader::new() else { return };
    let mut then = Some(then);
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(f) = then.take() else { return };
        if let Some(reader) = event.target().and_then(|t| t.dyn_into::<web_sys::FileReader>().ok()) {
            if let Ok(result) = reader.result() {
                if let Ok(buffer) = result.dyn_into::<js_sys::ArrayBuffer>() {
                    f(js_sys::Uint8Array::new(&buffer).to_vec());
                }
            }
        }
    });
    let onload = closure.as_ref().unchecked_ref();
    reader.set_onload(Some(onload));
    closure.forget();
    let _ = reader.read_as_array_buffer(&file);
}

// ---- DOM helpers ---------------------------------------------------------

fn read_value(container: &web_sys::Element, selector: &str) -> Option<String> {
    let el = container.query_selector(selector).ok().flatten()?;
    if let Some(input) = el.dyn_ref::<web_sys::HtmlInputElement>() {
        return Some(input.value());
    }
    if let Some(textarea) = el.dyn_ref::<web_sys::HtmlTextAreaElement>() {
        return Some(textarea.value());
    }
    if let Some(select) = el.dyn_ref::<web_sys::HtmlSelectElement>() {
        return Some(select.value());
    }
    None
}

fn write_value(container: &web_sys::Element, selector: &str, value: &str) {
    let Some(el) = container.query_selector(selector).ok().flatten() else { return };
    if let Some(input) = el.dyn_ref::<web_sys::HtmlInputElement>() {
        input.set_value(value);
    } else if let Some(textarea) = el.dyn_ref::<web_sys::HtmlTextAreaElement>() {
        textarea.set_value(value);
    } else if let Some(select) = el.dyn_ref::<web_sys::HtmlSelectElement>() {
        select.set_value(value);
    }
}

fn upsert_style(document: &web_sys::Document, id: &str, css: &str) {
    if let Ok(Some(existing)) = document.query_selector(&format!("style#{id}")) {
        existing.set_text_content(Some(css));
        return;
    }
    let Some(head) = document.head() else { return };
    if let Ok(style_el) = document.create_element("style") {
        let _ = style_el.set_attribute("id", id);
        style_el.set_text_content(Some(css));
        let _ = head.append_child(&style_el);
    }
}

/// Opens a TPT Solutions URL from wherever the app happens to be running —
/// new tab in the browser; via the desktop shell's allowlisted IPC bridge
/// when served over its `app://` protocol (see the desktop crate).
fn visit_site(url: &str) {
    let Some(window) = web_sys::window() else { return };
    let in_desktop_shell = window.location().protocol().map(|p| p == "app:").unwrap_or(false);
    if in_desktop_shell {
        if let Ok(appfront) = js_sys::Reflect::get(&window, &JsValue::from_str("__appfront")) {
            if let Ok(post) = js_sys::Reflect::get(&appfront, &JsValue::from_str("post")) {
                if let Ok(post_fn) = post.dyn_into::<js_sys::Function>() {
                    let params = js_sys::Object::new();
                    let _ = js_sys::Reflect::set(
                        &params,
                        &JsValue::from_str("url"),
                        &JsValue::from_str(url),
                    );
                    let _ = post_fn.call2(&appfront, &JsValue::from_str("open_external"), &params);
                }
            }
        }
    } else {
        let _ = window.open_with_url_and_target(url, "_blank");
    }
}

// ---- tree building -------------------------------------------------------

fn styled_text(
    c: &mut tpt_appfront_core::ContainerBuilder<Msg>,
    text: impl Into<String>,
    class: &str,
) {
    let text = text.into();
    c.container(move |c| {
        c.text(text);
    })
    .class(class);
}

fn label(c: &mut tpt_appfront_core::ContainerBuilder<Msg>, text: &str) {
    c.container(|field_label| {
        field_label.text(text);
    })
    .class("land-label");
}

/// The mounted-once shell: header, boundary card, (pro) analysis card,
/// upsell, map card, results placeholder.
fn shell_tree() -> UITree<Msg> {
    UITree::container(|c| {
        c.container(|header| {
            header.text("LC").class("land-brand");
            header.container(|group| {
                group.container(|row| {
                    row.heading(2, "Land Calc");
                    #[cfg(feature = "pro")]
                    row.text("Pro").class("land-chip");
                })
                .class("land-title-row");
                #[cfg(not(feature = "pro"))]
                group.text("Single-parcel area from GPS or planar boundary points — computed locally in your browser. Nothing is uploaded.")
                    .class("land-sub");
                #[cfg(feature = "pro")]
                group.text("Multi-parcel area, buffer zones and overlays from GPS points, GeoJSON or shapefiles — computed locally. Nothing is uploaded.")
                    .class("land-sub");
            })
            .class("land-title-group");
        })
        .class("land-header");

        c.container(|shell| {
            shell.container(|sidebar| {
                sidebar.container(|card| {
                    card.heading(3, "Boundary");
                    card.container(|row| {
                        row.container(|field| {
                            label(field, "Coordinate format");
                            field
                                .select(
                                    [
                                        ("auto", "Auto-detect"),
                                        ("latlon", "Lat/lon (WGS84)"),
                                        ("meters", "Planar metres"),
                                    ],
                                    "auto",
                                )
                                .class("land-input land-mode");
                        })
                        .class("land-field");
                    })
                    .class("land-row");
                    card.container(|field| {
                        label(field, "Boundary points (one per line)");
                        field
                            .textarea("")
                            .attr("placeholder", "-43.53, 172.63")
                            .attr("rows", "7")
                            .class("land-input land-points");
                    })
                    .class("land-field");
                    card.text("\"lat, lon\" (WGS84 — projected to local metres) or \"x, y\" in planar metres, one vertex per line; the ring closes itself. Comments (#) and blank lines are skipped.")
                        .class("land-hint");
                    card.container(|row| {
                        row.button("Calculate area")
                            .class("land-btn land-btn-primary")
                            .on_click(Msg::Calculate);
                        row.button("Clear").class("land-btn").on_click(Msg::Clear);
                    })
                    .class("land-btnrow");
                    card.container(|row| {
                        for (i, (name, _, _)) in SAMPLES.iter().enumerate() {
                            row.button(format!("Try: {name}"))
                                .class("land-btn")
                                .on_click(Msg::Sample(i));
                        }
                    })
                    .class("land-btnrow");
                    card.container(|row| {
                        row.button("Upload GeoJSON")
                            .class("land-btn")
                            .on_click(Msg::UploadGeoJson);
                        #[cfg(feature = "pro")]
                        row.button("Import shapefile (.shp)")
                            .class("land-btn")
                            .on_click(Msg::UploadShapefile);
                    })
                    .class("land-btnrow");
                })
                .class("land-card");

                #[cfg(feature = "pro")]
                sidebar.container(|card| {
                    card.heading(3, "Buffer & overlay");
                    card.container(|row| {
                        row.container(|field| {
                            label(field, "Buffer distance (m, 0 = off)");
                            field.input("0").class("land-input land-buffer");
                        })
                        .class("land-field");
                    })
                    .class("land-row");
                    card.container(|row| {
                        row.button("Import overlay zone (GeoJSON)")
                            .class("land-btn")
                            .on_click(Msg::UploadZone);
                        row.button("Export map (PNG)")
                            .class("land-btn")
                            .on_click(Msg::ExportPng);
                        row.button("Export report (PDF)")
                            .class("land-btn")
                            .on_click(Msg::ExportPdf);
                    })
                    .class("land-btnrow");
                    card.text("Buffer and overlay apply to the selected parcel (pick it under Results once several are loaded) and recompute on Calculate. The zone is a GeoJSON polygon; its intersection is drawn orange.")
                        .class("land-hint");
                })
                .class("land-card");

                #[cfg(not(feature = "pro"))]
                sidebar.container(|card| {
                    card.heading(3, "Get the Pro edition");
                    card.text("The free edition measures one parcel at a time, right here. TPT Land Calc Pro is the offline Windows app for real parcel work:")
                        .class("land-hint");
                    card.container(|list| {
                        list.container(|li| {
                            li.text("Multi-parcel batch import — GeoJSON & shapefile, with an area table and totals");
                        });
                        list.container(|li| {
                            li.text("Buffer zones and overlay analysis, drawn on the map");
                        });
                        list.container(|li| {
                            li.text("Exportable maps (PNG) and PDF reports");
                        });
                        list.container(|li| {
                            li.text("No GIS license, no subscription — one-time purchase");
                        });
                    })
                    .attr("role", "list");
                    card.container(|price| {
                        price.text("US$129 once — ");
                        price.text("tptsolutions.co.nz/tools/land-calc");
                    })
                    .class("land-price");
                    card.button("See TPT Land Calc Pro \u{2192}")
                        .class("land-footer-link")
                        .on_click(Msg::VisitSite(HUB_URL.to_string()));
                })
                .class("land-card land-upsell");

                #[cfg(feature = "pro")]
                sidebar.container(|card| {
                    card.text("Pro edition — multi-parcel import, buffers, overlays and exports unlocked.")
                        .class("land-hint");
                    card.button("tptsolutions.co.nz/tools/land-calc \u{2192}")
                        .class("land-footer-link")
                        .on_click(Msg::VisitSite(HUB_URL.to_string()));
                })
                .class("land-card");
            })
            .class("land-sidebar");

            shell.container(|content| {
                content.container(|card| {
                    card.heading(3, "Map");
                    card.container(|_holder| {}).class("land-canvas-holder");
                })
                .class("land-card");
                content.container(|card| {
                    card.heading(3, "Results");
                    card.container(|_body| {}).class("land-results-body");
                })
                .class("land-card");
            })
            .class("land-content");
        })
        .class("land-main");
    })
}

/// The reactive results section — a pure function of `Outcome`.
fn results_view(outcome_signal: Signal<Outcome>) -> UITree<Msg> {
    let outcome = outcome_signal.get();
    let mut root_builder = tpt_appfront_core::ContainerBuilder::<Msg>::new();
    root_builder
        .container(|c| {
            c.container(|summary| {
                let (headline, detail) = match &outcome {
                    Outcome::Idle => (
                        "Enter boundary points, then press Calculate area.".to_string(),
                        "Or try a sample, or upload a GeoJSON boundary.".to_string(),
                    ),
                    Outcome::Invalid(issues) => (
                        format!(
                            "Fix {} issue{} before calculating:",
                            issues.len(),
                            if issues.len() == 1 { "" } else { "s" }
                        ),
                        String::new(),
                    ),
                    Outcome::Solved(solved) => {
                        let first = &solved.parcels[0];
                        (
                            format!(
                                "{} \u{b7} {} \u{b7} {}",
                                fmt_area_full(first.analysis.area_m2),
                                fmt_area_metric(first.analysis.area_m2),
                                fmt_area_imperial(first.analysis.area_m2)
                            ),
                            format!(
                                "perimeter {} \u{b7} {} \u{b7} {}",
                                fmt_len(first.analysis.perimeter_m),
                                if solved.projected {
                                    "lat/lon projected to local metres"
                                } else {
                                    "planar metres"
                                },
                                solved.source,
                            ),
                        )
                    }
                };
                styled_text(summary, headline, "land-headline");
                if !detail.is_empty() {
                    styled_text(summary, detail, "land-detail");
                }
                styled_text(
                    summary,
                    "Engine: shoelace area over the tpt-gis geometry core; GPS input projected to local planar metres before measuring.",
                    "land-detail",
                );
            })
            .class("land-summary");

            c.container(|issues| {
                if let Outcome::Invalid(list) = &outcome {
                    for issue in list {
                        styled_text(issues, issue.clone(), "land-issue");
                    }
                }
            });

            if let Outcome::Solved(solved) = &outcome {
                    // (pro) Target picker for multi-parcel solves.
                    #[cfg(feature = "pro")]
                    if solved.parcels.len() > 1 {
                        let options: Vec<(String, String)> = solved
                            .parcels
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                (
                                    i.to_string(),
                                    format!("{} ({})", p.name, fmt_area_smart(p.analysis.area_m2)),
                                )
                            })
                            .collect();
                        let selected = solved.target.to_string();
                        c.container(|row| {
                            row.container(|field| {
                                label(field, "Analyse target (buffer & overlay apply to this parcel)");
                                field
                                    .select(options, &selected)
                                    .class("land-input land-target")
                                    .on_input(|_| Msg::Calculate);
                            })
                            .class("land-field");
                        })
                        .class("land-row");
                    }

                    c.container(|grid_host| {
                        let mut rows: Vec<Vec<String>> = Vec::new();
                        // The free edition always shows the single-parcel
                        // detail table, even when an imported file held
                        // more (that's the skipped-parcels upsell note).
                        #[cfg(not(feature = "pro"))]
                        let multi = false;
                        #[cfg(feature = "pro")]
                        let multi = solved.parcels.len() > 1;
                        if multi {
                            for parcel in solved.parcels.iter() {
                                let share = if solved.total_m2 > 0.0 {
                                    parcel.analysis.area_m2 / solved.total_m2
                                } else {
                                    0.0
                                };
                                rows.push(vec![
                                    parcel.name.clone(),
                                    fmt_area_smart(parcel.analysis.area_m2),
                                    format!("{} of total", fmt_pct(share)),
                                ]);
                            }
                        } else {
                            let parcel = &solved.parcels[0];
                            rows.push(vec![
                                "Area".to_string(),
                                fmt_area_full(parcel.analysis.area_m2),
                                format!(
                                    "{} \u{b7} {} \u{b7} {} ft\u{b2}",
                                    fmt_area_metric(parcel.analysis.area_m2),
                                    fmt_area_imperial(parcel.analysis.area_m2),
                                    fmt_thousands(parcel.analysis.area_m2 / M2_PER_FT2, 0)
                                ),
                            ]);
                            rows.push(vec![
                                "Perimeter".to_string(),
                                fmt_len(parcel.analysis.perimeter_m),
                                "exterior boundary length".to_string(),
                            ]);
                            rows.push(vec![
                                "Centroid".to_string(),
                                format!(
                                    "({:.1}, {:.1}) m",
                                    parcel.analysis.centroid.0, parcel.analysis.centroid.1
                                ),
                                "area-weighted centre".to_string(),
                            ]);
                            rows.push(vec![
                                "Boundary".to_string(),
                                format!(
                                    "{} vertices{}",
                                    parcel.analysis.vertex_count,
                                    if parcel.analysis.hole_count > 0 {
                                        format!(", {} hole(s)", parcel.analysis.hole_count)
                                    } else {
                                        String::new()
                                    }
                                ),
                                String::new(),
                            ]);
                        }
                        if multi {
                            rows.push(vec![
                                "Total".to_string(),
                                fmt_area_full(solved.total_m2),
                                format!("{} parcels", solved.parcels.len()),
                            ]);
                        }
                        #[cfg(feature = "pro")]
                        {
                            if let Some(buffer) = &solved.buffer {
                                rows.push(vec![
                                    "Buffer".to_string(),
                                    format!(
                                        "{} \u{2192} {}",
                                        fmt_signed_len(buffer.distance_m),
                                        fmt_area_smart(buffer.result.area_m2)
                                    ),
                                    format!(
                                        "{} vs the parcel",
                                        fmt_signed_area(buffer.result.added_m2)
                                    ),
                                ]);
                            }
                            if let Some(overlay) = &solved.overlay {
                                rows.push(vec![
                                    "Overlay".to_string(),
                                    format!(
                                        "{} ({})",
                                        fmt_area_smart(overlay.result.area_m2),
                                        fmt_pct(overlay.result.share)
                                    ),
                                    format!("zone: {}", overlay.zone_name),
                                ]);
                            }
                        }
                        let headers: [&str; 3] = if solved.parcels.len() > 1 {
                            ["Parcel", "Area", "Share"]
                        } else {
                            ["Quantity", "Value", "Detail"]
                        };
                        grid_host.data_grid(headers, rows);
                    });

                    if let Some(skipped) = solved.skipped {
                        styled_text(
                            c,
                            format!(
                                "This file holds {skipped} more parcel{} — the free edition measures the first. Pro imports them all with an area table.",
                                if skipped == 1 { "" } else { "s" }
                            ),
                            "land-note",
                        );
                    }
                    #[cfg(feature = "pro")]
                    for note in &solved.notes {
                        styled_text(c, note.clone(), "land-note");
                    }
            }
        })
        .class("land-results-view");
    root_builder
        .into_only_child()
        .expect("results view builds exactly one root container")
}

// ---- map redraw ----------------------------------------------------------

fn redraw(container: &web_sys::Element, canvas: &web_sys::HtmlCanvasElement, outcome: Outcome) {
    let Ok(Some(ctx)) = canvas.get_context("2d").map(|c| c.and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())) else {
        return;
    };
    let theme = viewer::Theme::from_container(container);
    match outcome {
        Outcome::Solved(solved) if !solved.parcels.is_empty() => {
            #[cfg(feature = "pro")]
            let target = solved.target;
            #[cfg(not(feature = "pro"))]
            let target = 0usize;
            // Free edition draws the measured parcel only; Pro draws all.
            #[cfg(not(feature = "pro"))]
            let count = solved.parcels.len().min(1);
            #[cfg(feature = "pro")]
            let count = solved.parcels.len();
            let parcels: Vec<ParcelDraw> = solved
                .parcels
                .iter()
                .take(count)
                .enumerate()
                .map(|(i, row)| ParcelDraw {
                    name: row.name.clone(),
                    area_label: fmt_area_smart(row.analysis.area_m2),
                    polygon: &row.polygon,
                    color_index: i,
                    selected: i == target,
                })
                .collect();
            #[cfg(feature = "pro")]
            let buffer = solved.buffer.as_ref().map(|b| &b.result.polygon);
            #[cfg(not(feature = "pro"))]
            let buffer: Option<&tpt_gis::Polygon> = None;
            let fragments: Vec<Vec<(f64, f64)>> = {
                #[cfg(feature = "pro")]
                {
                    solved
                        .overlay
                        .as_ref()
                        .map(|o| o.result.fragments.clone())
                        .unwrap_or_default()
                }
                #[cfg(not(feature = "pro"))]
                Vec::new()
            };
            let scene = Scene {
                parcels,
                buffer,
                fragments: &fragments,
                theme,
            };
            viewer::draw(&ctx, &scene);
        }
        _ => {
            ctx.set_fill_style_str(&theme.bg);
            ctx.fill_rect(0.0, 0.0, viewer::W, viewer::H);
            ctx.set_font("14px -apple-system, 'Segoe UI', Roboto, sans-serif");
            ctx.set_fill_style_str(&theme.muted);
            let text = match outcome {
                Outcome::Invalid(_) => "Fix the issues, then press Calculate area.",
                _ => "Enter boundary points, then press Calculate area.",
            };
            let _ = ctx.fill_text(text, viewer::W / 2.0 - 180.0, viewer::H / 2.0);
        }
    }
}

// ---- (pro) exports -------------------------------------------------------

#[cfg(feature = "pro")]
fn export_png(container: &web_sys::Element) {
    let Some(canvas) = container
        .query_selector(".land-canvas")
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        return;
    };
    let closure = Closure::<dyn FnMut(Option<web_sys::Blob>)>::once(move |blob: Option<web_sys::Blob>| {
        if let Some(blob) = blob {
            download_blob(&blob, "land-calc-map.png");
        }
    });
    let _ = canvas.to_blob(closure.as_ref().unchecked_ref());
    closure.forget();
}

#[cfg(feature = "pro")]
fn export_pdf(_container: &web_sys::Element, outcome: &Signal<Outcome>) {
    let Outcome::Solved(solved) = outcome.get() else { return };
    let mut rows: Vec<tpt_land_engine::report::Row> = Vec::new();
    rows.push(tpt_land_engine::report::Row::Title(
        "TPT Land Calc Pro — parcel analysis".to_string(),
    ));
    rows.push(tpt_land_engine::report::Row::Note(format!(
        "Generated locally \u{b7} {} \u{b7} {}",
        solved.source,
        if solved.projected { "lat/lon projected to local metres" } else { "planar metres" }
    )));
    rows.push(tpt_land_engine::report::Row::Gap(6.0));
    let table_rows: Vec<Vec<String>> = solved
        .parcels
        .iter()
        .map(|p| {
            vec![
                p.name.clone(),
                fmt_area_full(p.analysis.area_m2),
                fmt_len(p.analysis.perimeter_m),
            ]
        })
        .collect();
    rows.push(tpt_land_engine::report::Row::Table {
        columns: vec!["Parcel".to_string(), "Area".to_string(), "Perimeter".to_string()],
        rows: table_rows,
        weights: Some(vec![0.4, 0.35, 0.25]),
    });
    rows.push(tpt_land_engine::report::Row::Head("Totals".to_string()));
    rows.push(tpt_land_engine::report::Row::Body(format!(
        "Total area across {} parcel{}: {}",
        solved.parcels.len(),
        if solved.parcels.len() == 1 { "" } else { "s" },
        fmt_area_full(solved.total_m2)
    )));
    #[cfg(feature = "pro")]
    {
        if let Some(buffer) = &solved.buffer {
            rows.push(tpt_land_engine::report::Row::Body(format!(
                "Buffer {} m: area {} ({} vs the parcel)",
                fmt_signed_len(buffer.distance_m),
                fmt_area_smart(buffer.result.area_m2),
                fmt_signed_area(buffer.result.added_m2)
            )));
        }
        if let Some(overlay) = &solved.overlay {
            rows.push(tpt_land_engine::report::Row::Body(format!(
                "Overlay zone {}: {} ({}) of the target parcel",
                overlay.zone_name,
                fmt_area_smart(overlay.result.area_m2),
                fmt_pct(overlay.result.share)
            )));
        }
        for note in &solved.notes {
            rows.push(tpt_land_engine::report::Row::Note(note.clone()));
        }
    }
    rows.push(tpt_land_engine::report::Row::Gap(8.0));
    rows.push(tpt_land_engine::report::Row::Note(
        "Engine: tpt-gis shoelace area; GPS input projected to local planar metres (equirectangular, mean-latitude anchored). Indicative, not a survey.".to_string(),
    ));

    let pdf = tpt_land_engine::report::render_report_pdf("TPT Land Calc", &rows);
    let array = js_sys::Uint8Array::from(pdf.as_slice());
    let parts = js_sys::Array::new();
    parts.push(&array);
    if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence(&parts) {
        download_blob(&blob, "land-calc-report.pdf");
    }
}

#[cfg(feature = "pro")]
fn download_blob(blob: &web_sys::Blob, name: &str) {
    let Some(window) = web_sys::window() else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(blob) else { return };
    let document = window.document().expect("no document");
    if let Ok(anchor) = document.create_element("a") {
        let _ = anchor.set_attribute("href", &url);
        let _ = anchor.set_attribute("download", name);
        let _ = document.body().unwrap().append_child(&anchor);
        let anchor = anchor.dyn_into::<web_sys::HtmlAnchorElement>().unwrap();
        anchor.click();
        anchor.remove();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

// ---- formatting ----------------------------------------------------------

fn fmt_thousands(v: f64, decimals: usize) -> String {
    let negative = v < 0.0;
    let scaled = format!("{v:.decimals$}");
    let (int_part, dec_part) = match scaled.split_once('.') {
        Some((i, d)) => (i.to_string(), Some(d.to_string())),
        None => (scaled, None),
    };
    let digits: String = int_part.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if negative { "\u{2212}" } else { "" };
    match dec_part {
        Some(d) => format!("{sign}{grouped}.{d}"),
        None => format!("{sign}{grouped}"),
    }
}

fn fmt_len(m: f64) -> String {
    if m >= 1000.0 {
        format!("{:.2} km", m / 1000.0)
    } else {
        format!("{} m", fmt_thousands(m, 1))
    }
}

#[cfg_attr(not(feature = "pro"), allow(dead_code))]
fn fmt_signed_len(m: f64) -> String {
    if m >= 0.0 {
        format!("+{} m", fmt_thousands(m, 0))
    } else {
        format!("\u{2212}{} m", fmt_thousands(-m, 0))
    }
}

#[cfg_attr(not(feature = "pro"), allow(dead_code))]
fn fmt_signed_area(m2: f64) -> String {
    if m2 >= 0.0 {
        format!("+{}", fmt_area_smart(m2))
    } else {
        format!("\u{2212}{}", fmt_area_smart(-m2))
    }
}

fn fmt_area_full(m2: f64) -> String {
    format!("{} m\u{b2}", fmt_thousands(m2, 1))
}

fn fmt_area_metric(m2: f64) -> String {
    format!("{} ha", fmt_thousands(m2 / M2_PER_HA, 3))
}

fn fmt_area_imperial(m2: f64) -> String {
    format!("{} ac", fmt_thousands(m2 / M2_PER_ACRE, 2))
}

/// One human unit for the headline and map labels.
fn fmt_area_smart(m2: f64) -> String {
    if m2 >= 1_000_000.0 {
        format!("{:.2} km\u{b2}", m2 / 1_000_000.0)
    } else if m2 >= 10_000.0 {
        format!("{:.2} ha", m2 / M2_PER_HA)
    } else {
        format!("{} m\u{b2}", fmt_thousands(m2, 0))
    }
}

fn fmt_pct(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}
