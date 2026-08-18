#[cfg(feature = "csgrs")]
use luacad::export::{
  export_3mf_bytes, export_obj, export_ply, export_stl_ascii,
};
use luacad::lua_engine::execute_lua;
use luacad::scad_export::generate_scad;

fn load_example(name: &str) -> String {
  let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let workspace_root = manifest.parent().unwrap().parent().unwrap();
  // Each example lives in its own directory: examples/<stem>/<stem>.lua.
  let stem = name.trim_end_matches(".lua");
  let path = workspace_root.join("examples").join(stem).join(name);
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
#[cfg(feature = "csgrs")]
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
#[cfg(feature = "csgrs")]
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
#[cfg(feature = "csgrs")]
fn stl_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  export_stl_ascii(geometries, "LuaCAD_Studio").unwrap()
}

/// Export to 3MF (zip of XML), extract the model XML for snapshotting.
#[cfg(feature = "csgrs")]
fn threemf_output(geometries: &[luacad::geometry::CsgGeometry]) -> String {
  let bytes = export_3mf_bytes(geometries).unwrap();
  let cursor = std::io::Cursor::new(bytes);
  let mut archive = zip::ZipArchive::new(cursor).unwrap();
  let mut model = archive.by_name("3D/model.model").unwrap();
  let mut xml = String::new();
  std::io::Read::read_to_string(&mut model, &mut xml).unwrap();
  xml
}

/// Path to the stored snapshot file for a mesh-format test.
#[cfg(feature = "csgrs")]
fn snapshot_path(name: &str) -> std::path::PathBuf {
  std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests/snapshots")
    .join(format!("example_snapshots__{name}.snap"))
}

/// Split a line into numeric and literal tokens so numbers can be compared
/// with a tolerance while everything else must match exactly.
#[cfg(feature = "csgrs")]
fn tokens(line: &str) -> Vec<(bool, &str)> {
  let bytes = line.as_bytes();
  let mut out = Vec::new();
  let mut seg_start = 0;
  let mut i = 0;
  while i < bytes.len() {
    let b = bytes[i];
    let starts_number = b.is_ascii_digit()
      || ((b == b'-' || b == b'+' || b == b'.')
        && bytes.get(i + 1).is_some_and(|c| c.is_ascii_digit()));
    if !starts_number {
      i += 1;
      continue;
    }
    if seg_start < i {
      out.push((false, &line[seg_start..i]));
    }
    let start = i;
    if b == b'-' || b == b'+' {
      i += 1;
    }
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
      i += 1;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
      let mut j = i + 1;
      if j < bytes.len() && (bytes[j] == b'-' || bytes[j] == b'+') {
        j += 1;
      }
      if j < bytes.len() && bytes[j].is_ascii_digit() {
        i = j;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
          i += 1;
        }
      }
    }
    out.push((true, &line[start..i]));
    seg_start = i;
  }
  if seg_start < bytes.len() {
    out.push((false, &line[seg_start..]));
  }
  out
}

/// Whether two numeric tokens are equal within cross-platform float noise.
/// Integer tokens (counts, indices) must match exactly.
#[cfg(feature = "csgrs")]
fn nums_match(a: &str, b: &str) -> bool {
  if a == b {
    return true;
  }
  let is_int = |s: &str| !s.contains(['.', 'e', 'E']);
  if is_int(a) && is_int(b) {
    return false;
  }
  let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) else {
    return false;
  };
  (x - y).abs() <= 1e-5 + 1e-4 * x.abs().max(y.abs())
}

