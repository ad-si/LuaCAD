mod app;
mod csg_tree;
mod editor;
mod gl_context;
mod scene;
mod theme;
mod ui;

use app::{AppState, FileAction};
use editor::EditorAction;
use luacad::export::ExportFormat;
use luacad::scad_export;
use scene::{
  build_camera, camera_projection_matrix, camera_view_matrix,
  compute_camera_vectors, compute_fit_distance, gl_clear_screen,
  gl_set_viewport, render_axes, render_opencsg_scene,
};
use theme::ThemeMode;
use three_d::*;
use ui::render_ui;

/// Convert an egui CursorIcon to a winit CursorIcon.
fn egui_to_winit_cursor(cursor: egui::CursorIcon) -> winit::window::CursorIcon {
  match cursor {
    egui::CursorIcon::Default => winit::window::CursorIcon::Default,
    egui::CursorIcon::PointingHand => winit::window::CursorIcon::Hand,
    egui::CursorIcon::Text => winit::window::CursorIcon::Text,
    egui::CursorIcon::Crosshair => winit::window::CursorIcon::Crosshair,
    egui::CursorIcon::Grab => winit::window::CursorIcon::Grab,
    egui::CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
    egui::CursorIcon::Move => winit::window::CursorIcon::Move,
    egui::CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
    egui::CursorIcon::Wait => winit::window::CursorIcon::Wait,
    egui::CursorIcon::Progress => winit::window::CursorIcon::Progress,
    egui::CursorIcon::Help => winit::window::CursorIcon::Help,
    egui::CursorIcon::ResizeHorizontal => winit::window::CursorIcon::EwResize,
    egui::CursorIcon::ResizeVertical => winit::window::CursorIcon::NsResize,
    egui::CursorIcon::ResizeNeSw => winit::window::CursorIcon::NeswResize,
    egui::CursorIcon::ResizeNwSe => winit::window::CursorIcon::NwseResize,
    egui::CursorIcon::ResizeEast => winit::window::CursorIcon::EResize,
    egui::CursorIcon::ResizeWest => winit::window::CursorIcon::WResize,
    egui::CursorIcon::ResizeNorth => winit::window::CursorIcon::NResize,
    egui::CursorIcon::ResizeSouth => winit::window::CursorIcon::SResize,
    egui::CursorIcon::ZoomIn => winit::window::CursorIcon::ZoomIn,
    egui::CursorIcon::ZoomOut => winit::window::CursorIcon::ZoomOut,
    _ => winit::window::CursorIcon::Default,
  }
}

