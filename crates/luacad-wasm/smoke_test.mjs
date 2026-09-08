// Smoke test for the wasm artifact: loads the engine module the way the
// playground does, runs a script through it, and checks the CSG scene and the
// STL export that come back. Run it with `make test-wasm` after `make wasm`.
//
//   node crates/luacad-wasm/smoke_test.mjs <dir with luacad-wasm.js>

import { createRequire } from "node:module"
import { resolve } from "node:path"

const dir = resolve(process.argv[2] ?? "website/playground")
const require = createRequire(import.meta.url)
const createLuaCAD = require(resolve(dir, "luacad-wasm.js"))

const failures = []

function check(name, condition, detail) {
  if (condition) {
    console.log(`  ok  ${name}`)
  } else {
    console.log(`FAIL  ${name}${detail ? `: ${detail}` : ""}`)
    failures.push(name)
  }
}

const Module = await createLuaCAD({ print: () => {}, printErr: () => {} })

// --- Buffer protocol, mirroring website/playground/luacad.js -----------------

function call(fn, arg) {
  const argPtr = Module.stringToNewUTF8(arg)
  const ptr = Module.ccall(fn, "number", ["number"], [argPtr])
  Module._free(argPtr)
  const view = new DataView(Module.HEAPU8.buffer)
  const length = view.getUint32(ptr, true)
  const ok = view.getUint32(ptr + 4, true) === 1
  const payload = Module.HEAPU8.slice(ptr + 8, ptr + 4 + length)
  Module.ccall("luacad_free", null, ["number"], [ptr])
  return { ok, payload }
}

/// Decode the flattened CSG scene the engine returns, as documented in
/// crates/luacad-preview/src/wire.rs.
function decodeScene(payload) {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength)
  let at = 0
  const u32 = () => {
    const value = view.getUint32(at, true)
    at += 4
    return value
  }
  const floats = (count) => {
    const values = []
    for (let i = 0; i < count; i += 1) {
      values.push(view.getFloat32(at, true))
      at += 4
    }
    return values
  }
  const vertices = () => floats(u32() * 3)

  const magic = u32()
  if (magic !== 0x4753434c) {
    throw new Error(`not a scene buffer: magic 0x${magic.toString(16)}`)
  }
  const version = u32()
  if (version !== 1) {
    throw new Error(`scene buffer version ${version}, expected 1`)
  }

  const groups = []
  for (let g = u32(); g > 0; g -= 1) {
    const primitives = []
    for (let p = u32(); p > 0; p -= 1) {
      const operation = u32()
      const convexity = u32()
      const transform = floats(16)
      const color = floats(3)
      const specular = floats(4)
      const emission = floats(3)
      primitives.push({
        operation,
        convexity,
        transform,
        color,
        specular,
        emission,
        vertices: vertices(),
      })
    }
    groups.push({ primitives })
  }

  const overlays = []
  for (let o = u32(); o > 0; o -= 1) {
    const transform = floats(16)
    const color = floats(4)
    overlays.push({ transform, color, vertices: vertices() })
  }
  return { groups, overlays }
}

/// Every vertex of every primitive of a scene.
function allVertices(scene) {
  return scene.groups.flatMap((group) =>
    group.primitives.flatMap((primitive) => primitive.vertices),
  )
}

// --- Cases ------------------------------------------------------------------

const cube = call("luacad_run", `cube({ size = { 10, 10, 10 } })`)
check("a cube runs", cube.ok, new TextDecoder().decode(cube.payload))
if (cube.ok) {
  const scene = decodeScene(cube.payload)
  check("one CSG product comes back", scene.groups.length === 1, `${scene.groups.length}`)
  const [primitive, ...rest] = scene.groups[0].primitives
  check("it holds one primitive", rest.length === 0)
  check("the cube is tessellated", primitive.vertices.length === 12 * 3 * 3, `${primitive.vertices.length / 9} triangles`)
  const extent = Math.max(...primitive.vertices)
  check("the cube is 10 units across", Math.abs(extent - 10) < 1e-4, `${extent}`)
}

