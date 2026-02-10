use csgrs::mesh::Mesh as CsgMesh;
use csgrs::traits::CSG;
use egui_extras::syntax_highlighting;
use mlua::{Lua, Result as LuaResult, UserData, UserDataMethods};
use three_d::*;

use mlua::Value as LuaValue;
use threemf::Mesh as ThreemfMesh;

#[derive(Debug, Clone, Copy, PartialEq)]
enum ThemeMode {
  System,
  Light,
  Dark,
}

#[derive(Debug, Clone, Copy)]
struct ThemeColors {
  bg: (f32, f32, f32),
  egui_dark: bool,
}

impl ThemeColors {
  fn dark() -> Self {
    Self {
      bg: (0.12, 0.12, 0.16),
      egui_dark: true,
    }
  }

  fn light() -> Self {
    Self {
      bg: (0.85, 0.85, 0.88),
      egui_dark: false,
    }
  }
}

/// Detect system dark mode. On macOS, checks AppleInterfaceStyle.
fn system_is_dark_mode() -> bool {
  #[cfg(target_os = "macos")]
  {
    std::process::Command::new("defaults")
      .args(["read", "-g", "AppleInterfaceStyle"])
      .output()
      .map(|o| String::from_utf8_lossy(&o.stdout).contains("Dark"))
      .unwrap_or(false)
  }
  #[cfg(not(target_os = "macos"))]
  {
    true // default to dark on other platforms
  }
}

#[derive(Clone, Debug)]
struct CsgGeometry {
  mesh: CsgMesh<()>,
}

impl UserData for CsgGeometry {
  fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    methods.add_method("translate", |_, this, (x, y, z): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.translate(x, y, z),
      })
    });

    methods.add_method("rotate", |_, this, (x, y, z): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.rotate(x, y, z),
      })
    });

    methods.add_method("scale", |_, this, (sx, sy, sz): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.scale(sx, sy, sz),
      })
    });

    // CSG difference: a - b
    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.difference(&other_ref.mesh),
        })
      },
    );

    // CSG union: a + b
    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.union(&other_ref.mesh),
        })
      },
    );

    // CSG intersection: a * b
    methods.add_meta_method(
      mlua::MetaMethod::Mul,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.intersection(&other_ref.mesh),
        })
      },
    );

    methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
      Ok(format!(
        "CsgGeometry(polygons: {})",
        this.mesh.polygons.len()
      ))
    });
  }
}

/// Convert a csgrs mesh to a three-d CpuMesh.
/// Applies CAD Z-up → GL Y-up coordinate swap: (x,y,z) → (y,z,x)
fn csg_to_cpu_mesh(csg: &CsgMesh<()>) -> CpuMesh {
  let tri_mesh = csg.triangulate();

  let mut positions: Vec<Vec3> = Vec::new();
  let mut normals: Vec<Vec3> = Vec::new();
  let mut indices: Vec<u32> = Vec::new();

  for polygon in &tri_mesh.polygons {
    let base_idx = positions.len() as u32;
    for vertex in &polygon.vertices {
      let p = &vertex.pos;
      let n = &vertex.normal;
      // CAD (x,y,z) → GL (y,z,x)
      positions.push(vec3(p.y, p.z, p.x));
      normals.push(vec3(n.y, n.z, n.x));
    }
    // Fan-triangulate polygons with more than 3 vertices
    for i in 1..polygon.vertices.len().saturating_sub(1) {
      indices.push(base_idx);
      indices.push(base_idx + i as u32);
      indices.push(base_idx + i as u32 + 1);
    }
  }

  CpuMesh {
    positions: Positions::F32(positions),
    indices: Indices::U32(indices),
    normals: Some(normals),
    ..Default::default()
  }
}

type VKey = (i64, i64, i64);

/// Quantize a float to an integer key for vertex deduplication.
/// Uses micron precision (0.001mm) which is well beyond 3D printing accuracy.
fn quantize(v: f32) -> i64 {
  (v as f64 * 1000.0).round() as i64
}

fn vkey(x: f32, y: f32, z: f32) -> VKey {
  (quantize(x), quantize(y), quantize(z))
}

/// Check if point P lies on the line segment A→B (all in quantized coords).
/// Returns true if P is strictly between A and B (not equal to either endpoint).
fn point_on_segment(a: VKey, b: VKey, p: VKey) -> bool {
  if p == a || p == b {
    return false;
  }
  // Vector AB and AP
  let (abx, aby, abz) = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
  let (apx, apy, apz) = (p.0 - a.0, p.1 - a.1, p.2 - a.2);
  // Cross product must be zero (collinear)
  let cx = aby * apz - abz * apy;
  let cy = abz * apx - abx * apz;
  let cz = abx * apy - aby * apx;
  if cx != 0 || cy != 0 || cz != 0 {
    return false;
  }
  // Dot product must be positive and less than |AB|^2 (between A and B)
  let dot = apx * abx + apy * aby + apz * abz;
  let len_sq = abx * abx + aby * aby + abz * abz;
  dot > 0 && dot < len_sq
}

