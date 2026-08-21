# luacad-prime-core

A vendored copy of the `prime-core` crate from
[Prime](https://github.com/lizhaoliu/Prime), a headless, physically based
path tracer by Lizhao Liu.

It powers LuaCAD's ray-traced rendering (`luacad render --raytrace`).

## Provenance

- Upstream: <https://github.com/lizhaoliu/Prime>, `crates/prime-core`
- Vendored at commit `e1f33b24ed594d6a663f64f1063369d674313ad3` (2026-07-08)
- Upstream declares the MIT license in its README (the repository carries no
  standalone LICENSE file)

Republished under this name because upstream is not on crates.io — the
`prime-core` name there belongs to an unrelated crate — and crates.io does not
allow git dependencies. This mirrors the `luacad-manifold-sys` arrangement.

## Local changes

- `Cargo.toml` rewritten for this workspace (crate renamed, `serde` made
  non-default, benches dropped)
- `material.rs`: added `Material::Plastic` (diffuse base + untinted GGX
  specular coat), used for LuaCAD's studio-style glossy shading — a candidate
  for upstreaming
- `integrator.rs`: `offset_origin()` pushes secondary rays further off the
  surface — a larger relative epsilon, widened for grazing rays. Upstream's
  epsilon assumes hits as precise as well-shaped triangles give; a boolean
  result is triangulated into slivers, whose hits land far enough under the
  surface that a flat face shadows itself along its own triangulation. Also a
  candidate for upstreaming
- Source files are otherwise unmodified; keep it that way where possible so
  upstream updates stay a plain re-copy
