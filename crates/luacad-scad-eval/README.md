# luacad-scad-eval

Evaluator for the OpenSCAD language, used by `luacad` to open `.scad` files.

## Provenance

Vendored from [OpenRSCAD](https://github.com/matthova/openrscad), crate
`openrscad-eval`, at commit `a08461511ebd4c315be8b5cd83702187fbd0878e`. Upstream is a clean-room reimplementation
of the OpenSCAD language — its grammar was reconstructed from public
documentation and black-box observation of the OpenSCAD CLI, with no OpenSCAD
(GPL) source consulted.

The code is vendored rather than depended on because the upstream crates are not
published to crates.io, and a git dependency would make `luacad` itself
unpublishable. This mirrors what `luacad-manifold-sys` and `luacad-prime-core`
already do.

Dependent crates rename this package back to its upstream name:

```toml
openrscad-eval = { package = "luacad-scad-eval", path = "../luacad-scad-eval" }
```

so the vendored sources keep their `openrscad_eval::` paths and stay diffable
against upstream. **Fix bugs upstream first where you can**, then re-vendor.

## Divergence from upstream

- `src/text.rs` no longer bundles the twelve Liberation faces (4 MB of TTFs).
  LuaCAD's own `text()` is system-font-only, so both paths resolve fonts the
  same way. Upstream's byte-for-byte glyph match with OpenSCAD does not carry
  over.

## License

Apache-2.0 OR MIT, as upstream. See `LICENSE-APACHE` and `LICENSE-MIT`.
Note that `luacad` as a whole remains AGPL-3.0-or-later.
