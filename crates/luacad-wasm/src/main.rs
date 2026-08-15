//! Emscripten entry points for the browser playground.
//!
//! The website loads the module this crate links (`luacad-wasm.js` plus
//! `luacad-wasm.wasm`) in a web worker and drives it through three C
//! functions: run a script, export the last run to a mesh file, and free the
//! buffer either returned. Everything crosses the boundary as one heap buffer
//! rather than as JSON — a model of a few hundred thousand triangles is a
//! handful of megabytes of floats, which JSON would triple in size and make
//! the browser parse a character at a time.
//!
//! # Buffer layout
//!
//! Every buffer starts with its own length so the JavaScript side can copy it
//! out without a second call, and is 4-byte aligned throughout so typed-array
//! views can be taken directly onto the wasm heap:
//!
//! ```text
//! u32  payload_len   bytes that follow this field
//! u32  status        1 = ok, 0 = error
//!
//! error payload:
//!   u8[] message     UTF-8, fills the rest of the buffer
//!
//! run payload:
//!   u32  mesh_count
//!   per mesh:
//!     u32     name_len
//!     u8[]    name         UTF-8, zero-padded to a multiple of 4
//!     u32     has_color    1 = the script called color(), 0 = use the default
//!     f32[3]  color        RGB in 0..1, zeroed when has_color is 0
//!     u32     vert_count
//!     u32     tri_count
//!     f32[]   vertices     vert_count * 3, in CAD axes (x, y, z)
//!     u32[]   indices      tri_count * 3
//!
//! export payload:
//!   u8[] file        the exported mesh file, verbatim
//! ```

use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use luacad::export::{
  ManifoldMesh, clear_subtree_cache, describe_unsupported,
  extract_manifold_mesh, geometries_unsupported, materialize_scad_manifold,
};
use luacad::geometry::CsgGeometry;
use luacad::lua_engine::execute_lua;

// The geometries of the most recent successful run, kept so that exporting
// does not have to evaluate the script a second time. A thread local is
// enough: the module is single-threaded, and the worker that owns it runs one
// script at a time.
thread_local! {
  static LAST_RUN: RefCell<Vec<CsgGeometry>> = const { RefCell::new(Vec::new()) };
}

/// Emscripten runs `main` once while the module initialises, which is what
/// sets up the Rust runtime. The exported functions below are called
/// afterwards, from JavaScript.
fn main() {}

// ---------------------------------------------------------------------------
// Exported entry points
// ---------------------------------------------------------------------------

/// Evaluate a LuaCAD script and return its meshes.
///
/// The caller owns the returned buffer and must hand it back to
/// [`luacad_free`].
///
/// # Safety
///
/// `code` must be a valid NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn luacad_run(code: *const c_char) -> *mut u8 {
  let code = match unsafe { borrow_str(code) } {
    Ok(code) => code,
    Err(msg) => return error_buffer(msg),
  };

  match catch_unwind(AssertUnwindSafe(|| run(&code))) {
    Ok(Ok(payload)) => into_buffer(payload),
    Ok(Err(msg)) => error_buffer(&msg),
    Err(_) => error_buffer(PANIC_MESSAGE),
  }
}

/// Export the geometries of the last successful [`luacad_run`] to `format`
/// (`stl`, `3mf`, `obj`, `ply`, `off` or `amf`) and return the file's bytes.
///
/// The caller owns the returned buffer and must hand it back to
/// [`luacad_free`].
///
/// # Safety
///
/// `format` must be a valid NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn luacad_export(format: *const c_char) -> *mut u8 {
  let format = match unsafe { borrow_str(format) } {
    Ok(format) => format,
    Err(msg) => return error_buffer(msg),
  };

  match catch_unwind(AssertUnwindSafe(|| export(&format))) {
    Ok(Ok(payload)) => into_buffer(payload),
    Ok(Err(msg)) => error_buffer(&msg),
    Err(_) => error_buffer(PANIC_MESSAGE),
  }
}

/// Release a buffer returned by [`luacad_run`] or [`luacad_export`].
///
/// # Safety
///
/// `ptr` must come from one of those two functions and must not be used
/// afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn luacad_free(ptr: *mut u8) {
  if ptr.is_null() {
    return;
  }
  // The length prefix is what makes the original allocation recoverable.
  let len = LEN_PREFIX + unsafe { std::ptr::read(ptr as *const u32) } as usize;
  drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
}

