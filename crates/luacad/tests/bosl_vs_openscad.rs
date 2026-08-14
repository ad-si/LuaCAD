//! Differential tests: every natively-built BOSL2 shape against the real
//! thing, rendered by OpenSCAD with the BOSL2 library installed.
//!
//! The two paths start from the same script. The native one builds the shape
//! out of LuaCAD's own primitives, the reference one exports the BOSL2 call
//! to `.scad` and hands it to OpenSCAD. Comparing the resulting meshes is
//! what actually pins the ports down — a chamfer measured from the wrong end
//! or an anchor resolved to the wrong corner survives every unit test that
//! only checks a shape against itself.
//!
//! The tests skip themselves when OpenSCAD or BOSL2 is not installed, so CI
//! without them still passes; run them locally to check fidelity.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Volumes are compared as a ratio, because the two paths facet curves at
/// slightly different places even when they agree on the shape.
///
/// Kept tight on purpose: a rounding applied to the wrong four edges of a
/// box, or a mask that lands outside the solid and removes nothing, moves
/// the volume by only a percent or two on a shape this size.
const VOLUME_TOLERANCE: f64 = 0.005;
/// Bounding boxes are compared in millimetres, and a facetted circle's extent
/// depends on where its vertices land.
const BBOX_TOLERANCE: f64 = 0.25;

fn openscad_available() -> bool {
  Command::new("openscad")
    .arg("--version")
    .output()
    .is_ok_and(|o| o.status.success() || !o.stderr.is_empty())
}

/// Whether OpenSCAD can resolve `include <BOSL2/std.scad>`.
fn bosl2_available() -> bool {
  let dir = temp_dir();
  let scad = dir.join("probe.scad");
  let stl = dir.join("probe.stl");
  std::fs::write(&scad, "include <BOSL2/std.scad>\ncuboid([1,1,1]);\n")
    .expect("the temp directory is writable");
  let ok = Command::new("openscad")
    .arg("-o")
    .arg(&stl)
    .arg(&scad)
    .output()
    .is_ok_and(|o| o.status.success())
    && stl.exists();
  let _ = std::fs::remove_dir_all(&dir);
  ok
}

fn temp_dir() -> PathBuf {
  static COUNTER: AtomicUsize = AtomicUsize::new(0);
  let n = COUNTER.fetch_add(1, Ordering::Relaxed);
  let dir = std::env::temp_dir()
    .join(format!("luacad-bosl-diff-{}-{n}", std::process::id()));
  std::fs::create_dir_all(&dir).expect("the temp directory can be created");
  dir
}

/// The `luacad` binary built alongside this test.
fn luacad_bin() -> PathBuf {
  let mut path = std::env::current_exe().expect("the test binary has a path");
  path.pop(); // the deps/ directory
  if path.ends_with("deps") {
    path.pop();
  }
  path.join("luacad")
}

struct Measured {
  volume: f64,
  min: [f64; 3],
  max: [f64; 3],
}

/// Read an STL and measure what the two paths have to agree on.
fn measure(path: &Path) -> Measured {
  let mesh = luacad::mesh_import::import_mesh(
    path.to_str().expect("the path is valid UTF-8"),
  )
  .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

  let mut min = [f64::INFINITY; 3];
  let mut max = [f64::NEG_INFINITY; 3];
  for v in &mesh.vertices {
    for i in 0..3 {
      min[i] = min[i].min(v[i] as f64);
      max[i] = max[i].max(v[i] as f64);
    }
  }

  // The signed volume of the tetrahedra spanned from the origin to each face.
  let mut volume = 0.0;
  for t in &mesh.triangles {
    let a = mesh.vertices[t[0] as usize];
    let b = mesh.vertices[t[1] as usize];
    let c = mesh.vertices[t[2] as usize];
    let (a, b, c) = (
      [a[0] as f64, a[1] as f64, a[2] as f64],
      [b[0] as f64, b[1] as f64, b[2] as f64],
      [c[0] as f64, c[1] as f64, c[2] as f64],
    );
    volume += a[0] * (b[1] * c[2] - b[2] * c[1])
      - a[1] * (b[0] * c[2] - b[2] * c[0])
      + a[2] * (b[0] * c[1] - b[1] * c[0]);
  }

  Measured {
    volume: (volume / 6.0).abs(),
    min,
    max,
  }
}