/// Resolve T-junctions in a polygon by inserting vertices that lie on its edges.
/// Returns a new vertex list with extra vertices inserted along edges.
fn resolve_t_junctions(
  polygon_vkeys: &[VKey],
  all_vkeys: &std::collections::HashSet<VKey>,
) -> Vec<VKey> {
  let mut result = Vec::new();
  let n = polygon_vkeys.len();
  for i in 0..n {
    let a = polygon_vkeys[i];
    let b = polygon_vkeys[(i + 1) % n];
    result.push(a);
    // Find all vertices that lie on edge A→B
    let mut on_edge: Vec<(i64, VKey)> = Vec::new();
    for &v in all_vkeys {
      if point_on_segment(a, b, v) {
        // Parametric position along AB for sorting
        let ab = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
        let av = (v.0 - a.0, v.1 - a.1, v.2 - a.2);
        // Use the largest component for stable sorting
        let t = if ab.0.abs() >= ab.1.abs() && ab.0.abs() >= ab.2.abs() {
          av.0 * 1000 / ab.0
        } else if ab.1.abs() >= ab.2.abs() {
          av.1 * 1000 / ab.1
        } else {
          av.2 * 1000 / ab.2
        };
        on_edge.push((t, v));
      }
    }
    on_edge.sort_by_key(|&(t, _)| t);
    for (_, v) in on_edge {
      result.push(v);
    }
  }
  result
}

/// Export all geometries to a 3MF file. Uses original CAD coordinates (no GL transform).
/// Deduplicates vertices and resolves T-junctions so the mesh is manifold.
fn export_3mf(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  use std::collections::{HashMap, HashSet};

  // Step 1: Collect all unique vertex positions across all polygons
  let mut all_vkeys: HashSet<VKey> = HashSet::new();
  let mut poly_data: Vec<(Vec<VKey>, Vec<[f32; 3]>)> = Vec::new();

  for geom in geometries {
    if geom.mesh.polygons.is_empty() {
      continue;
    }
    let tri_mesh = geom.mesh.triangulate();

    for polygon in &tri_mesh.polygons {
      let keys: Vec<VKey> = polygon
        .vertices
        .iter()
        .map(|v| vkey(v.pos.x, v.pos.y, v.pos.z))
        .collect();
      let coords: Vec<[f32; 3]> = polygon
        .vertices
        .iter()
        .map(|v| [v.pos.x, v.pos.y, v.pos.z])
        .collect();
      for &k in &keys {
        all_vkeys.insert(k);
      }
      poly_data.push((keys, coords));
    }
  }

  if poly_data.is_empty() {
    return Err("No geometry to export".to_string());
  }

  // Step 2: Resolve T-junctions and build output mesh
  let mut vertices: Vec<threemf::model::mesh::Vertex> = Vec::new();
  let mut triangles = Vec::new();
  let mut vertex_map: HashMap<VKey, usize> = HashMap::new();

  // Map from VKey to actual f32 coordinates (first occurrence wins)
  let mut coord_map: HashMap<VKey, [f32; 3]> = HashMap::new();
  for (keys, coords) in &poly_data {
    for (k, c) in keys.iter().zip(coords.iter()) {
      coord_map.entry(*k).or_insert(*c);
    }
  }

  let get_idx = |vertex_map: &mut HashMap<VKey, usize>,
                 vertices: &mut Vec<threemf::model::mesh::Vertex>,
                 key: VKey|
   -> usize {
    *vertex_map.entry(key).or_insert_with(|| {
      let idx = vertices.len();
      let c = coord_map.get(&key).unwrap();
      vertices.push(threemf::model::mesh::Vertex {
        x: c[0] as f64,
        y: c[1] as f64,
        z: c[2] as f64,
      });
      idx
    })
  };

  for (keys, _coords) in &poly_data {
    let resolved = resolve_t_junctions(keys, &all_vkeys);
    let indices: Vec<usize> = resolved
      .iter()
      .map(|&k| get_idx(&mut vertex_map, &mut vertices, k))
      .collect();

    // Fan triangulate from first vertex
    for i in 1..indices.len().saturating_sub(1) {
      triangles.push(threemf::model::mesh::Triangle {
        v1: indices[0],
        v2: indices[i],
        v3: indices[i + 1],
      });
    }
  }

  let mesh = ThreemfMesh {
    vertices: threemf::model::mesh::Vertices { vertex: vertices },
    triangles: threemf::model::mesh::Triangles {
      triangle: triangles,
    },
  };

  let file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create file: {e}"))?;
  threemf::write(file, mesh)
    .map_err(|e| format!("Failed to write 3MF: {e}"))?;
  Ok(())
}

/// Merge all geometries into one csgrs mesh via union.
fn merge_geometries(geometries: &[CsgGeometry]) -> Result<CsgMesh<()>, String> {
  if geometries.is_empty() {
    return Err("No geometry to export".to_string());
  }
  let mut merged = geometries[0].mesh.clone();
  for geom in &geometries[1..] {
    if !geom.mesh.polygons.is_empty() {
      merged = merged.union(&geom.mesh);
    }
  }
  Ok(merged)
}

/// Export all geometries to a PLY file using csgrs's built-in PLY export.
fn export_ply(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let ply = merged.to_ply("Exported from LuaCAD Studio");
  std::fs::write(path, ply).map_err(|e| format!("Failed to write PLY: {e}"))?;
  Ok(())
}

/// Export all geometries to a binary STL file using csgrs's built-in STL export.
fn export_stl(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let stl_bytes = merged
    .to_stl_binary("LuaCAD Studio")
    .map_err(|e| format!("Failed to generate STL: {e}"))?;
  std::fs::write(path, stl_bytes)
    .map_err(|e| format!("Failed to write STL: {e}"))?;
  Ok(())
}

/// Export all geometries to an OBJ file using csgrs's built-in OBJ export.
fn export_obj(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let obj = merged.to_obj("LuaCAD_Studio");
  std::fs::write(path, obj).map_err(|e| format!("Failed to write OBJ: {e}"))?;
  Ok(())
}

fn lua_val_to_f32(v: &LuaValue) -> Option<f32> {
  match v {
    LuaValue::Number(n) => Some(*n as f32),
    LuaValue::Integer(n) => Some(*n as f32),
    _ => None,
  }
}

