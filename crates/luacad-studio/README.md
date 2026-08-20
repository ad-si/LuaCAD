# luacad-studio

Desktop application for [LuaCAD](https://github.com/ad-si/LuaCAD) — a code
editor and a live 3D viewport side by side. Edit Lua on the right, watch the
model update on the left.

```sh
cargo install luacad-studio
luacad-studio
```

```sh
luacad-studio            # Reopen the file from the last session
luacad-studio model.lua  # Open a file
luacad-studio --help     # Show the command line options and exit
luacad-studio --version  # Show the version and exit
```

`--version` (or `-v`) prints the crate version, followed by
`git describe --always --dirty --tags` for a binary built from a git
checkout, so a local build can be traced back to its commit. The same
information, plus the target the binary was built for, is in
Settings → About.

Building requires a C++ toolchain and OpenGL development headers, because the
CSG preview is rendered through [OpenCSG](http://www.opencsg.org/).

For the command line interface and the scripting engine, see
[`luacad`](https://crates.io/crates/luacad).
