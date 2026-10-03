//! Canvas map renderer: parcels, buffer rings, overlay fragments, grid and
//! scale bar, fitted to the drawn geometry. Raw web-sys `<canvas>` — the
//! DOM backend has no canvas node kind (same split as FEA Lite's viewer).
//! Colors resolve from the app's CSS custom properties so dark mode is
//! automatic.

use tpt_gis::{BBox, Point, Polygon};

/// Internal canvas resolution (CSS scales it responsively).
pub const W: f64 = 960.0;
pub const H: f64 = 560.0;

/// Theme colors pulled from the app's CSS custom properties.
pub struct Theme {
    pub bg: String,
    pub grid: String,
    pub fg: String,
    pub muted: String,
    pub accent: String,
    pub overlay: String,
    pub label_bg: String,
}

impl Theme {
    /// Reads the `--land-*` variables off the mount container's computed
    /// style, with light-theme fallbacks (the CSS defines dark values via
    /// `prefers-color-scheme`, which `getComputedStyle` already resolved).
    pub fn from_container(container: &web_sys::Element) -> Theme {
        let css = |name: &str, fallback: &str| -> String {
            web_sys::window()
                .and_then(|w| w.get_computed_style(container).ok().flatten())
                .and_then(|style| style.get_property_value(name).ok())
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| fallback.to_string())
        };
        Theme {
            bg: css("--land-map-bg", "#f4f6f8"),
            grid: css("--land-map-grid", "rgba(20,30,45,.08)"),
            fg: css("--land-fg", "#181d27"),
            muted: css("--land-muted", "#667085"),
            accent: css("--land-accent", "#0e7c6b"),
            overlay: css("--land-overlay", "#b7641e"),
            label_bg: css("--land-map-label-bg", "rgba(255,255,255,.82)"),
        }
    }
}

/// One parcel to draw: outline plus label.
pub struct ParcelDraw<'a> {
    pub name: String,
    pub area_label: String,
    pub polygon: &'a Polygon,
    /// Palette index for multi-parcel maps (cycles 6 hues).
    pub color_index: usize,
    pub selected: bool,
}

/// Everything one frame draws.
pub struct Scene<'a> {
    pub parcels: Vec<ParcelDraw<'a>>,
    /// (pro) dashed buffer outline for the selected parcel.
    pub buffer: Option<&'a Polygon>,
    /// (pro) overlay intersection fragments as coordinate loops.
    pub fragments: &'a [Vec<(f64, f64)>],
    pub theme: Theme,
}