/// Parse cube() arguments: cube(size), cube(w, d, h), or cube({w, d, h})
fn parse_cube_args(args: &mlua::MultiValue) -> mlua::Result<(f32, f32, f32)> {
  if args.is_empty() {
    return Err(mlua::Error::RuntimeError(
      "cube() requires at least 1 argument".to_string(),
    ));
  }

  let first = &args[0];

  // Table form: cube({w, d, h})
  if let LuaValue::Table(t) = first {
    let w: f32 = t.get::<f32>(1).unwrap_or(1.0);
    let d: f32 = t.get::<f32>(2).unwrap_or(1.0);
    let h: f32 = t.get::<f32>(3).unwrap_or(1.0);
    return Ok((w, d, h));
  }

  // Number forms
  let s = lua_val_to_f32(first).ok_or_else(|| {
    mlua::Error::RuntimeError(
      "cube() argument must be a number, three numbers, or {w, d, h} table"
        .to_string(),
    )
  })?;

  // cube(w, d, h) — three separate number args
  if args.len() >= 3 {
    let d = lua_val_to_f32(&args[1]).unwrap_or(s);
    let h = lua_val_to_f32(&args[2]).unwrap_or(s);
    return Ok((s, d, h));
  }

  // cube(size) — uniform
  Ok((s, s, s))
}

struct AppState {
  text_content: String,
  geometries: Vec<CsgGeometry>,
  lua_error: Option<String>,
  camera_azimuth: f32,
  camera_elevation: f32,
  camera_distance: f32,
  orthogonal_view: bool,
  scene_dirty: bool,
  theme_mode: ThemeMode,
  theme_colors: ThemeColors,
  /// Pending editor action triggered by keyboard shortcut
  pending_editor_action: Option<EditorAction>,
  /// Status message from last export attempt
  export_status: Option<(String, bool)>, // (message, is_error)
  /// Pending export format requested this frame
  pending_export: Option<ExportFormat>,
}

#[derive(Debug, Clone, Copy)]
enum ExportFormat {
  ThreeMF,
  PLY,
  STL,
  OBJ,
}

#[derive(Debug, Clone)]
enum EditorAction {
  SelectNextOccurrence, // Cmd+D
  SelectLine,           // Cmd+L
  ToggleComment,        // Cmd+G (Cmd+/ blocked by three-d#571)
}

impl AppState {
  fn new() -> Self {
    let is_dark = system_is_dark_mode();
    let mut app = Self {
            text_content: "-- Welcome to LuaCAD Studio\n-- Z-axis points upward\n-- Use + for union, - for difference, * for intersection\n\nrender(cube(2))\nrender(cube(3, 1, 1):translate(0, 3, 0))".to_string(),
            geometries: vec![],
            lua_error: None,
            camera_azimuth: -30.0,
            camera_elevation: 30.0,
            camera_distance: 5.0,
            orthogonal_view: true,
            scene_dirty: true,
            theme_mode: ThemeMode::System,
            theme_colors: if is_dark { ThemeColors::dark() } else { ThemeColors::light() },
            pending_editor_action: None,
            export_status: None,
            pending_export: None,
        };
    app.execute_lua_code();
    app
  }

  fn resolve_theme(&self) -> ThemeColors {
    match self.theme_mode {
      ThemeMode::Dark => ThemeColors::dark(),
      ThemeMode::Light => ThemeColors::light(),
      ThemeMode::System => {
        if system_is_dark_mode() {
          ThemeColors::dark()
        } else {
          ThemeColors::light()
        }
      }
    }
  }

  fn execute_lua_code(&mut self) {
    self.lua_error = None;
    self.geometries.clear();

    let lua = Lua::new();
    let collector =
      std::rc::Rc::new(std::cell::RefCell::new(Vec::<CsgGeometry>::new()));

    let result: LuaResult<mlua::MultiValue> = (|| {
      let print_fn =
        lua.create_function(|_, args: mlua::Variadic<mlua::Value>| {
          let output = args
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join("\t");
          println!("Lua output: {output}");
          Ok(())
        })?;
      lua.globals().set("print", print_fn)?;

      // cube() — returns CsgGeometry userdata
      let cube_fn = lua.create_function(|_, args: mlua::MultiValue| {
        let (w, d, h) = parse_cube_args(&args)?;
        // cuboid() places one corner at origin (0,0,0)
        let mesh = CsgMesh::<()>::cuboid(w, d, h, None);
        Ok(CsgGeometry { mesh })
      })?;
      lua.globals().set("cube", cube_fn)?;

      // sphere() — returns CsgGeometry userdata
      let sphere_fn =
        lua.create_function(|_, (radius, segments): (f32, Option<u32>)| {
          let segs = segments.unwrap_or(16);
          let mesh =
            CsgMesh::<()>::sphere(radius, segs as usize, segs as usize, None);
          Ok(CsgGeometry { mesh })
        })?;
      lua.globals().set("sphere", sphere_fn)?;

      // cylinder() — returns CsgGeometry userdata
      let cylinder_fn = lua.create_function(
        |_, (radius, height, segments): (f32, f32, Option<u32>)| {
          let segs = segments.unwrap_or(16);
          let mesh =
            CsgMesh::<()>::cylinder(radius, height, segs as usize, None);
          Ok(CsgGeometry { mesh })
        },
      )?;
      lua.globals().set("cylinder", cylinder_fn)?;

      // render() — adds geometry to scene
      let collector_clone = collector.clone();
      let render_fn =
        lua.create_function(move |_, ud: mlua::AnyUserData| {
          let geom = ud.borrow::<CsgGeometry>()?.clone();
          collector_clone.borrow_mut().push(geom);
          Ok(())
        })?;
      lua.globals().set("render", render_fn)?;

      lua.load(&self.text_content).eval::<mlua::MultiValue>()
    })();

    match result {
      Ok(returns) => {
        // Auto-render any CsgGeometry returned from top-level
        for val in returns.iter() {
          if let LuaValue::UserData(ud) = val
            && let Ok(geom) = ud.borrow::<CsgGeometry>()
          {
            collector.borrow_mut().push(geom.clone());
          }
        }
        self.geometries = collector.borrow().clone();
        if self.geometries.is_empty() {
          self.lua_error = Some(
            "No geometry to render. Use render(obj) or return a geometry object."
              .to_string(),
          );
        }
      }
      Err(e) => {
        self.lua_error = Some(format!("Lua error: {e}"));
      }
    }

    self.scene_dirty = true;
  }
}

