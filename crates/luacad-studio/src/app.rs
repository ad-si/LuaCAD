use std::path::PathBuf;

use crate::csg_tree::{CsgGroup, flatten_geometries};
use crate::editor::EditorAction;
use crate::theme::{ThemeColors, ThemeMode, system_is_dark_mode};
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
use luacad::export::ManifoldFormat;
use luacad::geometry::CsgGeometry;
use luacad::linter::LintDiagnostic;

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
  /// Pending export format requested this frame (csgrs only)
  #[cfg(feature = "csgrs")]
  pub pending_export: Option<ExportFormat>,
  /// Currently opened file path
  pub current_file: Option<PathBuf>,
  /// Pending file action (save/open) requested this frame
  pub pending_file_action: Option<FileAction>,
  /// Pending SCAD export requested this frame
  pub pending_scad_export: bool,
  /// Pending Manifold-based export requested this frame
  pub pending_manifold_export: Option<ManifoldFormat>,
  /// Auto-zoom to fit on next scene rebuild (initial load / file open)
  pub needs_fit_to_view: bool,
  /// Whether the keyboard shortcuts modal is open
  pub show_shortcuts: bool,
  /// Editor cursor character offset (updated each frame by UI)
  pub editor_cursor_pos: usize,
  /// Editor selection length in characters (updated each frame by UI)
  pub editor_selection_len: usize,
  /// True when clipboard contains a whole-line copy (Cmd+C with no selection)
  pub clipboard_is_line: bool,
  /// Flattened CSG groups for OpenCSG preview rendering
  pub csg_groups: Vec<CsgGroup>,
  /// Lint diagnostics for the current editor content
  pub lint_diagnostics: Vec<LintDiagnostic>,
  /// Snapshot of text_content used to detect changes for re-linting
  pub lint_text_snapshot: String,
}

impl AppState {
  pub fn new() -> Self {
    let is_dark = system_is_dark_mode();
    let mut app = Self {
      text_content: "-- Welcome to LuaCAD Studio\n-- Use + for union, - for difference, * for intersection\n\nlocal body = cube { 4, 2, 1, center = true }\nlocal hole = cylinder { h = 3, r = 0.5, center = true }\n\nrender(body - hole)".to_string(),
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
      #[cfg(feature = "csgrs")]
      pending_export: None,
      pending_scad_export: false,
      current_file: None,
      pending_file_action: None,
      pending_manifold_export: None,
      needs_fit_to_view: true,
      show_shortcuts: false,
      editor_cursor_pos: 0,
      editor_selection_len: 0,
      clipboard_is_line: false,
      csg_groups: vec![],
      lint_diagnostics: vec![],
      lint_text_snapshot: String::new(),
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

  pub fn execute_lua_code(&mut self) {
    self.lua_error = None;
    self.geometries.clear();

    match luacad::lua_engine::execute_lua(&self.text_content) {
      Ok(geometries) => {
        if geometries.is_empty() {
          self.lua_error = Some(
            "No geometry to render. Use render(obj) or return a geometry object."
              .to_string(),
          );
        }
        self.geometries = geometries;
      }
      Err(e) => {
        self.lua_error = Some(e);
      }
    }

    self.csg_groups = flatten_geometries(&self.geometries);
    self.scene_dirty = true;
  }

  /// Re-run the linter if the editor text has changed since last check.
  pub fn update_lint(&mut self) {
    if self.text_content == self.lint_text_snapshot {
      return;
    }
    self.lint_text_snapshot = self.text_content.clone();
    self.lint_diagnostics =
      luacad::linter::lint(&self.text_content).unwrap_or_default();
  }
}
