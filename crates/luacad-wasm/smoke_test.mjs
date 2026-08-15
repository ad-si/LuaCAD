// Smoke test for the wasm artifact: loads the module the way the playground
// does, runs a script through it, and checks the mesh and the STL export that
// come back. Run it with `make test-wasm` after `make wasm`.
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

function decodeMeshes(payload) {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength)
  let at = 0
  const count = view.getUint32(at, true)
  at += 4
  const meshes = []
  for (let i = 0; i < count; i += 1) {
    const nameLength = view.getUint32(at, true)
    at += 4
    const name = new TextDecoder().decode(payload.subarray(at, at + nameLength))
    at += Math.ceil(nameLength / 4) * 4
    const hasColor = view.getUint32(at, true) === 1
    at += 4
    const color = [
      view.getFloat32(at, true),
      view.getFloat32(at + 4, true),
      view.getFloat32(at + 8, true),
    ]
    at += 12
    const vertexCount = view.getUint32(at, true)
    const triangleCount = view.getUint32(at + 4, true)
    at += 8
    const vertices = new Float32Array(payload.buffer, payload.byteOffset + at, vertexCount * 3)
    at += vertexCount * 12
    const indices = new Uint32Array(payload.buffer, payload.byteOffset + at, triangleCount * 3)
    at += triangleCount * 12
    meshes.push({ name, hasColor, color, vertices, indices })
  }
  return meshes
}

// --- Cases ------------------------------------------------------------------

const cube = call("luacad_run", `cube({ size = { 10, 10, 10 } })`)
check("a cube runs", cube.ok, new TextDecoder().decode(cube.payload))
if (cube.ok) {
  const [mesh, ...rest] = decodeMeshes(cube.payload)
  check("one mesh comes back", rest.length === 0)
  check("the cube has 8 vertices", mesh.vertices.length === 8 * 3, `${mesh.vertices.length / 3}`)
  check("the cube has 12 triangles", mesh.indices.length === 12 * 3, `${mesh.indices.length / 3}`)
  const extent = Math.max(...mesh.vertices)
  check("the cube is 10 units across", Math.abs(extent - 10) < 1e-4, `${extent}`)
}

// A boolean between two primitives — the part that runs on Manifold's C++.
const difference = call(
  "luacad_run",
  `cube({ size = { 10, 10, 10 } }) - sphere({ r = 6 })`,
)
check("a difference runs", difference.ok, new TextDecoder().decode(difference.payload))
if (difference.ok) {
  const [mesh] = decodeMeshes(difference.payload)
  check("the difference cut geometry away", mesh.indices.length > 12 * 3)
}

// color() has to survive the trip, since the viewer paints with it.
const colored = call("luacad_run", `cube({ size = { 1, 1, 1 } }):color("red")`)
check("a colored cube runs", colored.ok, new TextDecoder().decode(colored.payload))
if (colored.ok) {
  const [mesh] = decodeMeshes(colored.payload)
  check("the color came through", mesh.hasColor && mesh.color[0] === 1, mesh.color.join(","))
}

// A 2D outline is output in its own right, drawn flat at z = 0.
const outline = call("luacad_run", `render(square { 30, 20 } - circle { r = 6 })`)
check("a 2D outline runs", outline.ok, new TextDecoder().decode(outline.payload))
if (outline.ok) {
  const [mesh] = decodeMeshes(outline.payload)
  check("the outline tessellated", mesh.indices.length > 0)
  check(
    "the outline is flat",
    mesh.vertices.filter((_, i) => i % 3 === 2).every((z) => z === 0),
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
