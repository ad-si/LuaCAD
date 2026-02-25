//! Custom OpenGL context creation that requests Compatibility/Legacy profile.
//!
//! three-d's `WindowedContext::from_winit_window` always creates a Core Profile
//! context on macOS (GL 4.1). OpenCSG requires legacy GL functions (glBegin,
//! glMatrixMode, etc.) which are only available in the Compatibility/Legacy
//! profile.
//!
//! On macOS, we use CGL directly to create an NSOpenGLPixelFormat with the
//! Legacy profile, then construct the context from that.
//!
//! On Linux, we use glutin with `GlProfile::Compatibility`.

/// Our custom windowed GL context that provides Compatibility/Legacy profile.
pub struct GlWindowContext {
  pub context: three_d::Context,
  #[cfg(target_os = "macos")]
  inner: macos::MacGlContext,
  #[cfg(target_os = "linux")]
  inner: linux::LinuxGlContext,
}

impl GlWindowContext {
  pub fn new(window: &winit::window::Window, stencil_bits: u8) -> Self {
    #[cfg(target_os = "macos")]
    let (context, inner) = macos::create_context(window, stencil_bits);
    #[cfg(target_os = "linux")]
    let (context, inner) = linux::create_context(window, stencil_bits);

    Self { context, inner }
  }

  pub fn swap_buffers(&self) {
    self.inner.swap_buffers();
  }

  pub fn resize(&self, physical_size: winit::dpi::PhysicalSize<u32>) {
    self.inner.resize(physical_size);
  }
}

impl std::ops::Deref for GlWindowContext {
  type Target = three_d::Context;

  fn deref(&self) -> &Self::Target {
    &self.context
  }
}

// ---- macOS: CGL + NSOpenGL with Legacy profile ----

#[cfg(target_os = "macos")]
#[allow(non_upper_case_globals)]
mod macos {
  use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
  use std::ffi::{c_char, c_void};
  use std::sync::Arc;

  // NSOpenGLPixelFormatAttribute constants (using Apple's naming convention)
  const NSOpenGLPFADoubleBuffer: u32 = 5;
  const NSOpenGLPFADepthSize: u32 = 12;
  const NSOpenGLPFAStencilSize: u32 = 13;
  const NSOpenGLPFAOpenGLProfile: u32 = 99;
  const NSOpenGLProfileVersionLegacy: u32 = 0x1000;
  const NSOpenGLPFAColorSize: u32 = 8;
  const NSOpenGLPFAAlphaSize: u32 = 11;
  const NSOpenGLPFAAccelerated: u32 = 73;

