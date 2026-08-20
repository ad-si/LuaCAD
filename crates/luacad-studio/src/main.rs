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

use app::{AppState, FileAction, file_mtime};
use camera::{Camera, Viewport, degrees, vec3};
use cgmath::InnerSpace;
use clap::{ArgAction, Parser};
use editor::EditorAction;
use editor::byte_index_of;
use editor::whole_line_at;
use egui_integration::EguiIntegration;
use input::{Event, FrameInputGenerator, Key, MouseButton, PhysicalPoint};
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
#[cfg(feature = "csgrs")]
use luacad::scad_export;
use scene::{
  SSAA_FACTOR, SceneFbo, build_camera, camera_projection_matrix,
  camera_view_matrix, compute_camera_vectors, fit_distance_for_extent,
  gl_clear_screen, gl_set_viewport, render_axes, render_opencsg_scene,
};
use theme::ThemeMode;
use ui::{PanelLayout, render_ui};

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Return the path to the state file (`~/.config/luacad/state.json`).
fn state_file_path() -> Option<PathBuf> {
  dirs::config_dir().map(|d| d.join("luacad").join("state.json"))
}

/// Read the state file as a JSON object (empty if missing or invalid).
fn load_state() -> serde_json::Map<String, serde_json::Value> {
  state_file_path()
    .and_then(|p| std::fs::read_to_string(p).ok())
    .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
    .and_then(|value| match value {
      serde_json::Value::Object(obj) => Some(obj),
      _ => None,
    })
    .unwrap_or_default()
}

/// Apply a change to the state file, keeping its other entries.
fn update_state(
  update: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) {
  let Some(state_path) = state_file_path() else {
    return;
  };
  let mut state = load_state();
  update(&mut state);
  if let Some(parent) = state_path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let _ =
    std::fs::write(&state_path, serde_json::Value::Object(state).to_string());
}

/// Persist the last opened file path (or clear it).
fn save_last_file(path: Option<&Path>) {
  update_state(|state| match path {
    Some(p) => {
      state.insert("last_file".to_string(), serde_json::json!(p));
    }
    None => {
      state.remove("last_file");
    }
  });
}

/// Persist whether the code editor panel is hidden
/// (named after OpenSCAD's `hideEditor` setting).
fn save_hide_editor(hidden: bool) {
  update_state(|state| {
    state.insert("hide_editor".to_string(), serde_json::json!(hidden));
  });
}

/// Whether the code editor panel was hidden when the app last ran.
fn load_hide_editor() -> bool {
  load_state()
    .get("hide_editor")
    .and_then(|v| v.as_bool())
    .unwrap_or(false)
}

/// Persist whether the opened file is watched and reloaded automatically
/// when it changes on disk.
fn save_auto_reload(enabled: bool) {
  update_state(|state| {
    state.insert("auto_reload".to_string(), serde_json::json!(enabled));
  });
}

/// Whether auto-reload was enabled when the app last ran (default: on).
fn load_auto_reload() -> bool {
  load_state()
    .get("auto_reload")
    .and_then(|v| v.as_bool())
    .unwrap_or(true)
}

/// Persist which projection the 3D view uses.
fn save_orthogonal_view(orthogonal: bool) {
  update_state(|state| {
    state.insert("orthogonal_view".to_string(), serde_json::json!(orthogonal));
  });
}

/// Which projection the 3D view used when the app last ran
/// (default: orthogonal, like OpenSCAD).
fn load_orthogonal_view() -> bool {
  load_state()
    .get("orthogonal_view")
    .and_then(|v| v.as_bool())
    .unwrap_or(true)
}

/// Normalize source code for saving: strip trailing whitespace from each
/// line and end a non-empty file with exactly one newline (POSIX).
fn normalize_source(text: &str) -> String {
  let mut normalized = text
    .lines()
    .map(|l| l.trim_end())
    .collect::<Vec<_>>()
    .join("\n");
  while normalized.ends_with('\n') {
    normalized.pop();
  }
  if !normalized.is_empty() {
    normalized.push('\n');
  }
  normalized
}