fn main() {
  let event_loop = winit::event_loop::EventLoop::new();
  let winit_window = winit::window::WindowBuilder::new()
    .with_title("LuaCAD Studio")
    .with_maximized(true)
    .build(&event_loop)
    .unwrap();
  winit_window.focus_window();

  // Create GL context with Compatibility/Legacy profile (required by OpenCSG)
  let gl = gl_context::GlWindowContext::new(&winit_window, 8);
  let context: Context = (*gl).clone();
  let mut gui = three_d::GUI::new(&context);
  let mut app = AppState::new();

  // Initialize OpenCSG's GLAD loader
  opencsg_sys::init_gl();

  // Auto-zoom to fit initial geometry
  if let Some(dist) = compute_fit_distance(&app.geometries, app.orthogonal_view)
  {
    app.camera_distance = dist;
  }
  app.needs_fit_to_view = false;
  app.scene_dirty = false;

  let initial_viewport = {
    let (w, h): (u32, u32) = winit_window.inner_size().into();
    Viewport::new_at_origo(w, h)
  };
  let mut camera = build_camera(initial_viewport, &app);
  let mut last_theme_check = 0.0_f64;

  let mut dragging_scene = false;
  let mut clipboard = arboard::Clipboard::new().ok();
  let mut frame_input_generator =
    FrameInputGenerator::from_winit_window(&winit_window);

  event_loop.run(move |event, _, control_flow| match event {
    winit::event::Event::MainEventsCleared => {
      winit_window.request_redraw();
    }
    winit::event::Event::RedrawRequested(_) => {
    // Update window title to reflect the current file
    let window_title = match &app.current_file {
      Some(path) => format!(
        "{} — LuaCAD Studio",
        path.file_name().unwrap_or_default().to_string_lossy()
      ),
      None => "LuaCAD Studio".to_string(),
    };
    winit_window.set_title(&window_title);

    let mut frame_input = frame_input_generator.generate(&gl);

    // Clear export status on any user interaction
    if app.export_status.is_some() {
      let has_interaction = frame_input.events.iter().any(|e| {
        matches!(
          e,
          Event::KeyPress { .. }
            | Event::MousePress { .. }
            | Event::MouseWheel { .. }
            | Event::Text(_)
        )
      });
      if has_interaction {
        app.export_status = None;
      }
    }

    // Detect clipboard key combos (Cmd+V/C/X) that three-d doesn't forward to egui
    let mut paste_text: Option<String> = None;
    let mut wants_copy = false;
    let mut wants_cut = false;
    let mut consume_tab = false;
    for event in frame_input.events.iter() {
      if let Event::KeyPress {
        kind, modifiers, ..
      } = event
      {
        if modifiers.command {
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
            Key::Enter => {
              app.execute_lua_code();
            }
            Key::Num4 => {
              app.camera_azimuth = -90.0;
              app.camera_elevation = 89.0;
            }
            Key::Num5 => {
              app.camera_azimuth = -90.0;
              app.camera_elevation = -89.0;
            }
            Key::Num6 => {
              app.camera_azimuth = 180.0;
              app.camera_elevation = 0.0;
            }
            Key::Num7 => {
              app.camera_azimuth = 0.0;
              app.camera_elevation = 0.0;
            }
            Key::Num8 => {
              app.camera_azimuth = -90.0;
              app.camera_elevation = 0.0;
            }
            Key::Num9 => {
              app.camera_azimuth = 90.0;
              app.camera_elevation = 0.0;
            }
            Key::Num0 => {
              app.camera_azimuth = -30.0;
              app.camera_elevation = 30.0;
            }
            _ => {}
          }
        } else if *kind == Key::Tab {
          if modifiers.shift {
            app.pending_editor_action = Some(EditorAction::Unindent);
          } else {
            app.pending_editor_action = Some(EditorAction::InsertTab);
          }
          consume_tab = true;
        }
      }
    }
    // Remove Tab events so egui doesn't also insert a \t
    if consume_tab {
      frame_input.events.retain(|e| {
        !matches!(e, Event::KeyPress { kind: Key::Tab, .. } | Event::KeyRelease { kind: Key::Tab, .. })
      });
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
        let vp = camera.viewport();
        let ex = px.x as f32 / dpr;
        let ey = (vp.height as f32 - px.y as f32) / dpr;
        result[i] = (egui::Pos2::new(ex, ey), labels[i], colors[i]);
      }
      result
    };

    // Process GUI (consumes events over egui panels)
    let mut panel_width = 0.0_f32;
    let mut egui_cursor = egui::CursorIcon::Default;
    let mut copied_text = String::new();
    gui.update(
      &mut frame_input.events,
      frame_input.accumulated_time,
      frame_input.viewport,
      frame_input.device_pixel_ratio,
      |gui_context| {
        if let Some(text) = &paste_text {
          if app.clipboard_is_line {
            app.pending_editor_action = Some(EditorAction::PasteLineAbove(text.clone()));
          } else {
            gui_context.input_mut(|i| {
              i.events.push(egui::Event::Paste(text.clone()));
            });
          }
        }
        if wants_copy {
          if app.editor_selection_len == 0 {
            let text = &app.text_content;
            let cursor = app.editor_cursor_pos.min(text.len());
            let line_start = text[..cursor].rfind('\n').map_or(0, |p| p + 1);
            let line_end = text[cursor..].find('\n').map_or(text.len(), |p| cursor + p + 1);
            let line = &text[line_start..line_end];
            if let Some(cb) = clipboard.as_mut() {
              let _ = cb.set_text(line.to_string());
            }
            app.clipboard_is_line = true;
          } else {
            gui_context.input_mut(|i| i.events.push(egui::Event::Copy));
            app.clipboard_is_line = false;
          }
        }
        if wants_cut {
          gui_context.input_mut(|i| i.events.push(egui::Event::Cut));
        }

        panel_width = render_ui(gui_context, &mut app);

        // Draw axis labels as overlay
        let screen_width = gui_context.screen_rect().width();
        let scene_right = screen_width - panel_width;
        let painter = gui_context.layer_painter(egui::LayerId::new(
          egui::Order::Foreground,
          egui::Id::new("axis_labels"),
        ));
        for (pos, label, color) in &axis_labels {
          if pos.x < scene_right {
            painter.text(
              *pos,
              egui::Align2::CENTER_CENTER,
              label,
              egui::FontId::proportional(14.0),
              *color,
            );
          }
        }

        egui_cursor = gui_context.output(|o| o.cursor_icon);
        gui_context.output_mut(|o| {
          if !o.copied_text.is_empty() {
            copied_text = std::mem::take(&mut o.copied_text);
          }
        });
      },
    );

    // Handle copy/cut output from egui → system clipboard
    if !copied_text.is_empty()
      && let Some(cb) = clipboard.as_mut() {
        let _ = cb.set_text(copied_text);
      }

    // Handle export requests
    if let Some(fmt) = app.pending_export.take() {
      let (title, filter_name, ext, default_name) = match fmt {
        ExportFormat::ThreeMF => ("Export 3MF", "3MF Files", "3mf", "model.3mf"),
        ExportFormat::STL => ("Export STL", "STL Files", "stl", "model.stl"),
        ExportFormat::OBJ => ("Export OBJ", "OBJ Files", "obj", "model.obj"),
        ExportFormat::PLY => ("Export PLY", "PLY Files", "ply", "model.ply"),
        ExportFormat::OpenSCAD => ("Export OpenSCAD", "OpenSCAD Files", "scad", "model.scad"),
      };
      if let Some(path) = rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter_name, &[ext])
        .set_file_name(default_name)
        .save_file()
      {
        let result = match fmt {
          ExportFormat::ThreeMF => luacad::export::export_3mf(&app.geometries, &path),
          ExportFormat::STL => luacad::export::export_stl(&app.geometries, &path),
          ExportFormat::OBJ => luacad::export::export_obj(&app.geometries, &path),
          ExportFormat::PLY => luacad::export::export_ply(&app.geometries, &path),
          ExportFormat::OpenSCAD => {
            let nodes: Vec<_> = app.geometries.iter().filter_map(|g| g.scad.clone()).collect();
            scad_export::export_scad(&nodes, &path)
          }
        };
        match result {
          Ok(()) => app.export_status = Some((format!("Exported to {}", path.display()), false)),
          Err(e) => app.export_status = Some((format!("Export failed: {e}"), true)),
        }
      }
    }

    // Handle OpenSCAD-based export
    if let Some(fmt) = app.pending_openscad_export.take() {
      let nodes: Vec<_> = app.geometries.iter().filter_map(|g| g.scad.clone()).collect();
      if nodes.is_empty() {
        app.export_status = Some(("No SCAD geometry to export".to_string(), true));
      } else if let Some(path) = rfd::FileDialog::new()
        .set_title(format!("Export via OpenSCAD — {}", fmt.label()))
        .add_filter(fmt.filter_name(), &[fmt.extension()])
        .set_file_name(fmt.default_filename())
        .save_file()
      {
        let scad_source = scad_export::generate_scad(&nodes);
        let tmp_dir = std::env::temp_dir().join("luacad_openscad");
        let _ = std::fs::create_dir_all(&tmp_dir);
        let tmp_scad = tmp_dir.join("export_temp.scad");
        match std::fs::write(&tmp_scad, &scad_source) {
          Ok(()) => {
            match std::process::Command::new("openscad").arg("-o").arg(&path).arg(&tmp_scad).output() {
              Ok(output) if output.status.success() => {
                app.export_status = Some((format!("Exported via OpenSCAD to {}", path.display()), false));
              }
              Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                app.export_status = Some((format!("OpenSCAD failed: {}", stderr.trim()), true));
              }
              Err(e) => {
                app.export_status = Some((format!("Failed to run OpenSCAD: {e}. Is OpenSCAD installed and in PATH?"), true));
              }
            }
          }
          Err(e) => app.export_status = Some((format!("Failed to write temp SCAD file: {e}"), true)),
        }
      }
    }

    // Handle file open/save requests
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
                app.execute_lua_code();
                app.scene_dirty = true;
                app.needs_fit_to_view = true;
              }
              Err(e) => app.export_status = Some((format!("Failed to open: {e}"), true)),
            }
          }
        }
        FileAction::Save => {
          if let Some(path) = app.current_file.clone() {
            match std::fs::write(&path, &app.text_content) {
              Ok(()) => app.export_status = Some((format!("Saved to {}", path.display()), false)),
              Err(e) => app.export_status = Some((format!("Failed to save: {e}"), true)),
            }
          } else if let Some(path) = rfd::FileDialog::new()
            .set_title("Save Lua File")
            .add_filter("Lua Files", &["lua"])
            .set_file_name("untitled.lua")
            .save_file()
          {
            match std::fs::write(&path, &app.text_content) {
              Ok(()) => {
                app.current_file = Some(path.clone());
                app.export_status = Some((format!("Saved to {}", path.display()), false));
              }
              Err(e) => app.export_status = Some((format!("Failed to save: {e}"), true)),
            }
          }
        }
        FileAction::SaveAs => {
          let default_name = app.current_file.as_ref()
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
                app.export_status = Some((format!("Saved to {}", path.display()), false));
              }
              Err(e) => app.export_status = Some((format!("Failed to save: {e}"), true)),
            }
          }
        }
      }
    }

    // Compute 3D viewport: left area excluding right panel
    let full = frame_input.viewport;
    let panel_px = (panel_width * frame_input.device_pixel_ratio).round() as u32;
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
    for event in frame_input.events.iter() {
      match event {
        Event::MousePress {
          button: MouseButton::Left, position, handled, ..
        } if !handled && position.x < scene_max_x => {
          dragging_scene = true;
        }
        Event::MouseRelease { button: MouseButton::Left, .. } => {
          dragging_scene = false;
        }
        Event::MouseMotion { delta, button: Some(MouseButton::Left), .. } if dragging_scene => {
          app.camera_azimuth -= delta.0 * 0.5;
          app.camera_elevation = (app.camera_elevation + delta.1 * 0.5).clamp(-85.0, 85.0);
        }
        Event::MouseWheel { delta, handled, position, .. }
          if !handled && position.x < scene_max_x =>
        {
          let zoom_factor = (-delta.1 * 0.01).exp();
          app.camera_distance = (app.camera_distance * zoom_factor).clamp(0.001, 10_000.0);
        }
        _ => {}
      }
    }

    // Handle scene rebuild on Lua re-execution
    if app.scene_dirty {
      if app.needs_fit_to_view {
        if let Some(dist) = compute_fit_distance(&app.geometries, app.orthogonal_view) {
          app.camera_distance = dist;
        }
        app.needs_fit_to_view = false;
      }
      app.scene_dirty = false;
    }

    // Update camera
    let (pos, target, up) = compute_camera_vectors(&app);
    camera.set_view(pos, target, up);
    if app.orthogonal_view {
      camera.set_orthographic_projection(2.0, -100.0 * app.camera_distance, 100.0 * app.camera_distance);
    } else {
      camera.set_perspective_projection(degrees(45.0), 0.1 * app.camera_distance, 100.0 * app.camera_distance);
    }

    // Re-check system theme
    if app.theme_mode == ThemeMode::System && frame_input.accumulated_time - last_theme_check > 2000.0 {
      last_theme_check = frame_input.accumulated_time;
      app.theme_colors = app.resolve_theme();
    }

    // --- Render ---
    let (bg_r, bg_g, bg_b) = app.theme_colors.bg;

    // Set GL viewport to 3D scene area and clear
    gl_set_viewport(
      scene_viewport.x, scene_viewport.y,
      scene_viewport.width as i32, scene_viewport.height as i32,
    );
    gl_clear_screen(bg_r, bg_g, bg_b);

    // Extract camera matrices for legacy GL
    let proj = camera_projection_matrix(&camera);
    let view = camera_view_matrix(&camera);

    // Render CSG scene via OpenCSG + fixed-function shading
    render_opencsg_scene(&app.csg_groups, &proj, &view);

    // Render 3D axes
    render_axes();

    // Restore full viewport for GUI overlay
    gl_set_viewport(full.x, full.y, full.width as i32, full.height as i32);

    // Render egui overlay
    let screen = frame_input.screen();
    screen.write(|| gui.render()).unwrap();

    winit_window.set_cursor_icon(egui_to_winit_cursor(egui_cursor));
    gl.swap_buffers();
    *control_flow = winit::event_loop::ControlFlow::Poll;
    winit_window.request_redraw();
    }
    winit::event::Event::WindowEvent { ref event, .. } => {
      frame_input_generator.handle_winit_window_event(event);
      match event {
        winit::event::WindowEvent::Resized(physical_size) => gl.resize(*physical_size),
        winit::event::WindowEvent::ScaleFactorChanged { new_inner_size, .. } => gl.resize(**new_inner_size),
        winit::event::WindowEvent::CloseRequested => *control_flow = winit::event_loop::ControlFlow::Exit,
        winit::event::WindowEvent::DroppedFile(path) => {
          if path.extension().and_then(|e| e.to_str()) == Some("lua") {
            match std::fs::read_to_string(path) {
              Ok(contents) => {
                app.text_content = contents;
                app.current_file = Some(path.clone());
                app.execute_lua_code();
                app.scene_dirty = true;
                app.needs_fit_to_view = true;
              }
              Err(e) => app.export_status = Some((format!("Failed to open: {e}"), true)),
            }
          } else {
            app.export_status = Some(("Only .lua files can be opened".to_string(), true));
          }
        }
        _ => {}
      }
    }
    _ => {}
  });
}