/// Build 3D mesh objects from CSG geometry.
/// Coordinate transform (CAD Z-up → GL Y-up) is done inside csg_to_cpu_mesh.
fn build_scene(
  context: &Context,
  app: &AppState,
) -> Vec<Gm<Mesh, PhysicalMaterial>> {
  app
    .geometries
    .iter()
    .filter(|geom| !geom.mesh.polygons.is_empty())
    .map(|geom| {
      let cpu_mesh = csg_to_cpu_mesh(&geom.mesh);
      Gm::new(
        Mesh::new(context, &cpu_mesh),
        PhysicalMaterial::new_opaque(
          context,
          &CpuMaterial {
            albedo: Srgba {
              r: 150,
              g: 150,
              b: 255,
              a: 255,
            },
            metallic: 0.0,
            roughness: 0.7,
            ..Default::default()
          },
        ),
      )
    })
    .collect()
}

/// Compute camera position from azimuth/elevation/distance.
/// Returns (position, target, up) in Y-up coordinate system.
fn compute_camera_vectors(app: &AppState) -> (Vec3, Vec3, Vec3) {
  let az = app.camera_azimuth.to_radians();
  let el = app.camera_elevation.to_radians();
  let d = app.camera_distance;

  let x = d * el.cos() * az.sin();
  let y = d * el.sin();
  let z = d * el.cos() * az.cos();

  let position = vec3(x, y, z);
  let target = vec3(0.0, 0.0, 0.0);
  let up = vec3(0.0, 1.0, 0.0);

  (position, target, up)
}

fn build_camera(viewport: Viewport, app: &AppState) -> Camera {
  let (pos, target, up) = compute_camera_vectors(app);
  if app.orthogonal_view {
    // three-d multiplies height by camera-to-target distance internally,
    // so use a fixed height. Negative z_near prevents any front clipping.
    Camera::new_orthographic(
      viewport,
      pos,
      target,
      up,
      2.0,
      -100.0 * app.camera_distance,
      100.0 * app.camera_distance,
    )
  } else {
    Camera::new_perspective(
      viewport,
      pos,
      target,
      up,
      degrees(45.0),
      0.1 * app.camera_distance,
      100.0 * app.camera_distance,
    )
  }
}

/// Get the word boundaries around a character index in the text.
/// Returns (start, end) character indices of the word.
fn word_at(text: &str, char_idx: usize) -> (usize, usize) {
  let chars: Vec<char> = text.chars().collect();
  if char_idx >= chars.len() {
    return (char_idx, char_idx);
  }

  let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

  if !is_word_char(chars[char_idx]) {
    return (char_idx, char_idx + 1);
  }

  let mut start = char_idx;
  while start > 0 && is_word_char(chars[start - 1]) {
    start -= 1;
  }

  let mut end = char_idx;
  while end < chars.len() && is_word_char(chars[end]) {
    end += 1;
  }

  (start, end)
}

/// Find the line start and end (including trailing newline) for the line containing `char_idx`.
fn line_range_at(text: &str, char_idx: usize) -> (usize, usize) {
  let chars: Vec<char> = text.chars().collect();
  let idx = char_idx.min(chars.len().saturating_sub(1));

  let mut start = idx;
  while start > 0 && chars[start - 1] != '\n' {
    start -= 1;
  }

  let mut end = idx;
  while end < chars.len() && chars[end] != '\n' {
    end += 1;
  }
  // Include the trailing newline if present
  if end < chars.len() && chars[end] == '\n' {
    end += 1;
  }

  (start, end)
}