/// Write the editor content to `path` and update app state accordingly.
/// Returns true if the write succeeded.
fn save_to_path(app: &mut AppState, path: &Path) -> bool {
  match std::fs::write(path, &app.text_content) {
    Ok(()) => {
      app.disk_mtime = file_mtime(path);
      app.mark_saved();
      app.export_status = Some((format!("Saved to {}", path.display()), false));
      app.execute_lua_code();
      true
    }
    Err(e) => {
      app.export_status = Some((format!("Failed to save: {e}"), true));
      false
    }
  }
}

/// Re-read the current file from disk into the editor and re-run it,
/// discarding the editor content. No-op without a current file.
fn reload_current_file(app: &mut AppState) {
  let Some(path) = app.current_file.clone() else {
    return;
  };
  match std::fs::read_to_string(&path) {
    Ok(contents) => {
      app.text_content = contents;
      app.mark_saved();
      app.disk_mtime = file_mtime(&path);
      app.execute_lua_code();
      app.export_status = Some((format!("Reloaded {}", path.display()), false));
    }
    Err(e) => {
      app.export_status = Some((format!("Failed to reload: {e}"), true))
    }
  }
}

/// Load the last opened file path from state, if the file still exists.
fn load_last_file() -> Option<PathBuf> {
  let state = load_state();
  let path_str = state.get("last_file")?.as_str()?;
  let path = PathBuf::from(path_str);
  if path.exists() { Some(path) } else { None }
}

/// Generate a timestamped default filename for export, e.g. `2026-03-01t2051_gear.3mf`.
/// Uses the stem of the currently open file, or `model` if none.
fn timestamped_filename(current_file: Option<&Path>, ext: &str) -> String {
  let stem = current_file
    .and_then(|p| p.file_stem())
    .and_then(|s| s.to_str())
    .unwrap_or("model");
  let now = time::OffsetDateTime::now_local()
    .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
  let format = time::format_description::parse_borrowed::<2>(
    "[year]-[month]-[day]t[hour][minute]",
  )
  .expect("valid format description");
  let stamp = now.format(&format).expect("format timestamp");
  format!("{stamp}_{stem}.{ext}")
}

