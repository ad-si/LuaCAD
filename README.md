# LuaSCAD

LuaSCAD is a scripting library which uses [OpenSCAD] as engine
to create 2D and 3D objects.

**Supported exports formats are:**

- [DXF](https://en.wikipedia.org/wiki/AutoCAD_DXF)
- [SVG](https://en.wikipedia.org/wiki/Scalable_Vector_Graphics)
- [STL](https://en.wikipedia.org/wiki/STL_(file_format))
- [OBJ](https://en.wikipedia.org/wiki/Wavefront_.obj_file)
- [WRL](https://en.wikipedia.org/wiki/VRML)

The LuaSCAD library creates [OpenSCAD] code
which is then used to create DXF, SVG, or STL files.
If the object should have a specific color then LuaSCAD can take an STL file
and convert it to OBJ or WRL with color.

**Main Features:**

- Using CAD objects
- STL to WRL converter with color
- STL to OBJ converter with color

**Installation:**

- Clone the repository: `git clone https://github.com/ad-si/LuaSCAD`
- Install [OpenSCAD] if not already installed
- Add a new `.lua` file to the root directory of the repository
- Implement your model
- ```sh
  lua your_model.lua
  ```

[OpenSCAD]: https://www.openscad.org/


## Example

**OpenSCAD:**

```openscad
cube([1,2,3]);
translate([5,0,0]) sphere(r = 2);
```

**LuaSCAD:**

```lua
require "lib_cad"

local cube = cad.cube{size={1, 2, 3}}
local sphere = cad.sphere{r = 2}

local model = cube + sphere
model:export("temp/model.scad")
```


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


## Tests

Several test files are provided in the `tests/` directory
to ensure functionality.


## History

The initial work was done by Michael Lutz at
[thechillcode/Lua_CAD](https://github.com/thechillcode/Lua_CAD).
