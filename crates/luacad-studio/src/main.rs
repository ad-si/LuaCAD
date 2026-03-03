mod app;
mod camera;
mod csg_tree;
mod editor;
mod egui_integration;
mod gl_context;
mod input;
mod scene;
mod theme;
mod ui;

use app::{AppState, FileAction};
use camera::{Viewport, degrees, vec3};
use editor::EditorAction;
use egui_integration::EguiIntegration;
use input::{Event, FrameInputGenerator, Key, MouseButton, PhysicalPoint};
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
#[cfg(feature = "csgrs")]
use luacad::scad_export;
use scene::{
  SceneFbo, build_camera, camera_projection_matrix, camera_view_matrix,
  compute_camera_vectors, compute_fit_distance, gl_clear_screen,
  gl_set_viewport, render_axes, render_opencsg_scene,
};
use theme::ThemeMode;
use ui::{PanelLayout, render_ui};

use std::path::{Path, PathBuf};

/// Return the path to the state file (`~/.config/luacad/state.json`).
fn state_file_path() -> Option<PathBuf> {
  dirs::config_dir().map(|d| d.join("luacad").join("state.json"))
}

/// Persist the last opened file path (or clear it).
fn save_last_file(path: Option<&Path>) {
  let Some(state_path) = state_file_path() else {
    return;
  };
  if let Some(parent) = state_path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let json = match path {
    Some(p) => serde_json::json!({ "last_file": p }),
    None => serde_json::json!({}),
  };
  let _ = std::fs::write(&state_path, json.to_string());
}

/// Load the last opened file path from state, if the file still exists.
fn load_last_file() -> Option<PathBuf> {
  let state_path = state_file_path()?;
  let data = std::fs::read_to_string(state_path).ok()?;
  let obj: serde_json::Value = serde_json::from_str(&data).ok()?;
  let path_str = obj.get("last_file")?.as_str()?;
  let path = PathBuf::from(path_str);
  if path.exists() { Some(path) } else { None }
}

