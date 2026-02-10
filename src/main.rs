use csgrs::mesh::Mesh as CsgMesh;
use csgrs::traits::CSG;
use egui_extras::syntax_highlighting;
use mlua::{Lua, Result as LuaResult, UserData, UserDataMethods};
use three_d::*;

use mlua::Value as LuaValue;

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

                    ui.add(
                        egui::TextEdit::multiline(&mut app.text_content)
                            .desired_width(ui.available_width())
                            .desired_rows(30)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .layouter(&mut layouter),
                    );
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

            ui.label(format!("Lines: {}", app.text_content.lines().count()));
            ui.label(format!(
                "Characters: {}",
                app.text_content.len()
            ));

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
