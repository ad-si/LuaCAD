// Runs the LuaCAD engine off the main thread.
//
// Everything the engine does — parsing Lua, evaluating CSG in Manifold — is
// synchronous and can take seconds on a heavy model, so it lives in a worker.
// That also gives the page a way out of a runaway script: terminate the
// worker and start a new one.
//
// The buffer layout decoded here is documented in crates/luacad-wasm/src/main.rs.

importScripts("luacad-wasm.js")

const decoder = new TextDecoder()

const ready = createLuaCAD({
  print: (text) => postMessage({ type: "log", text }),
  printErr: (text) => postMessage({ type: "log", text }),
}).then((module) => {
  postMessage({ type: "ready", version: module.UTF8ToString(module._luacad_version()) })
  return module
})

onmessage = async (event) => {
  const module = await ready
  const { type } = event.data
  try {
    if (type === "run") {
      handleRun(module, event.data.code)
    } else if (type === "export") {
      handleExport(module, event.data.format)
    }
  } catch (error) {
    postMessage({ type: "error", message: String(error?.message ?? error) })
  }
}

function handleRun(module, code) {
  const started = performance.now()
  const { ok, payload } = call(module, "luacad_run", code)
  if (!ok) {
    postMessage({ type: "error", message: decoder.decode(payload) })
    return
  }
  const meshes = decodeMeshes(payload)
  postMessage(
    { type: "meshes", meshes, milliseconds: performance.now() - started },
    meshes.flatMap((mesh) => [mesh.vertices.buffer, mesh.indices.buffer]),
  )
}

function handleExport(module, format) {
  const { ok, payload } = call(module, "luacad_export", format)
  if (!ok) {
    postMessage({ type: "error", message: decoder.decode(payload) })
    return
  }
  postMessage({ type: "file", format, bytes: payload }, [payload.buffer])
}

/// Call one of the engine's entry points with a string and copy the buffer it
/// returns out of the wasm heap.
function call(module, name, argument) {
  const argumentPointer = module.stringToNewUTF8(argument)
  let pointer = 0
  try {
    pointer = module.ccall(name, "number", ["number"], [argumentPointer])
    const view = new DataView(module.HEAPU8.buffer)
    const length = view.getUint32(pointer, true)
    const ok = view.getUint32(pointer + 4, true) === 1
    // `slice` copies: the heap can move under us on the next allocation.
    const payload = module.HEAPU8.slice(pointer + 8, pointer + 4 + length)
    return { ok, payload }
  } finally {
    module._free(argumentPointer)
    if (pointer !== 0) {
      module.ccall("luacad_free", null, ["number"], [pointer])
    }
  }
}

function decodeMeshes(payload) {
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength)
  let at = 0
  const meshCount = view.getUint32(at, true)
  at += 4

  const meshes = []
  for (let index = 0; index < meshCount; index += 1) {
    const nameLength = view.getUint32(at, true)
    at += 4
    const name = decoder.decode(payload.subarray(at, at + nameLength))
    at += Math.ceil(nameLength / 4) * 4

    const hasColor = view.getUint32(at, true) === 1
    at += 4
    const color = hasColor
      ? [
          view.getFloat32(at, true),
          view.getFloat32(at + 4, true),
          view.getFloat32(at + 8, true),
        ]
      : null
    at += 12

    const vertexCount = view.getUint32(at, true)
    const triangleCount = view.getUint32(at + 4, true)
    at += 8

    // Copied into buffers of their own so they can be transferred to the page
    // instead of cloned.
    const vertices = new Float32Array(
      payload.buffer.slice(
        payload.byteOffset + at,
        payload.byteOffset + at + vertexCount * 12,
      ),
    )
    at += vertexCount * 12
    const indices = new Uint32Array(
      payload.buffer.slice(
        payload.byteOffset + at,
        payload.byteOffset + at + triangleCount * 12,
      ),
    )
    at += triangleCount * 12

    meshes.push({ name, color, vertices, indices, triangleCount })
  }
  return meshes
}