  // Objective-C runtime bindings — use typed function pointers, NOT variadic,
  // because on ARM64 variadic and non-variadic have different ABIs.
  #[link(name = "objc", kind = "dylib")]
  unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
  }

  // Typed objc_msgSend variants — cast to the specific signature needed.
  // On ARM64, objc_msgSend is a trampoline with standard (non-variadic) ABI.
  // We MUST NOT declare it as variadic in Rust, as ARM64 uses different
  // registers for variadic vs non-variadic arguments.
  type MsgSendNoArgs =
    unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
  type MsgSendOnePtr = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *const c_void,
  ) -> *mut c_void;
  type MsgSendTwoPtr = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
  ) -> *mut c_void;

  unsafe fn get_objc_msgsend_addr() -> *mut c_void {
    unsafe {
      let lib = dlopen(c"/usr/lib/libobjc.A.dylib".as_ptr(), RTLD_LAZY);
      assert!(!lib.is_null(), "Failed to open libobjc");
      let addr = dlsym(lib, c"objc_msgSend".as_ptr());
      assert!(!addr.is_null(), "Failed to find objc_msgSend");
      addr
    }
  }

  fn msg_send_no_args() -> MsgSendNoArgs {
    unsafe { std::mem::transmute(get_objc_msgsend_addr()) }
  }

  fn msg_send_one_ptr() -> MsgSendOnePtr {
    unsafe { std::mem::transmute(get_objc_msgsend_addr()) }
  }

  fn msg_send_two_ptr() -> MsgSendTwoPtr {
    unsafe { std::mem::transmute(get_objc_msgsend_addr()) }
  }

  // For loading GL functions we use dlsym on the OpenGL framework
  #[link(name = "System")]
  unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
  }

  const RTLD_NOLOAD: i32 = 0x10;
  const RTLD_LAZY: i32 = 0x1;

  pub struct MacGlContext {
    ns_context: *mut c_void,
  }

  // SAFETY: We only use the context on the main thread (same thread that created it)
  unsafe impl Send for MacGlContext {}

  impl MacGlContext {
    pub fn swap_buffers(&self) {
      unsafe {
        let sel = sel_registerName(c"flushBuffer".as_ptr());
        msg_send_no_args()(self.ns_context, sel);
      }
    }

    pub fn resize(&self, _physical_size: winit::dpi::PhysicalSize<u32>) {
      unsafe {
        let sel = sel_registerName(c"update".as_ptr());
        msg_send_no_args()(self.ns_context, sel);
      }
    }
  }

  impl Drop for MacGlContext {
    fn drop(&mut self) {
      unsafe {
        let sel = sel_registerName(c"clearDrawable".as_ptr());
        msg_send_no_args()(self.ns_context, sel);
        let sel = sel_registerName(c"release".as_ptr());
        msg_send_no_args()(self.ns_context, sel);
      }
    }
  }

  // Stub VAO functions for GL 2.1 Legacy where VAOs don't exist.
  // three-d creates one global VAO; on GL 2.1 vertex state is already global.
  unsafe extern "C" fn stub_gen_vertex_arrays(n: i32, arrays: *mut u32) {
    for i in 0..n {
      unsafe {
        *arrays.offset(i as isize) = 1;
      } // return fake name "1"
    }
  }
  unsafe extern "C" fn stub_delete_vertex_arrays(_n: i32, _arrays: *const u32) {
  }
  unsafe extern "C" fn stub_bind_vertex_array(_array: u32) {}
  unsafe extern "C" fn stub_is_vertex_array(_array: u32) -> u8 {
    1
  }

  pub fn create_context(
    window: &winit::window::Window,
    stencil_bits: u8,
  ) -> (three_d::Context, MacGlContext) {
    unsafe {
      let alloc_sel = sel_registerName(c"alloc".as_ptr());

      // Build pixel format attributes for Legacy profile (GL 2.1 Compatibility)
      let attrs: Vec<u32> = vec![
        NSOpenGLPFAOpenGLProfile,
        NSOpenGLProfileVersionLegacy,
        NSOpenGLPFADoubleBuffer,
        NSOpenGLPFAAccelerated,
        NSOpenGLPFAColorSize,
        24,
        NSOpenGLPFAAlphaSize,
        8,
        NSOpenGLPFADepthSize,
        24,
        NSOpenGLPFAStencilSize,
        stencil_bits as u32,
        0, // terminator
      ];

      // Create NSOpenGLPixelFormat
      let pixel_format_class = objc_getClass(c"NSOpenGLPixelFormat".as_ptr());
      let pixel_format = msg_send_no_args()(pixel_format_class, alloc_sel);

      let init_sel = sel_registerName(c"initWithAttributes:".as_ptr());
      let pixel_format = msg_send_one_ptr()(
        pixel_format,
        init_sel,
        attrs.as_ptr() as *const c_void,
      );
      assert!(
        !pixel_format.is_null(),
        "Failed to create NSOpenGLPixelFormat with Legacy profile"
      );

      // Create NSOpenGLContext
      let context_class = objc_getClass(c"NSOpenGLContext".as_ptr());
      let context = msg_send_no_args()(context_class, alloc_sel);

      let init_context_sel =
        sel_registerName(c"initWithFormat:shareContext:".as_ptr());
      let context = msg_send_two_ptr()(
        context,
        init_context_sel,
        pixel_format,
        std::ptr::null_mut::<c_void>(),
      );
      assert!(!context.is_null(), "Failed to create NSOpenGLContext");

      // Release pixel format (context retains it)
      let release_sel = sel_registerName(c"release".as_ptr());
      msg_send_no_args()(pixel_format, release_sel);

      // Get the NSView from the winit window and set it as the context's view
      let ns_view = match window.raw_window_handle() {
        RawWindowHandle::AppKit(handle) => handle.ns_view,
        _ => panic!("Expected AppKit window handle on macOS"),
      };

      let set_view_sel = sel_registerName(c"setView:".as_ptr());
      msg_send_one_ptr()(context, set_view_sel, ns_view as *const c_void);

      // Make current
      let make_current_sel = sel_registerName(c"makeCurrentContext".as_ptr());
      msg_send_no_args()(context, make_current_sel);

      // Get the OpenGL framework handle for dlsym
      let gl_handle = dlopen(
        c"/System/Library/Frameworks/OpenGL.framework/OpenGL".as_ptr(),
        RTLD_NOLOAD | RTLD_LAZY,
      );

      // Create glow context.
      //
      // GL 2.1 Legacy doesn't have VAO functions (glGenVertexArrays etc.) which
      // are GL 3.0+. three-d::Context::from_gl_context() unconditionally creates
      // one VAO. We provide stub implementations that return a fake VAO name.
      // This works because:
      // - three-d creates exactly one VAO and binds it globally
      // - On GL 2.1 without VAOs, vertex attribute state is already global
      // - Our OpenCSG rendering uses raw GL calls that don't need VAOs
      // - egui_glow has its own VAO emulation fallback
      let gl_handle_copy = gl_handle;
      let glow_context = glow::Context::from_loader_function(|name| {
        // Provide stub VAO functions for GL 2.1 Legacy
        match name {
          "glGenVertexArrays" => {
            return stub_gen_vertex_arrays as *const c_void;
          }
          "glDeleteVertexArrays" => {
            return stub_delete_vertex_arrays as *const c_void;
          }
          "glBindVertexArray" => {
            return stub_bind_vertex_array as *const c_void;
          }
          "glIsVertexArray" => return stub_is_vertex_array as *const c_void,
          _ => {}
        }
        let cname = std::ffi::CString::new(name).unwrap();
        let ptr = dlsym(gl_handle_copy, cname.as_ptr());
        if !ptr.is_null() {
          return ptr as *const c_void;
        }
        // Try APPLE/ARB/EXT extension suffix fallbacks
        for suffix in &["APPLE", "ARB", "EXT"] {
          let ext_name = format!("{}{}", name, suffix);
          let cname_ext = std::ffi::CString::new(ext_name).unwrap();
          let ptr_ext = dlsym(gl_handle_copy, cname_ext.as_ptr());
          if !ptr_ext.is_null() {
            return ptr_ext as *const c_void;
          }
        }
        std::ptr::null()
      });

      let three_d_context =
        three_d::Context::from_gl_context(Arc::new(glow_context))
          .expect("failed to create three-d Context");

      let mac_ctx = MacGlContext {
        ns_context: context,
      };

      (three_d_context, mac_ctx)
    }
  }
}

