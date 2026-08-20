//! Read an OpenSCAD `.scad` file as LuaCAD geometry.
//!
//! The OpenSCAD language front end — lexer, parser and evaluator — is the
//! vendored `luacad-scad-*` trio (see their READMEs for provenance). It lowers
//! a program to `openrscad_ir::Node`, a CSG tree that is nearly variant-for-
//! variant [`ScadNode`]. This module is the adapter between the two, so a
//! `.scad` file joins LuaCAD's pipeline at the same point a Lua script does and
//! everything downstream — Manifold meshing, every export format, the PNG
//! renderer, the path tracer, Studio's live preview — works unchanged.
//!
//! What the two trees disagree about is listed under "Fidelity" below; each
//! case reports a warning rather than silently producing different geometry.

use std::path::{Path, PathBuf};

use openrscad_ir::{FragmentSpec, Node};

use crate::geometry::CsgGeometry;
use crate::scad_export::{ModifierKind, ScadNode};

/// A `.scad` file, read and lowered to LuaCAD geometry.
#[derive(Debug)]
pub struct ScadProgram {
  /// The whole program as one object. OpenSCAD unions its top-level children,
  /// so splitting them into separate geometries would export overlapping
  /// solids instead of a fused one.
  pub geometries: Vec<CsgGeometry>,
  /// Output from `echo()`, in evaluation order.
  pub echoes: Vec<String>,
  /// Everything the evaluator or this adapter had to warn about.
  pub warnings: Vec<String>,
}

/// Whether `file`'s extension is `.scad`. Says nothing about the contents.
pub fn is_scad_file(file: impl AsRef<Path>) -> bool {
  file
    .as_ref()
    .extension()
    .and_then(|e| e.to_str())
    .is_some_and(|e| e.eq_ignore_ascii_case("scad"))
}

/// Read and evaluate a `.scad` file.
///
/// `include <>` and `use <>` resolve relative to the file, then against each
/// directory in `OPENSCADPATH`.
pub fn load_scad_file(path: &Path) -> Result<ScadProgram, String> {
  let source = std::fs::read_to_string(path)
    .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
  let dir = path.parent().unwrap_or(Path::new("."));
  load_scad(&source, dir).map_err(|e| format!("{}: {e}", path.display()))
}

/// Evaluate OpenSCAD source, resolving `include`/`use` against `base_dir`.
pub fn load_scad(source: &str, base_dir: &Path) -> Result<ScadProgram, String> {
  let program = openrscad_syntax::parse(source).map_err(|e| {
    let (line, col) = line_col(source, e.span.start);
    format!("line {line}, column {col}: {}", e.message)
  })?;

  // text() needs a face, and the vendored evaluator bundles no fonts.
  openrscad_eval::register_system_fonts();

  let resolver = DiskResolver::new(base_dir);
  let out = openrscad_eval::eval_program_with(
    &program,
    &resolver,
    &base_dir.to_string_lossy(),
  )
  .map_err(|e| match &e.span {
    Some(span) => {
      let (line, col) = line_col(source, span.start);
      format!("line {line}, column {col}: {}", e.message)
    }
    None => e.message.clone(),
  })?;

  let mut cx = Convert::default();
  let node = cx.node(&out.node);

  let mut warnings: Vec<String> =
    out.warnings.into_iter().map(|w| w.message).collect();
  warnings.append(&mut cx.warnings);

  Ok(ScadProgram {
    geometries: vec![CsgGeometry {
      name: None,
      mesh: {
        #[cfg(feature = "csgrs")]
        {
          None
        }
        #[cfg(not(feature = "csgrs"))]
        {
          None
        }
      },
      color: None,
      material: None,
      scad: Some(node),
    }],
    echoes: out.echoes,
    warnings,
  })
}

/// Byte offset to a 1-based line and column, for error messages.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
  let upto = &source[..offset.min(source.len())];
  let line = upto.matches('\n').count() + 1;
  let col = upto.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
  (line, col)
}

