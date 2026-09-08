// Wires the editor, the worker running the engine, and the 3D viewer together.

import { EXAMPLES } from "./examples.js"
import { createViewer } from "./viewer.js"

const editor = document.querySelector("#code")
const canvas = document.querySelector("#view")
const output = document.querySelector("#output")
const status = document.querySelector("#status")
const runButton = document.querySelector("#run")
const stopButton = document.querySelector("#stop")
const fitButton = document.querySelector("#fit")
const shareButton = document.querySelector("#share")
const exportButton = document.querySelector("#export")
const exportFormat = document.querySelector("#export-format")
const exampleSelect = document.querySelector("#examples")

const STORAGE_KEY = "luacad-playground-script"

// The viewer is a WebAssembly module of its own and needs a GPU adapter, so
// it arrives a moment after the page does. A run that finishes first parks
// its scene here until it does. Without a GPU it never does, and the page
// carries on without a preview: scripts still run and still export.
let viewer = null
let viewerFailed = false
let pendingScene = null

createViewer(canvas)
  .then((created) => {
    viewer = created
    if (pendingScene) {
      showScene(pendingScene.scene, pendingScene.milliseconds)
      pendingScene = null
    }
  })
  .catch((error) => {
    viewerFailed = true
    canvas.replaceWith(
      Object.assign(document.createElement("p"), {
        className: "pg-fallback",
        textContent: String(error?.message ?? error),
      }),
    )
    if (pendingScene) {
      showScene(pendingScene.scene, pendingScene.milliseconds)
      pendingScene = null
    }
  })

let worker = null
let running = false

// --- Worker -----------------------------------------------------------------

function startWorker() {
  worker = new Worker("worker.js")
  worker.onmessage = (event) => handleMessage(event.data)
  worker.onerror = (event) => {
    setStatus(`Engine failed to load: ${event.message}`, "error")
    setRunning(false)
  }
}

function handleMessage(message) {
  switch (message.type) {
    case "ready":
      setStatus(`LuaCAD ${message.version} ready`)
      runButton.disabled = false
      // Show something on arrival rather than an empty canvas, but leave the
      // model alone when the worker is only being restarted after a stop.
      if (!hasRun) {
        run()
      }
      break

    case "log":
      log(message.text)
      break

    case "scene":
      showScene(message.scene, message.milliseconds)
      exportButton.disabled = false
      setRunning(false)
      break

    case "error":
      log(message.message, "error")
      setStatus("Failed — see the output below", "error")
      setRunning(false)
      break

    case "file":
      download(message.bytes, `model.${message.format}`)
      setStatus(`Exported model.${message.format}`)
      break
  }
}

/// Draw the scene of a finished run and report what it holds.
function showScene(scene, milliseconds) {
  if (!viewer) {
    if (!viewerFailed) {
      pendingScene = { scene, milliseconds }
      return
    }
    // No preview to draw it in, but the run itself is worth reporting: the
    // script worked, and Download will export it.
    setStatus(`Built in ${Math.round(milliseconds)} ms`)
    return
  }
  viewer.setScene(scene)
  if (shouldFit) {
    viewer.fit()
    shouldFit = false
  }
  viewer.draw()
  // One part per CSG product: a difference is one, a union of two shapes two.
  const parts = viewer.partCount()
  setStatus(
    `${parts === 1 ? "1 part" : `${parts} parts`}, ` +
      `${viewer.triangleCount().toLocaleString()} triangles, ` +
      `${Math.round(milliseconds)} ms`,
  )
}

// --- Actions ----------------------------------------------------------------

let shouldFit = true
let hasRun = false

function run() {
  if (running || !worker) {
    return
  }
  hasRun = true
  const code = editor.value
  localStorage.setItem(STORAGE_KEY, code)
  output.textContent = ""
  exportButton.disabled = true
  setRunning(true)
  setStatus("Building…")
  worker.postMessage({ type: "run", code })
}

function stop() {
  if (!worker) {
    return
  }
  // The engine is a single synchronous call into wasm; the only way to
  // interrupt it is to throw the whole worker away.
  worker.terminate()
  setStatus("Stopped", "error")
  setRunning(false)
  runButton.disabled = true
  exportButton.disabled = true
  startWorker()
}

function setRunning(value) {
  running = value
  runButton.disabled = value
  stopButton.disabled = !value
  document.body.classList.toggle("is-running", value)
}

function setStatus(text, kind = "") {
  status.textContent = text
  status.className = kind
}

function log(text, kind = "") {
  const line = document.createElement("div")
  line.textContent = text
  if (kind) {
    line.className = kind
  }
  output.append(line)
  output.scrollTop = output.scrollHeight
}

function download(bytes, filename) {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }))
  const link = Object.assign(document.createElement("a"), { href: url, download: filename })
  link.click()
  URL.revokeObjectURL(url)
}

// --- Sharing ----------------------------------------------------------------

// The script travels in the URL fragment, which never reaches the server.
function encodeScript(code) {
  const bytes = new TextEncoder().encode(code)
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_")
}

function decodeScript(fragment) {
  const base64 = fragment.replace(/-/g, "+").replace(/_/g, "/")
  const binary = atob(base64)
  return new TextDecoder().decode(Uint8Array.from(binary, (c) => c.charCodeAt(0)))
}

async function share() {
  const url = new URL(location.href)
  url.hash = `script=${encodeScript(editor.value)}`
  history.replaceState(null, "", url)
  try {
    await navigator.clipboard.writeText(url.href)
    setStatus("Link with this script copied to the clipboard")
  } catch {
    setStatus("The address bar now holds a link to this script")
  }
}

// --- Editor -----------------------------------------------------------------

editor.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault()
    run()
    return
  }
  // A CAD script is indented code; Tab should indent it, not leave the field.
  if (event.key === "Tab") {
    event.preventDefault()
    const { selectionStart, selectionEnd, value } = editor
    editor.value = `${value.slice(0, selectionStart)}  ${value.slice(selectionEnd)}`
    editor.selectionStart = editor.selectionEnd = selectionStart + 2
  }
})

for (const [index, example] of EXAMPLES.entries()) {
  exampleSelect.append(new Option(example.name, String(index)))
}
exampleSelect.addEventListener("change", () => {
  const example = EXAMPLES[Number(exampleSelect.value)]
  if (!example) {
    return
  }
  editor.value = example.code
  shouldFit = true
  run()
})

runButton.addEventListener("click", run)
stopButton.addEventListener("click", stop)
shareButton.addEventListener("click", share)
fitButton.addEventListener("click", () => {
  viewer?.fit()
  viewer?.draw()
})
exportButton.addEventListener("click", () => {
  setStatus(`Exporting ${exportFormat.value.toUpperCase()}…`)
  worker.postMessage({ type: "export", format: exportFormat.value })
})

// --- Boot -------------------------------------------------------------------

const fragment = new URLSearchParams(location.hash.slice(1)).get("script")
if (fragment) {
  try {
    editor.value = decodeScript(fragment)
  } catch {
    editor.value = EXAMPLES[0].code
  }
} else {
  editor.value = localStorage.getItem(STORAGE_KEY) || EXAMPLES[0].code
}

setStatus("Loading the engine…")
runButton.disabled = true
stopButton.disabled = true
exportButton.disabled = true
startWorker()