/// Assert `actual` matches the stored snapshot, allowing tiny numeric drift.
/// Mesh exports print f32 coordinates whose last digits differ between
/// platforms (libm/ulp noise), so exact insta comparison cannot be used.
/// Set `UPDATE_SNAPSHOTS=1` to rewrite the stored snapshots.
#[cfg(feature = "csgrs")]
fn assert_mesh_snapshot(name: &str, actual: &str) {
  let path = snapshot_path(name);
  if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
    let header = std::fs::read_to_string(&path)
      .ok()
      .and_then(|s| {
        s.split("---\n").nth(1).map(|m| format!("---\n{m}---\n"))
      })
      .unwrap_or_else(|| {
        format!(
          "---\nsource: crates/luacad/tests/example_snapshots.rs\nexpression: {name}\n---\n"
        )
      });
    std::fs::write(&path, format!("{header}{}\n", actual.trim_end())).unwrap();
    return;
  }
  let stored = std::fs::read_to_string(&path).unwrap_or_else(|e| {
    panic!(
      "{name}: cannot read {} ({e}); run with UPDATE_SNAPSHOTS=1 to create it",
      path.display()
    )
  });
  let reference = stored
    .splitn(3, "---\n")
    .nth(2)
    .unwrap_or_else(|| panic!("{name}: malformed snapshot header"));
  let ref_lines: Vec<&str> = reference.trim_end().lines().collect();
  let act_lines: Vec<&str> = actual.trim_end().lines().collect();
  assert_eq!(
    ref_lines.len(),
    act_lines.len(),
    "{name}: line count differs (snapshot {} vs actual {}); \
     run with UPDATE_SNAPSHOTS=1 to update",
    ref_lines.len(),
    act_lines.len()
  );
  for (i, (r, a)) in ref_lines.iter().zip(&act_lines).enumerate() {
    if r == a {
      continue;
    }
    let rt = tokens(r);
    let at = tokens(a);
    let matches = rt.len() == at.len()
      && rt.iter().zip(&at).all(|(&(rn, rs), &(an, av))| {
        if rn && an {
          nums_match(rs, av)
        } else {
          rn == an && rs == av
        }
      });
    assert!(
      matches,
      "{name}: line {} differs beyond tolerance:\n-{r}\n+{a}\n\
       run with UPDATE_SNAPSHOTS=1 to update",
      i + 1
    );
  }
}

// ── simple.lua ───────────────────────────────────────────────────────