// A difference stays a difference: the two primitives reach the viewer as one
// CSG product, and no Manifold boolean runs at all.
const difference = call(
  "luacad_run",
  `cube({ size = { 10, 10, 10 } }) - sphere({ r = 6 })`,
)
check("a difference runs", difference.ok, new TextDecoder().decode(difference.payload))
if (difference.ok) {
  const scene = decodeScene(difference.payload)
  check("the difference is one product", scene.groups.length === 1)
  const operations = scene.groups[0].primitives.map((p) => p.operation)
  check("the cube is intersected and the sphere subtracted", String(operations) === "0,1", String(operations))
}

// color() has to survive the trip, since the viewer paints with it.
const colored = call("luacad_run", `cube({ size = { 1, 1, 1 } }):color("red")`)
check("a colored cube runs", colored.ok, new TextDecoder().decode(colored.payload))
if (colored.ok) {
  const [primitive] = decodeScene(colored.payload).groups[0].primitives
  check(
    "the color came through",
    primitive.color[0] > 0.5 && primitive.color[1] < 0.1 && primitive.color[2] < 0.1,
    primitive.color.join(","),
  )
}

// The `#` modifier draws its subtree twice: once in the CSG, once as a
// translucent overlay the viewer blends on top.
const highlighted = call("luacad_run", `render(cube(10) - d(sphere({ r = 6 })))`)
check("a highlighted subtraction runs", highlighted.ok, new TextDecoder().decode(highlighted.payload))
if (highlighted.ok) {
  const scene = decodeScene(highlighted.payload)
  check("the highlight became an overlay", scene.overlays.length === 1, `${scene.overlays.length}`)
  check(
    "the overlay is translucent",
    scene.overlays.length === 1 && scene.overlays[0].color[3] > 0 && scene.overlays[0].color[3] < 1,
  )
}

// A 2D outline is output in its own right, drawn flat at z = 0. The scene is
// in the preview's GL axes, where CAD z is the second component.
const outline = call("luacad_run", `render(square { 30, 20 } - circle { r = 6 })`)
check("a 2D outline runs", outline.ok, new TextDecoder().decode(outline.payload))
if (outline.ok) {
  const vertices = allVertices(decodeScene(outline.payload))
  check("the outline tessellated", vertices.length > 0)
  check(
    "the outline is flat",
    vertices.filter((_, i) => i % 3 === 1).every((up) => up === 0),
  )
}

// Mesh formats need a solid, and have to say so rather than write an empty file.
const outlineExport = call("luacad_export", "stl")
check("exporting an outline to STL is refused", !outlineExport.ok)
check(
  "the refusal names the fix",
  new TextDecoder().decode(outlineExport.payload).includes("linear_extrude"),
)

// Lua errors have to arrive as messages rather than as a dead module.
const broken = call("luacad_run", `cube({ size = { 1, 1, 1 } })) -- unbalanced`)
check("a broken script fails cleanly", !broken.ok)
check("the module still works afterwards", call("luacad_run", `sphere({ r = 1 })`).ok)

// Exporting reuses the last run.
const stl = call("luacad_export", "stl")
check("STL export works", stl.ok, new TextDecoder().decode(stl.payload))
if (stl.ok) {
  const triangles = new DataView(stl.payload.buffer, stl.payload.byteOffset).getUint32(80, true)
  check("the STL header counts triangles", triangles > 0 && stl.payload.length === 84 + triangles * 50)
}

const badFormat = call("luacad_export", "xyz")
check("an unknown export format fails cleanly", !badFormat.ok)

// The playground's starter scripts are the first thing a visitor runs, so
// they are held to the same standard as the examples in the repository.
const { EXAMPLES } = await import(
  new URL("../../website/playground/examples.js", import.meta.url)
)
for (const example of EXAMPLES) {
  const result = call("luacad_run", example.code)
  check(
    `example "${example.name}" runs`,
    result.ok,
    result.ok ? "" : new TextDecoder().decode(result.payload),
  )
}

if (failures.length > 0) {
  console.error(`\n${failures.length} wasm smoke test(s) failed`)
  process.exit(1)
}
console.log("\n✅ wasm smoke tests passed!")
