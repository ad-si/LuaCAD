use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

use crate::csg_tree::{CsgGroup, CsgScene, OverlayMesh, flatten_geometries};
use crate::editor::EditorAction;
use crate::theme::{ThemeColors, ThemeMode, system_is_dark_mode};

/// Where the code editor panel is placed relative to the 3D viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPosition {
  Right,
  Left,
  Top,
  Bottom,
}

impl EditorPosition {
  pub const ALL: &'static [EditorPosition] = &[
    EditorPosition::Right,
    EditorPosition::Left,
    EditorPosition::Top,
    EditorPosition::Bottom,
  ];

  pub fn label(self) -> &'static str {
    match self {
      Self::Right => "Right",
      Self::Left => "Left",
      Self::Top => "Top",
      Self::Bottom => "Bottom",
    }
  }
}
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
use luacad::export::ManifoldFormat;
use luacad::geometry::CsgGeometry;
use luacad::linter::LintDiagnostic;

#[derive(Default)]
pub struct SearchState {
  /// Whether the search bar is visible
  pub open: bool,
  /// Whether the replace row is visible
  pub show_replace: bool,
  /// Search query string
  pub query: String,
  /// Replace string
  pub replace: String,
  /// Case-sensitive search toggle
  pub case_sensitive: bool,
  /// All match positions as byte ranges into text_content
  pub matches: Vec<Range<usize>>,
  /// Index of the currently focused match (0-based)
  pub current_match: Option<usize>,
  /// One-shot flag: focus and select-all in the search field
  pub focus_search_field: bool,
  /// One-shot flag: move cursor to the current match
  pub needs_cursor_update: bool,
  /// Cache key to avoid recomputing matches: (query, case_sensitive, text)
  pub last_computed: (String, bool, String),
}

/// The most recent mouse press inside the code editor, used to recognise a
/// double click on the second *press* (see `ui::render_ui`).
#[derive(Debug, Clone, Copy)]
pub struct EditorClick {
  /// egui input time of the press, in seconds
  pub time: f64,
  /// Press position in egui points
  pub pos: (f32, f32),
  /// How many presses in a row landed close together in time and space
  pub count: u32,
}

#[derive(Debug, Clone)]
pub enum FileAction {
  New,
  Open,
  Save,
  /// Save without checking whether the file changed on disk
  ForceSave,
  SaveAs,
  /// Re-read the current file from disk, discarding editor content
  Reload,
}

/// Modification time of a file on disk, if available.
pub fn file_mtime(path: &Path) -> Option<SystemTime> {
  std::fs::metadata(path).ok()?.modified().ok()
}

/// What a background Lua execution produces: the geometries, the error to
/// display (if any), and the scene already flattened for OpenCSG.
struct LuaJobResult {
  geometries: Vec<CsgGeometry>,
  lua_error: Option<String>,
  scene: CsgScene,
  /// Scene bounding radius for fit-to-view, precomputed here because it
  /// requires materializing the meshes — too slow for the render loop
  fit_extent: Option<f32>,
}

/// Camera pose the viewport starts with and returns to on document change.
pub const DEFAULT_CAMERA_AZIMUTH: f32 = -30.0;
pub const DEFAULT_CAMERA_ELEVATION: f32 = 30.0;
pub const DEFAULT_CAMERA_DISTANCE: f32 = 5.0;

