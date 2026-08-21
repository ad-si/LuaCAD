//! End-to-end tests for opening `.scad` files: the same path the CLI and
//! Studio take, from source on disk through to an exported mesh.
//!
//! The unit tests in `scad_import` cover the node-by-node lowering. These cover
//! what only shows up once real files and the export stack are involved —
//! multi-file `include`/`use`, `import()`, and the mesh that comes out.

use std::path::{Path, PathBuf};

use luacad::scad_import::{is_scad_file, load_scad_file};

/// A scratch directory that cleans itself up.
struct Scratch(PathBuf);

impl Scratch {
  fn new(name: &str) -> Self {
    let stamp = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos())
      .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
      "luacad_scad_input-{name}-{}-{stamp}",
      std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    Scratch(dir)
  }

  fn write(&self, name: &str, contents: &str) -> PathBuf {
    let path = self.0.join(name);
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&path, contents).expect("write fixture");
    path
  }

  fn path(&self, name: &str) -> PathBuf {
    self.0.join(name)
  }
}

impl Drop for Scratch {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

/// Mesh a `.scad` file the way `luacad convert` does.
fn mesh_of(path: &Path) -> (f64, ([f32; 3], [f32; 3])) {
  let program = load_scad_file(path).expect("loads");
  assert_eq!(program.geometries.len(), 1);
  let node = program.geometries[0].scad.as_ref().expect("has a tree");
  let m = luacad::export::materialize_scad_manifold(node);
  (m.volume(), m.bounding_box())
}

#[test]
fn a_scad_file_meshes_through_manifold() {
  let dir = Scratch::new("mesh");
  let path = dir.write(
    "box.scad",
    "difference() {\n\
       cube([20, 10, 4]);\n\
       translate([5, 5, -1]) cylinder(h = 6, r = 2, $fn = 64);\n\
     }\n",
  );
  let (volume, (min, max)) = mesh_of(&path);
  assert_eq!(min, [0.0, 0.0, 0.0]);
  assert_eq!(max, [20.0, 10.0, 4.0]);
  // 800 minus the drilled cylinder, at 64 facets ≈ π·2²·4.
  let expected = 800.0 - std::f64::consts::PI * 4.0 * 4.0;
  assert!(
    (volume - expected).abs() < 0.5,
    "volume {volume}, expected about {expected}"
  );
}

#[test]
fn include_and_use_resolve_relative_to_the_including_file() {
  let dir = Scratch::new("include");
  // `include` brings in geometry and definitions; `use` brings in only the
  // definitions. A nested include also has to resolve against *its own*
  // directory, not the top file's.
  dir.write(
    "lib/dims.scad",
    "plate_x = 30;\nplate_y = 20;\nplate_z = 3;\n",
  );
  dir.write(
    "lib/parts.scad",
    "include <dims.scad>\n\
     module plate() cube([plate_x, plate_y, plate_z]);\n\
     module post() cylinder(h = 10, r = 2, $fn = 6);\n",
  );
  let path = dir.write(
    "model.scad",
    "use <lib/parts.scad>\n\
     plate();\n\
     translate([5, 5, 3]) post();\n",
  );

  let program = load_scad_file(&path).expect("loads");
  assert!(program.warnings.is_empty(), "{:?}", program.warnings);
  let (_, (min, max)) = mesh_of(&path);
  assert_eq!(min, [0.0, 0.0, 0.0]);
  // The plate is 30 × 20 × 3 and the post reaches 10 above it.
  assert_eq!(max, [30.0, 20.0, 13.0]);
}

#[test]
fn an_unresolvable_include_warns_rather_than_failing() {
  let dir = Scratch::new("missing-include");
  let path = dir.write("model.scad", "include <nope/missing.scad>\ncube(1);\n");
  let program = load_scad_file(&path).expect("loads despite the missing file");
  assert!(
    program.warnings.iter().any(|w| w.contains("missing.scad")),
    "{:?}",
    program.warnings
  );
}

#[test]
fn import_reads_a_mesh_luacad_wrote() {
  // Round-trip: export an STL from a .scad, then import it from another one.
  let dir = Scratch::new("import");
  let source = dir.write("source.scad", "cube([10, 6, 2]);\n");
  let stl = dir.path("part.stl");
  let program = load_scad_file(&source).expect("loads");
  luacad::export::export_manifold(&program.geometries, "stl", &stl)
    .expect("exports");

  let path =
    dir.write("model.scad", "translate([0, 0, 5]) import(\"part.stl\");\n");
  let program = load_scad_file(&path).expect("loads");
  assert!(program.warnings.is_empty(), "{:?}", program.warnings);
  let (volume, (min, max)) = mesh_of(&path);
  assert!((volume - 120.0).abs() < 0.01, "volume {volume}");
  assert_eq!(min, [0.0, 0.0, 5.0]);
  assert_eq!(max, [10.0, 6.0, 7.0]);
}

#[test]
fn an_import_in_an_unreadable_format_warns_and_yields_nothing() {
  let dir = Scratch::new("import-bad");
  dir.write("drawing.dxf", "not really a dxf\n");
  let path = dir.write("model.scad", "import(\"drawing.dxf\");\n");
  let program = load_scad_file(&path).expect("loads");
  assert!(
    program.warnings.iter().any(|w| w.contains("dxf")),
    "{:?}",
    program.warnings
  );
}

#[test]
fn a_scad_file_exports_to_every_mesh_format() {
  let dir = Scratch::new("formats");
  let path = dir.write(
    "model.scad",
    "union() { cube(10); translate([10,0,0]) sphere(4, $fn = 24); }\n",
  );
  let program = load_scad_file(&path).expect("loads");
  for format in ["stl", "obj", "ply", "off", "amf", "3mf"] {
    let out = dir.path(&format!("model.{format}"));
    luacad::export::export_manifold(&program.geometries, format, &out)
      .unwrap_or_else(|e| panic!("{format}: {e}"));
    let size = std::fs::metadata(&out)
      .unwrap_or_else(|e| panic!("{format}: {e}"))
      .len();
    assert!(size > 0, "{format}: wrote an empty file");
  }
}

#[test]
fn a_scad_file_round_trips_back_to_scad() {
  let dir = Scratch::new("roundtrip");
  let path = dir.write(
    "model.scad",
    "module ring(r) difference() {\n\
       cylinder(h = 2, r = r, $fn = 32);\n\
       translate([0, 0, -1]) cylinder(h = 4, r = r - 1, $fn = 32);\n\
     }\n\
     ring(6);\n",
  );
  let program = load_scad_file(&path).expect("loads");
  let out = dir.path("out.scad");
  let nodes: Vec<_> = program
    .geometries
    .iter()
    .filter_map(|g| g.scad.clone())
    .collect();
  luacad::scad_export::export_scad(&nodes, &out).expect("exports");
  let written = std::fs::read_to_string(&out).expect("reads back");

  // Modules are inlined — the tree has no notion of them — but the geometry
  // and the resolved facet count survive.
  assert!(written.contains("difference()"), "{written}");
  assert!(written.contains("$fn = 32"), "{written}");

  // And the re-read file meshes to the same solid.
  let reread = dir.write("reread.scad", &written);
  let (a, _) = mesh_of(&path);
  let (b, _) = mesh_of(&reread);
  assert!((a - b).abs() < 0.01, "volume drifted: {a} then {b}");
}

#[test]
fn errors_name_the_file_and_the_line() {
  let dir = Scratch::new("errors");
  let path = dir.write("broken.scad", "cube(10);\ntranslate([1,2,3)\n");
  let err = load_scad_file(&path).unwrap_err();
  assert!(err.contains("broken.scad"), "{err}");
  assert!(err.contains("line 2"), "{err}");
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
  let dir = Scratch::new("absent");
  let err = load_scad_file(&dir.path("nope.scad")).unwrap_err();
  assert!(err.contains("nope.scad"), "{err}");
}

#[test]
fn only_a_scad_extension_takes_the_openscad_path() {
  assert!(is_scad_file("a.scad"));
  assert!(is_scad_file("A.SCAD"));
  assert!(is_scad_file(Path::new("dir/b.Scad")));
  assert!(!is_scad_file("a.lua"));
  assert!(!is_scad_file("a.scad.lua"));
}
