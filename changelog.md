# Changelog

All notable changes to this project are documented in this file.

The stable, semver-governed surface is the **Lua API** and the **`luacad`
command line interface**. The Rust modules of the `luacad` crate are published
so that the CLI and Studio can share them; they are internal and may change in
any release.


## Unreleased

### Added

- OpenSCAD files can be opened directly: `.scad` works anywhere `.lua` does,
  on the command line (`run`, `info`, `convert`, `watch`, `render`) and in
  Studio, through File → Open or by dropping one on the window. LuaCAD parses
  and evaluates the language itself, so no OpenSCAD installation is involved
  and this is unrelated to `--via-openscad`. `include`/`use` resolve relative
  to the file and then against `OPENSCADPATH`.

  The front end is a vendored copy of [OpenRSCAD]'s — a clean-room
  reimplementation of OpenSCAD 2021.01, Apache-2.0 OR MIT — as the new
  `luacad-scad-syntax`, `luacad-scad-ir` and `luacad-scad-eval` crates. Only
  its parser and evaluator are taken; Manifold still does the meshing. It
  bundles no fonts, unlike upstream, so `text()` uses installed system fonts
  the way LuaCAD's own `text()` already does.

  Both languages lower to the same tree, so an opened `.scad` file reaches
  every export format, the PNG renderer, the path tracer and Studio's live
  preview unchanged. `luacad convert model.scad out.scad` round-trips it with
  modules inlined and `$fn`/`$fa`/`$fs` resolved to facet counts.

  Three constructs cannot be carried across exactly and warn rather than
  quietly differing: `linear_extrude` with a non-uniform `scale`,
  `resize(auto = …)`, and an `import()` in a format LuaCAD cannot read.
  Studio does not lint `.scad` buffers, and the browser playground stays
  Lua-only.

  [OpenRSCAD]: https://github.com/matthova/openrscad

- The new `examples/chess/` is the first example written in OpenSCAD rather
  than Lua: a game in progress, laid out from a FEN string parsed by a
  recursive function in the `.scad` file itself. It puts SVG import, mesh
  import, `rotate_extrude`, `linear_extrude`, booleans, `search()` and
  recursion through the new front end in one model, and is meant to be seen
  with `--raytrace`. The pieces are from [scad-chess], CC-BY-4.0; the board,
  the parser and the position are new. `make example-images` now regenerates
  images for `.scad` entry points too, not only `.lua` ones.

  [scad-chess]: https://github.com/quaternionmedia/scad-chess