pub struct AppState {
  pub text_content: String,
  /// Snapshot of text_content as it was last loaded from or written to disk
  pub saved_text: String,
  pub geometries: Vec<CsgGeometry>,
  pub lua_error: Option<String>,
  pub camera_azimuth: f32,
  pub camera_elevation: f32,
  pub camera_distance: f32,
  /// World-space point the camera orbits around (moved by panning)
  pub camera_target: [f32; 3],
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
  /// Disk modification time of current_file when it was last loaded or saved
  pub disk_mtime: Option<SystemTime>,
  /// Whether the "file changed on disk" save confirmation dialog is open
  pub show_overwrite_confirm: bool,
  /// Whether the "unsaved changes" close confirmation dialog is open
  pub show_close_confirm: bool,
  /// Quit as soon as the pending save completes successfully
  pub quit_after_save: bool,
  /// Set when the app should exit at the end of the frame
  pub should_exit: bool,
  /// Pending file action (save/open) requested this frame
  pub pending_file_action: Option<FileAction>,
  /// Pending SCAD export requested this frame
  pub pending_scad_export: bool,
  /// Pending Manifold-based export requested this frame
  pub pending_manifold_export: Option<ManifoldFormat>,
  /// Auto-zoom to fit on next scene rebuild (initial load / file open)
  pub needs_fit_to_view: bool,
  /// Whether the settings dialog is open
  pub show_settings: bool,
  /// Which tab is active in the settings dialog (0=General, 1=Shortcuts)
  pub settings_tab: usize,
  /// Editor panel position relative to viewport
  pub editor_position: EditorPosition,
  /// Editor cursor character offset (updated each frame by UI)
  pub editor_cursor_pos: usize,
  /// Editor selection length in characters (updated each frame by UI)
  pub editor_selection_len: usize,
  /// Whether the code editor had keyboard focus in the last rendered frame
  pub editor_focused: bool,
  /// Last mouse press inside the code editor (for double/triple click)
  pub editor_click: Option<EditorClick>,
  /// True when clipboard contains a whole-line copy (Cmd+C with no selection)
  pub clipboard_is_line: bool,
  /// Flattened CSG groups for OpenCSG preview rendering
  pub csg_groups: Vec<CsgGroup>,
  /// Translucent modifier overlays (`#` highlight, `%` background)
  pub overlay_meshes: Vec<OverlayMesh>,
  /// Bumped whenever `csg_groups` or `overlay_meshes` change, so the renderer
  /// can tell a cached 3D image from a stale one
  pub scene_revision: u64,
  /// Receiver for the Lua execution currently running on a background thread.
  /// Starting a new execution replaces the receiver, so a superseded run's
  /// result can never arrive — its send just fails.
  lua_job: Option<mpsc::Receiver<LuaJobResult>>,
  /// Bounding radius of the current scene (precomputed off-thread), consumed
  /// by fit-to-view. `None` while the scene is empty.
  pub scene_fit_extent: Option<f32>,
  /// Lint diagnostics for the current editor content
  pub lint_diagnostics: Vec<LintDiagnostic>,
  /// Snapshot of text_content used to detect changes for re-linting
  pub lint_text_snapshot: String,
  /// Find/replace search state
  pub search: SearchState,
}

impl AppState {
  pub fn new(initial_file: Option<PathBuf>) -> Self {
    let is_dark = system_is_dark_mode();

    let (text_content, current_file) = if let Some(ref path) = initial_file {
      match std::fs::read_to_string(path) {
        Ok(contents) => (contents, Some(path.clone())),
        Err(_) => (Self::welcome_text().to_string(), None),
      }
    } else {
      (Self::welcome_text().to_string(), None)
    };
    let disk_mtime = current_file.as_deref().and_then(file_mtime);

    let mut app = Self {
      saved_text: text_content.clone(),
      text_content,
      geometries: vec![],
      lua_error: None,
      camera_azimuth: DEFAULT_CAMERA_AZIMUTH,
      camera_elevation: DEFAULT_CAMERA_ELEVATION,
      camera_distance: DEFAULT_CAMERA_DISTANCE,
      camera_target: [0.0; 3],
      orthogonal_view: true,
      scene_dirty: true,
      theme_mode: ThemeMode::System,
      theme_colors: if is_dark {
        ThemeColors::dark()
      } else {
        ThemeColors::light()
      },
      pending_editor_action: None,
      export_status: None,
      #[cfg(feature = "csgrs")]
      pending_export: None,
      pending_scad_export: false,
      current_file,
      disk_mtime,
      show_overwrite_confirm: false,
      show_close_confirm: false,
      quit_after_save: false,
      should_exit: false,
      pending_file_action: None,
      pending_manifold_export: None,
      needs_fit_to_view: true,
      show_settings: false,
      settings_tab: 0,
      editor_position: EditorPosition::Right,
      editor_cursor_pos: 0,
      editor_selection_len: 0,
      editor_focused: false,
      editor_click: None,
      clipboard_is_line: false,
      csg_groups: vec![],
      overlay_meshes: vec![],
      scene_revision: 0,
      lua_job: None,
      scene_fit_extent: None,
      lint_diagnostics: vec![],
      lint_text_snapshot: String::new(),
      search: SearchState::default(),
    };
    app.execute_lua_code();
    app
  }

