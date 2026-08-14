# Changelog

All notable changes to this project are documented in this file.

The stable, semver-governed surface is the **Lua API** and the **`luacad`
command line interface**. The Rust modules of the `luacad` crate are published
so that the CLI and Studio can share them; they are internal and may change in
any release.


## 2026-08-14 - 1.0.0

First stable release.

### Added

- Manifold as the default CSG backend. `csgrs` moved behind an optional
  feature flag, and OpenSCAD remains available via `--via-openscad`.
- Real-time CSG preview in Studio, rendered through OpenCSG.
- `luacad info` for geometry metadata, `luacad lint` for linting scripts with
  selene, and `luacad render` for writing a PNG preview.
- OFF and AMF export, alongside the existing 3MF, STL, OBJ, PLY and SCAD.
- Every value a script returns becomes its own 3MF object, so slicers load
  them as individually movable parts. Name them with `:name(…)`.
- The Belfry OpenSCAD Library v2 (BOSL2) under `bosl.*`, reimplemented in
  full. All 527 functions are built or computed by LuaCAD itself, so they
  render, preview and export to a mesh with neither OpenSCAD nor BOSL2
  installed:
  - The 2D and 3D shapes, with their `anchor`/`spin`/`orient` placement, the
    `edges`/`except` selectors, and per-end and per-corner rounding and
    chamfering.
  - The transforms, distributors, partitions and 2D and 3D masks.
  - Paths, drawing, Bézier curves and patches, rounding, skinning, sweeps
    and the VNF functions.
  - The parts libraries: threading, screws and nuts from the ISO metric
    tables, gears, joiners, sliders, bearings, NEMA steppers, wiring,
    walls, cubetruss, hinges, bottlecaps, polyhedra and tripod mounts.
  - The pure functions — math, vectors, coordinates, lists, linear algebra
    and geometry — which return real Lua values rather than OpenSCAD source,
    so a script can compute with them.

  The shapes are tested against the same call rendered by OpenSCAD with BOSL2
  installed, and agree to within half a percent of volume.
- Exporting to `.scad` still writes the BOSL2 call itself, so a model stays as
  short and readable as the script that produced it.
- 2D `bosl.*` shapes return a sketch, so `linear_extrude()`,
  `rotate_extrude()` and `offset()` apply to them.
- SVG and DXF import, returning a sketch.
- `import()` of a mesh returns a solid the Manifold backend can transform and
  combine, in every format LuaCAD exports: 3MF, STL, OBJ, PLY, OFF and AMF.
- `text()` and `text3d()` outline a system font into real geometry, so text
  exports to a mesh and previews in Studio without going through OpenSCAD.
  Fonts are selected by family and optional style (`"DejaVu Sans:style=Bold"`),
  with `halign`, `valign` and kerning applied.
- Support for the OpenSCAD modifier characters (`*`, `!`, `#`, `%`).
- All 147 CSS3/SVG named colors, matching OpenSCAD, plus hex color strings
  (`#RGB`, `#RRGGBB`, `#RGBA`, `#RRGGBBAA`).
- `require` resolves relative to the script being run, so models can be split
  across files regardless of the working directory. `require("luacad")` is a
  no-op returning the globals table.
- 2D shapes and extrusions in the Manifold path; `linear_extrude(height, opts)`
  accepts an options table.
- `scad()` for inserting verbatim OpenSCAD code, `var()` for customizer
  variables, and `cad()`.
- Studio: find and replace, a settings dialog, preset camera views and their
  shortcuts, panning, infinite axis lines, 2x supersampling, unsaved-change
  tracking with a confirmation before closing, reload with an external-change
  warning, timestamped default export filenames, and inline lint diagnostics.
- CI covering macOS, Linux and Windows, including both arm64 targets.

### Changed

- Unknown named parameters are now rejected instead of silently ignored.
  `cube { size = { 1, 2, 3 }, centre = true }` used to build an uncentred cube
  without complaint; it now raises an error naming the valid parameters and
  suggesting the one you probably meant. This also catches the OpenSCAD habit
  of writing `$fn` where LuaCAD expects `fn`. `bosl.*` shapes are held to the
  same rule, against the parameters the BOSL2 module actually declares, so
  `bosl.cuboid { …, center = … }` is an error pointing at `anchor`: BOSL2's
  `cuboid()` has no `center`, and OpenSCAD dropped it in silence and built a
  centred cuboid whatever you asked for.
