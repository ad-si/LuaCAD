# LuaCAD

Solid 3D CAD modeling with Lua.

Write parametric 2D and 3D models in Lua
and export them to 3MF, STL, OBJ, PLY, OFF, AMF, or SCAD,
or render them straight to a PNG.
Existing OpenSCAD files open too — LuaCAD evaluates the `.scad` language
itself, with no OpenSCAD installation involved.

![Screenshot of LuaCAD Studio previewing the MuSHR racecar model, a 43-part
assembly of 490,802 triangles, beside the Lua script that builds
it](images/screenshots/2026-08-13t1803_studio_racecar.png)

LuaCAD embeds Lua 5.4 in a Rust engine
that evaluates CSG operations directly (via [Manifold])
or generates [SCAD] code for external rendering.

[Manifold]: https://github.com/elalish/manifold
[SCAD]: https://openscad.org/documentation.html


## Installation

### Via Homebrew

Installs the prebuilt `luacad` and `luacad-studio` binaries,
so nothing has to be compiled:

```sh
brew install ad-si/tap/luacad
```


### Via Crates

```sh
cargo install luacad  # CLI for running and converting LuaCAD scripts
cargo install luacad-studio  # GUI desktop app with live 3D preview
```


### From source

Requires [Rust](https://www.rust-lang.org/tools/install).

```sh
git clone https://github.com/ad-si/LuaCAD.git
cd LuaCAD
make install
```


### Build requirements

Both crates vendor their C/C++ dependencies, so no system libraries need to
be installed — but a C++ compiler and CMake must be available to build them:

- `luacad` builds [Manifold] and Clipper2
- `luacad-studio` additionally builds [OpenCSG], which needs OpenGL
  development headers (on Debian/Ubuntu: `libgl1-mesa-dev`, `libx11-dev`,
  `libxcb1-dev`, `libxkbcommon-dev`, `libxrandr-dev`, `libwayland-dev`)

[OpenCSG]: http://www.opencsg.org/


## Usage

### CLI

```sh
luacad convert model.lua output.3mf   # Convert to 3MF
luacad convert model.lua output.stl   # Convert to STL
luacad convert model.lua output.scad  # Export as SCAD for OpenSCAD
luacad watch model.lua output.3mf     # Rebuild on file changes
luacad render model.lua preview.png   # Render to a PNG image
luacad info model.lua                 # Print triangle counts and bounding box
luacad lint model.lua                 # Lint with selene (also takes directories)
luacad run model.lua                  # Execute (side-effects only)
```

Every subcommand also takes a `.scad` file wherever it takes a `.lua` one:

```sh
luacad convert model.scad output.3mf  # Mesh an OpenSCAD file
luacad render model.scad preview.png  # Render one to a PNG
luacad info model.scad                # Its triangle count and bounding box
```

`convert` and `watch` infer the format from the output extension;
`--format <fmt>` overrides it.
`--via-openscad` hands the export to an installed OpenSCAD binary instead of
building the mesh with Manifold.
`render` shades flat by default, so the tessellation stays visible;
`--smooth` turns that off.
`--raytrace` renders with a path tracer instead of the rasterizer:
soft shadows and ambient occlusion, at a few seconds per image.
Path-traced renders always shade smooth (creases stay sharp).
`--samples N` sets the path tracer's samples per pixel (default: 128);
more samples mean less noise at proportionally longer render times.

`luacad --help` lists the subcommands, and `luacad --version` (or `-v`)
prints the version — see [Version information](#version-information).


### Studio

```sh
luacad-studio            # Reopen the file from the last session
luacad-studio model.lua  # Open a file
luacad-studio --help     # Show the command line options and exit
luacad-studio --version  # Show the version and exit
```

Desktop app with a code editor and 3D viewport.
Edit Lua code on the right, see the model update on the left.
The viewport draws the CSG tree itself through [OpenCSG],
so a boolean shows up as soon as the script runs,
without waiting for a mesh to be built for it.
Settings → About shows the same version information as `--version`,
plus the target the binary was built for.


### Version information

Both binaries accept `-v` / `--version` and `-h` / `--help`, and exit
immediately:

```sh
$ luacad --version
luacad 1.1.0 (v1.1.0-3-g23a0ea2-dirty)
```

A binary built from a git checkout appends
`git describe --always --dirty --tags` to the crate version, so a local build
can be traced back to the commit it came from — the hash alone when the
checkout has no tags, with `-dirty` marking a modified working tree.
Released binaries and installs from crates.io print the plain version
(`luacad 1.1.0`).
Set `LUACAD_GIT_DESCRIBE` at build time to pin the suffix (or to drop it,
with an empty value) for reproducible builds:

```sh
LUACAD_GIT_DESCRIBE=nixpkgs cargo build --release
```


### Playground

<https://luacad.ad-si.com/playground>

The same engine, compiled to WebAssembly and running in the browser —
no installation, and nothing is uploaded.
Write a script, see the model, download it as STL, 3MF, OBJ, PLY, OFF or AMF.
*Copy link* puts the script in the URL fragment, which makes a model
shareable without a server ever seeing it.

Building it locally needs [Emscripten](https://emscripten.org/) and a Rust
toolchain with the `wasm32-unknown-emscripten` target. The dev shell brings
both:

```sh
nix develop        # Or: source <emsdk>/emsdk_env.sh
make test-wasm     # Build the module and check that it still runs a script
make serve-website # Serve the site at http://localhost:8000/playground/
```

Without Nix, install the target with
`rustup target add wasm32-unknown-emscripten`.

The deployed copy is rebuilt by CI on every push to `main`.


## Example

```lua
my_cube = cube { size = { 1, 2, 3 } }

function my_sphere(radius)
  return sphere({ r = radius }):translate(5, 0, 0)
end

model = my_cube + my_sphere(2)

render(model)
```

**Equivalent OpenSCAD:**

```openscad
module my_cube() {
  cube(size=[1,2,3]);
}

module my_sphere(radius) {
  translate([5,0,0]) sphere(r = radius);
}

union() {
  my_cube();
  my_sphere(2);
}
```

More examples in the [examples/](examples/) directory.

[Check out the website](https://ad-si.github.io/LuaCAD/openscad-to-luacad.html)
to see all supported features!

For easier usage, LuaCAD has full support for the
[Belfry OpenSCAD Library v2][BOSL2].
You can use its functions like this:

```lua
bosl.cuboid { {40, 40, 40}, rounding = 2 }
bosl.regular_prism { 5, r = 10, h = 25 }
bosl.spur_gear { circ_pitch = 5, teeth = 20, thickness = 5 }
```

All 712 of its geometry functions are reimplemented in LuaCAD, so they
render, preview and export to a mesh without OpenSCAD or BOSL2 installed.
Exporting to `.scad` still writes the BOSL2 call itself, which keeps the
exported file as short as the script that produced it.

What is deliberately left out is the part of BOSL2 that exists to work
around OpenSCAD not being a real language — `fnliterals.scad`,
`strings.scad`, `utility.scad`, `structs.scad` and the BOSL1 compatibility
names. Lua has closures, a string library and tables of its own.

Two things read differently here, because a shape in LuaCAD is a value
rather than a child of the call that made it:

```lua
-- Attaching is a function of two shapes, not a wrapper around one.
local slab = bosl.cuboid { {40, 40, 10} }
local post = bosl.attach(slab, bosl.cyl { d = 10, h = 20 }, bosl.TOP)

-- A tag rides on the shape, and diff() reads it off the list.
bosl.diff { slab, post, bosl.tag(bosl.cyl { d = 5, h = 60 }, "remove") }
```

`surface()`, `scad()` and `import()` of a DXF file do still need OpenSCAD;
`luacad` names them rather than exporting a file without them.

[BOSL2]: https://github.com/BelfrySCAD/BOSL2/wiki


## Why Lua?

The OpenSCAD language is limited and has several issues:

- Confusing functions vs modules
- Weird variable scoping
- Not a well establised programming language
    - Bad editor support
    - Limited documentation
    - Limited libraries
    - Bad error messages
    - Bad performance

Therefore, a *real* programming language should be used
and it should ideally be interpreted and have good
[operator overloading support](https://en.wikipedia.org/wiki/Operator_overloading)

- Julia - Too complex
- Python - Too slow and while easy to get started, hard to master

Lua is a better fit:

- Well-established, embeddable language
- Operator overloading for natural CSG syntax (`a + b`, `a - b`)
- Consistent semantics and good performance
- Similar syntax to OpenSCAD's language
- Already used in other CAD software ([LibreCAD], [Autodesk Netfabb])

[LibreCAD]: https://github.com/LibreCAD/LibreCAD_3
[Autodesk Netfabb]:
  https://help.autodesk.com/view/NETF/2025/ENU/?guid=GUID-93C06838-2623-4573-9BFB-B1EF4628AC4A


## Supported Export Formats

- [SCAD](https://en.wikipedia.org/wiki/OpenSCAD)
- [3MF](https://en.wikipedia.org/wiki/3D_Manufacturing_Format)
- [STL](https://en.wikipedia.org/wiki/STL_(file_format))
- [OBJ](https://en.wikipedia.org/wiki/Wavefront_.obj_file)
- [PLY](https://en.wikipedia.org/wiki/PLY_(file_format))
- [OFF](https://en.wikipedia.org/wiki/OFF_(file_format))
- [AMF](https://en.wikipedia.org/wiki/Additive_manufacturing_file_format)

Every value your script returns becomes its own 3MF object,
so slicers like BambuStudio load them as individually movable objects
without needing "Split to Objects".
Label them with `:name(…)` to control how they appear in the object list:

```lua
return
  cube(10, 10, 10):name("base"),
  sphere(6):translate(25, 0, 0):name("knob")
```

The other formats cannot express separate objects,
so they flatten everything into a single mesh.

A 2D shape is output in its own right — render it without extruding first:

```lua
render(square { 60, 40, center = true } - circle { r = 8 })
```

It exports to SCAD as the 2D calls it is made of, and previews as a flat
area on the build plate, in the CLI's PNG renderer, in Studio and in the
playground. The mesh formats need a solid and say so, since an outline has
no volume to print — give it a `linear_extrude(height)` for those.


## Supported Import Formats

`import()` reads every mesh format LuaCAD writes — 3MF, STL, OBJ, PLY, OFF and
AMF — and returns a solid you can transform and combine like any primitive:

```lua
bracket = import("bracket.stl")
render(bracket - cylinder { r = 3, h = 30 }:translate(5, 5, -5))
```

SVG and DXF return a 2D sketch instead, ready to extrude:

```lua
render(import("logo.svg"):linear_extrude(2))
```

Only geometry is read; colors, materials and texture coordinates are dropped.
A DXF sketch reaches the SCAD tree only, so exporting one to a mesh needs
`--via-openscad`.


## Opening OpenSCAD Files

A `.scad` file works anywhere a `.lua` one does — on the command line and in
Studio, where it opens through File → Open or by dropping it on the window.
LuaCAD parses and evaluates the OpenSCAD language itself, so nothing has to be
installed and `--via-openscad` is not involved:

```sh
luacad convert bracket.scad bracket.3mf
luacad render bracket.scad bracket.png
```

The language support is a vendored copy of [OpenRSCAD]'s front end, a
clean-room reimplementation of OpenSCAD 2021.01 — modules, functions,
`include`/`use`, `for`, list comprehensions, recursion, dynamically scoped
`$` variables, the modifier characters, and the standard library. `include`
and `use` resolve relative to the file, then against `OPENSCADPATH`.

Both languages produce the same internal tree, so an opened `.scad` file
reaches every export format, the PNG renderer, the path tracer and Studio's
live preview unchanged. `luacad convert model.scad out.scad` round-trips it,
with modules inlined and `$fn`/`$fa`/`$fs` resolved to facet counts.

Three constructs cannot be carried across exactly and report a warning rather
than quietly differing: `linear_extrude` with a non-uniform `scale`
(LuaCAD scales both axes together), `resize(auto = …)`, and an `import()` in a
format LuaCAD's own `import()` cannot read. Anything OpenSCAD itself does
differently is listed in [OpenRSCAD's compatibility register][COMPAT].

Editing is still Lua's job: Studio does not lint `.scad` buffers, and the
browser playground remains Lua-only.

[OpenRSCAD]: https://github.com/matthova/openrscad
[COMPAT]: https://github.com/matthova/openrscad/blob/main/COMPAT.md


## Text

`text()` outlines a system font into a sketch, and `text3d()` extrudes it in
one step:

```lua
render(text("LuaCAD", { size = 12, halign = "center" }):linear_extrude(2))
render(text3d("v1.0", { size = 8, depth = 1.5 }))
```

Fonts are looked up by family, optionally with a style — `"DejaVu Sans"` or
`"DejaVu Sans:style=Bold"` — and an unknown family falls back to the default
sans-serif face. Layout is a single line of glyph advances plus kerning pairs;
ligatures and complex scripts need `--via-openscad`, which runs the text
through OpenSCAD's shaping engine instead.


## Related

Other CAD software with programmatic model creation:

- [3DScad] - Visual editor for OpenSCAD-style modeling
- [bevy_editor_cam] - Camera controller for 2D/3D editors and CAD in Bevy
- [BlocksCAD] - Blockly-based CAD
- [CadQuery] - Python module for parametric 3D CAD models
- [DSLCAD] - Programming language and interpreter for building 3D models
- [Flowscad] - Rust interface to OpenSCAD
- [ForgeCAD] - AI-native CAD for products, manufacturing, and robotics
- [FreeCAD] - Python scripting
- [HelloTriangle] - 3D modeling with Python
- [ImplicitCAD] - Haskell-based CAD
- [LibreCAD] - Lua scripting
- [Liquid CAD] - 2D constraint-solving CAD
- [ManifoldCAD] - JavaScript-based online CAD
- [NassCAD] - Browser-based parametric CAD with JavaScript scripting
- [OpenSCAD Rust] - Rust OpenSCAD VM
- [openscad-rs] - OpenSCAD parser library for Rust
- [OpenSCAD] - OpenSCAD language
- [Rust Scad] - Generate OpenSCAD from Rust
- [scad_tree] - Rust solid modeling via OpenSCAD
- [ScalaCad] - CSG in Scala
- [SolidRS] - Rust OpenSCAD model generation
- [SpaceCAD] - Model rocket design and simulation software
- [SynapsCAD] - AI-powered 3D CAD IDE

[3DScad]: https://github.com/42ne/3dscad
[bevy_editor_cam]: https://github.com/aevyrie/bevy_editor_cam
[BlocksCAD]: https://www.blockscad3d.com/editor/
[CadQuery]: https://github.com/CadQuery/cadquery
[DSLCAD]: https://dslcad.com
[Flowscad]: https://github.com/SmoothDragon/flowscad
[ForgeCAD]: https://forgecad.io/
[FreeCAD]: https://wiki.freecad.org/Python_scripting_tutorial
[HelloTriangle]: https://www.hellotriangle.io/
[ImplicitCAD]: https://implicitcad.org/
[Liquid CAD]: https://github.com/twitchyliquid64/liquid-cad
[ManifoldCAD]: https://manifoldcad.org/
[NassCAD]: https://www.nasscad.com/
[OpenSCAD Rust]: https://github.com/Michael-F-Bryan/scad-rs
[openscad-rs]: https://github.com/ierror/openscad-rs
[OpenSCAD]: https://openscad.org
[Rust Scad]: https://github.com/TheZoq2/Rust-Scad
[scad_tree]: https://github.com/mrclean71774/scad_tree
[ScalaCad]: https://github.com/joewing/ScalaCad
[SolidRS]: https://github.com/MnlPhlp/solidrs
[SpaceCAD]: https://www.spacecad.de/
[SynapsCAD]: https://github.com/ierror/synaps-cad


## History

The initial Lua implementation was done by Michael Lutz at
[thechillcode/Lua_CAD](https://github.com/thechillcode/Lua_CAD).
The project was later rewritten in Rust.