/// Draws the scene onto the canvas. Never panics on empty input — callers
/// render an empty-state note instead of calling this.
pub fn draw(ctx: &web_sys::CanvasRenderingContext2d, scene: &Scene) {
    let theme = &scene.theme;
    ctx.set_fill_style_str(&theme.bg);
    ctx.fill_rect(0.0, 0.0, W, H);

    let mut rings: Vec<&[Point]> = Vec::new();
    for parcel in &scene.parcels {
        rings.push(&parcel.polygon.exterior.points);
        for hole in &parcel.polygon.holes {
            rings.push(&hole.points);
        }
    }
    if let Some(buffer) = scene.buffer {
        rings.push(&buffer.exterior.points);
    }
    let Some(bbox) = fit_bbox(&rings) else { return };
    let view = ViewTransform::fit(bbox, W, H);

    draw_grid(ctx, &view, theme);

    // Overlay fragments first (under the parcels), filled and outlined.
    for frag in scene.fragments {
        if frag.len() < 3 {
            continue;
        }
        ctx.begin_path();
        for (i, (x, y)) in frag.iter().enumerate() {
            let (sx, sy) = view.to_screen(Point::new(*x, *y));
            if i == 0 {
                ctx.move_to(sx, sy);
            } else {
                ctx.line_to(sx, sy);
            }
        }
        ctx.close_path();
        // Fill only: fragments come from per-triangle clipping, so stroking
        // would draw their internal shared edges.
        ctx.set_fill_style_str(&theme.overlay);
        ctx.set_global_alpha(0.30);
        ctx.fill();
        ctx.set_global_alpha(1.0);
    }

    // Buffer ring (dashed) under the parcels.
    if let Some(buffer) = scene.buffer {
        ctx.begin_path();
        path_ring(ctx, &buffer.exterior, &view);
        for hole in &buffer.holes {
            path_ring(ctx, hole, &view);
        }
        ctx.set_stroke_style_str(&theme.accent);
        ctx.set_line_width(1.6);
        let _ = ctx.set_line_dash(&js_dash(7.0, 5.0));
        ctx.stroke();
        let _ = ctx.set_line_dash(&js_dash(0.0, 0.0));
    }

    // Parcels: fill, holes knocked out, outline, vertices, centroid label.
    for parcel in &scene.parcels {
        let color = palette(parcel.color_index, theme);
        ctx.begin_path();
        path_ring(ctx, &parcel.polygon.exterior, &view);
        for hole in &parcel.polygon.holes {
            path_ring(ctx, hole, &view);
        }
        ctx.set_fill_style_str(&color);
        ctx.set_global_alpha(if parcel.selected { 0.34 } else { 0.18 });
        ctx.fill_with_canvas_winding_rule(web_sys::CanvasWindingRule::Evenodd);
        ctx.set_global_alpha(1.0);

        ctx.begin_path();
        path_ring(ctx, &parcel.polygon.exterior, &view);
        ctx.set_stroke_style_str(&color);
        ctx.set_line_width(if parcel.selected { 2.6 } else { 1.4 });
        ctx.stroke();

        for p in &parcel.polygon.exterior.points {
            let (sx, sy) = view.to_screen(*p);
            ctx.set_fill_style_str(&color);
            ctx.begin_path();
            let _ = ctx.arc(sx, sy, if parcel.selected { 4.0 } else { 3.0 }, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }

        let c = parcel.polygon.centroid();
        let (cx, cy) = view.to_screen(c);
        ctx.set_fill_style_str(&theme.fg);
        ctx.begin_path();
        let _ = ctx.arc(cx, cy, 2.6, 0.0, std::f64::consts::TAU);
        ctx.fill();
        let label = if parcel.area_label.is_empty() {
            parcel.name.to_string()
        } else {
            format!("{} — {}", parcel.name, parcel.area_label)
        };
        draw_label(ctx, cx + 8.0, cy - 8.0, &label, theme);
    }

    draw_scale_bar(ctx, &view, theme);
}

struct ViewTransform {
    /// Screen px per world metre.
    scale: f64,
    origin: (f64, f64),
    bbox: BBox,
}

impl ViewTransform {
    /// Fits the bbox into the canvas with 8% padding; guards degenerate
    /// extents (a straight-line or single-point boundary) so scale stays
    /// finite.
    fn fit(bbox: BBox, w: f64, h: f64) -> Self {
        let width = (bbox.width() * 1.16).max(1e-6);
        let height = (bbox.height() * 1.16).max(1e-6);
        let scale = (w / width).min(h / height);
        let cx = bbox.min_x + bbox.width() / 2.0;
        let cy = bbox.min_y + bbox.height() / 2.0;
        ViewTransform {
            scale,
            origin: (w / 2.0 - cx * scale, h / 2.0 + cy * scale),
            bbox,
        }
    }

    fn to_screen(&self, p: Point) -> (f64, f64) {
        (self.origin.0 + p.x * self.scale, self.origin.1 - p.y * self.scale)
    }
}

fn fit_bbox(rings: &[&[Point]]) -> Option<BBox> {
    let mut bbox: Option<BBox> = None;
    for ring in rings {
        for p in ring.iter().copied() {
            bbox = Some(match bbox {
                None => BBox { min_x: p.x, min_y: p.y, max_x: p.x, max_y: p.y },
                Some(b) => BBox {
                    min_x: b.min_x.min(p.x),
                    min_y: b.min_y.min(p.y),
                    max_x: b.max_x.max(p.x),
                    max_y: b.max_y.max(p.y),
                },
            });
        }
    }
    bbox.map(|b| {
        let mut b = b;
        if b.width() < 1e-9 {
            b.min_x -= 1.0;
            b.max_x += 1.0;
        }
        if b.height() < 1e-9 {
            b.min_y -= 1.0;
            b.max_y += 1.0;
        }
        b
    })
}

fn path_ring(ctx: &web_sys::CanvasRenderingContext2d, ring: &tpt_gis::Ring, view: &ViewTransform) {
    for (i, p) in ring.points.iter().enumerate() {
        let (x, y) = view.to_screen(*p);
        if i == 0 {
            ctx.move_to(x, y);
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.close_path();
}

fn js_dash(on: f64, off: f64) -> wasm_bindgen::JsValue {
    let arr = js_sys::Array::new();
    if on > 0.0 {
        arr.push(&wasm_bindgen::JsValue::from_f64(on));
        arr.push(&wasm_bindgen::JsValue::from_f64(off));
    }
    arr.into()
}

fn draw_label(ctx: &web_sys::CanvasRenderingContext2d, x: f64, y: f64, text: &str, theme: &Theme) {
    ctx.set_font("600 13px -apple-system, 'Segoe UI', Roboto, sans-serif");
    let width = ctx.measure_text(text).map(|m| m.width()).unwrap_or(0.0);
    ctx.set_fill_style_str(&theme.label_bg);
    ctx.fill_rect(x - 3.0, y - 13.0, width + 6.0, 18.0);
    ctx.set_fill_style_str(&theme.fg);
    let _ = ctx.fill_text(text, x, y);
}

fn draw_grid(ctx: &web_sys::CanvasRenderingContext2d, view: &ViewTransform, theme: &Theme) {
    // ~6 divisions across, rounded to a nice 1/2/5 × 10ⁿ metre step.
    let step = nice_step(view.bbox.width() / 6.0);
    ctx.set_stroke_style_str(&theme.grid);
    ctx.set_line_width(1.0);
    let mut x = (view.bbox.min_x / step).floor() * step;
    while x <= view.bbox.max_x + step {
        let (sx, sy0) = view.to_screen(Point::new(x, view.bbox.max_y));
        let (_, sy1) = view.to_screen(Point::new(x, view.bbox.min_y));
        ctx.begin_path();
        ctx.move_to(sx, sy0);
        ctx.line_to(sx, sy1);
        ctx.stroke();
        x += step;
    }
    let mut y = (view.bbox.min_y / step).floor() * step;
    while y <= view.bbox.max_y + step {
        let (sx0, sy) = view.to_screen(Point::new(view.bbox.min_x, y));
        let (sx1, _) = view.to_screen(Point::new(view.bbox.max_x, y));
        ctx.begin_path();
        ctx.move_to(sx0, sy);
        ctx.line_to(sx1, sy);
        ctx.stroke();
        y += step;
    }
}

fn draw_scale_bar(ctx: &web_sys::CanvasRenderingContext2d, view: &ViewTransform, theme: &Theme) {
    let metres = nice_step(W * 0.18 / view.scale);
    let x0 = 18.0;
    let y = H - 22.0;
    let x1 = x0 + metres * view.scale;
    ctx.set_stroke_style_str(&theme.fg);
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.move_to(x0, y - 5.0);
    ctx.line_to(x0, y + 5.0);
    ctx.move_to(x0, y);
    ctx.line_to(x1, y);
    ctx.move_to(x1, y - 5.0);
    ctx.line_to(x1, y + 5.0);
    ctx.stroke();
    let label = if metres >= 1000.0 {
        format!("{} km", trim_num(metres / 1000.0))
    } else {
        format!("{} m", trim_num(metres))
    };
    ctx.set_font("11px -apple-system, 'Segoe UI', Roboto, sans-serif");
    ctx.set_fill_style_str(&theme.muted);
    let _ = ctx.fill_text(&label, x1 + 8.0, y + 4.0);
}

/// Rounds a target length up to 1/2/5 × 10ⁿ — keeps grid and scale-bar
/// numbers human.
fn nice_step(target: f64) -> f64 {
    if target <= 0.0 || !target.is_finite() {
        return 1.0;
    }
    let mag = 10f64.powf(target.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        let step = m * mag;
        if step >= target {
            return step;
        }
    }
    10.0 * mag
}

fn trim_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Six distinguishable parcel hues; index 0 is the app accent.
fn palette(index: usize, theme: &Theme) -> String {
    match index % 6 {
        0 => theme.accent.clone(),
        1 => "#7c5cbf".to_string(),
        2 => "#b7641e".to_string(),
        3 => "#2563a8".to_string(),
        4 => "#a83a6b".to_string(),
        _ => "#5f7a1e".to_string(),
    }
}