/// Resolves `include`/`use` from disk: relative to the including file first,
/// then each `OPENSCADPATH` entry, matching OpenSCAD's own search order.
struct DiskResolver {
  libs: Vec<PathBuf>,
}

impl DiskResolver {
  fn new(_base: &Path) -> Self {
    let libs = std::env::var_os("OPENSCADPATH")
      .map(|p| std::env::split_paths(&p).collect())
      .unwrap_or_default();
    DiskResolver { libs }
  }

  fn candidates(&self, path: &str, from_dir: &str) -> Vec<PathBuf> {
    std::iter::once(Path::new(from_dir).join(path))
      .chain(self.libs.iter().map(|l| l.join(path)))
      .collect()
  }
}

impl openrscad_eval::FileResolver for DiskResolver {
  fn load(
    &self,
    path: &str,
    from_dir: &str,
  ) -> Option<openrscad_eval::LoadedFile> {
    for c in self.candidates(path, from_dir) {
      let Ok(source) = std::fs::read_to_string(&c) else {
        continue;
      };
      let key = std::fs::canonicalize(&c)
        .unwrap_or_else(|_| c.clone())
        .to_string_lossy()
        .into_owned();
      let dir = c
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
      return Some(openrscad_eval::LoadedFile { key, source, dir });
    }
    None
  }

  fn load_bytes(&self, path: &str, from_dir: &str) -> Option<Vec<u8>> {
    self
      .candidates(path, from_dir)
      .into_iter()
      .find_map(|c| std::fs::read(&c).ok())
  }
}

/// OpenSCAD's `GRID_FINE`: below this radius a curved primitive collapses.
const GRID_FINE: f64 = 0.000_000_953_674_316_406_25;

/// How many fragments OpenSCAD puts on a full circle of radius `r` under the
/// `$fn`/`$fa`/`$fs` in effect where the primitive was written.
///
/// [`ScadNode`] stores a resolved fragment count where the IR keeps the three
/// variables, so this is where the OpenSCAD formula gets applied. It is the
/// same rule as [`crate::export`]'s `openscad_segments`, generalized past that
/// function's assumption of the default `$fa`/`$fs` and no `$fn`.
fn fragments(r: f64, f: FragmentSpec) -> u32 {
  if r < GRID_FINE || f.fn_.is_nan() || f.fn_.is_infinite() {
    return 3;
  }
  if f.fn_ > 0.0 {
    return if f.fn_ >= 3.0 { f.fn_ as u32 } else { 3 };
  }
  ((360.0 / f.fa).min(r * 2.0 * std::f64::consts::PI / f.fs)).max(5.0).ceil()
    as u32
}

/// Lowers `openrscad_ir::Node` to [`ScadNode`], collecting what it cannot carry
/// across exactly.
#[derive(Default)]
struct Convert {
  warnings: Vec<String>,
}

impl Convert {
  /// Warn once per distinct message — a construct inside a loop would
  /// otherwise report on every iteration.
  fn warn(&mut self, message: String) {
    if !self.warnings.contains(&message) {
      self.warnings.push(message);
    }
  }

  fn children(&mut self, nodes: &[Node]) -> Vec<ScadNode> {
    nodes.iter().map(|n| self.node(n)).collect()
  }

  fn child(&mut self, node: &Node) -> Box<ScadNode> {
    Box::new(self.node(node))
  }

