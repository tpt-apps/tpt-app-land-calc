# Gumroad listing

Everything needed to create/update the Gumroad product once this app is
built. Source copy is `apps.md` §T1-13 in the `tpt-electrician-nz-2` repo — if
that changes, update here too (and vice versa).

## Step-by-step: publish this listing

1. Build the Pro edition (`./build.ps1 -Pro`, once the build pipeline exists
   — see `todo.md` and mirror `tpt-app-fea-lite`/`tpt-app-cutlist-optimizer`'s
   pattern) — produces the packaged exe/zip in `release/`.
2. Log in to Gumroad → **New product** → **Digital product**.
3. Set the product name, price, and upload the packaged file (all below).
4. Paste the short description, bullets, and full description (below) into
   their respective fields.
5. Set category/tags (below) and add a cover image (see note below).
6. Publish, then copy the resulting product URL (`https://gum.co/...`).
7. Paste that URL into the `land-calc` entry's `gumroadUrl` field in
   `tpt-electrician-nz-2\src\lib\tpt-apps-registry.ts`, and set `price: '$129'`
   on the same entry.
8. Flip that entry's `status` to `'live'` once both the free demo and this
   Gumroad listing exist.
9. Commit and push the registry repo.

## File to upload

Built: `targetelease	pt-land-calc-pro.exe`, produced by
`./build.ps1 -Pro`. The exe embeds its own web bundle (extracts to the OS
temp dir on first launch), so the Gumroad zip needs nothing but the exe.
Smoke test after building: double-click → window opens with the Pro chip
visible.

## Product name

```
TPT Land Calc — Parcel Area & Boundary Analysis Without ArcGIS
```

## Price

**$129 USD, one-time purchase** (no subscription).

## Short description / summary field

```
Area, buffer zones, and boundary analysis for property parcels — built for small surveying firms, rural buyers, and forestry consultants who don't need a full GIS license.
```

## Bullets / feature list

```
Parcel area & boundary calculations
Buffer-zone/overlay analysis
Exportable maps/reports
No GIS license required
One-time purchase
```

## Full description (long-form field)

```
TPT Land Calc loads a property boundary (shapefile/GeoJSON/GPS points) and
computes area, buffer zones, and basic spatial overlays — the specific
calculations these users need without full GIS software.

Who it's for: small land surveying firms, rural property buyers, and
forestry/farm management consultants doing basic parcel analysis without an
ArcGIS license.

Pro edition (this download):
- Multi-parcel analysis (not just a single parcel)
- Buffer-zone/overlay calculations
- Exportable maps/reports

Try the free single-parcel area calculation first, right in your browser, no
install: tptsolutions.co.nz/tools/land-calc

Requirements: Windows 10 (20H2+) or Windows 11. Uses the WebView2 runtime,
preinstalled on both — nothing extra to download.
```

## Category / tags

- Category: **Software > Utility** (or Gumroad's closest equivalent)
- Tags: `gis`, `land-surveying`, `parcel-mapping`, `forestry`,
  `property-analysis`, `windows`

## Cover image / thumbnail

Not created yet. A screenshot of a multi-parcel map with buffer zones and an
overlay drawn is the most convincing cover — the Pro map card now produces
exactly that (import parcels, set a buffer, import a zone, export PNG). No
stock/AI art.

## After publishing

1. Copy the resulting product URL (`https://gum.co/...`).
2. Paste it into the `gumroadUrl` field of the `land-calc` entry in
   `tpt-electrician-nz-2\src\lib\tpt-apps-registry.ts`, and set `price: '$129'`.
3. Set `status` to `'live'`.
4. Commit and push the registry repo.