- Studio: the selected projection is remembered across restarts
  ([#18](https://github.com/ad-si/LuaCAD/issues/18)), so a perspective view
  no longer falls back to orthogonal on every launch (`orthogonal_view` in
  the state file, next to `hide_editor` and `auto_reload`). Resetting the
  camera now also uses the distance that matches the current projection
  instead of always the orthogonal one.

- Studio: `-h` / `--help` and `-v` / `--version` print their information and
  exit instead of starting the GUI
  ([#15](https://github.com/ad-si/LuaCAD/issues/15)). The file to open is now
  a declared argument (`luacad-studio [file.lua]`), so an unknown flag is
  reported instead of being taken for a file name. The same version
  information is in the GUI under Settings → About, reachable through the new
  `ℹ About` button (which is also in the bottom bar while the code editor is
  hidden), together with the target the binary was built for and a button
  that copies it all for a bug report.

- Both binaries append `git describe --always --dirty --tags` to their version
  when they are built from a git checkout, e.g.
  `luacad 1.1.0 (v1.1.0-3-g23a0ea2-dirty)`, so a locally built binary can be
  traced back to its commit. Builds from crates.io (and from a clean release
  tag) print the plain version. `LUACAD_GIT_DESCRIBE` overrides the suffix at
  build time — set it to an empty value to drop it, e.g. for a reproducible
  distro package.

- Studio: the opened file is watched and reloaded automatically when another
  program changes it on disk
  ([#14](https://github.com/ad-si/LuaCAD/issues/14)), matching `luacad watch`
  and OpenSCAD's automatic reload. Auto-reload is skipped while the editor has
  unsaved changes (the existing "File Changed on Disk" dialog resolves the
  conflict on the next save). It can be turned off in Settings → General; the
  choice is remembered across restarts (`auto_reload` in the state file).

- Studio: the code editor panel can be hidden to use an external editor and
  keep the whole window for the model
  ([#12](https://github.com/ad-si/LuaCAD/issues/12)). Toggle it with the
  checkbox in Settings → General, the `Editor` button in the bottom bar, or
  `Cmd`/`Ctrl` + `E`. The choice is remembered across restarts
  (`hide_editor` in the state file, like OpenSCAD's `hideEditor`). While the
  panel is hidden, its `Run` and `Reload` buttons move into the bottom bar
  (which now wraps in narrow windows), and errors show up there as well.

- Surface materials via `shape:material(...)`, on 3D geometry and on 2D
  sketches (where they survive extrusion). The kinds are `matte`, `plastic`
  (the implicit default look), `metal`, `glass`, and `emissive`, with
  parameter overrides per kind: `material("glass", {ior = 1.5, roughness =
  0.1})`, `material({kind = "emissive", strength = 4})`. The presets `steel`,
  `chrome`, `gold`, `copper`, `brass`, `rubber`, `wood`, and `ivory` also
  carry a default color, used only when no `color()` is set. `luacad render --raytrace` maps
  each kind onto a real BSDF (metals reflect, glass refracts, emissive shapes
  light the scene); the rasterizer and the Studio preview approximate them
  with per-object highlight parameters. Materials have no OpenSCAD
  equivalent, so a `.scad` export omits them. An unknown material name is an
  error rather than a silent fallback. See the new `examples/materials/`.
- Procedural wood grain: the `wood` preset now shows noise-warped growth
  rings instead of a flat color, in both the rasterizer and `--raytrace`.
  The rings are concentric around a configurable axis and darken the base
  color (an explicit `color()` is kept as the earlywood tone). Options:
  `material("wood", {ring_width = 4, grain_axis = {1, 0, 0}, grain_offset =
  {0, 0, -250}, grain_contrast = 0.5, grain_distortion = 0.3, grain =
  false})` — `ring_width` in model units, `grain_axis` the log's long
  direction (default z), `grain_offset` a point that axis passes through
  (the log's center line; move it away from a part for flatter, more even
  rings), `grain_contrast` how dark the latewood bands are,
  `grain_distortion` the ring waviness, and `grain = false` restores the
  flat color. Setting any grain option on
  another matte/plastic/metal material enables grain there too. The grain is
  anchored in world space, so a moved part is "cut from a different spot in
  the log".

### Changed

- `--via-openscad` runs the binary named by the `OPENSCAD` environment
  variable when it is set, and `openscad` from `PATH` otherwise — so a
  development snapshot can be used without displacing a distribution's
  release. The differential tests against BOSL2 take the same variable, and
  now skip themselves against OpenSCAD 2021.01 rather than measuring LuaCAD
  against a reference five years behind the behavior it tracks.

- The minimum supported Rust version of `luacad` and `luacad-studio` is now
  1.96, raised by the vendored path tracer behind `--raytrace`. The
  `luacad-manifold-sys` and `opencsg-sys` crates still build on 1.89.

- `import()` of an SVG reads unitless coordinates at 72 dpi rather than 96,
  the default OpenSCAD uses, so a drawing without physical units no longer
  comes in 4/3 too small compared to the same file there. An SVG that states
  its size in mm, cm or inches is unaffected.

- The `legacy_lua/` directory — the pure Lua implementation the project was
  rewritten from in February 2026 — is no longer part of the repository. It
  had not been touched since the rewrite; it can be read at the `v1.1.0` tag
  and lives on at
  [thechillcode/Lua_CAD](https://github.com/thechillcode/Lua_CAD).

- `polygon()` in the SCAD tree carries optional contour index lists, resolved
  with the even-odd rule, so a polygon can have holes. The Lua `polygon()` is
  unchanged; this is what lets an imported `polygon(points, paths)` — and the
  counters in an OpenSCAD `text()` — come through as holes rather than
  filling in.

### Fixed

- A part that cannot be built — `linear_extrude()` with a height of zero or
  less, `cube(0)`, `sphere(r = 0)`, `cylinder(h = 0)` — emptied everything it
  was combined with instead of just contributing nothing, in both languages:
  `union() { cube(2); sphere(r = 0); }` came out empty, and a single such part
  anywhere in a `.scad` file could leave the whole model blank. Manifold
  refuses to build a solid from a non-positive measurement and reports the
  result as an error rather than as empty, and every boolean an error solid
  reaches inherits it. Such a part is now dropped before it gets that far, as
  OpenSCAD does with the same input. Anything that builds keeps its exact
  geometry — the measurements are checked directly rather than by asking
  Manifold afterwards, which would force its deferred evaluation and shift the
  triangulation of models that are perfectly fine.

- `linear_extrude()` in a `.scad` file warns when its height is zero,
  negative or not a number, rather than silently producing nothing. OpenSCAD
  leaves the object empty without a word; a height that came out of a
  parameter is easy to get wrong, and a part that quietly disappears is hard
  to trace back to the call that dropped it.

- `render --raytrace` drew the triangulation of large flat faces into the
  image as thin dark lines, most visible where the light grazes the face —
  a fan of them across the side of a big brick, a seam down a baseplate.
  Manifold cuts such a face into long, thin triangles, and hits on those
  land far enough beneath the plane of the face that a shadow or bounce ray
  leaving at a shallow angle was blocked by the triangle next door. Secondary
  rays now start further off the surface, by an amount that also grows as the
  ray leaves more shallowly.
- A C-style list comprehension with an empty init or update clause —
  `[for (; is_list(l); l = l[0]) len(l)]`, which BOSL2 writes to walk a
  variable from the enclosing scope — was a parse error, taking the whole
  library down with it. Both clauses may now be empty, as in OpenSCAD.
- `import()` of an SVG in a `.scad` file warned that the format cannot be read
  and imported nothing. It now returns a 2D sketch ready for
  `linear_extrude()`, with the contours read even-odd — so a shape drawn
  inside another one is a hole, however the two wind, which is what OpenSCAD
  makes of the same file. `import(center = true)` is honored too, on 2D
  formats only, as in OpenSCAD.
- An OpenSCAD `text()` naming a font with no outlines to give — a bitmap-only
  face such as macOS's "GB18030 Bitmap" — warned "no font found" and emitted
  nothing, even with hundreds of usable fonts installed. Font resolution now
  walks on past a face it cannot read to the next candidate.
- Studio crashed on scripts containing multi-byte characters like `ß` or an
  emoji: the status line turned the caret, which counts characters, into a
  byte offset directly and sliced the text in the middle of a character. The
  column it shows now counts characters too, as does the character count next
  to it.
- A case-insensitive find in Studio searched a lowercased copy of the text,
  whose byte offsets drift away from the original wherever lowercasing
  changes a character's length (`İ`, `ẞ`, …), so matches were highlighted at
  the wrong place, or crashed the editor outright.


## 2026-08-17 - 1.1.0

### Added

- A playground at <https://luacad.ad-si.com/playground>, which builds and
  views a script in the browser. LuaCAD is compiled to WebAssembly, so
  nothing is installed and nothing leaves the page.
- A 2D shape is an output in its own right. `render(circle{r=10})` writes a
  PNG of the flat area, `info` measures it, Studio and the playground preview
  it, and returning it as a script's last value does the same. An export to
  `.scad` writes the 2D calls it is made of. The mesh formats still refuse
  one, since an outline has no volume to print.
- The 2D shapes double as outlines, so `join_prism()`, `offset_sweep()`,
  `offset()` and the region functions take `bosl.circle{r=15}` wherever they
  took a list of points.

### Changed

- `luacad convert` and `luacad watch` point at `luacad render` when asked for
  a `.png` output — whether by extension or `--format png` — instead of
  reporting it as an unknown format. Images stay the renderer's job.

### Fixed

- `bosl.deriv2()` and `bosl.deriv3()` were a whole order less accurate at the
  first and last entries of an open list, which `path_curvature()` and
  `path_torsion()` inherited. All six cases now agree with BOSL2 exactly.
- Reading the outline out of a 2D shape dropped every transform on the way,
  so offsets, rotations and translations were silently ignored and the
  untransformed points came back.
- Building on Fedora and other distributions whose CMake installs into `lib64`
  no longer fails to link with "could not find native static library
  `manifoldc`". Manifold's libraries are now always installed into `lib`.
- When bindgen cannot parse the Manifold headers because libclang does not
  find the compiler-provided ones such as `stddef.h`, the build script now
  retries with the resource directory reported by `clang`, instead of leaving
  it to `BINDGEN_EXTRA_CLANG_ARGS`.


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
- The Belfry OpenSCAD Library v2 (BOSL2) under `bosl.*`. All 712 of its
  geometry functions are built or computed by LuaCAD itself, so they render,
  preview and export to a mesh with neither OpenSCAD nor BOSL2 installed:
  - The 2D and 3D shapes, with their `anchor`/`spin`/`orient` placement, the
    `edges`/`except` selectors, and per-end and per-corner rounding and
    chamfering.
  - The transforms, distributors, partitions and 2D and 3D masks.
  - Paths, regions, drawing, turtle graphics, Bézier curves and patches,
    NURBS curves and surfaces, rounding, skinning, sweeps and the VNF
    functions.
  - `join_prism()`, which blends a prism into a plane, cylinder or sphere,
    so a fillet round a boss wraps its corners rather than stopping short at
    each one.
  - The `os_*` end treatments, and the 22 named textures.
  - Metaballs and arbitrary isosurfaces, meshed by marching tetrahedra.
  - The parts libraries: threading, screws and nuts from the ISO metric
    tables, gears, joiners, sliders, bearings, NEMA steppers, wiring, walls,
    cubetruss, hinges, bottlecaps, polyhedra, hose fittings and tripod
    mounts.
  - The pure functions — math, vectors, coordinates, lists, linear algebra
    and geometry — which return real Lua values rather than OpenSCAD source,
    so a script can compute with them.

  Attachments read differently, because a shape here is a value rather than
  a child of the call that made it: `attach(parent, child, TOP)` is a
  function of two shapes, and `diff{ body, tag(hole, "remove") }` reads its
  tags off a list. Anchors resolve against a shape's own measurements.

  Left out on purpose is the part of BOSL2 that works around OpenSCAD not
  being a real language — `fnliterals.scad`, `strings.scad`, `utility.scad`,
  `structs.scad` and the BOSL1 compatibility names. Lua has closures, a
  string library and tables already.

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
developed until February 2026, when the project was rewritten in Rust. It was
kept in `legacy_lua/` until 1.2.0 and can still be read there at the `v1.1.0`
tag. It was never published as a versioned release, so it has no entries
above.
