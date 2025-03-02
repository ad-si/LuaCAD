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
