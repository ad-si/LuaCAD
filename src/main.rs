mod app;
mod editor;
mod export;
mod geometry;
mod lua_engine;
mod scene;
mod theme;
mod ui;

use app::{AppState, FileAction};
use editor::EditorAction;
use export::ExportFormat;
use scene::{build_camera, build_scene, compute_camera_vectors};
use theme::ThemeMode;
use three_d::*;
use ui::render_ui;

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
          Key::S => {
            app.pending_file_action = Some(FileAction::Save);
          }
          Key::O => {
            app.pending_file_action = Some(FileAction::Open);
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
          ExportFormat::ThreeMF => export::export_3mf(&app.geometries, &path),
          ExportFormat::STL => export::export_stl(&app.geometries, &path),
          ExportFormat::OBJ => export::export_obj(&app.geometries, &path),
          ExportFormat::PLY => export::export_ply(&app.geometries, &path),
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

    // Handle file open/save requests (outside egui context so file dialog works)
    if let Some(action) = app.pending_file_action.take() {
      match action {
        FileAction::Open => {
          if let Some(path) = rfd::FileDialog::new()
            .set_title("Open Lua File")
            .add_filter("Lua Files", &["lua"])
            .pick_file()
          {
            match std::fs::read_to_string(&path) {
              Ok(contents) => {
                app.text_content = contents;
                app.current_file = Some(path);
                app.geometries.clear();
                app.lua_error = None;
                app.scene_dirty = true;
              }
              Err(e) => {
                app.export_status =
                  Some((format!("Failed to open: {e}"), true));
              }
            }
          }
        }
        FileAction::Save => {
          if let Some(path) = app.current_file.clone() {
            match std::fs::write(&path, &app.text_content) {
              Ok(()) => {
                app.export_status =
                  Some((format!("Saved to {}", path.display()), false));
              }
              Err(e) => {
                app.export_status =
                  Some((format!("Failed to save: {e}"), true));
              }
            }
          } else {
            // No current file — behave like Save As
            if let Some(path) = rfd::FileDialog::new()
              .set_title("Save Lua File")
              .add_filter("Lua Files", &["lua"])
              .set_file_name("untitled.lua")
              .save_file()
            {
              match std::fs::write(&path, &app.text_content) {
                Ok(()) => {
                  app.current_file = Some(path.clone());
                  app.export_status =
                    Some((format!("Saved to {}", path.display()), false));
                }
                Err(e) => {
                  app.export_status =
                    Some((format!("Failed to save: {e}"), true));
                }
              }
            }
          }
        }
        FileAction::SaveAs => {
          let default_name = app
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled.lua".to_string());
          if let Some(path) = rfd::FileDialog::new()
            .set_title("Save Lua File As")
            .add_filter("Lua Files", &["lua"])
            .set_file_name(&default_name)
            .save_file()
          {
            match std::fs::write(&path, &app.text_content) {
              Ok(()) => {
                app.current_file = Some(path.clone());
                app.export_status =
                  Some((format!("Saved to {}", path.display()), false));
              }
              Err(e) => {
                app.export_status =
                  Some((format!("Failed to save: {e}"), true));
              }
            }
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