/// Apply a pending editor action, returning the new cursor range (as char indices).
fn apply_editor_action(
  action: &EditorAction,
  text: &mut String,
  cursor_start: usize,
  cursor_end: usize,
) -> (usize, usize) {
  match action {
    EditorAction::SelectNextOccurrence => {
      if cursor_start == cursor_end {
        // No selection: select the word under cursor
        let (ws, we) = word_at(text, cursor_start);
        (ws, we)
      } else {
        // Has selection: find next occurrence of selected text
        let chars: Vec<char> = text.chars().collect();
        let selected: String = chars[cursor_start..cursor_end].iter().collect();
        let after_selection: String = chars[cursor_end..].iter().collect();

        if let Some(rel_pos) = after_selection.find(&selected) {
          // Convert byte offset from find() to char offset
          let char_offset = after_selection[..rel_pos].chars().count();
          let new_start = cursor_end + char_offset;
          let new_end = new_start + (cursor_end - cursor_start);
          (new_start, new_end)
        } else {
          // Wrap around: search from beginning
          let before_selection: String = chars[..cursor_start].iter().collect();
          if let Some(rel_pos) = before_selection.find(&selected) {
            let char_offset = before_selection[..rel_pos].chars().count();
            let new_end = char_offset + (cursor_end - cursor_start);
            (char_offset, new_end)
          } else {
            // Only one occurrence, keep current selection
            (cursor_start, cursor_end)
          }
        }
      }
    }

    EditorAction::SelectLine => {
      if cursor_start == cursor_end {
        // No selection: select current line
        line_range_at(text, cursor_start)
      } else {
        // Already have selection: extend to include next line
        let (_, end) = line_range_at(text, cursor_end.saturating_sub(1));
        if end < text.chars().count() {
          let (_, next_end) = line_range_at(text, end);
          (cursor_start, next_end)
        } else {
          (cursor_start, end)
        }
      }
    }

    EditorAction::ToggleComment => {
      let chars: Vec<char> = text.chars().collect();
      let total_chars = chars.len();

      // Find all lines that overlap the selection
      let sel_start = cursor_start.min(cursor_end);
      let sel_end = if cursor_start == cursor_end {
        cursor_end
      } else {
        // Don't include a line if selection ends at its very start
        cursor_end.saturating_sub(1)
      };

      // Collect line ranges
      let mut line_ranges: Vec<(usize, usize)> = Vec::new();
      let (first_start, first_end) = line_range_at(text, sel_start);
      line_ranges.push((first_start, first_end));

      let mut pos = first_end;
      while pos <= sel_end && pos < total_chars {
        let (ls, le) = line_range_at(text, pos);
        line_ranges.push((ls, le));
        if le == pos {
          break; // prevent infinite loop
        }
        pos = le;
      }

      // Check if all lines are already commented
      let all_commented = line_ranges.iter().all(|(ls, le)| {
        let line: String = chars[*ls..*le].iter().collect();
        let trimmed = line.trim_start();
        trimmed.starts_with("--") || trimmed.is_empty()
      });

      // Build new text by processing lines in reverse order to maintain char indices
      let mut new_text = text.clone();
      let mut offset: i64 = 0;

      // Process lines front-to-back, tracking the cumulative offset
      for (ls, _le) in &line_ranges {
        let adjusted_start = (*ls as i64 + offset) as usize;
        let line_chars: Vec<char> = new_text.chars().collect();
        // Find the first non-whitespace position in this line
        let mut first_non_ws = adjusted_start;
        while first_non_ws < line_chars.len()
          && line_chars[first_non_ws] != '\n'
          && line_chars[first_non_ws].is_whitespace()
        {
          first_non_ws += 1;
        }

        // Skip empty lines (or lines that are just a newline)
        if first_non_ws >= line_chars.len() || line_chars[first_non_ws] == '\n'
        {
          continue;
        }

        // Convert char index to byte index for string operations
        let byte_idx: usize =
          line_chars[..first_non_ws].iter().collect::<String>().len();

        if all_commented {
          // Remove "-- " or "--"
          if new_text[byte_idx..].starts_with("-- ") {
            new_text.replace_range(byte_idx..byte_idx + 3, "");
            offset -= 3;
          } else if new_text[byte_idx..].starts_with("--") {
            new_text.replace_range(byte_idx..byte_idx + 2, "");
            offset -= 2;
          }
        } else {
          // Add "-- "
          new_text.insert_str(byte_idx, "-- ");
          offset += 3;
        }
      }

      let new_len = new_text.chars().count();
      let new_cursor_end = (cursor_end as i64 + offset).max(0) as usize;
      let new_cursor_end = new_cursor_end.min(new_len);
      let new_cursor_start = if cursor_start == cursor_end {
        new_cursor_end
      } else {
        cursor_start.min(new_len)
      };

      *text = new_text;
      (new_cursor_start, new_cursor_end)
    }
  }
}

