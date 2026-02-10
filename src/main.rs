use eframe::egui;
use egui_extras::syntax_highlighting;
use mlua::{Lua, Result as LuaResult};
use nalgebra::{Matrix4, Point3};

fn main() -> Result<(), eframe::Error> {
  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
    ..Default::default()
  };

  eframe::run_native(
    "LuaCAD Studio",
    options,
    Box::new(|_cc| Ok(Box::new(MyApp::new()))),
  )
}

#[derive(Debug, Clone)]
struct Cube {
  position: [f32; 3],
  size: [f32; 3],
}

struct MyApp {
  text_content: String,
  camera_angles: (f32, f32), // (azimuth, elevation)
  camera_distance: f32,
  cubes: Vec<Cube>,
  lua_error: Option<String>,
  orthogonal_view: bool,
}

impl MyApp {
  fn new() -> Self {
    let mut app = Self {
            text_content: "-- Welcome to LuaCAD Studio\n-- Z-axis points upward\n\ncube({0, 0, 0}, {2, 2, 2})\ncube({3, 0, 0}, {1, 1, 3})\ncube({0, 3, 1}, {1.5, 1.5, 1})".to_string(),
            camera_angles: (-30.0, 30.0), // X forward-right, Y backward-right
            camera_distance: 5.0,
            cubes: vec![],
            lua_error: None,
            orthogonal_view: true,
        };

    // Execute the initial code on startup
    app.execute_lua_code();
    app
  }

  fn draw_coordinate_axes(
    &self,
    painter: &egui::Painter,
    center: egui::Pos2,
    axis_length: f32,
  ) {
    // Apply same rotation as cube
    let azimuth = self.camera_angles.0.to_radians();
    let elevation = self.camera_angles.1.to_radians();
    let scale_factor = 25.0; // Fixed scale for axes (50% of original)

    // Define axis endpoints in 3D space (Z-up coordinate system)
    let axes_3d = [
      ([0.0, 0.0, 0.0], [axis_length, 0.0, 0.0]), // X axis
      ([0.0, 0.0, 0.0], [0.0, axis_length, 0.0]), // Y axis
      ([0.0, 0.0, 0.0], [0.0, 0.0, axis_length]), // Z axis (up)
    ];

    let colors = [
      egui::Color32::RED,   // X axis
      egui::Color32::GREEN, // Y axis
      egui::Color32::BLUE,  // Z axis
    ];

    let labels = ["X", "Y", "Z"];

    for (i, ((start, end), color)) in
      axes_3d.iter().zip(colors.iter()).enumerate()
    {
      // Transform start point (Z-up coordinate system)
      let start_x = start[0] * scale_factor;
      let start_y = start[1] * scale_factor;
      let start_z = start[2] * scale_factor;

      // Use nalgebra for proper 3D transformation
      let start_point = Point3::new(start_x, start_y, start_z);

      // Create rotation matrix: first around Z (azimuth), then around X (elevation)
      let rotation_z = Matrix4::from_euler_angles(0.0, 0.0, azimuth);
      let rotation_x = Matrix4::from_euler_angles(-elevation, 0.0, 0.0);
      let transform = rotation_x * rotation_z;

      let start_transformed = transform.transform_point(&start_point);
      let start_x_rot = start_transformed.x;
      let start_y_rot = start_transformed.y;
      let start_z_rot = start_transformed.z;

      let (start_screen_x, start_screen_y) = if self.orthogonal_view {
        // Orthogonal projection - no perspective
        (center.x + start_x_rot, center.y - start_z_rot)
      } else {
        // Perspective projection - simple focal length based
        let focal_length = 300.0; // Fixed focal length for perspective
        let depth = self.camera_distance * 50.0 - start_y_rot; // Depth from camera

        if depth > 10.0 {
          // Avoid near clipping
          let perspective_scale = focal_length / depth;
          (
            center.x + start_x_rot * perspective_scale,
            center.y - start_z_rot * perspective_scale,
          )
        } else {
          (center.x + start_x_rot, center.y - start_z_rot)
        }
      };

      // Transform end point (Z-up coordinate system)
      let end_x = end[0] * scale_factor;
      let end_y = end[1] * scale_factor;
      let end_z = end[2] * scale_factor;

      // Use nalgebra for proper 3D transformation
      let end_point = Point3::new(end_x, end_y, end_z);

      let end_transformed = transform.transform_point(&end_point);
      let end_x_rot = end_transformed.x;
      let end_y_rot = end_transformed.y;
      let end_z_rot = end_transformed.z;

      let (end_screen_x, end_screen_y) = if self.orthogonal_view {
        // Orthogonal projection - no perspective
        (center.x + end_x_rot, center.y - end_z_rot)
      } else {
        // Perspective projection - simple focal length based
        let focal_length = 300.0; // Fixed focal length for perspective
        let depth = self.camera_distance * 50.0 - end_y_rot; // Depth from camera

        if depth > 10.0 {
          // Avoid near clipping
          let perspective_scale = focal_length / depth;
          (
            center.x + end_x_rot * perspective_scale,
            center.y - end_z_rot * perspective_scale,
          )
        } else {
          (center.x + end_x_rot, center.y - end_z_rot)
        }
      };

      let start_pos = egui::Pos2::new(start_screen_x, start_screen_y);
      let end_pos = egui::Pos2::new(end_screen_x, end_screen_y);

      // Draw axis line
      painter
        .line_segment([start_pos, end_pos], egui::Stroke::new(2.0, *color));

      // Draw axis label
      painter.text(
        end_pos + egui::Vec2::new(5.0, -5.0),
        egui::Align2::LEFT_CENTER,
        labels[i],
        egui::FontId::default(),
        *color,
      );
    }
  }

