use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
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

/// What a background raytrace produces: interleaved RGB8 pixels.
pub struct RaytraceImage {
  pub width: usize,
  pub height: usize,
  pub rgb: Vec<u8>,
}

/// Camera pose and scene revision a displayed raytraced still belongs to;
/// any change means the still no longer shows the current view.
pub type RaytraceSnapshot = (f32, f32, f32, [f32; 3], u64);

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
  /// Whether the code editor panel is shown at all
  pub editor_visible: bool,
  /// Watch the opened file and reload it automatically when another program
  /// changes it on disk (skipped while the editor has unsaved changes)
  pub auto_reload: bool,
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
  /// Receiver for the raytrace currently running on a background thread
  raytrace_job: Option<mpsc::Receiver<Result<RaytraceImage, String>>>,
  /// Scanlines finished by the running raytrace, written by its worker
  /// threads and read by the progress readout
  raytrace_rows_done: Option<Arc<AtomicUsize>>,
  /// Total scanlines of the running raytrace
  raytrace_rows_total: usize,
  /// One-shot flag set by the Raytrace button; the UI starts the job once
  /// the viewport size is known
  pub pending_raytrace: bool,
  /// Finished raytraced image waiting for the UI to upload it as a texture
  pub raytrace_image: Option<RaytraceImage>,
  /// Uploaded raytraced still shown over the viewport
  pub raytrace_texture: Option<egui::TextureHandle>,
  /// Letterbox fill continuing the raytraced image's background color
  pub raytrace_bg: [u8; 3],
  /// Camera pose and scene revision the displayed still was taken at; the
  /// still is dismissed as soon as either changes
  pub raytrace_snapshot: Option<RaytraceSnapshot>,
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
      editor_visible: true,
      auto_reload: true,
      editor_cursor_pos: 0,
      editor_selection_len: 0,
      editor_focused: false,
      editor_click: None,
      clipboard_is_line: false,
      csg_groups: vec![],
      overlay_meshes: vec![],
      scene_revision: 0,
      lua_job: None,
      raytrace_job: None,
      raytrace_rows_done: None,
      raytrace_rows_total: 0,
      pending_raytrace: false,
      raytrace_image: None,
      raytrace_texture: None,
      raytrace_bg: [0; 3],
      raytrace_snapshot: None,
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

  /// Start path tracing the current geometries at the given resolution on a
  /// background thread, reproducing the studio's current view: orbit
  /// angles, pan target, and zoom. The result arrives via
  /// [`Self::poll_raytrace_job`].
  pub fn start_raytrace(&mut self, width: usize, height: usize) {
    let geometries = self.geometries.clone();
    let camera = (self.camera_azimuth, self.camera_elevation);

    // The viewport camera lives in GL coordinates (gl = (cad_y, cad_z,
    // cad_x)); the raytrace API takes the target in CAD coordinates.
    let [gx, gy, gz] = self.camera_target;
    // The path tracer is perspective-only: an orthographic view uses the
    // equivalent perspective distance, exactly like the studio's own
    // projection toggle (same visible height at the target, see
    // `ui::render_ui`).
    let distance = if self.orthogonal_view {
      self.camera_distance / (22.5_f32).to_radians().tan()
    } else {
      self.camera_distance
    };
    let framing = luacad::raytrace::Framing {
      target: [gz, gx, gy],
      distance,
      // The studio's perspective projection FOV (`scene::build_camera`)
      vfov: 45.0,
    };

    let rows_done = Arc::new(AtomicUsize::new(0));
    self.raytrace_rows_done = Some(rows_done.clone());
    self.raytrace_rows_total = height;

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
      let result = luacad::raytrace::render_to_rgb8(
        &geometries,
        width,
        height,
        None,
        Some(camera),
        Some(framing),
        || {
          rows_done.fetch_add(1, Ordering::Relaxed);
        },
      )
      .map(|rgb| RaytraceImage { width, height, rgb });
      let _ = tx.send(result);
    });
    self.raytrace_job = Some(rx);
  }

  /// Whether a raytrace is currently running in the background.
  pub fn is_raytracing(&self) -> bool {
    self.raytrace_job.is_some()
  }

  /// Fraction of the running raytrace's scanlines that are finished.
  pub fn raytrace_progress(&self) -> f32 {
    match &self.raytrace_rows_done {
      Some(rows) if self.raytrace_rows_total > 0 => {
        rows.load(Ordering::Relaxed) as f32 / self.raytrace_rows_total as f32
      }
      _ => 0.0,
    }
  }

  /// The camera pose and scene revision a raytrace of the current state
  /// would depict.
  pub fn raytrace_view(&self) -> RaytraceSnapshot {
    (
      self.camera_azimuth,
      self.camera_elevation,
      self.camera_distance,
      self.camera_target,
      self.scene_revision,
    )
  }

  /// Apply the result of a finished background raytrace, if one arrived.
  /// Called once per frame from the render loop.
  pub fn poll_raytrace_job(&mut self) {
    let Some(rx) = &self.raytrace_job else {
      return;
    };
    match rx.try_recv() {
      Ok(result) => {
        self.raytrace_job = None;
        self.raytrace_rows_done = None;
        match result {
          Ok(image) => {
            self.raytrace_snapshot = Some(self.raytrace_view());
            self.raytrace_image = Some(image);
          }
          Err(e) => {
            self.export_status = Some((format!("Raytrace failed: {e}"), true));
          }
        }
      }
      Err(mpsc::TryRecvError::Empty) => {}
      Err(mpsc::TryRecvError::Disconnected) => {
        // The worker thread panicked before sending its result
        self.raytrace_job = None;
        self.raytrace_rows_done = None;
        self.export_status =
          Some(("Internal error: raytrace crashed".to_string(), true));
      }
    }
  }

  /// Dismiss the raytraced still and return to the live preview.
  pub fn clear_raytrace(&mut self) {
    self.raytrace_image = None;
    self.raytrace_texture = None;
    self.raytrace_snapshot = None;
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
