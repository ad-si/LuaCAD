# luacad-studio

Desktop application for [LuaCAD](https://github.com/ad-si/LuaCAD) — a code
editor and a live 3D viewport side by side. Edit Lua on the right, watch the
model update on the left.

```sh
cargo install luacad-studio
luacad-studio
```

Building requires a C++ toolchain and OpenGL development headers, because the
CSG preview is rendered through [OpenCSG](http://www.opencsg.org/).

For the command line interface and the scripting engine, see
[`luacad`](https://crates.io/crates/luacad).