/// Convert an egui CursorIcon to a winit CursorIcon.
fn egui_to_winit_cursor(cursor: egui::CursorIcon) -> winit::window::CursorIcon {
  match cursor {
    egui::CursorIcon::Default => winit::window::CursorIcon::Default,
    egui::CursorIcon::PointingHand => winit::window::CursorIcon::Pointer,
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

/// Everything the image in the scene FBO depends on.
///
/// The FBO keeps its contents between frames, so as long as this is unchanged
/// the previous render is still valid and can simply be blitted again.
#[derive(PartialEq)]
struct SceneSignature {
  projection: [f32; 16],
  view: [f32; 16],
  background: (f32, f32, f32),
  scene_revision: u64,
}

/// Everything that exists once the window and its GL context are up.
///
/// Field order is drop order: the egui painter and the GL context both hold
/// resources owned by the window, so they have to go before it.
struct Studio {
  gui: EguiIntegration,
  gl: gl_context::GlWindowContext,
  app: AppState,
  camera: Camera,
  scene_fbo: SceneFbo,
  /// What the scene FBO currently holds, or `None` while it is undefined
  last_scene_signature: Option<SceneSignature>,
  frame_input_generator: FrameInputGenerator,
  clipboard: Option<arboard::Clipboard>,
  last_theme_check: f64,
  /// When the opened file's mtime was last compared against `disk_mtime`
  /// (auto-reload watch), in egui accumulated time (ms)
  last_watch_check: f64,
  dragging_scene: bool,
  panning_scene: bool,
  window: winit::window::Window,
}

/// winit only hands out a window once the event loop is running, so the whole
/// application is built lazily on the first `resumed`.
struct StudioApp {
  initial_file: Option<PathBuf>,
  studio: Option<Studio>,
}

impl winit::application::ApplicationHandler for StudioApp {
  fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
    if self.studio.is_some() {
      return;
    }

    let window_attributes = winit::window::Window::default_attributes()
      .with_title("LuaCAD Studio")
      .with_maximized(true);
    let winit_window = event_loop
      .create_window(window_attributes)
      .expect("failed to create window");
    winit_window.focus_window();

    // Create GL context with Compatibility/Legacy profile (required by OpenCSG)
    let gl = gl_context::GlWindowContext::new(&winit_window, 8);
    let gui = EguiIntegration::new(gl.gl.clone());
    let mut app = AppState::new(self.initial_file.take());
    app.editor_visible = !load_hide_editor();
    app.auto_reload = load_auto_reload();
    // Moves the camera to the distance the restored projection needs, so a
    // scene without geometry to fit to still starts at the default zoom.
    app.set_orthogonal_view(load_orthogonal_view());

    // Persist the initial file if it was loaded successfully
    if let Some(ref path) = app.current_file {
      save_last_file(Some(path));
    }

    // Initialize OpenCSG's GLAD loader
    opencsg_sys::init_gl();

    // The initial Lua execution runs on a background thread; the redraw loop
    // auto-zooms to fit as soon as its geometry arrives (needs_fit_to_view).

    let initial_viewport = {
      let (w, h): (u32, u32) = winit_window.inner_size().into();
      Viewport::new_at_origo(w, h)
    };
    let camera = build_camera(initial_viewport, &app);
    let scene_fbo = SceneFbo::new(
      initial_viewport.width * SSAA_FACTOR,
      initial_viewport.height * SSAA_FACTOR,
    );
    let frame_input_generator =
      FrameInputGenerator::from_winit_window(&winit_window);

    self.studio = Some(Studio {
      gui,
      gl,
      app,
      camera,
      scene_fbo,
      last_scene_signature: None,
      frame_input_generator,
      clipboard: arboard::Clipboard::new().ok(),
      last_theme_check: 0.0,
      last_watch_check: 0.0,
      dragging_scene: false,
      panning_scene: false,
      window: winit_window,
    });
  }

  fn about_to_wait(&mut self, _: &winit::event_loop::ActiveEventLoop) {
    if let Some(studio) = self.studio.as_ref() {
      studio.window.request_redraw();
    }
  }

  fn window_event(
    &mut self,
    event_loop: &winit::event_loop::ActiveEventLoop,
    _window_id: winit::window::WindowId,
    event: winit::event::WindowEvent,
  ) {
    let Some(studio) = self.studio.as_mut() else {
      return;
    };
    if matches!(event, winit::event::WindowEvent::RedrawRequested) {
      studio.redraw(event_loop);
    } else {
      studio.window_event(event_loop, &event);
    }
  }
}

impl Studio {
  fn redraw(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
    let Studio {
      gui,
      gl,
      app,
      camera,
      scene_fbo,
      last_scene_signature,
      frame_input_generator,
      clipboard,
      last_theme_check,
      last_watch_check,
      dragging_scene,
      panning_scene,
      window: winit_window,
    } = self;
    {
      let editor_was_visible = app.editor_visible;
      let auto_reload_was_enabled = app.auto_reload;
      let was_orthogonal_view = app.orthogonal_view;

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
                    let end = app.editor_cursor_pos;
                    let start = end.saturating_sub(app.editor_selection_len);
                    let byte_start = byte_index_of(&app.text_content, start);
                    let byte_end = byte_index_of(&app.text_content, end);
                    app.search.query =
                      app.text_content[byte_start..byte_end].to_string();
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
              Key::E => {
                app.editor_visible = !app.editor_visible;
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
                app.camera_target = [0.0; 3];
              }
              _ => {}
            }
          } else if *kind == Key::Escape && app.search.open {
            app.search.open = false;
            app.search.matches.clear();
            app.search.current_match = None;
            app.search.last_computed = Default::default();
            consume_escape = true;
          } else if *kind == Key::Tab && app.editor_focused {
            if modifiers.shift {
              app.pending_editor_action = Some(EditorAction::Unindent);
            } else {
              app.pending_editor_action = Some(EditorAction::InsertTab);
            }
            consume_tab = true;
          }
        }
      }
      // Wrap selection with brackets when typing (, [, or { while text is
      // selected. Only when the editor itself has focus — otherwise typing a
      // bracket into the find/replace fields would be swallowed here.
      let mut consume_bracket_text: Option<String> = None;
      if app.editor_focused
        && app.editor_selection_len > 0
        && app.pending_editor_action.is_none()
      {
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
        |root_ui| {
          let gui_context = root_ui.ctx().clone();
          let gui_context = &gui_context;
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
              let line =
                whole_line_at(&app.text_content, app.editor_cursor_pos);
              if let Some(cb) = clipboard.as_mut() {
                let _ = cb.set_text(line);
              }
              app.clipboard_is_line = true;
            } else {
              gui_context.input_mut(|i| i.events.push(egui::Event::Copy));
              app.clipboard_is_line = false;
            }
          }
          if wants_cut {
            // Without a selection, cut the whole line — a following Cmd+V
            // then puts it back as a line of its own
            let line = if app.editor_focused && app.editor_selection_len == 0 {
              whole_line_at(&app.text_content, app.editor_cursor_pos)
            } else {
              String::new()
            };
            if line.is_empty() {
              gui_context.input_mut(|i| i.events.push(egui::Event::Cut));
              app.clipboard_is_line = false;
            } else {
              if let Some(cb) = clipboard.as_mut() {
                let _ = cb.set_text(line);
              }
              app.clipboard_is_line = true;
              app.pending_editor_action = Some(EditorAction::CutLine);
            }
          }

          panel_layout = render_ui(root_ui, app);

          // Draw axis labels as overlay within the 3D scene area — unless a
          // raytraced still covers it (they would be misplaced) or one is
          // rendering (they would scribble over the progress readout).
          // Camera viewport is at origin (0,0), so pixel_at_position returns
          // coordinates relative to the scene area. Offset by scene_rect.
          let scene_rect = panel_layout.scene_rect;
          if app.raytrace_texture.is_none() && !app.is_raytracing() {
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
            // The camera viewport is supersampled, so its pixels are that
            // much smaller than physical ones.
            let px_per_point = dpr * SSAA_FACTOR as f32;
            for i in 0..3 {
              let px = camera.pixel_at_position(tips_gl[i]);
              // pixel_at_position returns render pixels relative to origin
              // viewport. Convert to logical and offset by scene_rect.
              let ex = px.x as f32 / px_per_point + scene_rect.left();
              let ey = (vp.height as f32 - px.y as f32) / px_per_point
                + scene_rect.top();
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

      // Persist the editor visibility when it was toggled this frame
      // (shortcut, bottom bar, or settings dialog)
      if app.editor_visible != editor_was_visible {
        save_hide_editor(!app.editor_visible);
      }

      // Persist the auto-reload setting when it was toggled this frame
      if app.auto_reload != auto_reload_was_enabled {
        save_auto_reload(app.auto_reload);
      }

      // Persist the projection when it was switched this frame
      if app.orthogonal_view != was_orthogonal_view {
        save_orthogonal_view(app.orthogonal_view);
      }

      // Watch the opened file and pick up external changes (issue #14),
      // polling the mtime like `luacad watch` does. 100 ms keeps the
      // save-to-render delay below the point where it reads as lag, at a
      // negligible 10 stat calls per second. Skipped while the editor
      // has unsaved changes — those win, and the conflict surfaces through
      // the existing "File Changed on Disk" dialog on the next save.
      if app.auto_reload
        && frame_input.accumulated_time - *last_watch_check > 100.0
      {
        *last_watch_check = frame_input.accumulated_time;
        if let Some(path) = app.current_file.as_deref() {
          let mtime = file_mtime(path);
          // `None` usually means the file is mid-replace (editors save by
          // rename); a later poll sees the new mtime.
          if mtime.is_some()
            && mtime != app.disk_mtime
            && !app.has_unsaved_changes()
          {
            reload_current_file(app);
          }
        }
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
          .set_file_name(timestamped_filename(app.current_file.as_deref(), ext))
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
          .set_file_name(timestamped_filename(
            app.current_file.as_deref(),
            "scad",
          ))
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
          .set_file_name(timestamped_filename(
            app.current_file.as_deref(),
            fmt.extension(),
          ))
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
            app.new_document();
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
                  app.mark_saved();
                  app.current_file = Some(path.clone());
                  app.disk_mtime = file_mtime(&path);
                  app.reset_render_area();
                  app.execute_lua_code();
                  save_last_file(Some(&path));
                }
                Err(e) => {
                  app.export_status =
                    Some((format!("Failed to open: {e}"), true))
                }
              }
            }
          }
          FileAction::Save | FileAction::ForceSave => {
            app.text_content = normalize_source(&app.text_content);
            if let Some(path) = app.current_file.clone() {
              // Confirm before overwriting a file modified since load/save
              let changed_on_disk = matches!(action, FileAction::Save)
                && file_mtime(&path).is_some_and(|m| Some(m) != app.disk_mtime);
              if changed_on_disk {
                app.show_overwrite_confirm = true;
              } else {
                save_to_path(app, &path);
              }
            } else if let Some(path) = rfd::FileDialog::new()
              .set_title("Save Lua File")
              .add_filter("Lua Files", &["lua"])
              .set_file_name("untitled.lua")
              .save_file()
              && save_to_path(app, &path)
            {
              app.current_file = Some(path.clone());
              save_last_file(Some(&path));
            }
          }
          FileAction::SaveAs => {
            app.text_content = normalize_source(&app.text_content);
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
              && save_to_path(app, &path)
            {
              app.current_file = Some(path.clone());
              save_last_file(Some(&path));
            }
          }
          FileAction::Reload => reload_current_file(app),
        }
      }

      // Quit once the save requested from the close dialog went through.
      // If the save was cancelled or failed, stay open and forget the request
      // (unless the overwrite confirmation is still waiting for an answer).
      if app.quit_after_save {
        if !app.has_unsaved_changes() {
          app.quit_after_save = false;
          app.should_exit = true;
        } else if !app.show_overwrite_confirm {
          app.quit_after_save = false;
        }
      }
      if app.should_exit {
        event_loop.exit();
        return;
      }

      // Compute scene area in physical pixels from the logical scene_rect
      let full = frame_input.viewport;
      let dpr = frame_input.device_pixel_ratio;
      let sr = panel_layout.scene_rect;
      let scene_w = (sr.width() * dpr).round() as u32;
      let scene_h = (sr.height() * dpr).round() as u32;

      // The scene is supersampled: rendered at SSAA_FACTOR× the on-screen size
      // and filtered back down when blitted.
      let render_w = scene_w * SSAA_FACTOR;
      let render_h = scene_h * SSAA_FACTOR;

      // Camera viewport is at origin — matches the FBO we'll render into
      let scene_viewport = Viewport::new_at_origo(render_w, render_h);
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
            modifiers,
            handled,
          } if !handled && in_scene(position) => {
            // Ctrl + drag pans, plain drag rotates.
            if modifiers.ctrl {
              *panning_scene = true;
            } else {
              *dragging_scene = true;
            }
          }
          Event::MouseRelease {
            button: MouseButton::Left,
            ..
          } => {
            *dragging_scene = false;
            *panning_scene = false;
          }
          Event::MousePress {
            button: MouseButton::Middle,
            position,
            handled,
            ..
          } if !handled && in_scene(position) => {
            *panning_scene = true;
          }
          Event::MouseRelease {
            button: MouseButton::Middle,
            ..
          } => {
            *panning_scene = false;
          }
          Event::MouseMotion {
            delta,
            button: Some(MouseButton::Left),
            ..
          } if *dragging_scene => {
            app.camera_azimuth -= delta.0 * 0.5;
            app.camera_elevation =
              (app.camera_elevation + delta.1 * 0.5).clamp(-85.0, 85.0);
          }
          Event::MouseMotion {
            delta,
            button: Some(MouseButton::Left | MouseButton::Middle),
            ..
          } if *panning_scene => {
            // World-space size of one logical pixel at the camera target,
            // so the model tracks the cursor while panning.
            let visible_height = if app.orthogonal_view {
              2.0 * app.camera_distance
            } else {
              2.0 * app.camera_distance * 22.5_f32.to_radians().tan()
            };
            let world_per_pixel = visible_height * dpr / scene_h as f32;
            let az = app.camera_azimuth.to_radians();
            let el = app.camera_elevation.to_radians();
            // Unit vector from the target toward the camera
            let to_camera =
              vec3(el.cos() * az.sin(), el.sin(), el.cos() * az.cos());
            let right = (-to_camera).cross(vec3(0.0, 1.0, 0.0)).normalize();
            let up = right.cross(-to_camera);
            let shift = right * (-delta.0 * world_per_pixel)
              + up * (delta.1 * world_per_pixel);
            app.camera_target[0] += shift.x;
            app.camera_target[1] += shift.y;
            app.camera_target[2] += shift.z;
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

      // Pick up the result of a background Lua execution, if one finished
      app.poll_lua_job();

      // Pick up the result of a background raytrace, if one finished
      app.poll_raytrace_job();

      // Dismiss the raytraced still as soon as the camera moves or the
      // scene changes: it no longer shows the current view, and hiding it
      // is what makes the live preview respond to the interaction again.
      if let Some(snapshot) = app.raytrace_snapshot
        && snapshot != app.raytrace_view()
      {
        app.clear_raytrace();
      }

      // Handle scene rebuild on Lua re-execution
      if app.scene_dirty {
        if app.needs_fit_to_view
          && let Some(extent) = app.scene_fit_extent
        {
          // Keep the request pending while the scene is empty, so the fit
          // happens as soon as the document produces geometry
          app.camera_distance =
            fit_distance_for_extent(extent, app.orthogonal_view);
          app.camera_target = [0.0; 3];
          app.needs_fit_to_view = false;
        }
        app.scene_dirty = false;
      }

      // Update camera
      let (pos, target, up) = compute_camera_vectors(app);
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
        && frame_input.accumulated_time - *last_theme_check > 2000.0
      {
        *last_theme_check = frame_input.accumulated_time;
        app.theme_colors = app.resolve_theme();
      }

      // --- Render ---
      let (bg_r, bg_g, bg_b) = app.theme_colors.bg;

      // Resize the offscreen FBO if the scene area changed. A resize leaves its
      // contents undefined, so it always forces a re-render.
      let resized = scene_fbo.ensure_size(render_w, render_h);

      let proj = camera_projection_matrix(camera);
      let view = camera_view_matrix(camera);
      let signature = SceneSignature {
        projection: proj,
        view,
        background: app.theme_colors.bg,
        scene_revision: app.scene_revision,
      };

      // Redraw the 3D scene only when it would actually differ from what the
      // FBO already holds. The frame loop runs at vsync regardless, so without
      // this the whole OpenCSG pass — supersampled, and fill-rate bound by
      // construction — would run every frame even while the app sits idle.
      if resized || last_scene_signature.as_ref() != Some(&signature) {
        // Render the 3D scene into the offscreen FBO at (0,0).
        // OpenCSG's internal FBO/blit logic requires viewport at origin.
        scene_fbo.bind();
        gl_clear_screen(bg_r, bg_g, bg_b);

        render_opencsg_scene(
          &app.csg_groups,
          &app.overlay_meshes,
          &proj,
          &view,
        );
        render_axes();

        scene_fbo.unbind();
        *last_scene_signature = Some(signature);
      }

      // Clear the default framebuffer, then blit the FBO to the scene area.
      // GL blit coordinates use bottom-left origin, so convert from top-left.
      gl_set_viewport(full.x, full.y, full.width as i32, full.height as i32);
      gl_clear_screen(bg_r, bg_g, bg_b);

      let dst_x = (sr.left() * dpr).round() as i32;
      let dst_y = (full.height as f32 - sr.bottom() * dpr).round() as i32;
      scene_fbo.blit_to_screen(dst_x, dst_y, scene_w, scene_h);

      // Render egui overlay
      gui.render();

      winit_window.set_cursor(egui_to_winit_cursor(egui_cursor));
      gl.swap_buffers();
      event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
      winit_window.request_redraw();
    }
  }

  fn window_event(
    &mut self,
    event_loop: &winit::event_loop::ActiveEventLoop,
    event: &winit::event::WindowEvent,
  ) {
    let Studio {
      gl,
      app,
      frame_input_generator,
      window: winit_window,
      ..
    } = self;
    {
      frame_input_generator.handle_winit_window_event(event);
      match event {
        winit::event::WindowEvent::Resized(physical_size) => {
          gl.resize(*physical_size)
        }
        // winit 0.30 resizes the window itself before delivering this, so the
        // surface only has to follow the window's new size.
        winit::event::WindowEvent::ScaleFactorChanged { .. } => {
          gl.resize(winit_window.inner_size())
        }
        winit::event::WindowEvent::CloseRequested => {
          if app.has_unsaved_changes() {
            app.show_close_confirm = true;
            winit_window.request_redraw();
          } else {
            event_loop.exit()
          }
        }
        winit::event::WindowEvent::DroppedFile(path) => {
          if path.extension().and_then(|e| e.to_str()) == Some("lua") {
            match std::fs::read_to_string(path) {
              Ok(contents) => {
                app.text_content = contents;
                app.mark_saved();
                app.current_file = Some(path.clone());
                app.disk_mtime = file_mtime(path);
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
  }
}

/// Command line interface of the GUI binary: a file to open, plus the
/// `--help` and `--version` queries that exit before any window is created.
#[derive(Parser)]
#[command(
  name = "luacad-studio",
  version = luacad::version::VERSION,
  about = "A 3D CAD studio with Lua scripting and live preview",
  after_help = "Without a file, Studio reopens the one from the last session.",
  // Spelled out below so that `-v` works, like it does for the `luacad` CLI
  // (clap's built-in flag is `-V`).
  disable_version_flag = true
)]
struct Cli {
  /// Show version
  #[arg(short = 'v', long = "version", action = ArgAction::Version)]
  version: Option<bool>,

  /// LuaCAD file to open
  #[arg(value_name = "file.lua")]
  file: Option<PathBuf>,
}

/// Drop the `-psn_…` process serial number that macOS' LaunchServices passes
/// to an app started from Finder — clap would reject it as an unknown flag,
/// and the app would not come up at all.
fn without_launch_serial(
  args: impl Iterator<Item = OsString>,
) -> impl Iterator<Item = OsString> {
  args.filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
}

fn main() {
  // Resolve initial file: CLI argument takes priority, then last opened file.
  let initial_file =
    Cli::parse_from(without_launch_serial(std::env::args_os()))
      .file
      .map(|path| {
        // Canonicalize relative paths so the state file stores absolute paths
        std::fs::canonicalize(&path).unwrap_or(path)
      })
      .or_else(load_last_file);

  let event_loop =
    winit::event_loop::EventLoop::new().expect("failed to create event loop");
  let mut studio_app = StudioApp {
    initial_file,
    studio: None,
  };
  event_loop
    .run_app(&mut studio_app)
    .expect("event loop failed");
}

#[cfg(test)]
mod tests {
  use super::{
    Cli, OsString, PathBuf, normalize_source, without_launch_serial,
  };
  use clap::Parser;

  #[test]
  fn cli_definition_is_consistent() {
    use clap::CommandFactory;
    Cli::command().debug_assert();
  }

  #[test]
  fn finder_process_serial_number_is_ignored() {
    let args = ["luacad-studio", "-psn_0_12345", "model.lua"]
      .into_iter()
      .map(OsString::from);
    let cli = Cli::parse_from(without_launch_serial(args));
    assert_eq!(cli.file, Some(PathBuf::from("model.lua")));
  }

  #[test]
  fn adds_final_newline() {
    assert_eq!(normalize_source("a = 1"), "a = 1\n");
  }

  #[test]
  fn keeps_single_final_newline() {
    assert_eq!(normalize_source("a = 1\n"), "a = 1\n");
  }

  #[test]
  fn collapses_trailing_blank_lines() {
    assert_eq!(normalize_source("a = 1\n\n\n"), "a = 1\n");
  }

  #[test]
  fn trims_trailing_whitespace_per_line() {
    assert_eq!(normalize_source("a = 1  \nb = 2\t"), "a = 1\nb = 2\n");
  }

  #[test]
  fn empty_input_stays_empty() {
    assert_eq!(normalize_source(""), "");
    assert_eq!(normalize_source("\n\n"), "");
  }
}