  fn node(&mut self, node: &Node) -> ScadNode {
    match node {
      // An empty union is LuaCAD's nothing; the materializers already treat it
      // as an empty manifold.
      Node::Empty => ScadNode::Union(Vec::new()),
      // OpenSCAD's implicit group unions for rendering.
      Node::Group(children) => ScadNode::Union(self.children(children)),

      Node::Cube { size, center } => ScadNode::Cube {
        w: size[0] as f32,
        d: size[1] as f32,
        h: size[2] as f32,
        center: *center,
      },
      Node::Sphere { r, frags } => ScadNode::Sphere {
        r: *r as f32,
        segments: fragments(*r, *frags),
      },
      Node::Cylinder {
        h,
        r1,
        r2,
        center,
        frags,
      } => ScadNode::Cylinder {
        r1: *r1 as f32,
        r2: *r2 as f32,
        h: *h as f32,
        // OpenSCAD refines a cone by its widest radius.
        segments: fragments(r1.max(*r2), *frags),
        center: *center,
      },
      Node::Polyhedron { points, faces } => ScadNode::Polyhedron {
        points: points
          .iter()
          .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
          .collect(),
        faces: faces
          .iter()
          .map(|f| f.iter().map(|&i| i as usize).collect())
          .collect(),
      },

      Node::Square { size, center } => ScadNode::Square {
        w: size[0] as f32,
        h: size[1] as f32,
        center: *center,
      },
      Node::Circle { r, frags } => ScadNode::Circle {
        r: *r as f32,
        segments: fragments(*r, *frags),
      },
      Node::Polygon { points, paths } => ScadNode::Polygon {
        points: points.iter().map(|p| [p[0] as f32, p[1] as f32]).collect(),
        paths: paths.as_ref().map(|paths| {
          paths
            .iter()
            .map(|p| p.iter().map(|&i| i as usize).collect())
            .collect()
        }),
      },

      Node::LinearExtrude {
        height,
        center,
        twist,
        scale,
        slices,
        child,
      } => {
        // LuaCAD's scale is one factor for both axes.
        if (scale[0] - scale[1]).abs() > 1e-9 {
          self.warn(format!(
            "linear_extrude(scale = [{}, {}]): LuaCAD scales both axes \
             together, so the x factor is used for both",
            scale[0], scale[1]
          ));
        }
        ScadNode::LinearExtrude {
          height: *height as f32,
          center: *center,
          twist: *twist as f32,
          slices: *slices,
          scale: scale[0] as f32,
          child: self.child(child),
        }
      }
      Node::RotateExtrude {
        angle,
        frags,
        child,
      } => {
        let child = self.child(child);
        // OpenSCAD refines the revolution by how far the profile reaches from
        // the axis, which needs the profile itself — so build it and measure.
        let radius = profile_radius(&child);
        ScadNode::RotateExtrude {
          angle: *angle as f32,
          segments: fragments(radius, *frags),
          child,
        }
      }
      Node::Offset {
        r,
        delta,
        chamfer,
        child,
        ..
      } => {
        // Upstream normalizes to (r, 0) or (0, delta); `r` wins when both are
        // given, and it is what a bare offset() defaults to.
        let (r, delta) = if *delta != 0.0 && *r == 0.0 {
          (None, Some(*delta as f32))
        } else {
          (Some(*r as f32), None)
        };
        ScadNode::Offset {
          delta,
          r,
          chamfer: *chamfer,
          child: self.child(child),
        }
      }

      Node::Translate { v, child } => ScadNode::Translate {
        x: v[0] as f32,
        y: v[1] as f32,
        z: v[2] as f32,
        child: self.child(child),
      },
      Node::Rotate { deg, child } => ScadNode::Rotate {
        x: deg[0] as f32,
        y: deg[1] as f32,
        z: deg[2] as f32,
        child: self.child(child),
      },
      Node::Scale { v, child } => ScadNode::Scale {
        x: v[0] as f32,
        y: v[1] as f32,
        z: v[2] as f32,
        child: self.child(child),
      },
      Node::Mirror { v, child } => ScadNode::Mirror {
        x: v[0] as f32,
        y: v[1] as f32,
        z: v[2] as f32,
        child: self.child(child),
      },
      Node::MultMatrix { m, child } => {
        // Both sides are row-major 4×4 with the translation in column 3.
        let mut matrix = [0.0f32; 16];
        for (row, values) in m.iter().enumerate() {
          for (col, v) in values.iter().enumerate() {
            matrix[row * 4 + col] = *v as f32;
          }
        }
        ScadNode::Multmatrix {
          matrix,
          child: self.child(child),
        }
      }
      Node::Resize { new, auto, child } => {
        if auto.iter().any(|a| *a) {
          self.warn(
            "resize(auto = …): LuaCAD has no auto flag, so an axis left at 0 \
             keeps its size instead of scaling proportionally"
              .to_string(),
          );
        }
        ScadNode::Resize {
          x: new[0] as f32,
          y: new[1] as f32,
          z: new[2] as f32,
          child: self.child(child),
        }
      }

      Node::Union(children) => ScadNode::Union(self.children(children)),
      Node::Difference(children) => {
        ScadNode::Difference(self.children(children))
      }
      Node::Intersection(children) => {
        ScadNode::Intersection(self.children(children))
      }
      // LuaCAD hulls a single subtree; the hull of a union of the children is
      // the hull of the children.
      Node::Hull(children) => {
        ScadNode::Hull(Box::new(ScadNode::Union(self.children(children))))
      }
      Node::Minkowski(children) => {
        ScadNode::Minkowski(self.children(children))
      }

      Node::Projection { cut, child } => ScadNode::Projection {
        cut: *cut,
        child: self.child(child),
      },
      Node::Color { rgba, child } => ScadNode::Color {
        r: rgba[0],
        g: rgba[1],
        b: rgba[2],
        a: rgba[3],
        child: self.child(child),
      },
      // `#` stays in the mesh, `%` does not — which is how LuaCAD already
      // reads these two modifiers.
      Node::Highlight(child) => ScadNode::Modifier {
        kind: ModifierKind::Debug,
        child: self.child(child),
      },
      Node::Background(child) => ScadNode::Modifier {
        kind: ModifierKind::Transparent,
        child: self.child(child),
      },
      // Editor↔preview source spans; no geometry of its own.
      Node::Provenance { child, .. } => self.node(child),

      Node::Import { data, format } => self.import(data, format),
    }
  }

