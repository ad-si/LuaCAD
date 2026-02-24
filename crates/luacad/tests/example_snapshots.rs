use luacad::export::{
  export_3mf_bytes, export_obj, export_ply, export_stl_ascii,
};
use luacad::lua_engine::execute_lua;
use luacad::scad_export::generate_scad;

fn load_example(name: &str) -> String {
  let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let workspace_root = manifest.parent().unwrap().parent().unwrap();
  let path = workspace_root.join("examples").join(name);
  std::fs::read_to_string(&path)
    .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()))
}

fn run_lua(name: &str) -> Vec<luacad::geometry::CsgGeometry> {
  let code = load_example(name);
  execute_lua(&code).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Generate OpenSCAD output from the ScadNode AST attached to each geometry.
fn scad_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  let nodes: Vec<_> =
    geometries.iter().filter_map(|g| g.scad.clone()).collect();
  generate_scad(&nodes)
}

/// Export to OBJ (text format) via a temp file and read back.
fn obj_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  let dir = std::env::temp_dir().join("luacad_snapshot_tests");
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("test.obj");
  export_obj(geometries, &path).unwrap();
  let content = std::fs::read_to_string(&path).unwrap();
  let _ = std::fs::remove_file(&path);
  content
}

/// Export to PLY (text format) via a temp file and read back.
fn ply_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  let dir = std::env::temp_dir().join("luacad_snapshot_tests");
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("test.ply");
  export_ply(geometries, &path).unwrap();
  let content = std::fs::read_to_string(&path).unwrap();
  let _ = std::fs::remove_file(&path);
  content
}

/// Export to ASCII STL via csgrs.
fn stl_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  export_stl_ascii(geometries, "LuaCAD_Studio").unwrap()
}

/// Export to 3MF (zip of XML), extract the model XML for snapshotting.
fn threemf_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  let bytes = export_3mf_bytes(geometries).unwrap();
  let cursor = std::io::Cursor::new(bytes);
  let mut archive = zip::ZipArchive::new(cursor).unwrap();
  let mut model = archive.by_name("3D/model.model").unwrap();
  let mut xml = String::new();
  std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
  xml
}

// ── simple.lua ───────────────────────────────────────────────────────

#[test]
fn simple_scad() {
  let geoms = run_lua("simple.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[test]
fn simple_obj() {
  let geoms = run_lua("simple.lua");
  insta::assert_snapshot!(obj_output(&geoms));
}

#[test]
fn simple_ply() {
  let geoms = run_lua("simple.lua");
  insta::assert_snapshot!(ply_output(&geoms));
}

#[test]
fn simple_stl() {
  let geoms = run_lua("simple.lua");
  insta::assert_snapshot!(stl_output(&geoms));
}

#[test]
fn simple_3mf() {
  let geoms = run_lua("simple.lua");
  insta::assert_snapshot!(threemf_output(&geoms));
}

// ── box.lua ──────────────────────────────────────────────────────────

#[test]
fn box_scad() {
  let geoms = run_lua("box.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[test]
fn box_obj() {
  let geoms = run_lua("box.lua");
  insta::assert_snapshot!(obj_output(&geoms));
}

#[test]
fn box_ply() {
  let geoms = run_lua("box.lua");
  insta::assert_snapshot!(ply_output(&geoms));
}

#[test]
fn box_stl() {
  let geoms = run_lua("box.lua");
  insta::assert_snapshot!(stl_output(&geoms));
}

#[test]
fn box_3mf() {
  let geoms = run_lua("box.lua");
  insta::assert_snapshot!(threemf_output(&geoms));
}

// ── gear.lua ─────────────────────────────────────────────────────────

#[test]
fn gear_scad() {
  let geoms = run_lua("gear.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[test]
fn gear_obj() {
  let geoms = run_lua("gear.lua");
  insta::assert_snapshot!(obj_output(&geoms));
}

#[test]
fn gear_ply() {
  let geoms = run_lua("gear.lua");
  insta::assert_snapshot!(ply_output(&geoms));
}

#[test]
fn gear_stl() {
  let geoms = run_lua("gear.lua");
  insta::assert_snapshot!(stl_output(&geoms));
}

#[test]
fn gear_3mf() {
  let geoms = run_lua("gear.lua");
  insta::assert_snapshot!(threemf_output(&geoms));
}

// ── simple_car.lua ───────────────────────────────────────────────────

#[test]
fn simple_car_scad() {
  let geoms = run_lua("simple_car.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[test]
fn simple_car_obj() {
  let geoms = run_lua("simple_car.lua");
  insta::assert_snapshot!(obj_output(&geoms));
}

#[test]
fn simple_car_ply() {
  let geoms = run_lua("simple_car.lua");
  insta::assert_snapshot!(ply_output(&geoms));
}

#[test]
fn simple_car_stl() {
  let geoms = run_lua("simple_car.lua");
  insta::assert_snapshot!(stl_output(&geoms));
}

#[test]
fn simple_car_3mf() {
  let geoms = run_lua("simple_car.lua");
  insta::assert_snapshot!(threemf_output(&geoms));
}

// ── difference.lua ───────────────────────────────────────────────────

#[test]
fn difference_scad() {
  let geoms = run_lua("difference.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[test]
fn difference_obj() {
  let geoms = run_lua("difference.lua");
  insta::assert_snapshot!(obj_output(&geoms));
}

#[test]
fn difference_ply() {
  let geoms = run_lua("difference.lua");
  insta::assert_snapshot!(ply_output(&geoms));
}

#[test]
fn difference_stl() {
  let geoms = run_lua("difference.lua");
  insta::assert_snapshot!(stl_output(&geoms));
}

#[test]
fn difference_3mf() {
  let geoms = run_lua("difference.lua");
  insta::assert_snapshot!(threemf_output(&geoms));
}

// ── rounded_rectangle.lua ────────────────────────────────────────────

#[test]
fn rounded_rectangle_scad() {
  let geoms = run_lua("rounded_rectangle.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[test]
fn rounded_rectangle_obj() {
  let geoms = run_lua("rounded_rectangle.lua");
  insta::assert_snapshot!(obj_output(&geoms));
}

#[test]
fn rounded_rectangle_ply() {
  let geoms = run_lua("rounded_rectangle.lua");
  insta::assert_snapshot!(ply_output(&geoms));
}

#[test]
fn rounded_rectangle_stl() {
  let geoms = run_lua("rounded_rectangle.lua");
  insta::assert_snapshot!(stl_output(&geoms));
}

#[test]
fn rounded_rectangle_3mf() {
  let geoms = run_lua("rounded_rectangle.lua");
  insta::assert_snapshot!(threemf_output(&geoms));
}
