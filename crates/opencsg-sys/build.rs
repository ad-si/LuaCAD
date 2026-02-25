use std::path::PathBuf;

fn main() {
  let opencsg_dir: PathBuf = ["../../OpenCSG"].iter().collect();
  let src_dir = opencsg_dir.join("src");
  let include_dir = opencsg_dir.join("include");
  let glad_include = src_dir.join("glad").join("include");

  let cpp_files = [
    // OpenCSG core
    src_dir.join("area.cpp"),
    src_dir.join("batch.cpp"),
    src_dir.join("channelManager.cpp"),
    src_dir.join("context.cpp"),
    src_dir.join("frameBufferObject.cpp"),
    src_dir.join("frameBufferObjectExt.cpp"),
    src_dir.join("occlusionQuery.cpp"),
    src_dir.join("opencsgRender.cpp"),
    src_dir.join("openglHelper.cpp"),
    src_dir.join("primitive.cpp"),
    src_dir.join("primitiveHelper.cpp"),
    src_dir.join("renderGoldfeather.cpp"),
    src_dir.join("renderSCS.cpp"),
    src_dir.join("scissorMemo.cpp"),
    src_dir.join("settings.cpp"),
    // GLAD loader (inside OpenCSG namespace)
    src_dir.join("glad").join("src").join("gl.cpp"),
    // C FFI wrapper
    src_dir.join("opencsg_ffi.cpp"),
  ];

  let mut build = cc::Build::new();
  build
    .cpp(true)
    .std("c++11")
    .include(&include_dir)
    .include(&src_dir)
    .include(&glad_include)
    .warnings(false);

  for file in &cpp_files {
    build.file(file);
  }

  // Platform-specific GL linking
  let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  match target_os.as_str() {
    "macos" => {
      println!("cargo:rustc-link-lib=framework=OpenGL");
    }
    "linux" => {
      println!("cargo:rustc-link-lib=GL");
      println!("cargo:rustc-link-lib=dl");
    }
    "windows" => {
      println!("cargo:rustc-link-lib=opengl32");
    }
    _ => {}
  }

  build.compile("opencsg");

  // Re-run build if OpenCSG sources change
  println!("cargo:rerun-if-changed=../../OpenCSG/src/");
  println!("cargo:rerun-if-changed=../../OpenCSG/include/");
}
