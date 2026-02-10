use egui_extras::syntax_highlighting;
use mlua::{Lua, Result as LuaResult};
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

#[derive(Debug, Clone, Copy)]
struct Cube {
  size: [f32; 3],
  center: bool,
}

struct AppState {
  text_content: String,
  cubes: Vec<Cube>,
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
            text_content: "-- Welcome to LuaCAD Studio\n-- Z-axis points upward\n\ncube(2)\ncube({1, 1, 3})\ncube({1.5, 1.5, 1}, true)".to_string(),
            cubes: vec![],
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
    self.cubes.clear();

    let lua = Lua::new();
    let cube_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    let result: LuaResult<()> = (|| {
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

      let cube_calls_clone = cube_calls.clone();
      let cube_fn = lua.create_function(
        move |_, args: mlua::MultiValue| {
          let mut iter = args.into_iter();
          let first = iter.next().ok_or_else(|| {
            mlua::Error::RuntimeError(
              "cube() requires at least 1 argument".to_string(),
            )
          })?;
          let second = iter.next();

          // Parse size from first argument
          let size_arr = match &first {
            LuaValue::Number(n) => {
              let s = *n as f32;
              [s, s, s]
            }
            LuaValue::Integer(n) => {
              let s = *n as f32;
              [s, s, s]
            }
            LuaValue::Table(t) => [
              t.get::<f32>(1).unwrap_or(1.0),
              t.get::<f32>(2).unwrap_or(1.0),
              t.get::<f32>(3).unwrap_or(1.0),
            ],
            _ => {
              return Err(mlua::Error::RuntimeError(
                "cube() first argument must be a number or {width, depth, height} table".to_string(),
              ));
            }
          };

          // Parse optional center from second argument
          let center = match second {
            Some(LuaValue::Boolean(b)) => b,
            Some(_) => {
              return Err(mlua::Error::RuntimeError(
                "cube() second argument (center) must be a boolean".to_string(),
              ));
            }
            None => false,
          };

          cube_calls_clone
            .borrow_mut()
            .push(Cube { size: size_arr, center });

          Ok(())
        },
      )?;

      lua.globals().set("cube", cube_fn)?;
      lua.load(&self.text_content).exec()?;

      Ok(())
    })();

    match result {
      Ok(_) => {
        self.cubes = cube_calls.borrow().clone();
        if self.cubes.is_empty() {
          self.lua_error = Some(
            "No cubes found. Use cube(size) or cube({w, d, h})".to_string(),
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

/// Build 3D mesh objects from cube data.
/// Applies Z-up (CAD) to Y-up (OpenGL) coordinate swap.
fn build_scene(
  context: &Context,
  app: &AppState,
) -> Vec<Gm<Mesh, PhysicalMaterial>> {
  app
    .cubes
    .iter()
    .map(|cube| {
      let mut cpu_mesh = CpuMesh::cube();

      // CpuMesh::cube() is a unit cube centered at origin (-0.5 to 0.5).
      // When center=false (OpenSCAD default), shift so corner is at origin.
      let offset = if cube.center {
        vec3(0.0, 0.0, 0.0)
      } else {
        // In CAD (Z-up) space: offset by half size in each axis
        // Then convert to Y-up: (x, y, z) -> (x, z, -y)
        vec3(cube.size[0] * 0.5, cube.size[2] * 0.5, -cube.size[1] * 0.5)
      };

      // Z-up to Y-up: scale axes (x, y, z) -> (x, z, y)
      let transform = Mat4::from_translation(offset)
        * Mat4::from_nonuniform_scale(cube.size[0], cube.size[2], cube.size[1]);
      cpu_mesh.transform(transform).unwrap();

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
    Camera::new_orthographic(
      viewport,
      pos,
      target,
      up,
      app.camera_distance * 2.0,
      0.01,
      1000.0,
    )
  } else {
    Camera::new_perspective(
      viewport,
      pos,
      target,
      up,
      degrees(45.0),
      0.01,
      1000.0,
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
                    app.cubes.clear();
                    app.lua_error = None;
                    app.scene_dirty = true;
                }

                if ui.button("Load Example").clicked() {
                    app.text_content = "-- LuaCAD Example\n-- cube(size) — uniform cube\n-- cube({w, d, h}) — sized cube\n-- cube(size, true) — centered\n\ncube(2)\ncube({3, 1, 1})\ncube({1.5, 1, 2}, true)\n\n-- You can also use variables\nlocal size = {1, 1, 1}\ncube(size)".to_string();
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

            if !app.cubes.is_empty() {
                ui.separator();
                ui.colored_label(
                    egui::Color32::GREEN,
                    format!("{} cube(s) rendered", app.cubes.len()),
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

  // 3D axes at origin
  let axes = Axes::new(&context, 0.02, 5.0);
  let mut dragging_scene = false;

  window.render_loop(move |mut frame_input| {
    // Process GUI (consumes events over egui panels)
    let mut panel_width = 0.0_f32;
    gui.update(
      &mut frame_input.events,
      frame_input.accumulated_time,
      frame_input.viewport,
      frame_input.device_pixel_ratio,
      |gui_context| {
        panel_width = render_ui(gui_context, &mut app);
      },
    );

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
            (app.camera_distance * zoom_factor).clamp(0.1, 50.0);
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
        app.camera_distance * 2.0,
        0.01,
        1000.0,
      );
    } else {
      camera.set_perspective_projection(degrees(45.0), 0.01, 1000.0);
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
    for obj in axes.into_iter() {
      objects.push(obj);
    }
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