fn render_ui(gui_context: &egui::Context, app: &mut AppState) -> f32 {
  // Apply theme visuals
  if app.theme_colors.egui_dark {
    gui_context.set_visuals(egui::Visuals::dark());
  } else {
    gui_context.set_visuals(egui::Visuals::light());
  }

  // Right panel: code editor
  let panel_response = egui::SidePanel::right("code_editor")
        .default_width(400.0)
        .show(gui_context, |ui| {
            ui.heading("Code Editor");

            let editor_height = ui.available_height() - 100.0;
            egui::ScrollArea::vertical()
                .max_height(editor_height)
                .show(ui, |ui| {
                    let mut layouter =
                        |ui: &egui::Ui, string: &str, wrap_width: f32| {
                            let theme = if ui.style().visuals.dark_mode {
                                syntax_highlighting::CodeTheme::dark(14.0)
                            } else {
                                syntax_highlighting::CodeTheme::light(14.0)
                            };

                            let mut layout_job =
                                syntax_highlighting::highlight(
                                    ui.ctx(),
                                    ui.style(),
                                    &theme,
                                    string,
                                    "lua",
                                );
                            layout_job.wrap.max_width = wrap_width;
                            ui.fonts(|f| f.layout_job(layout_job))
                        };

                    let te_output = egui::TextEdit::multiline(&mut app.text_content)
                            .desired_width(ui.available_width())
                            .desired_rows(30)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .layouter(&mut layouter)
                            .show(ui);

                    // Apply pending editor action (Cmd+D, Cmd+L, Cmd+G)
                    if let Some(action) = app.pending_editor_action.take() {
                        let (cursor_start, cursor_end) = if let Some(range) = te_output.cursor_range {
                            let sorted = range.as_sorted_char_range();
                            (sorted.start, sorted.end)
                        } else {
                            (0, 0)
                        };

                        let (new_start, new_end) = apply_editor_action(
                            &action,
                            &mut app.text_content,
                            cursor_start,
                            cursor_end,
                        );

                        // Update cursor/selection state
                        let mut state = te_output.state.clone();
                        use egui::text::CCursor;
                        use egui::text_selection::CCursorRange;
                        state.cursor.set_char_range(Some(CCursorRange::two(
                            CCursor::new(new_start),
                            CCursor::new(new_end),
                        )));
                        state.store(ui.ctx(), te_output.response.id);
                    }
                });

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Clear").clicked() {
                    app.text_content.clear();
                    app.geometries.clear();
                    app.lua_error = None;
                    app.scene_dirty = true;
                }

                if ui.button("Load Example").clicked() {
                    app.text_content = "-- CSG Boolean Operations Demo\n\n-- Create a hollow box\nlocal outer = cube(30, 20, 15)\nlocal inner = cube(26, 16, 15):translate(2, 2, 2)\nlocal box = outer - inner\n\n-- Cut a window in the front\nlocal window = cube(10, 4, 8):translate(10, -1, 4)\nlocal result = box - window\n\nrender(result)".to_string();
                }

                let remaining = ui.available_width();
                ui.add_space(remaining - 60.0);
                let run_btn = egui::Button::new(
                    egui::RichText::new("Run").size(18.0),
                ).min_size(egui::vec2(60.0, 30.0));
                if ui.add(run_btn).clicked() {
                    app.execute_lua_code();
                }
            });

            ui.horizontal(|ui| {
                let has_geometry = !app.geometries.is_empty();
                if ui.add_enabled(has_geometry, egui::Button::new("Export 3MF")).clicked() {
                    app.pending_export = Some(ExportFormat::ThreeMF);
                }
                if ui.add_enabled(has_geometry, egui::Button::new("Export STL")).clicked() {
                    app.pending_export = Some(ExportFormat::STL);
                }
                if ui.add_enabled(has_geometry, egui::Button::new("Export OBJ")).clicked() {
                    app.pending_export = Some(ExportFormat::OBJ);
                }
                if ui.add_enabled(has_geometry, egui::Button::new("Export PLY")).clicked() {
                    app.pending_export = Some(ExportFormat::PLY);
                }
            });

            ui.add_space(6.0);
            ui.label(format!("Lines: {}  Chars: {}", app.text_content.lines().count(), app.text_content.len()));
            ui.add_space(4.0);
            ui.label("⌘D Select Word/Next  ⌘L Select Line  ⌘G Toggle Comment");

            if let Some(error) = &app.lua_error {
                ui.separator();
                ui.colored_label(
                    egui::Color32::RED,
                    format!("Error: {error}"),
                );
            }

            if !app.geometries.is_empty() {
                let total_polys: usize = app.geometries.iter()
                    .map(|g| g.mesh.polygons.len()).sum();
                ui.separator();
                ui.colored_label(
                    egui::Color32::GREEN,
                    format!("{} object(s), {} polygons", app.geometries.len(), total_polys),
                );
            }

            if let Some((msg, is_error)) = &app.export_status {
                ui.separator();
                let color = if *is_error { egui::Color32::RED } else { egui::Color32::GREEN };
                ui.colored_label(color, msg.as_str());
            }
        });
  let right_panel_width = panel_response.response.rect.width();

  // Bottom panel: camera controls and view presets
  egui::TopBottomPanel::bottom("controls").show(gui_context, |ui| {
    ui.horizontal(|ui| {
      ui.label(format!(
        "Az: {:.1} El: {:.1} Dist: {:.1}",
        app.camera_azimuth, app.camera_elevation, app.camera_distance
      ));
      ui.separator();

      ui.label("Projection:");
      if ui
        .selectable_label(app.orthogonal_view, "Orthogonal")
        .clicked()
      {
        app.orthogonal_view = true;
      }
      if ui
        .selectable_label(!app.orthogonal_view, "Perspective")
        .clicked()
      {
        app.orthogonal_view = false;
      }

      ui.separator();
      ui.label("Views:");

      let (az, el) = (app.camera_azimuth, app.camera_elevation);
      let is = |a: f32, e: f32| (az - a).abs() < 1.0 && (el - e).abs() < 1.0;

      if ui.selectable_label(is(-30.0, 30.0), "Default").clicked() {
        app.camera_azimuth = -30.0;
        app.camera_elevation = 30.0;
      }
      if ui.selectable_label(is(0.0, 90.0), "Top").clicked() {
        app.camera_azimuth = 0.0;
        app.camera_elevation = 89.0;
      }
      if ui.selectable_label(is(0.0, -90.0), "Bottom").clicked() {
        app.camera_azimuth = 0.0;
        app.camera_elevation = -89.0;
      }
      if ui.selectable_label(is(0.0, 0.0), "Front").clicked() {
        app.camera_azimuth = 0.0;
        app.camera_elevation = 0.0;
      }
      if ui.selectable_label(is(180.0, 0.0), "Back").clicked() {
        app.camera_azimuth = 180.0;
        app.camera_elevation = 0.0;
      }
      if ui.selectable_label(is(-90.0, 0.0), "Left").clicked() {
        app.camera_azimuth = -90.0;
        app.camera_elevation = 0.0;
      }
      if ui.selectable_label(is(90.0, 0.0), "Right").clicked() {
        app.camera_azimuth = 90.0;
        app.camera_elevation = 0.0;
      }
      ui.separator();
      ui.label("Theme:");
      if ui
        .selectable_label(app.theme_mode == ThemeMode::System, "Auto")
        .clicked()
      {
        app.theme_mode = ThemeMode::System;
        app.theme_colors = app.resolve_theme();
      }
      if ui
        .selectable_label(app.theme_mode == ThemeMode::Light, "Light")
        .clicked()
      {
        app.theme_mode = ThemeMode::Light;
        app.theme_colors = app.resolve_theme();
      }
      if ui
        .selectable_label(app.theme_mode == ThemeMode::Dark, "Dark")
        .clicked()
      {
        app.theme_mode = ThemeMode::Dark;
        app.theme_colors = app.resolve_theme();
      }
    });

    ui.label("Mouse: Drag to rotate, Scroll to zoom");
  });

  right_panel_width
}

