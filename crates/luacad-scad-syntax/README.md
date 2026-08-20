# luacad-scad-syntax

Lexer and parser for the OpenSCAD language, used by `luacad` to open `.scad` files.

## Provenance

Vendored from [OpenRSCAD](https://github.com/matthova/openrscad), crate
`openrscad-syntax`, at commit `a08461511ebd4c315be8b5cd83702187fbd0878e`. Upstream is a clean-room reimplementation
of the OpenSCAD language — its grammar was reconstructed from public
documentation and black-box observation of the OpenSCAD CLI, with no OpenSCAD
(GPL) source consulted.

The code is vendored rather than depended on because the upstream crates are not
published to crates.io, and a git dependency would make `luacad` itself
unpublishable. This mirrors what `luacad-manifold-sys` and `luacad-prime-core`
already do.

Dependent crates rename this package back to its upstream name:

```toml
openrscad-syntax = { package = "luacad-scad-syntax", path = "../luacad-scad-syntax" }
```

so the vendored sources keep their `openrscad_syntax::` paths and stay diffable
against upstream. **Fix bugs upstream first where you can**, then re-vendor.

## Divergence from upstream

- None; a verbatim copy of `crates/openrscad-syntax/src`.

## License

Apache-2.0 OR MIT, as upstream. See `LICENSE-APACHE` and `LICENSE-MIT`.
Note that `luacad` as a whole remains AGPL-3.0-or-later.