fn convert(
  dir: &Path,
  script: &str,
  name: &str,
  via_openscad: bool,
) -> PathBuf {
  let lua = dir.join(format!("{name}.lua"));
  let stl = dir.join(format!("{name}.stl"));
  std::fs::write(&lua, script).expect("the script can be written");

  let mut cmd = Command::new(luacad_bin());
  cmd.arg("convert").arg(&lua).arg(&stl);
  if via_openscad {
    cmd.arg("--via-openscad");
  }
  let out = cmd.output().expect("the luacad binary runs");
  assert!(
    out.status.success() && stl.exists(),
    "converting {name}{} failed:\n{}\n{}",
    if via_openscad { " via OpenSCAD" } else { "" },
    String::from_utf8_lossy(&out.stdout),
    String::from_utf8_lossy(&out.stderr),
  );
  stl
}

/// Build one `bosl.*` call both ways and check the meshes agree.
fn assert_matches_bosl2(name: &str, call: &str) {
  let dir = temp_dir();
  let script = format!("render({call})\n");

  let native = measure(&convert(&dir, &script, "native", false));
  let reference = measure(&convert(&dir, &script, "reference", true));

  let ratio = native.volume / reference.volume;
  assert!(
    (ratio - 1.0).abs() < VOLUME_TOLERANCE,
    "{name}: volume {} differs from BOSL2's {} by {:.1}%\n  {call}",
    native.volume,
    reference.volume,
    (ratio - 1.0).abs() * 100.0,
  );

  for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
    assert!(
      (native.min[i] - reference.min[i]).abs() < BBOX_TOLERANCE,
      "{name}: {axis} starts at {} but BOSL2 puts it at {}\n  {call}",
      native.min[i],
      reference.min[i],
    );
    assert!(
      (native.max[i] - reference.max[i]).abs() < BBOX_TOLERANCE,
      "{name}: {axis} ends at {} but BOSL2 puts it at {}\n  {call}",
      native.max[i],
      reference.max[i],
    );
  }

  let _ = std::fs::remove_dir_all(&dir);
}

/// Every case, so one run reports all the mismatches rather than the first.
fn check_all(cases: &[(&str, &str)]) {
  if !openscad_available() {
    eprintln!("skipping: OpenSCAD is not installed");
    return;
  }
  if !bosl2_available() {
    eprintln!("skipping: the BOSL2 library is not installed for OpenSCAD");
    return;
  }

  let mut failures = Vec::new();
  for (name, call) in cases {
    if let Err(panic) =
      std::panic::catch_unwind(|| assert_matches_bosl2(name, call))
    {
      let msg = panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| format!("{name} failed"));
      failures.push(msg);
    }
  }
  assert!(
    failures.is_empty(),
    "{} of {} shapes disagree with BOSL2:\n\n{}",
    failures.len(),
    cases.len(),
    failures.join("\n\n"),
  );
}