- Lua errors and tracebacks now point at the user's script rather than at
  LuaCAD's own Rust source. An error on line 4 of `model.lua` reports
  `model.lua:4` instead of `crates/luacad/src/lua_engine.rs:1527:4`.
- `luacad <unknown-command>` reports an unknown command and lists the valid
  ones, rather than trying to open the command as a file.
- Constructs that only exist as OpenSCAD — `surface()`, `scad()` and
  `import()` of a DXF file — are now named when a mesh export or
  `luacad render` cannot represent them, instead of being dropped from the
  output without a word. `luacad info` reports them as a warning next to the
  triangle counts that exclude them.
- `offset(r = …)` facets its rounded corners the way OpenSCAD does, at the
  same `$fa`/`$fs` defaults, so a rounded outline has the same vertices
  whichever backend renders it.
- `--via-openscad` and `--via-manifold` are rejected as a pair instead of the
  latter silently winning.
- The default segment count for round primitives went from 16 to 32.
- Studio dropped `three-d` in favour of using egui, glow and cgmath directly.
- The vendored Manifold and OpenCSG sources moved inside their respective
  `-sys` crates, and Clipper2 is now vendored too. Building no longer requires
  network access.
- `hull()` and `minkowski()` are evaluated lazily and their subtrees memoized.

### Fixed

- Polyhedron winding and quad triangulation.
- `difference()` not subtracting union children.
- Minkowski sums with operands positioned away from the origin, stray-triangle
  artifacts in hull and Minkowski results, and a rendering hole in models
  combining Minkowski with booleans.
- Preview of CSG trees that do not fit into a single OpenCSG product.
- Deep BSP recursion overflowing the stack on models with many nearly-coplanar
  polygons; everything now runs on a 512 MB stack.
- Missing models on Linux aarch64 and on GL 3.0+ contexts, and the Windows
  build under MSVC.
- A panic when reading the mesh of a geometry that had neither a mesh nor a
  SCAD tree; such a geometry is now simply empty.
- `--via-openscad` staged its generated SCAD at a fixed temporary path, so two
  exports running at once — a `watch` in another terminal, a parallel build —
  could overwrite each other's source between writing it and OpenSCAD reading
  it, and quietly export the wrong model.


## 2026-02-24 - 0.1.0

First release of the Rust rewrite, published as the `luacad` and
`luacad-studio` crates.

### Added

- Lua 5.4 scripting engine embedding OpenSCAD-compatible primitives and
  transformations, with operator overloading for CSG (`a + b`, `a - b`).
- `luacad` CLI with `run`, `convert` and `watch` subcommands, exporting to
  STL, OBJ, PLY, 3MF and SCAD. `--via-openscad` delegates export to an
  installed OpenSCAD.
- `luacad-studio`, a desktop app pairing a code editor with a 3D viewport,
  built on `three-d`.
- csgrs as the CSG evaluation backend.
- 2D and 3D primitives (`cube`, `sphere`, `cylinder`, `polyhedron`, `torus`,
  `ellipsoid`, `octahedron`, `icosahedron`, `pyramid`, `circle`, `rect`,
  `polygon`, `text`, `text3d`, `surface`, `import`) and the transformations
  and operations to combine them (`translate`, `rotate`, `scale`, `mirror`,
  `multmatrix`, `resize`, `hull`, `minkowski`, `linear_extrude`,
  `rotate_extrude`, `projection`, `color`).


## Earlier history

LuaCAD began in 2015 as a pure Lua implementation by Michael Lutz at
[thechillcode/Lua_CAD](https://github.com/thechillcode/Lua_CAD), which
generated OpenSCAD code for external rendering. That implementation was
developed until February 2026, when the project was rewritten in Rust; it is
kept in `legacy_lua/` for reference. It was never published as a versioned
release, so it has no entries above.
