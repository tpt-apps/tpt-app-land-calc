# TPT Land Calc — Build Todo

> Hub listing: `tptsolutions.co.nz/tools/land-calc` (registry slug
> `land-calc`, category `engineering`). Spec: `apps.md` §T1-13 in the
> tpt-electrician-nz-2 repo. Free WASM edition: single-parcel area calculation
> from uploaded boundary points. Paid offline edition ($129 USD, Gumroad):
> multi-parcel analysis, buffer-zone/overlay calculations, and exportable
> maps/reports. Solver wraps `tpt-gis`. UI on `tpt-appfront`.
>
> Hub contract: `wasm-bindgen --target web` glue exposes `mount(container)`
> (called by `WasmAppRunner` after `default()` init). Standalone `trunk serve`
> mounts into the `#tpt-appfront-root` marker div instead — never
> `document.body` — so the two hosts never double-mount.

## Phase 0 — Scaffold & pipeline proof
- [x] Scaffold (workspace mirroring fea-lite: root wasm crate + `engine/` +
      `desktop/`, path deps on the tpt-appfront checkout; the `tpt-appfront
      init` CLI was skipped — the fea-lite structure was mirrored by hand)
- [x] Hub contract: `#[wasm_bindgen] pub fn mount(el)` + `#[wasm_bindgen(start)]`
      no-op unless `#tpt-appfront-root` present
- [x] `index.html` carries the `#tpt-appfront-root` marker div
- [x] Build pipeline: `build.ps1` / `build.sh` mirroring fea-lite's
      (`--target web` glue → web repo `public/apps/land-calc/`; `-Pro` builds
      the trunk bundle + desktop exe and never touches the free bundle)
- [x] Standalone dev page verified (`trunk serve`, browser-tested)
- [x] git init + commits
- [x] Base repo `tpt-gis` did not exist on this machine — created it
      (`../tpt-gis`): shoelace area/perimeter/centroid, local lat/lon
      projection, miter/bevel buffer, ear-clip + Sutherland–Hodgman overlay,
      GeoJSON parse/serialize, geometry-only `.shp` reader; 18
      hand-calculated validation tests

## Phase 1 — Solver integration (wrap tpt-gis)
- [x] `engine/` adapter crate `tpt-land-engine`: point-list parsing,
      coordinate-mode detection, GeoJSON/shapefile loaders with a *shared
      projection frame* (parcels and overlay zones land in one frame —
      per-load anchors would shift overlays), `analyze_parcel` → plain
      structs; `pro` gates: buffer, overlay, shapefile batch, PDF report
- [x] Validation: `engine/tests/validation.rs` — 12 hand-calculated cases
      (5,000 m² rectangle, lat/lon projection vs scales, buffer Steiner
      checks, 50% overlays, frame sharing, shapefile bytes, PDF envelope)

## Phase 2 — Free WASM UI
- [x] Boundary point entry (`lat, lon` / `x, y`, comments + closing-point
      tolerant) + GeoJSON upload; single-parcel area/perimeter/centroid in
      m² · ha · ac · ft²
- [x] Canvas map: grid, scale bar, vertices, centroid + area label; dark
      mode via CSS custom properties
- [x] Free-tier upsell card + "this file holds N more parcels" note when a
      multi-parcel GeoJSON is uploaded
- [x] Browser-verified: mount, all three samples (metric exact 5,000 m²,
      L-shape 1,400 m², GPS projection 8,969 m²), synthetic GeoJSON upload,
      map renders

## Phase 3 — Paid desktop edition (cargo feature `pro`)
- [x] `pro` gates: multi-parcel GeoJSON + shapefile batch import (area
      table, shares, total, target picker), buffer zones, overlay analysis,
      PNG map + PDF report export
- [x] Pro UI: map draws all parcels (palette), dashed buffer ring, filled
      overlay fragments; PDF via the engine's dependency-free writer
- [x] Desktop shell `tpt-land-desktop` (webview, embedded dist, single
      instance, allowlisted `open_external`); `tpt-land-calc-pro.exe` builds
      and launches (smoke-tested)
- [x] Browser-verified pro build: 3-parcel import (28,000 m² total, shares),
      buffer +10 m → +6,000 m² exact, east-half lat/lon zone → 50.0%
      overlay exact, both exports fire downloads, small planar shapefile
      with the metres selector → 1,200 m² exact
- [ ] Gumroad product from GUMROAD.md listing copy ($129), exe zip upload,
      URL into registry — manual step; artifact is
      `target\release\tpt-land-calc-pro.exe` (embeds its own dist, zip alone
      is enough)

## Phase 4 — Ship & measure
- [x] Registry entry added (`engineering`, `wasm: { entry:
      '/apps/land-calc/land-calc.js' }`, `coming-soon` until the Gumroad
      listing exists) + `public/apps/land-calc/` populated by `build.ps1`
- [x] WASM size budget: 278,816 bytes raw ≈ 112 KB gzipped (budget ~2 MB)
- [ ] Verify hub card/detail/sitemap/runner in dev (`npm run dev`), then in
      prod after deploy; flip registry `status` to `'live'` once the
      Gumroad listing is up (see GUMROAD.md step 8)
