use egui_extras::syntax_highlighting;
use mlua::{Lua, Result as LuaResult};
use three_d::*;

#[derive(Debug, Clone)]
struct Cube {
  position: [f32; 3],
  size: [f32; 3],
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
}

impl AppState {
  fn new() -> Self {
    let mut app = Self {
            text_content: "-- Welcome to LuaCAD Studio\n-- Z-axis points upward\n\ncube({0, 0, 0}, {2, 2, 2})\ncube({3, 0, 0}, {1, 1, 3})\ncube({0, 3, 1}, {1.5, 1.5, 1})".to_string(),
            cubes: vec![],
            lua_error: None,
            camera_azimuth: -30.0,
            camera_elevation: 30.0,
            camera_distance: 5.0,
            orthogonal_view: true,
            scene_dirty: true,
        };
    app.execute_lua_code();
    app
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
        move |_, (pos, size): (mlua::Table, mlua::Table)| {
          let position = [
            pos.get::<f32>(1).unwrap_or(0.0),
            pos.get::<f32>(2).unwrap_or(0.0),
            pos.get::<f32>(3).unwrap_or(0.0),
          ];
          let size_arr = [
            size.get::<f32>(1).unwrap_or(1.0),
            size.get::<f32>(2).unwrap_or(1.0),
            size.get::<f32>(3).unwrap_or(1.0),
          ];

          cube_calls_clone.borrow_mut().push(Cube {
            position,
            size: size_arr,
          });

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
            "No cubes found. Use cube({x, y, z}, {width, height, depth})"
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

      // Z-up to Y-up: (x, y, z) -> (x, z, -y)
      let transform = Mat4::from_translation(vec3(
        cube.position[0],
        cube.position[2],
        -cube.position[1],
      )) * Mat4::from_nonuniform_scale(
        cube.size[0],
        cube.size[2],
        cube.size[1],
      );
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

fn render_ui(gui_context: &egui::Context, app: &mut AppState) {
  // Right panel: code editor
  egui::SidePanel::right("code_editor")
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
                if ui.button("Run").clicked() {
                    app.execute_lua_code();
                }

                if ui.button("Clear").clicked() {
                    app.text_content.clear();
                    app.cubes.clear();
                    app.lua_error = None;
                    app.scene_dirty = true;
                }

                if ui.button("Load Example").clicked() {
                    app.text_content = "-- LuaCAD Example\n-- Create cubes with position and size\ncube({0, 0, 0}, {2, 2, 2})\ncube({3, 0, 0}, {1, 3, 1})\ncube({-2, 2, 1}, {1.5, 1, 2})\n\n-- You can also use variables\nlocal pos = {1, -2, 0}\nlocal size = {1, 1, 1}\ncube(pos, size)".to_string();
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
    });

    ui.label("Mouse: Drag to rotate, Scroll to zoom");
  });
}

fn main() {
  let window = Window::new(WindowSettings {
    title: "LuaCAD Studio".to_string(),
    max_size: Some((1200, 800)),
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

  // Lighting: ambient + two directional for CAD-style illumination
  let ambient = AmbientLight::new(&context, 0.3, Srgba::WHITE);
  let light0 =
    DirectionalLight::new(&context, 1.0, Srgba::WHITE, vec3(0.5, 1.0, 0.5));
  let light1 =
    DirectionalLight::new(&context, 0.3, Srgba::WHITE, vec3(-0.5, -0.5, -0.5));

  // 3D axes at origin
  let axes = Axes::new(&context, 0.02, 5.0);

  window.render_loop(move |mut frame_input| {
    // Update camera viewport on resize
    camera.set_viewport(frame_input.viewport);

    // Process GUI (consumes events over egui panels)
    gui.update(
      &mut frame_input.events,
      frame_input.accumulated_time,
      frame_input.viewport,
      frame_input.device_pixel_ratio,
      |gui_context| {
        render_ui(gui_context, &mut app);
      },
    );

    // Handle camera input from remaining events (not consumed by GUI)
    let mut camera_changed = false;
    for event in frame_input.events.iter() {
      match event {
        Event::MouseMotion {
          delta,
          button: Some(MouseButton::Left),
          ..
        } => {
          app.camera_azimuth -= delta.0 * 0.5;
          app.camera_elevation =
            (app.camera_elevation + delta.1 * 0.5).clamp(-85.0, 85.0);
          camera_changed = true;
        }
        Event::MouseWheel { delta, .. } => {
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

    screen
      .clear(ClearState::color_and_depth(0.12, 0.12, 0.16, 1.0, 1.0))
      .render(
        &camera,
        objects,
        &[
          &ambient as &dyn Light,
          &light0 as &dyn Light,
          &light1 as &dyn Light,
        ],
      );
    screen.write(|| gui.render()).unwrap();

    FrameOutput::default()
  });
}
