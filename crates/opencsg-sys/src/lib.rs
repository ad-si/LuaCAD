//! FFI bindings to OpenCSG (image-based CSG rendering for OpenGL).
//!
//! OpenCSG renders CSG (Constructive Solid Geometry) results directly into
//! the OpenGL depth buffer using the Goldfeather or SCS algorithms. After
//! calling [`render`], re-render the primitives with `GL_EQUAL` depth
//! test to shade them.

use std::ffi::c_void;
use std::os::raw::{c_int, c_uint};

/// Operation: the primitive is intersected into the CSG product.
pub const INTERSECTION: c_int = 0;
/// Operation: the primitive is subtracted from the CSG product.
pub const SUBTRACTION: c_int = 1;

/// Algorithm: automatic selection based on primitive properties.
pub const ALGORITHM_AUTOMATIC: c_int = 0;
/// Algorithm: Goldfeather (robust, handles concave primitives).
pub const ALGORITHM_GOLDFEATHER: c_int = 1;
/// Algorithm: SCS (fast, convex primitives only).
pub const ALGORITHM_SCS: c_int = 2;

/// Option: which CSG algorithm to use.
pub const OPTION_ALGORITHM: c_int = 0;
/// Option: depth complexity sampling strategy.
pub const OPTION_DEPTH_COMPLEXITY: c_int = 1;
/// Option: offscreen buffer type.
pub const OPTION_OFFSCREEN: c_int = 2;

/// C callback type for rendering a primitive's geometry.
pub type RenderFn = unsafe extern "C" fn(*mut c_void);

/// Opaque handle to an OpenCSG primitive.
#[repr(C)]
pub struct OcsgPrimitive {
  _opaque: [u8; 0],
}

unsafe extern "C" {
  pub safe fn opencsg_init_gl();
  pub safe fn opencsg_glad_version() -> std::os::raw::c_int;
  pub fn opencsg_glad_fbo_support(
    out_arb_fbo: *mut std::os::raw::c_int,
    out_ext_fbo: *mut std::os::raw::c_int,
    out_ext_pds: *mut std::os::raw::c_int,
  );

  pub fn opencsg_primitive_new(
    operation: c_int,
    convexity: c_uint,
    callback: RenderFn,
    user_data: *mut c_void,
  ) -> *mut OcsgPrimitive;

  pub fn opencsg_primitive_free(prim: *mut OcsgPrimitive);

  pub fn opencsg_primitive_set_bounding_box(
    prim: *mut OcsgPrimitive,
    minx: f32,
    miny: f32,
    minz: f32,
    maxx: f32,
    maxy: f32,
    maxz: f32,
  );

  pub fn opencsg_render_primitives(
    primitives: *mut *mut OcsgPrimitive,
    count: c_int,
  );

  pub safe fn opencsg_set_option(option_type: c_int, setting: c_int);

  pub safe fn opencsg_free_resources();
}

// --- Safe wrappers ---

/// Initialize OpenCSG's internal GL loader. Call once after GL context is current.
pub fn init_gl() {
  opencsg_init_gl();
}

/// Set an OpenCSG option (algorithm, depth complexity, offscreen type).
pub fn set_option(option_type: c_int, setting: c_int) {
  opencsg_set_option(option_type, setting);
}

/// Release OpenGL resources allocated by OpenCSG for the current context.
pub fn free_resources() {
  opencsg_free_resources();
}

/// Perform CSG rendering on a list of primitive pointers.
///
/// # Safety
/// All pointers in `primitives` must be valid `OcsgPrimitive` handles.
/// The render callbacks must only issue valid GL calls.
pub unsafe fn render(primitives: &mut [*mut OcsgPrimitive]) {
  if primitives.is_empty() {
    return;
  }
  unsafe {
    opencsg_render_primitives(
      primitives.as_mut_ptr(),
      primitives.len() as c_int,
    );
  }
}

/// Create a new OpenCSG primitive.
///
/// # Safety
/// `callback` will be invoked by OpenCSG during rendering passes.
/// `user_data` must remain valid for the lifetime of the primitive.
pub unsafe fn primitive_new(
  operation: c_int,
  convexity: u32,
  callback: RenderFn,
  user_data: *mut c_void,
) -> *mut OcsgPrimitive {
  unsafe {
    opencsg_primitive_new(operation, convexity as c_uint, callback, user_data)
  }
}

/// Free an OpenCSG primitive.
///
/// # Safety
/// `prim` must be a valid handle returned by `primitive_new`.
pub unsafe fn primitive_free(prim: *mut OcsgPrimitive) {
  unsafe {
    opencsg_primitive_free(prim);
  }
}

/// Set the bounding box on a primitive (normalized device coordinates).
///
/// # Safety
/// `prim` must be a valid handle.
pub unsafe fn primitive_set_bbox(
  prim: *mut OcsgPrimitive,
  minx: f32,
  miny: f32,
  minz: f32,
  maxx: f32,
  maxy: f32,
  maxz: f32,
) {
  unsafe {
    opencsg_primitive_set_bounding_box(
      prim, minx, miny, minz, maxx, maxy, maxz,
    );
  }
}
