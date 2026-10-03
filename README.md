# TPT Land Calc

Parcel area, boundary, and buffer-zone analysis for property parcels —
built for small surveying firms, rural property buyers, and forestry/farm
consultants who don't need an ArcGIS license.

- **Free in-browser edition** (this repo's default build): single-parcel
  area from pasted GPS/planar points or an uploaded GeoJSON boundary.
  Ships to `tptsolutions.co.nz/tools/land-calc` as a WASM bundle.
- **Pro desktop edition** (`--features pro`, $129 USD on Gumroad):
  multi-parcel GeoJSON/shapefile batch import, buffer-zone and overlay
  analysis, and exportable PNG maps / PDF reports, offline in a Windows
  webview shell.

Stack: [`tpt-appfront`](../tpt-appfront) DOM backend UI over the
[`tpt-land-engine`](engine/) adapter crate, which wraps the
[`tpt-gis`](../tpt-gis) geometry primitives (shoelace area, local
equirectangular lat/lon projection, miter buffering, ear-clip +
Sutherland–Hodgman overlay, GeoJSON, geometry-only shapefile reader).

## Repo layout

```
src/       wasm UI crate (hub contract: mount(container) + #tpt-appfront-root)
engine/    tpt-land-engine — boundary in, plain result structs out; host-testable
desktop/   tpt-land-desktop — native webview shell embedding the Pro bundle
```

## Commands

```sh
cargo test                              # engine validation (hand-calculated cases)
cargo test -p tpt-land-engine --features pro
trunk serve                             # standalone dev (mounts into #tpt-appfront-root)
./build.ps1                             # free bundle → web repo public/apps/land-calc/
./build.ps1 -Pro                        # pro trunk bundle (dist/) + desktop exe
```

`build.ps1` copies the free bundle into
`D:\Programming\2 WIP\TPT Electrician\tpt-electrician-nz-2\public\apps\land-calc\`
by default (override with `TPT_WEB_REPO`). The free and pro builds are
separate cargo builds on purpose — never build the shipped free bundle with
`--features pro`.

## Coordinate handling

Point lists accept `lat, lon` (WGS84) or `x, y` (planar metres), one vertex
per line; the format selector (auto-detect / lat/lon / metres) also governs
file imports. Lat/lon geometry is projected through a local
equirectangular frame anchored at the dataset's mean position, and *every*
geometry in one analysis shares that frame (parcels and overlay zones
alike), so overlays stay correct. Areas are indicative, not a survey.

## Status

See [todo.md](todo.md) for the phase list and
[GUMROAD.md](GUMROAD.md) for the Pro listing copy/recipe.