#[test]
fn boxes_match_bosl2() {
  check_all(&[
    ("cuboid", "bosl.cuboid { {30, 20, 10} }"),
    ("cuboid scalar", "bosl.cuboid(25)"),
    (
      "cuboid rounded",
      "bosl.cuboid { {30, 20, 10}, rounding = 2 }",
    ),
    (
      "cuboid chamfered",
      "bosl.cuboid { {30, 20, 10}, chamfer = 2 }",
    ),
    (
      "cuboid rounded Z edges",
      "bosl.cuboid { {30, 20, 10}, rounding = 3, edges = 'Z' }",
    ),
    (
      "cuboid rounded top",
      "bosl.cuboid { {30, 20, 10}, rounding = 2, edges = bosl.TOP }",
    ),
    (
      "cuboid except bottom",
      "bosl.cuboid { {30, 20, 10}, rounding = 2, except = bosl.BOTTOM }",
    ),
    (
      "cuboid untrimmed corners",
      "bosl.cuboid { {30, 20, 10}, rounding = 2, trimcorners = false }",
    ),
    (
      "cuboid anchored",
      "bosl.cuboid { {30, 20, 10}, anchor = bosl.BOTTOM }",
    ),
    (
      "cuboid anchored corner",
      "bosl.cuboid { {30, 20, 10}, anchor = {1, 1, 1} }",
    ),
    ("wedge", "bosl.wedge { {30, 30, 20} }"),
    ("octahedron", "bosl.octahedron { size = 35 }"),
    (
      "prismoid",
      "bosl.prismoid { size1 = {40, 40}, size2 = {20, 20}, h = 30 }",
    ),
    (
      "prismoid shifted",
      "bosl.prismoid { size1 = {40, 40}, size2 = {20, 20}, h = 30, shift = {5, 5} }",
    ),
    (
      "prismoid rounded",
      "bosl.prismoid { size1 = {40, 40}, size2 = {20, 20}, h = 30, rounding = 4 }",
    ),
    (
      "rect_tube",
      "bosl.rect_tube { size = {40, 40}, wall = 5, h = 30 }",
    ),
    (
      "rect_tube tapered",
      "bosl.rect_tube { size1 = {40, 40}, size2 = {25, 25}, wall = 5, h = 30 }",
    ),
  ]);
}

#[test]
fn cylinders_match_bosl2() {
  check_all(&[
    ("cyl", "bosl.cyl { h = 20, r = 10 }"),
    ("cyl by diameter", "bosl.cyl { h = 20, d = 15 }"),
    ("cone", "bosl.cyl { h = 20, r1 = 10, r2 = 4 }"),
    (
      "cyl uncentred",
      "bosl.cyl { h = 20, r = 10, center = false }",
    ),
    ("cyl rounded", "bosl.cyl { h = 20, r = 10, rounding = 3 }"),
    ("cyl chamfered", "bosl.cyl { h = 20, r = 10, chamfer = 2 }"),
    (
      "cyl one end rounded",
      "bosl.cyl { h = 20, r = 10, rounding1 = 3 }",
    ),
    (
      "cyl chamfer from end",
      "bosl.cyl { h = 20, r = 10, chamfer = 2, from_end = true }",
    ),
    (
      "cone chamfered",
      "bosl.cyl { h = 20, r1 = 12, r2 = 6, chamfer = 2 }",
    ),
    ("cyl shifted", "bosl.cyl { h = 20, r = 8, shift = {4, 2} }"),
    (
      "cyl circumscribed",
      "bosl.cyl { h = 20, r = 10, circum = true }",
    ),
    (
      "cyl realigned",
      "bosl.cyl { h = 20, r = 10, realign = true }",
    ),
    ("xcyl", "bosl.xcyl { h = 20, r = 5 }"),
    ("ycyl", "bosl.ycyl { h = 20, r = 5 }"),
    ("zcyl", "bosl.zcyl { h = 20, r = 5 }"),
    ("tube", "bosl.tube { h = 30, ['or'] = 20, wall = 4 }"),
    (
      "tube by radii",
      "bosl.tube { h = 30, ['or'] = 20, ir = 14 }",
    ),
    (
      "tube tapered",
      "bosl.tube { h = 30, or1 = 25, or2 = 15, wall = 4 }",
    ),
    ("pie_slice", "bosl.pie_slice { r = 25, h = 20, ang = 120 }"),
    (
      "pie_slice reflex",
      "bosl.pie_slice { r = 25, h = 15, ang = 270 }",
    ),
    (
      "pie_slice cone",
      "bosl.pie_slice { r1 = 25, r2 = 10, h = 20, ang = 90 }",
    ),
    (
      "regular_prism",
      "bosl.regular_prism { n = 6, r = 20, h = 30 }",
    ),
    (
      "regular_prism by side",
      "bosl.regular_prism { n = 5, side = 12, h = 20 }",
    ),
    (
      "regular_prism inscribed",
      "bosl.regular_prism { n = 8, ir = 15, h = 20 }",
    ),
  ]);
}

