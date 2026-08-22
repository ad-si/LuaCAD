pub mod bosl;
pub mod export;
pub mod geometry;
pub mod linter;
pub mod lua_engine;
pub mod material;
pub mod mesh_import;
#[cfg(feature = "raytrace")]
pub mod raytrace;
pub mod render;
pub mod scad_export;
pub mod scad_import;
pub mod svg_import;
pub mod text_render;
pub mod version;

/// The OpenSCAD executable to run: `$OPENSCAD` when it is set and not empty,
/// otherwise `openscad` from `PATH`.
///
/// `--via-openscad` hands an export to whatever OpenSCAD the machine has, and
/// for that the `PATH` copy is the right one. The differential tests are the
/// other case: they measure LuaCAD against OpenSCAD's own output, and a
/// distribution's `openscad` is often years behind the behaviour being
/// matched, so the build they compare against has to be nameable.
pub fn openscad_binary() -> std::ffi::OsString {
  match std::env::var_os("OPENSCAD") {
    Some(binary) if !binary.is_empty() => binary,
    _ => std::ffi::OsString::from("openscad"),
  }
}