  /// Turn an `import()`ed mesh into a polyhedron.
  ///
  /// The evaluator resolves the file and hands over its bytes, while
  /// [`crate::mesh_import`] reads from a path, so the bytes go back to disk
  /// briefly. Emitting a polyhedron rather than [`ScadNode::Import`] also means
  /// the mesh survives into formats that cannot reference an external file.
  fn import(&mut self, data: &[u8], format: &str) -> ScadNode {
    let empty = ScadNode::Union(Vec::new());
    if !crate::mesh_import::IMPORT_FORMATS.contains(&format) {
      self.warn(format!(
        "import(): cannot read .{format} files.\nSupported: {}",
        crate::mesh_import::IMPORT_FORMATS.join(", ")
      ));
      return empty;
    }
    let stamp = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos())
      .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
      "luacad_scad_import-{}-{stamp}.{format}",
      std::process::id()
    ));
    if let Err(e) = std::fs::write(&path, data) {
      self.warn(format!("import(): cannot stage the mesh for reading: {e}"));
      return empty;
    }
    let mesh = crate::mesh_import::import_mesh(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    match mesh {
      Ok(mesh) => ScadNode::Polyhedron {
        points: mesh.vertices,
        faces: mesh
          .triangles
          .iter()
          .map(|t| vec![t[0] as usize, t[1] as usize, t[2] as usize])
          .collect(),
      },
      Err(e) => {
        self.warn(e);
        empty
      }
    }
  }
}

