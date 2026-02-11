use std::path::PathBuf;

use crate::editor::EditorAction;
use crate::export::{ExportFormat, OpenScadFormat};
use crate::geometry::CsgGeometry;
use crate::theme::{ThemeColors, ThemeMode, system_is_dark_mode};

#[derive(Debug, Clone)]
pub enum FileAction {
  Open,
  Save,
  SaveAs,
}

pub struct AppState {
  pub text_content: String,
  pub geometries: Vec<CsgGeometry>,
  pub lua_error: Option<String>,
  pub camera_azimuth: f32,
  pub camera_elevation: f32,
  pub camera_distance: f32,
  pub orthogonal_view: bool,
  pub scene_dirty: bool,
  pub theme_mode: ThemeMode,
  pub theme_colors: ThemeColors,
  /// Pending editor action triggered by keyboard shortcut
  pub pending_editor_action: Option<EditorAction>,
  /// Status message from last export attempt
  pub export_status: Option<(String, bool)>, // (message, is_error)
  /// Pending export format requested this frame
  pub pending_export: Option<ExportFormat>,
  /// Currently opened file path
  pub current_file: Option<PathBuf>,
  /// Pending file action (save/open) requested this frame
  pub pending_file_action: Option<FileAction>,
  /// Pending OpenSCAD-based export requested this frame
  pub pending_openscad_export: Option<OpenScadFormat>,
  /// Auto-zoom to fit on next scene rebuild (initial load / file open)
  pub needs_fit_to_view: bool,
}

impl AppState {
  pub fn new() -> Self {
    let is_dark = system_is_dark_mode();
    let mut app = Self {
      text_content: "-- Welcome to LuaCAD Studio\n-- Z-axis points upward\n-- Use + for union, - for difference, * for intersection\n\nlocal body = cube { 4, 2, 1, center = true }\nlocal hole = cylinder { h = 3, r = 0.5, center = true }\n\nrender(body - hole)".to_string(),
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
      current_file: None,
      pending_file_action: None,
      pending_openscad_export: None,
      needs_fit_to_view: true,
    };
    app.execute_lua_code();
    app
  }

  pub fn resolve_theme(&self) -> ThemeColors {
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
}
