# luacad-preview

The 3D preview of [LuaCAD](https://github.com/ad-si/LuaCAD): the viewport of
LuaCAD Studio and the canvas of the browser playground draw the same picture
from this crate.

A model is not booleaned before it is shown. It is flattened into CSG
products — a list of primitives each tagged as intersected or subtracted —
and those are drawn by [WebCSG](https://github.com/ad-si/WebCSG), which fills
the depth buffer with the visible surface of the product on the GPU. A
subtraction therefore appears as soon as the script runs, without waiting for
a mesh to be built for it. Whatever does not fit a single product (a hull, an
extrusion, a nested union inside a difference) is materialized by Manifold and
drawn as an ordinary mesh.

The two halves are separate features, because the playground splits them
across two WebAssembly modules:

- `flatten` turns the geometries the engine produced into CSG products. Needs
  `luacad`, and through it Manifold.
- `render` draws them on [wgpu](https://wgpu.rs), into a color target the
  caller owns. Needs a GPU, nothing else.

`wire` encodes a flattened scene as a byte buffer, which is how the
playground's engine module (Emscripten, in a worker) hands the scene to its
viewer module (wasm-bindgen, on the page).

Both features are on by default, which is what the studio uses.
