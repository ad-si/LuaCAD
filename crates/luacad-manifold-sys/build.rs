use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    // GNUInstallDirs picks a distribution-specific library directory —
    // `lib64` on Fedora and its relatives — and the static archives would
    // then land somewhere this script does not look. Pin it.
    .define("CMAKE_INSTALL_LIBDIR", "lib")
    // Manifold turns its Emscripten JS bindings on by default when it detects
    // an Emscripten toolchain. They live in `bindings/wasm`, which this crate
    // does not vendor, and they are useless here anyway — the wasm build
    // reaches Manifold through the same C API as every other target.
    .define("MANIFOLD_JSBIND", "OFF")
    .define(
      "FETCHCONTENT_SOURCE_DIR_CLIPPER2",
      clipper2_dir.to_str().expect("non-UTF-8 manifest path"),
    )
    .define("FETCHCONTENT_FULLY_DISCONNECTED", "ON")
    .out_dir(out_dir.clone());

  if target_os == "windows" {
    cmake_config.cxxflag("/EHsc");
  }

  if target_os == "emscripten" {
    // CMake cannot reconfigure an existing Emscripten build directory on
    // macOS: the second pass re-runs the compiler test, and this time hands
    // `em++` the host's `-arch arm64` and `-isysroot`, which it rejects —
    // after which the directory stays broken however often it is retried.
    // Switching between an emsdk and the Emscripten from `nix develop` lands
    // in the same state. A first configure always succeeds, so give it an
    // empty directory; this costs a Manifold rebuild only when the build
    // script re-runs at all, which needs one of its inputs to have changed.
    let _ = fs::remove_dir_all(out_dir.join("build"));

    cmake_config
      // The `cmake` crate derives the system name from the target triple and
      // arrives at a lowercase `emscripten`, which only finds CMake's
      // `Platform/Emscripten` module on a case-insensitive filesystem.
      // Naming it here also suppresses the crate's own guess.
      .define("CMAKE_SYSTEM_NAME", "Emscripten")
      .define("CMAKE_SYSTEM_PROCESSOR", "wasm32");
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
    // rustc drives the link through `emcc`, which unlike `em++` does not pull
    // in the C++ runtime on its own.
    ("emscripten", _) => {
      println!("cargo:rustc-link-lib=c++");
      println!("cargo:rustc-link-lib=c++abi");
    }
    ("windows", "msvc") => {}
    _ => {}
  }

  generate_bindings(&out_dir, &target_os);
}

fn generate_bindings(out_dir: &Path, target_os: &str) {
  let mut builder = bindgen::Builder::default()
    .header("vendor/manifold/bindings/c/include/manifold/manifoldc.h")
    .clang_arg("-Ivendor/manifold/bindings/c/include")
    .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

  // bindgen runs libclang, not emcc, so for a wasm target it would otherwise
  // describe the host's type layout — `max_align_t` and pointer width differ,
  // and the layout assertions bindgen emits then fail to compile. Aim libclang
  // at the Emscripten sysroot instead.
  if target_os == "emscripten" {
    let target = env::var("TARGET").unwrap();
    builder = builder
      .clang_arg(format!("--target={target}"))
      .clang_arg(format!("--sysroot={}", emscripten_sysroot().display()))
      // clang makes symbols hidden by default for wasm targets, and bindgen
      // skips every function that is not of default visibility — without this
      // the generated bindings contain the types but not one function.
      .clang_arg("-fvisibility=default")
      // Emscripten's `max_align_t` holds a `long double`, which bindgen maps
      // to `u128`; that has a stricter alignment in Rust than on wasm, so the
      // emitted layout assertion is unsatisfiable. Nothing in the Manifold C
      // API uses the type, so drop it.
      .blocklist_type("max_align_t");
  }

  let bindings = match try_generate(builder.clone()) {
    Ok(bindings) => bindings,
    Err(first_error) => {
      // libclang looks for `stddef.h` and the other compiler-provided headers
      // in a resource directory next to itself. When the two come from
      // different packages — as on Fedora, where clang keeps them under a
      // versioned `/usr/lib/clang/<version>/include` — it finds none of them
      // and every header that includes one fails to parse. Ask the clang
      // binary where the directory actually is and try again.
      let include = clang_resource_include().unwrap_or_else(|| {
        panic!("{first_error}");
      });

      println!(
        "cargo:warning=bindgen could not parse the Manifold headers; \
         retrying with the clang resource directory at {}",
        include.display()
      );

      try_generate(builder.clang_arg(format!("-isystem{}", include.display())))
        .unwrap_or_else(|second_error| {
          panic!(
            "{second_error}\n\nthe first attempt, without the clang resource \
             directory, failed with:\n{first_error}"
          )
        })
    }
  };

  bindings
    .write_to_file(out_dir.join("bindings.rs"))
    .expect("Couldn't write bindings!");
}

fn try_generate(
  builder: bindgen::Builder,
) -> Result<bindgen::Bindings, String> {
  let bindings = builder
    .generate()
    .map_err(|err| format!("unable to generate bindings: {err}"))?;

  // bindgen drops declarations it cannot make sense of and still reports
  // success, which surfaces a hundred "cannot find function in this scope"
  // errors in the crates downstream instead of pointing at the real cause —
  // usually clang args that do not suit the target. Catch it here.
  if !bindings.to_string().contains("fn manifold_cube") {
    return Err(
      "bindgen produced no Manifold functions; check the clang arguments for \
       this target"
        .into(),
    );
  }

  Ok(bindings)
}

/// The include directory of the resource directory of the `clang` binary,
/// which holds the compiler-provided headers such as `stddef.h`.
fn clang_resource_include() -> Option<PathBuf> {
  println!("cargo:rerun-if-env-changed=CLANG_PATH");

  let clang = env::var("CLANG_PATH").unwrap_or_else(|_| "clang".into());
  let output = Command::new(clang)
    .arg("-print-resource-dir")
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }

  let dir = String::from_utf8(output.stdout).ok()?;
  let include = PathBuf::from(dir.trim()).join("include");
  include.is_dir().then_some(include)
}

/// Locate the sysroot of the active Emscripten toolchain.
///
/// Three ways in, because the toolchains lay themselves out differently:
/// `EMSCRIPTEN_SYSROOT` is the explicit override this crate defines and the
/// one the dev shell in `flake.nix` sets, `EM_CACHE` is Emscripten's own
/// variable for the directory holding the sysroot, and `EMSDK` is what
/// `emsdk_env.sh` and the `setup-emsdk` CI action export.
fn emscripten_sysroot() -> PathBuf {
  println!("cargo:rerun-if-env-changed=EMSCRIPTEN_SYSROOT");
  println!("cargo:rerun-if-env-changed=EM_CACHE");
  println!("cargo:rerun-if-env-changed=EMSDK");

  if let Ok(sysroot) = env::var("EMSCRIPTEN_SYSROOT") {
    return PathBuf::from(sysroot);
  }
  if let Ok(cache) = env::var("EM_CACHE") {
    return PathBuf::from(cache).join("sysroot");
  }
  let emsdk = env::var("EMSDK").expect(
    "building for Emscripten needs a toolchain to point at: set \
     EMSCRIPTEN_SYSROOT, or EMSDK by sourcing emsdk_env.sh, or enter the dev \
     shell with `nix develop`",
  );
  PathBuf::from(emsdk)
    .join("upstream")
    .join("emscripten")
    .join("cache")
    .join("sysroot")
}
