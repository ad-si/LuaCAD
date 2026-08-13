# luacad

Solid 3D CAD modeling with Lua — engine and command line interface.

Write parametric 2D and 3D models in Lua and export them to
3MF, STL, OBJ, PLY, OFF, AMF, or SCAD.

```sh
cargo install luacad
```

```lua
my_cube = cube { size = { 1, 2, 3 } }

function my_sphere(radius)
  return sphere({ r = radius }):translate(5, 0, 0)
end

render(my_cube + my_sphere(2))
```

```sh
luacad convert model.lua output.3mf   # Convert to 3MF
luacad render model.lua preview.png   # Render to a PNG image
luacad watch model.lua output.stl     # Rebuild on file changes
luacad info model.lua                 # Print geometry metadata
luacad lint model.lua                 # Lint with selene
```

For the GUI application with a live 3D preview, see
[`luacad-studio`](https://crates.io/crates/luacad-studio).

Full documentation, examples and the OpenSCAD translation guide live in the
[main repository](https://github.com/ad-si/LuaCAD).

## Stability

The stable, semver-governed surface of this project is the **Lua API** and the
**`luacad` command line interface**. The Rust modules are exposed so the CLI and
Studio can share them; they are internal and may change in any release.