// ---- Linux: glutin with Compatibility profile ----

#[cfg(target_os = "linux")]
mod linux {
  use glutin::config::ConfigTemplateBuilder;
  use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, Version,
  };
  use glutin::display::Display;
  use glutin::prelude::*;
  use glutin::surface::{
    SurfaceAttributesBuilder, SwapInterval, WindowSurface,
  };
  use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
  use std::num::NonZeroU32;
  use std::sync::Arc;

  pub struct LinuxGlContext {
    glutin_context: glutin::context::PossiblyCurrentContext,
    surface: glutin::surface::Surface<WindowSurface>,
  }

  impl LinuxGlContext {
    pub fn swap_buffers(&self) {
      self
        .surface
        .swap_buffers(&self.glutin_context)
        .expect("failed to swap buffers");
    }

    pub fn resize(&self, physical_size: winit::dpi::PhysicalSize<u32>) {
      let width = NonZeroU32::new(physical_size.width.max(1)).unwrap();
      let height = NonZeroU32::new(physical_size.height.max(1)).unwrap();
      self.surface.resize(&self.glutin_context, width, height);
    }
  }

  pub fn create_context(
    window: &winit::window::Window,
    stencil_bits: u8,
  ) -> (three_d::Context, LinuxGlContext) {
    let raw_display_handle = window.raw_display_handle();
    let raw_window_handle = window.raw_window_handle();

    let preference = glutin::display::DisplayApiPreference::EglThenGlx(
      Box::new(winit::platform::x11::register_xlib_error_hook),
    );

    let gl_display = unsafe {
      Display::new(raw_display_handle, preference)
        .expect("failed to create GL display")
    };

    let config_template = ConfigTemplateBuilder::new()
      .with_depth_size(24)
      .with_stencil_size(stencil_bits)
      .compatible_with_native_window(raw_window_handle)
      .build();

    let config = unsafe {
      gl_display
        .find_configs(config_template)
        .expect("failed to find GL configs")
        .next()
        .expect("no GL config found")
    };

    // Request GL 2.1 Compatibility profile
    let context_attributes = ContextAttributesBuilder::new()
      .with_profile(GlProfile::Compatibility)
      .with_context_api(ContextApi::OpenGl(Some(Version::new(2, 1))))
      .build(Some(raw_window_handle));

    let (width, height): (u32, u32) = window.inner_size().into();
    let width = NonZeroU32::new(width.max(1)).unwrap();
    let height = NonZeroU32::new(height.max(1)).unwrap();

    let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new()
      .build(raw_window_handle, width, height);

    let gl_context = unsafe {
      gl_display
        .create_context(&config, &context_attributes)
        .expect("failed to create GL context")
    };

    let gl_surface = unsafe {
      gl_display
        .create_window_surface(&config, &surface_attributes)
        .expect("failed to create GL surface")
    };

    let gl_context = gl_context
      .make_current(&gl_surface)
      .expect("failed to make GL context current");

    let _ = gl_surface.set_swap_interval(
      &gl_context,
      SwapInterval::Wait(NonZeroU32::new(1).unwrap()),
    );

    let glow_context = unsafe {
      glow::Context::from_loader_function(|s| {
        let cstr = std::ffi::CString::new(s)
          .expect("failed to create CString for GL proc");
        gl_display.get_proc_address(&cstr)
      })
    };

    let three_d_context =
      three_d::Context::from_gl_context(Arc::new(glow_context))
        .expect("failed to create three-d Context");

    let inner = LinuxGlContext {
      glutin_context: gl_context,
      surface: gl_surface,
    };

    (three_d_context, inner)
  }
}