#[test]
fn simple_scad() {
  let geoms = run_lua("simple.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_obj() {
  let geoms = run_lua("simple.lua");
  assert_mesh_snapshot("simple_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_ply() {
  let geoms = run_lua("simple.lua");
  assert_mesh_snapshot("simple_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_stl() {
  let geoms = run_lua("simple.lua");
  assert_mesh_snapshot("simple_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_3mf() {
  let geoms = run_lua("simple.lua");
  assert_mesh_snapshot("simple_3mf", &threemf_output(&geoms));
}

// ── box.lua ──────────────────────────────────────────────────────────

#[test]
fn box_scad() {
  let geoms = run_lua("box.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn box_obj() {
  let geoms = run_lua("box.lua");
  assert_mesh_snapshot("box_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn box_ply() {
  let geoms = run_lua("box.lua");
  assert_mesh_snapshot("box_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn box_stl() {
  let geoms = run_lua("box.lua");
  assert_mesh_snapshot("box_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn box_3mf() {
  let geoms = run_lua("box.lua");
  assert_mesh_snapshot("box_3mf", &threemf_output(&geoms));
}

// ── gear.lua ─────────────────────────────────────────────────────────

#[test]
fn gear_scad() {
  let geoms = run_lua("gear.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn gear_obj() {
  let geoms = run_lua("gear.lua");
  assert_mesh_snapshot("gear_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn gear_ply() {
  let geoms = run_lua("gear.lua");
  assert_mesh_snapshot("gear_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn gear_stl() {
  let geoms = run_lua("gear.lua");
  assert_mesh_snapshot("gear_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn gear_3mf() {
  let geoms = run_lua("gear.lua");
  assert_mesh_snapshot("gear_3mf", &threemf_output(&geoms));
}

// ── simple_car.lua ───────────────────────────────────────────────────

#[test]
fn simple_car_scad() {
  let geoms = run_lua("simple_car.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_car_obj() {
  let geoms = run_lua("simple_car.lua");
  assert_mesh_snapshot("simple_car_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_car_ply() {
  let geoms = run_lua("simple_car.lua");
  assert_mesh_snapshot("simple_car_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_car_stl() {
  let geoms = run_lua("simple_car.lua");
  assert_mesh_snapshot("simple_car_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn simple_car_3mf() {
  let geoms = run_lua("simple_car.lua");
  assert_mesh_snapshot("simple_car_3mf", &threemf_output(&geoms));
}

// ── difference.lua ───────────────────────────────────────────────────

#[test]
fn difference_scad() {
  let geoms = run_lua("difference.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn difference_obj() {
  let geoms = run_lua("difference.lua");
  assert_mesh_snapshot("difference_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn difference_ply() {
  let geoms = run_lua("difference.lua");
  assert_mesh_snapshot("difference_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn difference_stl() {
  let geoms = run_lua("difference.lua");
  assert_mesh_snapshot("difference_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn difference_3mf() {
  let geoms = run_lua("difference.lua");
  assert_mesh_snapshot("difference_3mf", &threemf_output(&geoms));
}

// ── rounded_rectangle.lua ────────────────────────────────────────────

#[test]
fn rounded_rectangle_scad() {
  let geoms = run_lua("rounded_rectangle.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn rounded_rectangle_obj() {
  let geoms = run_lua("rounded_rectangle.lua");
  assert_mesh_snapshot("rounded_rectangle_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn rounded_rectangle_ply() {
  let geoms = run_lua("rounded_rectangle.lua");
  assert_mesh_snapshot("rounded_rectangle_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn rounded_rectangle_stl() {
  let geoms = run_lua("rounded_rectangle.lua");
  assert_mesh_snapshot("rounded_rectangle_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn rounded_rectangle_3mf() {
  let geoms = run_lua("rounded_rectangle.lua");
  assert_mesh_snapshot("rounded_rectangle_3mf", &threemf_output(&geoms));
}

// ── customizer.lua ───────────────────────────────────────────────────

#[test]
fn customizer_scad() {
  let geoms = run_lua("customizer.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn customizer_obj() {
  let geoms = run_lua("customizer.lua");
  assert_mesh_snapshot("customizer_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn customizer_ply() {
  let geoms = run_lua("customizer.lua");
  assert_mesh_snapshot("customizer_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn customizer_stl() {
  let geoms = run_lua("customizer.lua");
  assert_mesh_snapshot("customizer_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn customizer_3mf() {
  let geoms = run_lua("customizer.lua");
  assert_mesh_snapshot("customizer_3mf", &threemf_output(&geoms));
}

// ── literal_openscad.lua ─────────────────────────────────────────────

#[test]
fn literal_openscad_scad() {
  let geoms = run_lua("literal_openscad.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn literal_openscad_obj() {
  let geoms = run_lua("literal_openscad.lua");
  assert_mesh_snapshot("literal_openscad_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn literal_openscad_ply() {
  let geoms = run_lua("literal_openscad.lua");
  assert_mesh_snapshot("literal_openscad_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn literal_openscad_stl() {
  let geoms = run_lua("literal_openscad.lua");
  assert_mesh_snapshot("literal_openscad_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn literal_openscad_3mf() {
  let geoms = run_lua("literal_openscad.lua");
  assert_mesh_snapshot("literal_openscad_3mf", &threemf_output(&geoms));
}

// ── tostring_demo.lua ────────────────────────────────────────────────

#[test]
fn tostring_demo_scad() {
  let geoms = run_lua("tostring_demo.lua");
  insta::assert_snapshot!(scad_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn tostring_demo_obj() {
  let geoms = run_lua("tostring_demo.lua");
  assert_mesh_snapshot("tostring_demo_obj", &obj_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn tostring_demo_ply() {
  let geoms = run_lua("tostring_demo.lua");
  assert_mesh_snapshot("tostring_demo_ply", &ply_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn tostring_demo_stl() {
  let geoms = run_lua("tostring_demo.lua");
  assert_mesh_snapshot("tostring_demo_stl", &stl_output(&geoms));
}

#[cfg(feature = "csgrs")]
#[test]
fn tostring_demo_3mf() {
  let geoms = run_lua("tostring_demo.lua");
  assert_mesh_snapshot("tostring_demo_3mf", &threemf_output(&geoms));
}
