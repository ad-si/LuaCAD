use std::env;
use std::path::{Path, PathBuf};

fn main() {
  // bindgen's CargoCallbacks emits rerun-if-changed for the parsed headers,
  // which disables cargo's default rerun-on-any-package-change heuristic —
  // so the vendored C++ sources must be watched explicitly.
  println!("cargo:rerun-if-changed=vendor/manifold/src");
  println!("cargo:rerun-if-changed=vendor/manifold/include");
  println!("cargo:rerun-if-changed=vendor/manifold/bindings/c");
  println!("cargo:rerun-if-changed=vendor/clipper2/CPP");

  let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
  let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
  let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap();

  // Manifold's CMake falls back to cloning Clipper2 from GitHub when it isn't
  // found on the system. Point FetchContent at the vendored copy instead so
  // the build needs no network — required for `cargo publish` verification,
  // docs.rs, and any sandboxed or offline build.
  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
  let clipper2_dir = manifest_dir.join("vendor").join("clipper2");

  let mut cmake_config = cmake::Config::new("vendor/manifold");

  cmake_config
    .define("BUILD_SHARED_LIBS", "OFF")
    .define("MANIFOLD_TEST", "OFF")
    .define("MANIFOLD_CBIND", "ON")
    .define("MANIFOLD_CROSS_SECTION", "ON")
    .define("MANIFOLD_PAR", "OFF")
    .define("MANIFOLD_EXPORT", "OFF")
    .define("MANIFOLD_USE_BUILTIN_CLIPPER2", "ON")
    .define(
      "FETCHCONTENT_SOURCE_DIR_CLIPPER2",
      clipper2_dir.to_str().expect("non-UTF-8 manifest path"),
    )
    .define("FETCHCONTENT_FULLY_DISCONNECTED", "ON")
    .out_dir(out_dir.clone());

  if target_os == "windows" {
    cmake_config.cxxflag("/EHsc");
  }

  let dst = cmake_config.build();

  println!("cargo:rustc-link-search=native={}/lib", dst.display());
  println!("cargo:rustc-link-lib=static=manifoldc");
  println!("cargo:rustc-link-lib=static=manifold");
  println!("cargo:rustc-link-lib=static=Clipper2");

  match (target_os.as_str(), target_env.as_str()) {
    ("linux", _) | ("windows", "gnu") | ("android", _) => {
      println!("cargo:rustc-link-lib=dylib=stdc++")
    }
    ("macos", _) | ("ios", _) => println!("cargo:rustc-link-lib=dylib=c++"),
    ("windows", "msvc") => {}
    _ => {}
  }

  generate_bindings(&out_dir);
}

fn generate_bindings(out_dir: &Path) {
  bindgen::Builder::default()
    .header("vendor/manifold/bindings/c/include/manifold/manifoldc.h")
    .clang_arg("-Ivendor/manifold/bindings/c/include")
    .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file(out_dir.join("bindings.rs"))
    .expect("Couldn't write bindings!");
}