fn main() {
  let window = Window::new(WindowSettings {
    title: "LuaCAD Studio".to_string(),
    max_size: None,
    ..Default::default()
  })
  .unwrap();

  let context = window.gl();
  let mut gui = three_d::GUI::new(&context);
  let mut app = AppState::new();

  // Build initial scene
  let mut scene_objects = build_scene(&context, &app);
  app.scene_dirty = false;

  let mut camera = build_camera(window.viewport(), &app);
  let mut last_theme_check = 0.0_f64;

  // Lighting: low ambient + strong top-right key + soft fill
  let ambient = AmbientLight::new(&context, 0.15, Srgba::WHITE);
  let light0 =
    DirectionalLight::new(&context, 0.7, Srgba::WHITE, vec3(1.0, 1.0, 0.5));
  let light1 =
    DirectionalLight::new(&context, 0.25, Srgba::WHITE, vec3(-1.0, 0.3, -0.5));
  let light2 =
    DirectionalLight::new(&context, 0.15, Srgba::WHITE, vec3(0.0, -1.0, 0.0));

  // 3D axes at origin — CAD convention: Red=X(depth), Green=Y(right), Blue=Z(up)
  // Mapping: CAD (x,y,z) → GL (y, z, x)
  let cad_axes = {
    let mut cpu_mesh = CpuMesh::arrow(0.95, 0.8, 16);
    cpu_mesh
      .transform(Mat4::from_nonuniform_scale(5.0, 0.02, 0.02))
      .unwrap();
    Gm::new(
      InstancedMesh::new(
        &context,
        &Instances {
          transformations: vec![
            Mat4::from_angle_y(degrees(-90.0)), // GL +Z = CAD +X (Red, depth)
            Mat4::identity(),                   // GL +X = CAD +Y (Green, right)
            Mat4::from_angle_z(degrees(90.0)),  // GL +Y = CAD +Z (Blue, up)
          ],
          texture_transformations: None,
          colors: Some(vec![Srgba::RED, Srgba::GREEN, Srgba::BLUE]),
        },
        &cpu_mesh,
      ),
      ColorMaterial::default(),
    )
  };
  let mut dragging_scene = false;
  let mut clipboard = arboard::Clipboard::new().ok();

  window.render_loop(move |mut frame_input| {
    // Detect clipboard key combos (Cmd+V/C/X) that three-d doesn't forward to egui
    let mut paste_text: Option<String> = None;
    let mut wants_copy = false;
    let mut wants_cut = false;
    for event in frame_input.events.iter() {
      if let Event::KeyPress {
        kind, modifiers, ..
      } = event
        && modifiers.command
      {
        match kind {
          Key::V => {
            if let Some(cb) = clipboard.as_mut() {
              paste_text = cb.get_text().ok();
            }
          }
          Key::C => wants_copy = true,
          Key::X => wants_cut = true,
          Key::D => {
            app.pending_editor_action =
              Some(EditorAction::SelectNextOccurrence);
          }
          Key::L => {
            app.pending_editor_action = Some(EditorAction::SelectLine);
          }
          Key::G => {
            app.pending_editor_action = Some(EditorAction::ToggleComment);
          }
          _ => {}
        }
      }
    }

    // Project axis label positions (using camera from previous frame)
    let dpr = frame_input.device_pixel_ratio;
    let axis_labels: [(egui::Pos2, &str, egui::Color32); 3] = {
      let tips_gl = [
        vec3(0.0, 0.0, 5.2), // CAD +X → GL +Z (depth)
        vec3(5.2, 0.0, 0.0), // CAD +Y → GL +X (right)
        vec3(0.0, 5.2, 0.0), // CAD +Z → GL +Y (up)
      ];
      let labels = ["X", "Y", "Z"];
      let colors = [
        egui::Color32::RED,
        egui::Color32::GREEN,
        egui::Color32::from_rgb(80, 80, 255),
      ];
      let mut result = [
        (egui::Pos2::ZERO, "X", egui::Color32::RED),
        (egui::Pos2::ZERO, "Y", egui::Color32::GREEN),
        (egui::Pos2::ZERO, "Z", egui::Color32::from_rgb(80, 80, 255)),
      ];
      for i in 0..3 {
        let px = camera.pixel_at_position(tips_gl[i]);
        // pixel_at_position returns physical pixels with Y from bottom;
        // convert to egui logical coords (Y from top)
        let vp = camera.viewport();
        let ex = px.x as f32 / dpr;
        let ey = (vp.height as f32 - px.y as f32) / dpr;
        result[i] = (egui::Pos2::new(ex, ey), labels[i], colors[i]);
      }
      result
    };

    // Process GUI (consumes events over egui panels)
    let mut panel_width = 0.0_f32;
    gui.update(
      &mut frame_input.events,
      frame_input.accumulated_time,
      frame_input.viewport,
      frame_input.device_pixel_ratio,
      |gui_context| {
        // Inject clipboard events that three-d doesn't handle
        if let Some(text) = &paste_text {
          gui_context.input_mut(|i| {
            i.events.push(egui::Event::Paste(text.clone()));
          });
        }
        if wants_copy {
          gui_context.input_mut(|i| i.events.push(egui::Event::Copy));
        }
        if wants_cut {
          gui_context.input_mut(|i| i.events.push(egui::Event::Cut));
        }

        panel_width = render_ui(gui_context, &mut app);

        // Draw axis labels as overlay
        let painter = gui_context.layer_painter(egui::LayerId::new(
          egui::Order::Foreground,
          egui::Id::new("axis_labels"),
        ));
        for (pos, label, color) in &axis_labels {
          painter.text(
            *pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(14.0),
            *color,
          );
        }
      },
    );

    // Handle copy/cut output from egui → system clipboard
    gui.context().output_mut(|o| {
      if !o.copied_text.is_empty()
        && let Some(cb) = clipboard.as_mut()
      {
        let _ = cb.set_text(std::mem::take(&mut o.copied_text));
      }
    });

    // Handle export requests (outside egui context so file dialog works)
    if let Some(fmt) = app.pending_export.take() {
      let (title, filter_name, ext, default_name) = match fmt {
        ExportFormat::ThreeMF => {
          ("Export 3MF", "3MF Files", "3mf", "model.3mf")
        }
        ExportFormat::STL => ("Export STL", "STL Files", "stl", "model.stl"),
        ExportFormat::OBJ => ("Export OBJ", "OBJ Files", "obj", "model.obj"),
        ExportFormat::PLY => ("Export PLY", "PLY Files", "ply", "model.ply"),
      };
      if let Some(path) = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter_name, &[ext])
        .set_file_name(default_name)
        .save_file()
      {
        let result = match fmt {
          ExportFormat::ThreeMF => export_3mf(&app.geometries, &path),
          ExportFormat::STL => export_stl(&app.geometries, &path),
          ExportFormat::OBJ => export_obj(&app.geometries, &path),
          ExportFormat::PLY => export_ply(&app.geometries, &path),
        };
        match result {
          Ok(()) => {
            app.export_status =
              Some((format!("Exported to {}", path.display()), false));
          }
          Err(e) => {
            app.export_status = Some((format!("Export failed: {e}"), true));
          }
        }
      }
    }

    // Compute 3D viewport: left area excluding right panel
    let full = frame_input.viewport;
    let panel_px =
      (panel_width * frame_input.device_pixel_ratio).round() as u32;
    let scene_width = full.width.saturating_sub(panel_px);
    let scene_viewport = Viewport {
      x: full.x,
      y: full.y,
      width: scene_width,
      height: full.height,
    };
    camera.set_viewport(scene_viewport);

    // Handle camera input from remaining events (not consumed by GUI)
    let scene_max_x = scene_width as f32;
    let mut camera_changed = false;
    for event in frame_input.events.iter() {
      match event {
        Event::MousePress {
          button: MouseButton::Left,
          position,
          handled,
          ..
        } if !handled && position.x < scene_max_x => {
          dragging_scene = true;
        }
        Event::MouseRelease {
          button: MouseButton::Left,
          ..
        } => {
          dragging_scene = false;
        }
        Event::MouseMotion {
          delta,
          button: Some(MouseButton::Left),
          ..
        } if dragging_scene => {
          app.camera_azimuth -= delta.0 * 0.5;
          app.camera_elevation =
            (app.camera_elevation + delta.1 * 0.5).clamp(-85.0, 85.0);
          camera_changed = true;
        }
        Event::MouseWheel {
          delta,
          handled,
          position,
          ..
        } if !handled && position.x < scene_max_x => {
          let zoom_factor = (-delta.1 * 0.01).exp();
          app.camera_distance =
            (app.camera_distance * zoom_factor).clamp(0.001, 10_000.0);
          camera_changed = true;
        }
        _ => {}
      }
    }

    // Update camera if angles or projection changed
    if camera_changed {
      let (pos, target, up) = compute_camera_vectors(&app);
      camera.set_view(pos, target, up);
    }

    // Update projection mode
    if app.orthogonal_view {
      camera.set_orthographic_projection(
        2.0,
        -100.0 * app.camera_distance,
        100.0 * app.camera_distance,
      );
    } else {
      camera.set_perspective_projection(
        degrees(45.0),
        0.1 * app.camera_distance,
        100.0 * app.camera_distance,
      );
    }

    // Rebuild scene if Lua was re-executed
    if app.scene_dirty {
      scene_objects = build_scene(&context, &app);
      app.scene_dirty = false;
    }

    // Render
    let screen = frame_input.screen();

    // Collect all renderable objects as trait object references
    let mut objects: Vec<&dyn Object> = Vec::new();
    objects.push(&cad_axes);
    for obj in scene_objects.iter() {
      objects.push(obj);
    }

    // Re-check system theme every 2 seconds when in Auto mode
    if app.theme_mode == ThemeMode::System
      && frame_input.accumulated_time - last_theme_check > 2000.0
    {
      last_theme_check = frame_input.accumulated_time;
      app.theme_colors = app.resolve_theme();
    }

    let (bg_r, bg_g, bg_b) = app.theme_colors.bg;
    screen
      .clear(ClearState::color_and_depth(bg_r, bg_g, bg_b, 1.0, 1.0))
      .render(
        &camera,
        objects,
        &[
          &ambient as &dyn Light,
          &light0 as &dyn Light,
          &light1 as &dyn Light,
          &light2 as &dyn Light,
        ],
      );
    screen.write(|| gui.render()).unwrap();

    FrameOutput::default()
  });
}
