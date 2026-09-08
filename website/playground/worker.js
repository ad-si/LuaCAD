// Runs the LuaCAD engine off the main thread.
//
// Everything the engine does — parsing Lua, evaluating CSG in Manifold — is
// synchronous and can take seconds on a heavy model, so it lives in a worker.
// That also gives the page a way out of a runaway script: terminate the
// worker and start a new one.
//
// The buffers passed on from here are documented in
// crates/luacad-wasm/src/main.rs.

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
  // The payload is the flattened CSG scene; the viewer module on the page
  // decodes and draws it. It is transferred rather than cloned.
  postMessage(
    { type: "scene", scene: payload, milliseconds: performance.now() - started },
    [payload.buffer],
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