  fn welcome_text() -> &'static str {
    "-- Welcome to LuaCAD Studio\n-- Use + for union, - for difference, * for intersection\n\nlocal body = cube { 4, 2, 1, center = true }\nlocal hole = cylinder { h = 3, r = 0.5, center = true }\n\nrender(body - hole)"
  }

  /// Whether the editor content differs from what was last loaded or saved.
  pub fn has_unsaved_changes(&self) -> bool {
    self.text_content != self.saved_text
  }

  /// Record the current editor content as the on-disk state.
  pub fn mark_saved(&mut self) {
    self.saved_text = self.text_content.clone();
  }

  /// Move the camera back to the pose the app starts with.
  pub fn reset_camera(&mut self) {
    self.camera_azimuth = DEFAULT_CAMERA_AZIMUTH;
    self.camera_elevation = DEFAULT_CAMERA_ELEVATION;
    self.camera_distance = DEFAULT_CAMERA_DISTANCE;
    self.camera_target = [0.0; 3];
  }

  /// Empty the viewport: drop all geometry and restore the default camera.
  pub fn reset_render_area(&mut self) {
    self.geometries.clear();
    self.csg_groups.clear();
    self.overlay_meshes.clear();
    self.scene_fit_extent = None;
    self.scene_revision += 1;
    self.reset_camera();
    self.scene_dirty = true;
    // Fit as soon as the next geometry appears
    self.needs_fit_to_view = true;
  }

  /// Start a blank, unsaved document with an empty viewport.
  pub fn new_document(&mut self) {
    self.text_content.clear();
    self.mark_saved();
    self.lua_error = None;
    self.current_file = None;
    self.disk_mtime = None;
    self.reset_render_area();
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

  /// Start executing the editor content on a background thread, so the UI
  /// stays responsive while a complex model builds. The previous scene keeps
  /// showing until [`Self::poll_lua_job`] picks up the result.
  pub fn execute_lua_code(&mut self) {
    // Resolve relative paths (e.g. import("tracings/outline.svg"))
    // against the opened file's directory, like OpenSCAD does
    if let Some(dir) = self.current_file.as_ref().and_then(|f| f.parent()) {
      if dir.as_os_str().is_empty() {
        // A bare filename like `glasses.lua` has an empty parent
      } else if let Err(e) = std::env::set_current_dir(dir) {
        eprintln!("Warning: cannot enter {}: {e}", dir.display());
      }
    }

    let code = self.text_content.clone();
    let path = self.current_file.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
      let (geometries, lua_error) =
        match luacad::lua_engine::execute_lua_with_path(&code, path.as_deref())
        {
          Ok(geometries) => {
            let error = geometries.is_empty().then(|| {
              "No geometry to render. Use render(obj) or return a geometry object."
                .to_string()
            });
            (geometries, error)
          }
          Err(e) => (vec![], Some(e)),
        };
      let scene = flatten_geometries(&geometries);
      let fit_extent = crate::scene::compute_scene_extent(&geometries);
      let _ = tx.send(LuaJobResult {
        geometries,
        lua_error,
        scene,
        fit_extent,
      });
    });
    self.lua_job = Some(rx);
  }

  /// Whether a Lua execution is currently running in the background.
  pub fn is_lua_executing(&self) -> bool {
    self.lua_job.is_some()
  }

  /// Apply the result of a finished background Lua execution, if one arrived.
  /// Called once per frame from the render loop.
  pub fn poll_lua_job(&mut self) {
    let Some(rx) = &self.lua_job else {
      return;
    };
    match rx.try_recv() {
      Ok(result) => {
        self.lua_job = None;
        self.geometries = result.geometries;
        self.lua_error = result.lua_error;
        self.csg_groups = result.scene.groups;
        self.overlay_meshes = result.scene.overlays;
        self.scene_fit_extent = result.fit_extent;
        self.scene_revision += 1;
        self.scene_dirty = true;
      }
      Err(mpsc::TryRecvError::Empty) => {}
      Err(mpsc::TryRecvError::Disconnected) => {
        // The worker thread panicked before sending its result
        self.lua_job = None;
        self.lua_error =
          Some("Internal error: model evaluation crashed".to_string());
      }
    }
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
