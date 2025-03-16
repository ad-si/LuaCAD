# LuaCAD

LuaCAD is a scripting library which uses [OpenSCAD] as engine
to create 2D and 3D objects.

[OpenSCAD]: https://www.openscad.org/

The LuaCAD library creates [OpenSCAD] code
which is then used to create DXF, SVG, or STL files.
If the object should have a specific color then LuaCAD can take an STL file
and convert it to OBJ or WRL with color.
Full support or [Belfry OpenSCAD Library v2][BOLS2] library is planned.

[BOLS2]: https://github.com/BelfrySCAD/BOSL2/wiki


## Installation

- Install [OpenSCAD] if not already installed
- [Download the project](https://github.com/ad-si/LuaCAD/archive/refs/heads/main.zip)
- Unzip the project
- Add a new `.lua` file to the root directory
- Implement your model as LusSCAD code
- Execute code to produce output:
    ```sh
    lua your_model.lua
    ```


## Example

**OpenSCAD:**

```openscad
cube([1,2,3]);
translate([5,0,0]) sphere(r = 2);
```

**LuaCAD:**

```lua
require "luacad"

local cube_model = cube{size={1, 2, 3}}
local sphere_model = sphere{r = 2}

local model = cube_model + sphere_model
model:export("temp/model.scad")
```

## Additional Functions

In comparison to OpenSCAD, LuaCAD provides several additional functions
that make it easier to create complex models:

- 2D Objects
  - `ellipse()`
  - `regular_polygon()`
- 3D Objects
  - `cone()` (can use cylinder with different radii)
  - `prism()`
  - `wedge()`
  - `torus()`
  - `surface()`
- Extrusions
  - `rotate_extrude()` with angle parameter
- Mathematical/Programming
  - Most math functions (`cross()`, `norm()`, trigonometric functions)
  - List operations (`len()`, `each`, `range()`)
- Special
  - `import_3mf()`, `rands()`, `search()`


## Why Lua?

- Similar syntax as the OpenSCAD language, but better:
  - More powerful
  - More consistent
  - Faster
  - Easily embeddable and could therefore be directly integrated
      into OpenSCAD or a fork
- Already used in other CAD software like [LibreCAD] and [Autodesk Netfabb]

[LibreCAD]: https://wiki.librecad.org/index.php/LibreCAD_3_-_Lua_Scripting
[Autodesk Netfabb]:
  https://help.autodesk.com/view/NETF/2025/ENU/?guid=GUID-93C06838-2623-4573-9BFB-B1EF4628AC4A


## Supported Export Formats

- [3MF](https://en.wikipedia.org/wiki/3D_Manufacturing_Format)
- [DXF](https://en.wikipedia.org/wiki/AutoCAD_DXF)
- [OBJ](https://en.wikipedia.org/wiki/Wavefront_.obj_file)
- [STL](https://en.wikipedia.org/wiki/STL_(file_format))
- [SVG](https://en.wikipedia.org/wiki/Scalable_Vector_Graphics)
- [WRL](https://en.wikipedia.org/wiki/VRML)


## Related

Other CAD software that support programmatic creation of 3D models:

- [BlocksCAD] - Blockly-based CAD software
- [FreeCAD] - Python scripting
- [ImplicitCAD] - Haskell-based CAD software
- [LibreCAD] - Lua scripting
- [Liquid CAD] - 2D constraint-solving CAD for rapid prototyping
- [ManifoldCAD] - Online CAD software using JavaScript
- [OpenSCAD Rust] - Rust implementation of the OpenSCAD virtual machine
- [OpenSCAD] - OpenSCAD language

[BlocksCAD]: https://www.blockscad3d.com/editor/
[FreeCAD]: https://wiki.freecad.org/Python_scripting_tutorial
[ImplicitCAD]: https://implicitcad.org/
[Liquid CAD]: https://github.com/twitchyliquid64/liquid-cad
[ManifoldCAD]: https://manifoldcad.org/
[OpenSCAD Rust]: https://github.com/Michael-F-Bryan/scad-rs


## History

The initial work was done by Michael Lutz at
[thechillcode/Lua_CAD](https://github.com/thechillcode/Lua_CAD).


[BOSL2]: https://github.com/BelfrySCAD/BOSL2

