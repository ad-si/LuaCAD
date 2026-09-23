{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    # The `rustc` in nixpkgs ships std for the host and little else, and the
    # browser build needs `wasm32-unknown-emscripten`. This overlay hands out
    # the same toolchains rustup does, with the targets chosen here.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      rust-overlay,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "clippy"
            "rust-analyzer"
            "rust-src"
            "rustfmt"
          ];
          # The playground's engine is built with Emscripten (Manifold's C++),
          # its viewer with wasm-bindgen (wgpu).
          targets = [
            "wasm32-unknown-emscripten"
            "wasm32-unknown-unknown"
          ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            bash
            cargo-insta
            cmake
            coreutils
            # `make wasm`: compiles Lua and Manifold for the browser.
            emscripten
            gnumake
            # `make example-images` and `make website-examples`: `cwebp`
            # converts the rendered PNGs to the WebP files that are checked in.
            libwebp
            # `make test-wasm` runs the smoke test on the built module.
            nodejs
            # `make wasm-viewer`: turns the viewer module into something the
            # page can import. The CLI and the `wasm-bindgen` crate have to be
            # the same version, which is why this names one rather than taking
            # whatever `wasm-bindgen-cli` currently points at: bump it here and
            # in `crates/luacad-viewer/Cargo.toml` together, and `make
            # wasm-viewer` will tell you if they ever drift apart.
            wasm-bindgen-cli_0_2_126
            # `make serve-website` serves ./website over HTTP.
            python3
            rust
            watchexec
          ];

          # bindgen loads libclang directly rather than going through the
          # compiler on PATH, so it has to be told where that library is.
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

          # Emscripten keeps its sysroot and prebuilt system libraries in a
          # cache it writes to, which cannot be the read-only copy in the Nix
          # store. Seed a writable one next to the checkout — that keeps the
          # first wasm build from rebuilding libc — and point the bindgen
          # sysroot lookup in luacad-manifold-sys/build.rs at it.
          shellHook = ''
            export EM_CACHE="''${EM_CACHE:-$PWD/.emscripten-cache}"
            if [ ! -d "$EM_CACHE" ]; then
              echo "Seeding the Emscripten cache in $EM_CACHE …"
              cp -r ${pkgs.emscripten}/share/emscripten/cache "$EM_CACHE"
              chmod -R u+w "$EM_CACHE"
            fi
            export EMSCRIPTEN_SYSROOT="$EM_CACHE/sysroot"
          '';
        };
        formatter = pkgs.nixfmt-tree; # Format this file with `nix fmt`
      }
    );
}