#[test]
fn round_shapes_match_bosl2() {
  check_all(&[
    ("spheroid", "bosl.spheroid { r = 15 }"),
    ("spheroid by diameter", "bosl.spheroid { d = 24 }"),
    ("torus", "bosl.torus { r_maj = 18, r_min = 5 }"),
    ("torus by or/ir", "bosl.torus { ['or'] = 25, ir = 15 }"),
    ("onion", "bosl.onion { r = 15 }"),
    ("onion capped", "bosl.onion { r = 15, cap_h = 18 }"),
    ("teardrop", "bosl.teardrop { r = 15, h = 20 }"),
    (
      "teardrop capped",
      "bosl.teardrop { r = 15, h = 20, cap_h = 18 }",
    ),
  ]);
}

#[test]
fn transforms_match_bosl2() {
  check_all(&[
    ("up", "bosl.up(10, bosl.cuboid { {20, 20, 20} })"),
    ("down", "bosl.down(10, bosl.cuboid { {20, 20, 20} })"),
    ("left", "bosl.left(10, bosl.cuboid { {20, 20, 20} })"),
    ("right", "bosl.right(10, bosl.cuboid { {20, 20, 20} })"),
    ("fwd", "bosl.fwd(10, bosl.cuboid { {20, 20, 20} })"),
    ("back", "bosl.back(10, bosl.cuboid { {20, 20, 20} })"),
    (
      "move",
      "bosl.move({5, 10, 15}, bosl.cuboid { {20, 20, 20} })",
    ),
    (
      "rot about z",
      "bosl.rot({a = 30, p = bosl.cuboid { {30, 10, 10} }})",
    ),
    (
      "rot by euler angles",
      "bosl.rot({a = {10, 20, 30}, p = bosl.cuboid { {30, 10, 10} }})",
    ),
    (
      "rot about an axis",
      "bosl.rot({a = 45, v = {1, 1, 0}, p = bosl.cuboid { {30, 10, 10} }})",
    ),
    (
      "rot from one direction to another",
      "bosl.rot({from = {0,0,1}, to = {1,0,0}, p = bosl.cuboid { {30,10,10} }})",
    ),
    (
      "rot about a centre",
      "bosl.rot({a = 90, cp = {20, 0, 0}, p = bosl.cuboid { {10, 10, 10} }})",
    ),
    ("xrot", "bosl.xrot(45, bosl.cuboid { {30, 10, 10} })"),
    ("yrot", "bosl.yrot(45, bosl.cuboid { {30, 10, 10} })"),
    ("zrot", "bosl.zrot(45, bosl.cuboid { {30, 10, 10} })"),
    ("xscale", "bosl.xscale(2, bosl.cuboid { {10, 10, 10} })"),
    ("zscale", "bosl.zscale(0.5, bosl.cuboid { {10, 10, 10} })"),
    (
      "xflip",
      "bosl.xflip(bosl.cuboid { {20, 20, 20}, anchor = {1, 0, 0} })",
    ),
    (
      "zflip about an offset plane",
      "bosl.zflip(bosl.cuboid { {20, 20, 20}, anchor = {0,0,-1} }, 10)",
    ),
    (
      "skew",
      "bosl.skew({p = bosl.cuboid { {20, 20, 20} }, sxz = 0.5})",
    ),
    (
      "tilt",
      "bosl.tilt({to = {1, 0, 0}, p = bosl.cuboid { {30, 10, 10} }})",
    ),
    (
      "nested transforms",
      "bosl.up(10, bosl.zrot(45, bosl.cuboid { {30, 10, 10} }))",
    ),
  ]);
}