  fn execute_lua_code(&mut self) {
    self.lua_error = None;
    self.cubes.clear();

    let lua = Lua::new();
    let cube_calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    let result: LuaResult<()> = (|| {
      // Create a custom print function that captures output
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

      // Create cube function that captures cube calls
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

      // Execute the user's code
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
  }

  fn draw_cubes(&self, painter: &egui::Painter, center: egui::Pos2) {
    for cube in &self.cubes {
      self.draw_single_cube(painter, center, cube);
    }
  }

  fn draw_single_cube(
    &self,
    painter: &egui::Painter,
    center: egui::Pos2,
    cube: &Cube,
  ) {
    let color = egui::Color32::from_rgb(150, 150, 255);
    let stroke = egui::Stroke::new(1.5, color);

    // Apply simple 3D projection based on camera angles
    let azimuth = self.camera_angles.0.to_radians();
    let elevation = self.camera_angles.1.to_radians();

    // Define cube vertices relative to cube position and size
    let half_size =
      [cube.size[0] / 2.0, cube.size[1] / 2.0, cube.size[2] / 2.0];
    let vertices_3d = [
      [
        cube.position[0] - half_size[0],
        cube.position[1] - half_size[1],
        cube.position[2] - half_size[2],
      ],
      [
        cube.position[0] + half_size[0],
        cube.position[1] - half_size[1],
        cube.position[2] - half_size[2],
      ],
      [
        cube.position[0] + half_size[0],
        cube.position[1] + half_size[1],
        cube.position[2] - half_size[2],
      ],
      [
        cube.position[0] - half_size[0],
        cube.position[1] + half_size[1],
        cube.position[2] - half_size[2],
      ],
      [
        cube.position[0] - half_size[0],
        cube.position[1] - half_size[1],
        cube.position[2] + half_size[2],
      ],
      [
        cube.position[0] + half_size[0],
        cube.position[1] - half_size[1],
        cube.position[2] + half_size[2],
      ],
      [
        cube.position[0] + half_size[0],
        cube.position[1] + half_size[1],
        cube.position[2] + half_size[2],
      ],
      [
        cube.position[0] - half_size[0],
        cube.position[1] + half_size[1],
        cube.position[2] + half_size[2],
      ],
    ];

    // Project 3D vertices to 2D (Z-up coordinate system)
    let mut vertices_2d = Vec::new();
    let scale_factor = 100.0 / self.camera_distance; // Use camera distance for zoom
    for vertex in &vertices_3d {
      let x = vertex[0] * scale_factor;
      let y = vertex[1] * scale_factor;
      let z = vertex[2] * scale_factor;

      // Use nalgebra for proper 3D transformation
      let point = Point3::new(x, y, z);

      // Create rotation matrix: first around Z (azimuth), then around X (elevation)
      let rotation_z = Matrix4::from_euler_angles(0.0, 0.0, azimuth);
      let rotation_x = Matrix4::from_euler_angles(-elevation, 0.0, 0.0);
      let transform = rotation_x * rotation_z;

      let transformed = transform.transform_point(&point);
      let x_rot = transformed.x;
      let y_rot = transformed.y;
      let z_rot = transformed.z;

      // Apply projection (orthogonal or perspective)
      let (screen_x, screen_y) = if self.orthogonal_view {
        // Orthogonal projection - no perspective
        (center.x + x_rot, center.y - z_rot)
      } else {
        // Perspective projection - simple focal length based
        let focal_length = 300.0; // Fixed focal length for perspective
        let depth = self.camera_distance * 50.0 - y_rot; // Depth from camera (y_rot is toward/away from camera)

        if depth > 10.0 {
          // Avoid near clipping
          let perspective_scale = focal_length / depth;
          (
            center.x + x_rot * perspective_scale,
            center.y - z_rot * perspective_scale,
          )
        } else {
          (center.x + x_rot, center.y - z_rot)
        }
      };

      vertices_2d.push(egui::Pos2::new(screen_x, screen_y));
    }

    // Draw cube edges
    let edges = [
      // Front face
      (0, 1),
      (1, 2),
      (2, 3),
      (3, 0),
      // Back face
      (4, 5),
      (5, 6),
      (6, 7),
      (7, 4),
      // Connecting edges
      (0, 4),
      (1, 5),
      (2, 6),
      (3, 7),
    ];

    for (i, j) in edges {
      painter.line_segment([vertices_2d[i], vertices_2d[j]], stroke);
    }
  }
}

impl eframe::App for MyApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    egui::CentralPanel::default().show(ctx, |ui| {
            let total_height = ui.available_height();
            let total_width = ui.available_width();

            ui.horizontal(|ui| {
                // Left panel - 3D viewer
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(total_width * 0.5, total_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.heading("3D Viewer");

                        let viewer_height = ui.available_height() - 120.0; // More space for controls at bottom
                        let (rect, response) = ui.allocate_exact_size(
                            egui::Vec2::new(ui.available_width(), viewer_height),
                            egui::Sense::drag(),
                        );

                        // Handle camera controls
                        if response.dragged() {
                            let delta = response.drag_delta();
                            self.camera_angles.0 -= delta.x * 0.5; // Invert horizontal rotation
                            self.camera_angles.1 = (self.camera_angles.1 + delta.y * 0.5)
                                .clamp(-85.0, 85.0); // Limit elevation to prevent gimbal lock
                        }

                        if response.hovered() {
                            let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                            if scroll_delta != 0.0 {
                                self.camera_distance = (self.camera_distance - scroll_delta * 0.1)
                                    .clamp(1.0, 20.0);
                            }
                        }

                        // Render 3D scene background
                        ui.painter().rect_filled(
                            rect,
                            egui::Rounding::same(4.0),
                            egui::Color32::from_rgb(30, 30, 40),
                        );

                        // Clip all 3D content to the viewer rectangle
                        let clipped_painter = ui.painter().with_clip_rect(rect);

                        // Draw cubes from Lua execution
                        let center = rect.center();
                        self.draw_cubes(&clipped_painter, center);

                        // Draw coordinate axes in the 3D view (half length)
                        self.draw_coordinate_axes(&clipped_painter, center, 5.0);

                        // Calculate rotation values for gizmo
                        let azimuth = self.camera_angles.0.to_radians();
                        let elevation = self.camera_angles.1.to_radians();
                        let cos_az = azimuth.cos();
                        let sin_az = azimuth.sin();
                        let cos_el = elevation.cos();

                        // View gizmo in upper right corner
                        let gizmo_center = rect.right_top() + egui::Vec2::new(-50.0, 50.0);
                        let gizmo_size = 30.0;

                        // Gizmo background circle
                        ui.painter().circle_filled(
                            gizmo_center,
                            gizmo_size,
                            egui::Color32::from_rgba_unmultiplied(50, 50, 50, 180),
                        );
                        ui.painter().circle_stroke(
                            gizmo_center,
                            gizmo_size,
                            egui::Stroke::new(1.0, egui::Color32::GRAY),
                        );

                        // Gizmo axes (same rotation as main view)
                        let gizmo_axis_length = 20.0;

                        // X axis gizmo (Z-up coordinate system)
                        let gizmo_x_end = gizmo_center + egui::Vec2::new(
                            gizmo_axis_length * cos_az * 0.8,
                            0.0,
                        );
                        ui.painter().line_segment(
                            [gizmo_center, gizmo_x_end],
                            egui::Stroke::new(2.0, egui::Color32::RED),
                        );
                        ui.painter().text(
                            gizmo_x_end + egui::Vec2::new(3.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            "X",
                            egui::FontId::monospace(10.0),
                            egui::Color32::RED,
                        );

                        // Y axis gizmo (Z-up coordinate system)
                        let gizmo_y_end = gizmo_center + egui::Vec2::new(
                            -gizmo_axis_length * sin_az * 0.8,
                            0.0,
                        );
                        ui.painter().line_segment(
                            [gizmo_center, gizmo_y_end],
                            egui::Stroke::new(2.0, egui::Color32::GREEN),
                        );
                        ui.painter().text(
                            gizmo_y_end + egui::Vec2::new(0.0, -5.0),
                            egui::Align2::CENTER_BOTTOM,
                            "Y",
                            egui::FontId::monospace(10.0),
                            egui::Color32::GREEN,
                        );

                        // Z axis gizmo (Z-up coordinate system)
                        let gizmo_z_end = gizmo_center + egui::Vec2::new(
                            0.0,
                            -gizmo_axis_length * cos_el * 0.8,
                        );
                        ui.painter().line_segment(
                            [gizmo_center, gizmo_z_end],
                            egui::Stroke::new(2.0, egui::Color32::BLUE),
                        );
                        ui.painter().text(
                            gizmo_z_end + egui::Vec2::new(3.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            "Z",
                            egui::FontId::monospace(10.0),
                            egui::Color32::BLUE,
                        );

                        // Camera controls info
                        ui.separator();
                        ui.label("Mouse: Drag to rotate, Scroll to zoom");
                        ui.label(format!("Azimuth: {:.1}°, Elevation: {:.1}°",
                            self.camera_angles.0, self.camera_angles.1));
                        ui.label(format!("Distance: {:.1}", self.camera_distance));

                        // Projection and view controls
                        ui.separator();
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Projection:");
                            if ui.selectable_label(self.orthogonal_view, "Orthogonal").clicked() {
                                self.orthogonal_view = true;
                            }
                            if ui.selectable_label(!self.orthogonal_view, "Perspective").clicked() {
                                self.orthogonal_view = false;
                            }

                            ui.separator();
                            ui.label("Views:");

                            // Check current view to highlight the selected one
                            let current_angles = self.camera_angles;
                            let is_default = (current_angles.0 - (-30.0)).abs() < 1.0 && (current_angles.1 - 30.0).abs() < 1.0;
                            let is_top = (current_angles.0 - 0.0).abs() < 1.0 && (current_angles.1 - 90.0).abs() < 1.0;
                            let is_bottom = (current_angles.0 - 0.0).abs() < 1.0 && (current_angles.1 - (-90.0)).abs() < 1.0;
                            let is_front = (current_angles.0 - 0.0).abs() < 1.0 && (current_angles.1 - 0.0).abs() < 1.0;
                            let is_back = (current_angles.0 - 180.0).abs() < 1.0 && (current_angles.1 - 0.0).abs() < 1.0;
                            let is_left = (current_angles.0 - (-90.0)).abs() < 1.0 && (current_angles.1 - 0.0).abs() < 1.0;
                            let is_right = (current_angles.0 - 90.0).abs() < 1.0 && (current_angles.1 - 0.0).abs() < 1.0;

                            if ui.selectable_label(is_default, "Default").clicked() {
                                self.camera_angles = (-30.0, 30.0);
                            }
                            if ui.selectable_label(is_top, "Top").clicked() {
                                self.camera_angles = (0.0, 90.0);
                            }
                            if ui.selectable_label(is_bottom, "Bottom").clicked() {
                                self.camera_angles = (0.0, -90.0);
                            }
                            if ui.selectable_label(is_front, "Front").clicked() {
                                self.camera_angles = (0.0, 0.0);
                            }
                            if ui.selectable_label(is_back, "Back").clicked() {
                                self.camera_angles = (180.0, 0.0);
                            }
                            if ui.selectable_label(is_left, "Left").clicked() {
                                self.camera_angles = (-90.0, 0.0);
                            }
                            if ui.selectable_label(is_right, "Right").clicked() {
                                self.camera_angles = (90.0, 0.0);
                            }
                        });
                    }
                );

                ui.separator();

                // Right panel - Text editor
                ui.allocate_ui_with_layout(
                    egui::Vec2::new(ui.available_width(), total_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.heading("Code Editor");

                        let editor_height = ui.available_height() - 60.0; // Space for controls at bottom
                        egui::ScrollArea::vertical()
                            .max_height(editor_height)
                            .show(ui, |ui| {
                                let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
                                    // Choose theme based on current UI style
                                    let theme = if ui.style().visuals.dark_mode {
                                        syntax_highlighting::CodeTheme::dark(14.0)
                                    } else {
                                        syntax_highlighting::CodeTheme::light(14.0)
                                    };

                                    let mut layout_job = syntax_highlighting::highlight(
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
                                    egui::TextEdit::multiline(&mut self.text_content)
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
                                self.execute_lua_code();
                            }

                            if ui.button("Clear").clicked() {
                                self.text_content.clear();
                                self.cubes.clear();
                                self.lua_error = None;
                            }

                            if ui.button("Load Example").clicked() {
                                self.text_content = "-- LuaCAD Example\n-- Create cubes with position and size\ncube({0, 0, 0}, {2, 2, 2})\ncube({3, 0, 0}, {1, 3, 1})\ncube({-2, 2, 1}, {1.5, 1, 2})\n\n-- You can also use variables\nlocal pos = {1, -2, 0}\nlocal size = {1, 1, 1}\ncube(pos, size)".to_string();
                            }
                        });

                        ui.label(format!("Lines: {}", self.text_content.lines().count()));
                        ui.label(format!("Characters: {}", self.text_content.len()));

                        // Show error messages if any
                        if let Some(error) = &self.lua_error {
                            ui.separator();
                            ui.colored_label(egui::Color32::RED, format!("Error: {error}"));
                        }

                        // Show cube count
                        if !self.cubes.is_empty() {
                            ui.separator();
                            ui.colored_label(egui::Color32::GREEN, format!("✓ {} cube(s) rendered", self.cubes.len()));
                        }
                    }
                );
            });
        });
  }
}
