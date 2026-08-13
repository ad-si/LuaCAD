# Changelog

All notable changes to this project are documented in this file.

The stable, semver-governed surface is the **Lua API** and the **`luacad`
command line interface**. The Rust modules of the `luacad` crate are published
so that the CLI and Studio can share them; they are internal and may change in
any release.


## 2026-08-13 - 1.0.0

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
- Full support for the Belfry OpenSCAD Library v2 (BOSL2) under `bosl.*`.
- SVG and DXF import, returning a sketch.
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
  of writing `$fn` where LuaCAD expects `fn`.
- Lua errors and tracebacks now point at the user's script rather than at
  LuaCAD's own Rust source. An error on line 4 of `model.lua` reports
  `model.lua:4` instead of `crates/luacad/src/lua_engine.rs:1527:4`.
- `luacad <unknown-command>` reports an unknown command and lists the valid
  ones, rather than trying to open the command as a file.
- Constructs that only exist as OpenSCAD — `bosl.*`, `text()`, `text3d()`,
  `surface()`, `scad()` and `import()` of a mesh file — are now named when a
  mesh export or `luacad render` cannot represent them, instead of being
  dropped from the output without a word. `luacad info` reports them as a
  warning next to the triangle counts that exclude them.
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