/// How far a `rotate_extrude` profile reaches from the axis of revolution.
///
/// Builds the profile and measures it, which is exact but costs one extra
/// materialization of a (small) 2D shape. An empty profile revolves to nothing
/// anyway, so its fragment count does not matter.
fn profile_radius(child: &ScadNode) -> f64 {
  crate::export::materialize_scad_cross_section(child)
    .outlines()
    .iter()
    .flatten()
    .map(|p| p[0].abs())
    .fold(0.0f64, f64::max)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn load(src: &str) -> ScadProgram {
    load_scad(src, Path::new(".")).expect("evaluates")
  }

  fn node(src: &str) -> ScadNode {
    load(src).geometries[0].scad.clone().expect("has a tree")
  }

  /// The one geometry's mesh, as (triangles, bounding box).
  fn mesh(src: &str) -> (usize, ([f32; 3], [f32; 3])) {
    let m = crate::export::materialize_scad_manifold(&node(src));
    (m.num_tri(), m.bounding_box())
  }

  #[test]
  fn primitives_become_scad_nodes() {
    assert!(matches!(
      node("cube(10);"),
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: false
      }
    ));
    assert!(matches!(
      node("sphere(r = 5, $fn = 12);"),
      ScadNode::Sphere {
        r: 5.0,
        segments: 12
      }
    ));
    assert!(matches!(
      node("cylinder(h = 4, r1 = 2, r2 = 0, center = true, $fn = 8);"),
      ScadNode::Cylinder {
        r1: 2.0,
        r2: 0.0,
        h: 4.0,
        segments: 8,
        center: true
      }
    ));
  }

  #[test]
  fn fragment_count_follows_fn_fa_fs() {
    // $fn wins outright, and is floored at 3.
    assert_eq!(fragments(10.0, spec(7.0, 12.0, 2.0)), 7);
    assert_eq!(fragments(10.0, spec(1.0, 12.0, 2.0)), 3);
    // Without $fn it is min(360/$fa, circumference/$fs), floored at 5.
    assert_eq!(fragments(10.0, spec(0.0, 12.0, 2.0)), 30);
    assert_eq!(fragments(1.0, spec(0.0, 12.0, 2.0)), 5);
    assert_eq!(fragments(100.0, spec(0.0, 12.0, 2.0)), 30);
    assert_eq!(fragments(100.0, spec(0.0, 1.0, 2.0)), 315);
  }

  fn spec(fn_: f64, fa: f64, fs: f64) -> FragmentSpec {
    FragmentSpec { fn_, fa, fs }
  }

  #[test]
  fn dollar_fn_is_dynamically_scoped_through_modules() {
    // The call site's $fn reaches into the module body — the dynamic scoping
    // that distinguishes `$` variables from ordinary ones.
    assert!(matches!(
      node("module m() sphere(4); m($fn = 9);"),
      ScadNode::Sphere { segments: 9, .. }
    ));
    // …and it does not leak back out to a later call that sets none.
    let ScadNode::Union(children) =
      node("module m() sphere(4); m($fn = 9); m();")
    else {
      panic!("expected a group");
    };
    assert!(matches!(children[0], ScadNode::Sphere { segments: 9, .. }));
    assert!(matches!(children[1], ScadNode::Sphere { segments: 13, .. }));
  }

  #[test]
  fn booleans_and_transforms_nest() {
    let tree = node("difference() { cube(10); translate([1,1,1]) sphere(3); }");
    let ScadNode::Difference(children) = tree else {
      panic!("expected a difference");
    };
    assert_eq!(children.len(), 2);
    assert!(matches!(children[1], ScadNode::Translate { .. }));
  }

  #[test]
  fn a_difference_removes_material() {
    // The end-to-end path: parse, evaluate, convert, mesh with Manifold.
    let (tris, (min, max)) = mesh("difference() { cube(10); cube(4); }");
    assert!(tris > 0);
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert_eq!(max, [10.0, 10.0, 10.0]);
    let solid = crate::export::materialize_scad_manifold(&node("cube(10);"));
    let cut =
      crate::export::materialize_scad_manifold(&node(
        "difference() { cube(10); cube(4); }",
      ));
    assert!(cut.volume() < solid.volume());
  }

  #[test]
  fn for_loops_and_expressions_evaluate() {
    let ScadNode::Union(children) =
      node("for (i = [0:3]) translate([i * 10, 0, 0]) cube(1);")
    else {
      panic!("expected a group");
    };
    assert_eq!(children.len(), 4);
    let ScadNode::Translate { x, .. } = &children[3] else {
      panic!("expected a translate");
    };
    assert_eq!(*x, 30.0);
  }

  #[test]
  fn user_functions_and_list_comprehensions_evaluate() {
    let tree = node(
      "function sq(x) = x * x;\n\
       polygon([for (i = [0:2]) [i, sq(i)]]);",
    );
    let ScadNode::Polygon { points, paths } = tree else {
      panic!("expected a polygon");
    };
    assert_eq!(points, vec![[0.0, 0.0], [1.0, 1.0], [2.0, 4.0]]);
    assert!(paths.is_none());
  }

  #[test]
  fn polygon_paths_carry_holes_through() {
    // A square with a square hole: the hole must survive into the mesh, which
    // is what the `paths` field on ScadNode::Polygon was added for.
    let src = "linear_extrude(1) polygon(\
       points = [[0,0],[10,0],[10,10],[0,10],[3,3],[7,3],[7,7],[3,7]],\
       paths = [[0,1,2,3],[4,5,6,7]]);";
    let m = crate::export::materialize_scad_manifold(&node(src));
    // 100 minus the 16 the hole takes out.
    assert!((m.volume() - 84.0).abs() < 0.01, "volume {}", m.volume());
  }

  #[test]
  fn modifier_characters_map_to_luacad_modifiers() {
    // `%` leaves the mesh, `#` stays in it — OpenSCAD's own reading.
    let m = crate::export::materialize_scad_manifold(&node(
      "cube(10); %cube(20);",
    ));
    assert!((m.volume() - 1000.0).abs() < 0.01, "volume {}", m.volume());
    let m = crate::export::materialize_scad_manifold(&node(
      "cube(10); #translate([20,0,0]) cube(10);",
    ));
    assert!((m.volume() - 2000.0).abs() < 0.01, "volume {}", m.volume());
  }

  #[test]
  fn color_survives_as_a_scad_node() {
    let ScadNode::Color { r, g, b, a, .. } = node("color(\"red\") cube(1);")
    else {
      panic!("expected a color");
    };
    assert_eq!((r, g, b, a), (1.0, 0.0, 0.0, 1.0));
  }

  #[test]
  fn rotate_extrude_refines_by_the_profile_radius() {
    // The profile sits 10 to 12 from the axis, so $fa/$fs resolve against 12,
    // not against the profile's own 2 mm width.
    let ScadNode::RotateExtrude { segments, .. } =
      node("rotate_extrude() translate([10, 0]) square(2);")
    else {
      panic!("expected a rotate_extrude");
    };
    assert_eq!(segments, fragments(12.0, spec(0.0, 12.0, 2.0)));
  }

  #[test]
  fn linear_extrude_with_uneven_scale_warns() {
    let out = load("linear_extrude(10, scale = [2, 3]) square(1);");
    assert!(
      out.warnings.iter().any(|w| w.contains("scales both axes")),
      "{:?}",
      out.warnings
    );
    // An even scale is carried exactly, so it must not warn.
    let out = load("linear_extrude(10, scale = 2) square(1);");
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
  }

  #[test]
  fn echo_output_is_collected() {
    assert_eq!(load("echo(1 + 1);").echoes, vec!["ECHO: 2"]);
  }

  #[test]
  fn a_syntax_error_reports_a_line_and_column() {
    let err = load_scad("cube(10)\ncube(", Path::new(".")).unwrap_err();
    assert!(err.starts_with("line 2, column "), "{err}");
  }

  #[test]
  fn an_assertion_failure_is_an_error_not_a_panic() {
    let err = load_scad("assert(1 == 2);", Path::new(".")).unwrap_err();
    assert!(err.to_lowercase().contains("assert"), "{err}");
  }

  #[test]
  fn is_scad_file_matches_the_extension_case_insensitively() {
    assert!(is_scad_file("model.scad"));
    assert!(is_scad_file("model.SCAD"));
    assert!(!is_scad_file("model.lua"));
    assert!(!is_scad_file("model"));
  }
}