/// Generate a timestamped default filename for export, e.g. `2026-03-01t2051_model.3mf`.
fn timestamped_filename(ext: &str) -> String {
  let now = time::OffsetDateTime::now_local()
    .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
  let format =
    time::format_description::parse("[year]-[month]-[day]t[hour][minute]")
      .expect("valid format description");
  let stamp = now.format(&format).expect("format timestamp");
  format!("{stamp}_model.{ext}")
}

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
  // Resolve initial file: CLI argument takes priority, then last opened file.
  let initial_file = std::env::args()
    .nth(1)
    .map(|arg| {
      let path = PathBuf::from(&arg);
      // Canonicalize relative paths so the state file stores absolute paths
      std::fs::canonicalize(&path).unwrap_or(path)
    })
    .or_else(load_last_file);

  let event_loop = winit::event_loop::EventLoop::new();
  let winit_window = winit::window::WindowBuilder::new()
    .with_title("LuaCAD Studio")
    .with_maximized(true)
    .build(&event_loop)
    .unwrap();
  winit_window.focus_window();

  // Create GL context with Compatibility/Legacy profile (required by OpenCSG)
  let gl = gl_context::GlWindowContext::new(&winit_window, 8);
  let mut gui = EguiIntegration::new(gl.gl.clone());
  let mut app = AppState::new(initial_file);

  // Persist the initial file if it was loaded successfully
  if let Some(ref path) = app.current_file {
    save_last_file(Some(path));
  }

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

  let mut scene_fbo =
    SceneFbo::new(initial_viewport.width, initial_viewport.height);
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

      let mut frame_input = frame_input_generator.generate();

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

      // Detect clipboard key combos (Cmd+V/C/X) before egui processes them
      let mut paste_text: Option<String> = None;
      let mut wants_copy = false;
      let mut wants_cut = false;
      let mut consume_tab = false;
      let mut consume_escape = false;
      let mut consume_ctrl_key: Option<Key> = None;
      let mut consume_cmd_keys: Vec<Key> = Vec::new();
      for event in frame_input.events.iter() {
        if let Event::KeyPress {
          kind, modifiers, ..
        } = event
        {
          if modifiers.ctrl && !modifiers.command {
            match kind {
              Key::D => {
                app.pending_editor_action = Some(EditorAction::DeleteCharRight);
                consume_ctrl_key = Some(Key::D);
              }
              Key::W => {
                app.pending_editor_action = Some(EditorAction::DeleteWordLeft);
                consume_ctrl_key = Some(Key::W);
              }
              _ => {}
            }
          } else if modifiers.command {
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
              Key::Slash => {
                app.pending_editor_action = Some(EditorAction::ToggleComment);
              }
              Key::N => {
                app.pending_file_action = Some(FileAction::New);
              }
              Key::S => {
                app.pending_file_action = Some(FileAction::Save);
              }
              Key::F => {
                if app.search.open {
                  app.search.focus_search_field = true;
                } else {
                  app.search.open = true;
                  app.search.focus_search_field = true;
                  // Pre-fill query from selection
                  if app.editor_selection_len > 0 {
                    let chars: Vec<char> = app.text_content.chars().collect();
                    let end = app.editor_cursor_pos.min(chars.len());
                    let start = end.saturating_sub(app.editor_selection_len);
                    let byte_start: usize =
                      chars[..start].iter().collect::<String>().len();
                    let byte_end: usize =
                      chars[..end].iter().collect::<String>().len();
                    if byte_end <= app.text_content.len() {
                      app.search.query =
                        app.text_content[byte_start..byte_end].to_string();
                    }
                  }
                }
                consume_cmd_keys.push(Key::F);
              }
              Key::H => {
                app.search.open = true;
                app.search.show_replace = true;
                app.search.focus_search_field = true;
                consume_cmd_keys.push(Key::H);
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
          } else if *kind == Key::Escape && app.search.open {
            app.search.open = false;
            app.search.matches.clear();
            app.search.current_match = None;
            app.search.last_computed = Default::default();
            consume_escape = true;
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
      // Wrap selection with brackets when typing (, [, or { while text is selected
      let mut consume_bracket_text: Option<String> = None;
      if app.editor_selection_len > 0 && app.pending_editor_action.is_none() {
        for event in frame_input.events.iter() {
          if let Event::Text(s) = event {
            let ch = s.chars().next();
            if matches!(ch, Some('(' | '[' | '{')) {
              app.pending_editor_action =
                Some(EditorAction::WrapSelection(ch.unwrap()));
              consume_bracket_text = Some(s.clone());
              break;
            }
          }
        }
      }

      // Remove consumed events so egui doesn't also process them
      if let Some(ref bracket) = consume_bracket_text {
        frame_input
          .events
          .retain(|e| !matches!(e, Event::Text(s) if s == bracket));
      }
      if let Some(ctrl_key) = consume_ctrl_key {
        frame_input.events.retain(|e| match e {
          Event::KeyPress { kind, .. } | Event::KeyRelease { kind, .. }
            if *kind == ctrl_key =>
          {
            false
          }
          // Ctrl+W produces \x17, Ctrl+D produces \x04
          Event::Text(s)
            if s.chars().next().is_some_and(|c| c.is_control()) =>
          {
            false
          }
          _ => true,
        });
      }
      if consume_tab {
        frame_input.events.retain(|e| {
          !matches!(
            e,
            Event::KeyPress { kind: Key::Tab, .. }
              | Event::KeyRelease { kind: Key::Tab, .. }
          )
        });
      }
      if consume_escape {
        frame_input.events.retain(|e| {
          !matches!(
            e,
            Event::KeyPress {
              kind: Key::Escape,
              ..
            } | Event::KeyRelease {
              kind: Key::Escape,
              ..
            }
          )
        });
      }
      if !consume_cmd_keys.is_empty() {
        frame_input.events.retain(|e| {
          !matches!(
            e,
            Event::KeyPress { kind, .. } | Event::KeyRelease { kind, .. }
              if consume_cmd_keys.contains(kind)
          )
        });
      }

      let dpr = frame_input.device_pixel_ratio;

      // Process GUI (consumes events over egui panels)
      let mut panel_layout = PanelLayout {
        scene_rect: egui::Rect::NOTHING,
      };
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
              app.pending_editor_action =
                Some(EditorAction::PasteLineAbove(text.clone()));
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
              let line_end = text[cursor..]
                .find('\n')
                .map_or(text.len(), |p| cursor + p + 1);
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

          panel_layout = render_ui(gui_context, &mut app);

          // Draw axis labels as overlay within the 3D scene area.
          // Camera viewport is at origin (0,0), so pixel_at_position returns
          // coordinates relative to the scene area. Offset by scene_rect.
          let scene_rect = panel_layout.scene_rect;
          let tips_gl = [
            vec3(0.0, 0.0, 5.2), // CAD +X → GL +Z
            vec3(5.2, 0.0, 0.0), // CAD +Y → GL +X
            vec3(0.0, 5.2, 0.0), // CAD +Z → GL +Y
          ];
          let labels = ["X", "Y", "Z"];
          let colors = [
            egui::Color32::RED,
            egui::Color32::GREEN,
            egui::Color32::from_rgb(80, 80, 255),
          ];
          let painter = gui_context.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("axis_labels"),
          ));
          let vp = camera.viewport();
          for i in 0..3 {
            let px = camera.pixel_at_position(tips_gl[i]);
            // pixel_at_position returns physical pixels relative to origin viewport.
            // Convert to logical and offset by scene_rect position.
            let ex = px.x as f32 / dpr + scene_rect.left();
            let ey = (vp.height as f32 - px.y as f32) / dpr + scene_rect.top();
            let pos = egui::Pos2::new(ex, ey);
            if scene_rect.contains(pos) {
              painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                labels[i],
                egui::FontId::proportional(14.0),
                colors[i],
              );
            }
          }

          egui_cursor = gui_context.output(|o| o.cursor_icon);
          gui_context.output_mut(|o| {
            for cmd in o.commands.drain(..) {
              if let egui::OutputCommand::CopyText(text) = cmd {
                copied_text = text;
              }
            }
          });
        },
      );

      // Handle copy/cut output from egui → system clipboard
      if !copied_text.is_empty()
        && let Some(cb) = clipboard.as_mut()
      {
        let _ = cb.set_text(copied_text);
      }

      // Handle csgrs export requests
      #[cfg(feature = "csgrs")]
      if let Some(fmt) = app.pending_export.take() {
        let (title, filter_name, ext) = match fmt {
          ExportFormat::ThreeMF => ("Export 3MF", "3MF Files", "3mf"),
          ExportFormat::STL => ("Export STL", "STL Files", "stl"),
          ExportFormat::OBJ => ("Export OBJ", "OBJ Files", "obj"),
          ExportFormat::PLY => ("Export PLY", "PLY Files", "ply"),
          ExportFormat::OpenSCAD => {
            ("Export OpenSCAD", "OpenSCAD Files", "scad")
          }
        };
        if let Some(path) = rfd::FileDialog::new()
          .set_title(title)
          .add_filter(filter_name, &[ext])
          .set_file_name(timestamped_filename(ext))
          .save_file()
        {
          let result = match fmt {
            ExportFormat::ThreeMF => {
              luacad::export::export_3mf(&app.geometries, &path)
            }
            ExportFormat::STL => {
              luacad::export::export_stl(&app.geometries, &path)
            }
            ExportFormat::OBJ => {
              luacad::export::export_obj(&app.geometries, &path)
            }
            ExportFormat::PLY => {
              luacad::export::export_ply(&app.geometries, &path)
            }
            ExportFormat::OpenSCAD => {
              let nodes: Vec<_> = app
                .geometries
                .iter()
                .filter_map(|g| g.scad.clone())
                .collect();
              scad_export::export_scad(&nodes, &path)
            }
          };
          match result {
            Ok(()) => {
              app.export_status =
                Some((format!("Exported to {}", path.display()), false))
            }
            Err(e) => {
              app.export_status = Some((format!("Export failed: {e}"), true))
            }
          }
        }
      }

      // Handle SCAD export
      if app.pending_scad_export {
        app.pending_scad_export = false;
        if let Some(path) = rfd::FileDialog::new()
          .set_title("Export OpenSCAD")
          .add_filter("OpenSCAD Files", &["scad"])
          .set_file_name(timestamped_filename("scad"))
          .save_file()
        {
          let nodes: Vec<_> = app
            .geometries
            .iter()
            .filter_map(|g| g.scad.clone())
            .collect();
          match luacad::scad_export::export_scad(&nodes, &path) {
            Ok(()) => {
              app.export_status =
                Some((format!("Exported to {}", path.display()), false))
            }
            Err(e) => {
              app.export_status = Some((format!("Export failed: {e}"), true))
            }
          }
        }
      }

      // Handle Manifold-based export
      if let Some(fmt) = app.pending_manifold_export.take() {
        if app.geometries.is_empty() {
          app.export_status = Some(("No geometry to export".to_string(), true));
        } else if let Some(path) = rfd::FileDialog::new()
          .set_title(format!("Export via Manifold — {}", fmt.label()))
          .add_filter(fmt.filter_name(), &[fmt.extension()])
          .set_file_name(timestamped_filename(fmt.extension()))
          .save_file()
        {
          let result = luacad::export::export_manifold(
            &app.geometries,
            fmt.extension(),
            &path,
          );
          match result {
            Ok(()) => {
              app.export_status = Some((
                format!("Exported via Manifold to {}", path.display()),
                false,
              ))
            }
            Err(e) => {
              app.export_status =
                Some((format!("Manifold export failed: {e}"), true))
            }
          }
        }
      }

      // Handle file open/save requests
      if let Some(action) = app.pending_file_action.take() {
        match action {
          FileAction::New => {
            app.text_content.clear();
            app.geometries.clear();
            app.lua_error = None;
            app.current_file = None;
            app.scene_dirty = true;
            save_last_file(None);
          }
          FileAction::Open => {
            if let Some(path) = rfd::FileDialog::new()
              .set_title("Open Lua File")
              .add_filter("Lua Files", &["lua"])
              .pick_file()
            {
              match std::fs::read_to_string(&path) {
                Ok(contents) => {
                  app.text_content = contents;
                  app.current_file = Some(path.clone());
                  app.execute_lua_code();
                  app.scene_dirty = true;
                  app.needs_fit_to_view = true;
                  save_last_file(Some(&path));
                }
                Err(e) => {
                  app.export_status =
                    Some((format!("Failed to open: {e}"), true))
                }
              }
            }
          }
          FileAction::Save => {
            app.text_content = app
              .text_content
              .lines()
              .map(|l| l.trim_end())
              .collect::<Vec<_>>()
              .join("\n");
            if let Some(path) = app.current_file.clone() {
              match std::fs::write(&path, &app.text_content) {
                Ok(()) => {
                  app.export_status =
                    Some((format!("Saved to {}", path.display()), false));
                  app.execute_lua_code();
                }
                Err(e) => {
                  app.export_status =
                    Some((format!("Failed to save: {e}"), true))
                }
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
                  app.export_status =
                    Some((format!("Saved to {}", path.display()), false));
                  app.execute_lua_code();
                  save_last_file(Some(&path));
                }
                Err(e) => {
                  app.export_status =
                    Some((format!("Failed to save: {e}"), true))
                }
              }
            }
          }
          FileAction::SaveAs => {
            app.text_content = app
              .text_content
              .lines()
              .map(|l| l.trim_end())
              .collect::<Vec<_>>()
              .join("\n");
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
                  app.execute_lua_code();
                  save_last_file(Some(&path));
                }
                Err(e) => {
                  app.export_status =
                    Some((format!("Failed to save: {e}"), true))
                }
              }
            }
          }
        }
      }

      // Compute scene area in physical pixels from the logical scene_rect
      let full = frame_input.viewport;
      let dpr = frame_input.device_pixel_ratio;
      let sr = panel_layout.scene_rect;
      let scene_w = (sr.width() * dpr).round() as u32;
      let scene_h = (sr.height() * dpr).round() as u32;

      // Camera viewport is at origin — matches the FBO we'll render into
      let scene_viewport = Viewport::new_at_origo(scene_w, scene_h);
      camera.set_viewport(scene_viewport);

      // Scene rect in physical pixels (for mouse hit-testing).
      // PhysicalPoint has y=0 at bottom (GL convention), so convert
      // the egui top-left scene_rect to bottom-left coordinates.
      let scene_phys_x = sr.left() * dpr;
      let scene_phys_r = scene_phys_x + scene_w as f32;
      // Bottom edge in GL coords (y=0 at bottom of window)
      let scene_phys_bottom = full.height as f32 - sr.bottom() * dpr;
      let scene_phys_top = scene_phys_bottom + scene_h as f32;

      let in_scene = |pos: &PhysicalPoint| -> bool {
        pos.x >= scene_phys_x
          && pos.x < scene_phys_r
          && pos.y >= scene_phys_bottom
          && pos.y < scene_phys_top
      };
      for event in frame_input.events.iter() {
        match event {
          Event::MousePress {
            button: MouseButton::Left,
            position,
            handled,
            ..
          } if !handled && in_scene(position) => {
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
          }
          Event::MouseWheel {
            delta,
            handled,
            position,
            ..
          } if !handled && in_scene(position) => {
            let zoom_factor = (-delta.1 * 0.01).exp();
            app.camera_distance =
              (app.camera_distance * zoom_factor).clamp(0.001, 10_000.0);
          }
          _ => {}
        }
      }

      // Handle scene rebuild on Lua re-execution
      if app.scene_dirty {
        if app.needs_fit_to_view {
          if let Some(dist) =
            compute_fit_distance(&app.geometries, app.orthogonal_view)
          {
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

      // Re-check system theme
      if app.theme_mode == ThemeMode::System
        && frame_input.accumulated_time - last_theme_check > 2000.0
      {
        last_theme_check = frame_input.accumulated_time;
        app.theme_colors = app.resolve_theme();
      }

      // --- Render ---
      let (bg_r, bg_g, bg_b) = app.theme_colors.bg;

      // Resize the offscreen FBO if the scene area changed
      scene_fbo.ensure_size(scene_w, scene_h);

      // Render the 3D scene into the offscreen FBO at (0,0).
      // OpenCSG's internal FBO/blit logic requires viewport at origin.
      scene_fbo.bind();
      gl_clear_screen(bg_r, bg_g, bg_b);

      let proj = camera_projection_matrix(&camera);
      let view = camera_view_matrix(&camera);
      render_opencsg_scene(&app.csg_groups, &proj, &view);
      render_axes();

      scene_fbo.unbind();

      // Clear the default framebuffer, then blit the FBO to the scene area.
      // GL blit coordinates use bottom-left origin, so convert from top-left.
      gl_set_viewport(full.x, full.y, full.width as i32, full.height as i32);
      gl_clear_screen(bg_r, bg_g, bg_b);

      let dst_x = (sr.left() * dpr).round() as i32;
      let dst_y = (full.height as f32 - sr.bottom() * dpr).round() as i32;
      scene_fbo.blit_to_screen(dst_x, dst_y, scene_w, scene_h);

      // Render egui overlay
      gui.render();

      winit_window.set_cursor_icon(egui_to_winit_cursor(egui_cursor));
      gl.swap_buffers();
      *control_flow = winit::event_loop::ControlFlow::Poll;
      winit_window.request_redraw();
    }
    winit::event::Event::WindowEvent { ref event, .. } => {
      frame_input_generator.handle_winit_window_event(event);
      match event {
        winit::event::WindowEvent::Resized(physical_size) => {
          gl.resize(*physical_size)
        }
        winit::event::WindowEvent::ScaleFactorChanged {
          new_inner_size,
          ..
        } => gl.resize(**new_inner_size),
        winit::event::WindowEvent::CloseRequested => {
          *control_flow = winit::event_loop::ControlFlow::Exit
        }
        winit::event::WindowEvent::DroppedFile(path) => {
          if path.extension().and_then(|e| e.to_str()) == Some("lua") {
            match std::fs::read_to_string(path) {
              Ok(contents) => {
                app.text_content = contents;
                app.current_file = Some(path.clone());
                app.execute_lua_code();
                app.scene_dirty = true;
                app.needs_fit_to_view = true;
                save_last_file(Some(path));
              }
              Err(e) => {
                app.export_status = Some((format!("Failed to open: {e}"), true))
              }
            }
          } else {
            app.export_status =
              Some(("Only .lua files can be opened".to_string(), true));
          }
        }
        _ => {}
      }
    }
    _ => {}
  });
}