#[test]
fn distributors_match_bosl2() {
  let cube = "bosl.cuboid { {10, 10, 10} }";
  let cases: Vec<(String, String)> = [
    ("xcopies", format!("bosl.xcopies {{spacing = 20, n = 3, p = {cube}}}")),
    ("ycopies", format!("bosl.ycopies {{spacing = 20, n = 3, p = {cube}}}")),
    ("zcopies", format!("bosl.zcopies {{spacing = 20, n = 3, p = {cube}}}")),
    (
      "xcopies by length",
      format!("bosl.xcopies {{l = 60, n = 4, p = {cube}}}"),
    ),
    (
      "move_copies",
      format!("bosl.move_copies({{{{0,0,0}},{{30,0,0}},{{0,30,0}}}}, {cube})"),
    ),
    (
      "line_copies",
      format!("bosl.line_copies {{spacing = {{20, 10, 0}}, n = 4, p = {cube}}}"),
    ),
    (
      "grid_copies",
      format!("bosl.grid_copies {{spacing = 20, n = {{3, 2}}, p = {cube}}}"),
    ),
    (
      "zrot_copies",
      format!("bosl.zrot_copies {{n = 6, r = 30, p = {cube}}}"),
    ),
    (
      "xrot_copies",
      format!("bosl.xrot_copies {{n = 4, r = 30, p = {cube}}}"),
    ),
    (
      "rot_copies about an axis",
      format!("bosl.rot_copies {{n = 5, v = {{0,0,1}}, delta = {{25,0,0}}, p = {cube}}}"),
    ),
    (
      "arc_copies",
      format!("bosl.arc_copies {{n = 5, r = 30, sa = 0, ea = 180, p = {cube}}}"),
    ),
    (
      "xflip_copy",
      format!("bosl.xflip_copy {{offset = 20, p = {cube}}}"),
    ),
    (
      "mirror_copy",
      format!("bosl.mirror_copy {{v = {{1,1,0}}, offset = 20, p = {cube}}}"),
    ),
  ]
  .into_iter()
  .map(|(n, c)| (n.to_string(), c))
  .collect();

  let refs: Vec<(&str, &str)> =
    cases.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();
  check_all(&refs);
}

#[test]
fn extruded_2d_shapes_match_bosl2() {
  // The 2D shapes are compared through a linear extrusion, since a flat
  // sketch has no volume of its own to measure.
  let wrap = |call: &str| format!("({call}):linear_extrude(5)");
  let cases: Vec<(String, String)> = [
    ("rect", "bosl.rect { {30, 20} }"),
    ("rect rounded", "bosl.rect { {30, 20}, rounding = 4 }"),
    ("rect chamfered", "bosl.rect { {30, 20}, chamfer = 4 }"),
    (
      "rect per corner",
      "bosl.rect { {30, 20}, rounding = {5, 0, 3, 0} }",
    ),
    ("ellipse", "bosl.ellipse { r = {15, 8} }"),
    ("circle", "bosl.ellipse { r = 12 }"),
    (
      "circle circumscribed",
      "bosl.ellipse { r = 12, circum = true }",
    ),
    ("hexagon", "bosl.hexagon { r = 15 }"),
    ("pentagon", "bosl.pentagon { r = 15 }"),
    ("octagon", "bosl.octagon { r = 15 }"),
    ("ngon by side", "bosl.regular_ngon { n = 7, side = 10 }"),
    (
      "ngon rounded",
      "bosl.regular_ngon { n = 6, r = 20, rounding = 3 }",
    ),
    ("right_triangle", "bosl.right_triangle { {20, 15} }"),
    ("trapezoid", "bosl.trapezoid { h = 20, w1 = 30, w2 = 10 }"),
    ("star", "bosl.star { n = 5, r = 20, ir = 9 }"),
    ("star by step", "bosl.star { n = 7, r = 20, step = 2 }"),
    ("teardrop2d", "bosl.teardrop2d { r = 15 }"),
    ("egg", "bosl.egg { length = 50, r1 = 8, r2 = 12, R = 40 }"),
    (
      "glued_circles",
      "bosl.glued_circles { r = 10, spread = 30 }",
    ),
    ("squircle", "bosl.squircle { size = 25, squareness = 0.6 }"),
    ("keyhole", "bosl.keyhole { l = 15, r1 = 5, r2 = 10 }"),
    ("reuleaux", "bosl.reuleaux_polygon { n = 5, r = 15 }"),
    ("supershape", "bosl.supershape { m1 = 6, n1 = 1, r = 15 }"),
  ]
  .iter()
  .map(|(n, c)| (n.to_string(), wrap(c)))
  .collect();

  let refs: Vec<(&str, &str)> = cases
    .iter()
    .map(|(n, c)| (n.as_str(), c.as_str()))
    .collect();
  check_all(&refs);
}