/// The LuaCAD version this module was built from, as a static C string.
#[unsafe(no_mangle)]
pub extern "C" fn luacad_version() -> *const c_char {
  static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
  VERSION.as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

const PANIC_MESSAGE: &str =
  "The LuaCAD engine hit an internal error while building this model.";

fn run(code: &str) -> Result<Vec<u8>, String> {
  let geometries = execute_lua(code)?;

  // Constructs the Manifold backend cannot build produce a silently empty
  // preview otherwise, which reads as "my script is broken".
  let unsupported = geometries_unsupported(&geometries);
  if !unsupported.is_empty() {
    return Err(describe_unsupported(&unsupported));
  }

  let meshes: Vec<(ManifoldMesh, &CsgGeometry)> = geometries
    .iter()
    .filter_map(|geom| {
      let scad = geom.scad.as_ref()?;
      let manifold = materialize_scad_manifold(scad);
      if manifold.num_tri() == 0 {
        return None;
      }
      Some((extract_manifold_mesh(&manifold), geom))
    })
    .collect();

  // Every mesh has been extracted, so the shared subtrees are dead weight —
  // and in a 32-bit address space that weight is worth reclaiming eagerly.
  clear_subtree_cache();

  if meshes.is_empty() {
    return Err(
      "The script produced no 3D geometry. A model has to end up as a \
       solid — a 2D outline on its own has nothing to show."
        .to_string(),
    );
  }

  let mut payload = Vec::new();
  push_u32(&mut payload, meshes.len() as u32);
  for (mesh, geom) in &meshes {
    encode_mesh(&mut payload, mesh, geom);
  }

  LAST_RUN.with(|last| *last.borrow_mut() = geometries);
  Ok(payload)
}

fn export(format: &str) -> Result<Vec<u8>, String> {
  LAST_RUN.with(|last| {
    let geometries = last.borrow();
    if geometries.is_empty() {
      return Err("Run the script before exporting it.".to_string());
    }

    // Emscripten's in-memory filesystem gives the exporters the real file
    // they write to, so the playground shares one code path with the CLI
    // instead of a second, wasm-only serializer.
    let path = std::path::PathBuf::from(format!("/tmp/model.{format}"));
    luacad::export::export_manifold(&geometries, format, &path)?;
    let bytes = std::fs::read(&path)
      .map_err(|e| format!("Failed to read back the exported file: {e}"))?;
    let _ = std::fs::remove_file(&path);
    Ok(bytes)
  })
}

fn encode_mesh(out: &mut Vec<u8>, mesh: &ManifoldMesh, geom: &CsgGeometry) {
  let name = geom.name.as_deref().unwrap_or_default();
  push_u32(out, name.len() as u32);
  out.extend_from_slice(name.as_bytes());
  out.resize(out.len().next_multiple_of(4), 0);

  match geom.color {
    Some(rgb) => {
      push_u32(out, 1);
      for channel in rgb {
        push_f32(out, channel);
      }
    }
    None => {
      push_u32(out, 0);
      out.extend_from_slice(&[0; 12]);
    }
  }

  push_u32(out, mesh.vertices.len() as u32);
  push_u32(out, mesh.triangles.len() as u32);
  for vertex in &mesh.vertices {
    for coord in vertex {
      push_f32(out, *coord);
    }
  }
  for triangle in &mesh.triangles {
    for index in triangle {
      push_u32(out, *index);
    }
  }
}

// ---------------------------------------------------------------------------
// Buffer plumbing
// ---------------------------------------------------------------------------

/// The `payload_len` field itself, which its own value excludes.
const LEN_PREFIX: usize = 4;
/// `payload_len` + `status`.
const HEADER_LEN: usize = 8;

fn push_u32(out: &mut Vec<u8>, value: u32) {
  out.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(out: &mut Vec<u8>, value: f32) {
  out.extend_from_slice(&value.to_le_bytes());
}

/// Prefix `payload` with the length and an ok status, and leak it to the
/// caller.
fn into_buffer(payload: Vec<u8>) -> *mut u8 {
  build_buffer(1, &payload)
}

fn error_buffer(message: &str) -> *mut u8 {
  build_buffer(0, message.as_bytes())
}

fn build_buffer(status: u32, payload: &[u8]) -> *mut u8 {
  let mut buffer = Vec::with_capacity(HEADER_LEN + payload.len());
  push_u32(&mut buffer, (payload.len() + 4) as u32);
  push_u32(&mut buffer, status);
  buffer.extend_from_slice(payload);
  // Exactly the capacity `luacad_free` reconstructs from the length prefix.
  buffer.shrink_to_fit();
  let ptr = buffer.as_mut_ptr();
  std::mem::forget(buffer);
  ptr
}

/// # Safety
///
/// `ptr` must be a valid NUL-terminated C string.
unsafe fn borrow_str(ptr: *const c_char) -> Result<String, &'static str> {
  if ptr.is_null() {
    return Err("Missing argument.");
  }
  unsafe { CStr::from_ptr(ptr) }
    .to_str()
    .map(str::to_owned)
    .map_err(|_| "Argument is not valid UTF-8.")
}
