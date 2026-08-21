use egui_extras::syntax_highlighting;
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
use luacad::export::ManifoldFormat;
use luacad::linter::LintSeverity;
use luacad::version;

use crate::app::{
  AppState, EditorClick, EditorPosition, FileAction, SearchState,
};
use crate::editor::{
  apply_editor_action, byte_index_of, char_index_of, double_click_range,
  find_matches, triple_click_range,
};
use crate::theme::ThemeMode;

/// Index of the "About" tab in the settings dialog.
const ABOUT_TAB: usize = 2;

/// Fill color of the Save button while there are unsaved changes.
const UNSAVED_ORANGE: egui::Color32 = egui::Color32::from_rgb(230, 140, 40);

/// How long after a click a following click still counts as a double click.
/// macOS and Windows both use 500 ms; egui's own default of 300 ms drops
/// double clicks made at a normal pace.
pub const DOUBLE_CLICK_SECS: f64 = 0.5;

/// Return the platform modifier key label: ⌘ on macOS, Ctrl elsewhere.
fn modifier_label() -> &'static str {
  #[cfg(target_os = "macos")]
  {
    "⌘"
  }
  #[cfg(not(target_os = "macos"))]
  {
    "Ctrl"
  }
}

/// Paint a small down-pointing triangle at the right edge of a button's rect.
fn paint_dropdown_arrow(ui: &egui::Ui, response: &egui::Response) {
  let rect = response.rect;
  let color = ui.visuals().text_color();
  let cx = rect.right() - 8.0;
  let cy = rect.center().y;
  let half = 3.0;
  let points = vec![
    egui::pos2(cx - half, cy - half * 0.5),
    egui::pos2(cx + half, cy - half * 0.5),
    egui::pos2(cx, cy + half * 0.5),
  ];
  ui.painter().add(egui::Shape::convex_polygon(
    points,
    color,
    egui::Stroke::NONE,
  ));
}

/// Lucide icon identifiers used in the search bar.
/// Icon SVG data from lucide.dev (ISC license).
enum LucideIcon {
  ChevronUp,
  ChevronDown,
  ChevronRight,
  X,
}

/// Paint a Lucide icon into a given rect, scaled to fit.
/// The icons are drawn as strokes in a 24x24 coordinate system.
fn paint_lucide_icon(
  painter: &egui::Painter,
  rect: egui::Rect,
  icon: LucideIcon,
  color: egui::Color32,
) {
  // Map from 24x24 SVG coords to the target rect
  let to_pos = |x: f32, y: f32| -> egui::Pos2 {
    egui::pos2(
      rect.left() + x / 24.0 * rect.width(),
      rect.top() + y / 24.0 * rect.height(),
    )
  };
  let stroke = egui::Stroke::new(1.5_f32, color);

  match icon {
    LucideIcon::X => {
      // M18 6 L6 18 and M6 6 L12 12 (two diagonal lines)
      painter.line_segment([to_pos(18.0, 6.0), to_pos(6.0, 18.0)], stroke);
      painter.line_segment([to_pos(6.0, 6.0), to_pos(18.0, 18.0)], stroke);
    }
    LucideIcon::ChevronUp => {
      // m18 15-6-6-6 6  → polyline (18,15) (12,9) (6,15)
      let points =
        vec![to_pos(18.0, 15.0), to_pos(12.0, 9.0), to_pos(6.0, 15.0)];
      painter.add(egui::Shape::line(points, stroke));
    }
    LucideIcon::ChevronDown => {
      // m6 9 6 6 6-6  → polyline (6,9) (12,15) (18,9)
      let points =
        vec![to_pos(6.0, 9.0), to_pos(12.0, 15.0), to_pos(18.0, 9.0)];
      painter.add(egui::Shape::line(points, stroke));
    }
    LucideIcon::ChevronRight => {
      // m9 18 6-6-6-6  → polyline (9,18) (15,12) (9,6)
      let points =
        vec![to_pos(9.0, 18.0), to_pos(15.0, 12.0), to_pos(9.0, 6.0)];
      painter.add(egui::Shape::line(points, stroke));
    }
  }
}

/// An icon button: a small square button with a Lucide icon painted inside.
/// Returns the `Response` so callers can chain `.on_hover_text()` / `.clicked()`.
fn icon_button(ui: &mut egui::Ui, icon: LucideIcon) -> egui::Response {
  let size = egui::vec2(20.0, 20.0);
  let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

  if ui.is_rect_visible(rect) {
    let visuals = ui.style().interact(&response);
    // Draw a subtle rounded background on hover/click
    if response.hovered() || response.has_focus() {
      ui.painter().rect_filled(rect, 2.0, visuals.bg_fill);
    }
    let icon_rect = rect.shrink(2.0);
    paint_lucide_icon(ui.painter(), icon_rect, icon, visuals.text_color());
  }

  response
}

/// Navigate to the next (+1) or previous (-1) search match, wrapping around.
fn navigate_match(search: &mut SearchState, direction: i32) {
  if search.matches.is_empty() {
    return;
  }
  let count = search.matches.len() as i32;
  let current = search.current_match.unwrap_or(0) as i32;
  let next = ((current + direction) % count + count) % count;
  search.current_match = Some(next as usize);
  search.needs_cursor_update = true;
}

/// Render the "General" settings tab content.
fn render_settings_general(ui: &mut egui::Ui, app: &mut AppState) {
  ui.label(egui::RichText::new("Code Editor").strong().size(14.0));
  ui.add_space(4.0);
  ui.checkbox(&mut app.editor_visible, "Show the code editor panel")
    .on_hover_text(format!(
      "Hide the panel to use an external editor \
       and keep the full window for the model ({} E)",
      modifier_label()
    ));
  ui.add_space(8.0);
  ui.label(egui::RichText::new("File").strong().size(14.0));
  ui.add_space(4.0);
  ui.checkbox(
    &mut app.auto_reload,
    "Reload the file when it changes on disk",
  )
  .on_hover_text(
    "Watch the opened file and re-render automatically when another \
     program saves it. Skipped while the editor has unsaved changes.",
  );
  ui.add_space(8.0);
  ui.label(egui::RichText::new("Editor Position").strong().size(14.0));
  ui.add_space(4.0);
  for &pos in EditorPosition::ALL {
    if ui
      .selectable_label(app.editor_position == pos, pos.label())
      .on_hover_cursor(egui::CursorIcon::PointingHand)
      .clicked()
    {
      app.editor_position = pos;
    }
  }
}

/// One line of Studio's About tab: a bold label and a selectable value.
fn about_row(ui: &mut egui::Ui, label: &str, value: &str) {
  ui.horizontal(|ui| {
    ui.label(egui::RichText::new(label).strong());
    // Selectable so the value can be pasted into a bug report.
    ui.add(egui::Label::new(value).selectable(true));
  });
}

/// Render the "About" settings tab content: which binary is running.
fn render_settings_about(ui: &mut egui::Ui) {
  ui.label(egui::RichText::new("LuaCAD Studio").strong().size(16.0));
  ui.add_space(6.0);
  about_row(ui, "Version", version::CRATE_VERSION);
  if !version::GIT_DESCRIBE.is_empty() {
    about_row(ui, "Commit", version::GIT_DESCRIBE);
  }
  about_row(ui, "Target", version::BUILD_TARGET);
  ui.add_space(8.0);
  ui.hyperlink_to("github.com/ad-si/LuaCAD", "https://github.com/ad-si/LuaCAD")
    .on_hover_cursor(egui::CursorIcon::PointingHand);
  ui.add_space(8.0);
  if ui
    .button("Copy version info")
    .on_hover_text("Same text as `luacad-studio --version`")
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
  {
    ui.ctx()
      .copy_text(format!("luacad-studio {}", version::VERSION));
  }
}

/// Render the "Shortcuts" settings tab content.
fn render_settings_shortcuts(ui: &mut egui::Ui) {
  let m = modifier_label();
  let sections: &[(&str, Vec<(String, &str)>)] = &[
    (
      "File",
      vec![
        (format!("{m} O"), "Open file"),
        (format!("{m} S"), "Save file"),
        (format!("{m} ↵"), "Run code"),
        (format!("{m} E"), "Show / hide the code editor"),
      ],
    ),
    (
      "Editing",
      vec![
        (format!("{m} C"), "Copy (whole line if nothing selected)"),
        (format!("{m} X"), "Cut (whole line if nothing selected)"),
        (
          format!("{m} V"),
          "Paste (as a line above if a line was cut)",
        ),
        (format!("{m} F"), "Find"),
        (format!("{m} H"), "Find and replace"),
        (format!("{m} D"), "Select word / next occurrence"),
        (format!("{m} L"), "Select line"),
        (format!("{m} /"), "Toggle comment"),
        ("⌃ D".into(), "Delete character right"),
        ("⌃ W".into(), "Delete word left"),
        ("( [ {".into(), "Wrap selection"),
        ("Tab".into(), "Indent selection"),
        ("⇧ Tab".into(), "Unindent selection"),
      ],
    ),
    (
      "Viewport",
      vec![
        ("Drag".into(), "Rotate camera"),
        ("⌃ Drag".into(), "Pan camera"),
        ("Middle Drag".into(), "Pan camera"),
        ("Scroll".into(), "Zoom in / out"),
        (format!("{m} 4"), "Top"),
        (format!("{m} 5"), "Bottom"),
        (format!("{m} 6"), "Left"),
        (format!("{m} 7"), "Right"),
        (format!("{m} 8"), "Front"),
        (format!("{m} 9"), "Back"),
        (format!("{m} 0"), "Diagonal"),
      ],
    ),
  ];
  for (i, (section, shortcuts)) in sections.iter().enumerate() {
    if i > 0 {
      ui.add_space(6.0);
    }
    ui.label(egui::RichText::new(*section).strong().size(14.0));
    egui::Grid::new(format!("shortcuts_{section}"))
      .num_columns(2)
      .spacing([20.0, 4.0])
      .show(ui, |ui| {
        for (key, desc) in shortcuts {
          ui.label(egui::RichText::new(key.as_str()).monospace());
          ui.label(*desc);
          ui.end_row();
        }
      });
  }
}

/// Describes the layout result so the caller can position the 3D viewport.
pub struct PanelLayout {
  /// The screen rect (in logical pixels) that the 3D scene occupies.
  pub scene_rect: egui::Rect,
}

pub fn render_ui(root_ui: &mut egui::Ui, app: &mut AppState) -> PanelLayout {
  // Re-lint whenever the editor text changes
  app.update_lint();

  let gui_context = root_ui.ctx().clone();
  let gui_context = &gui_context;

  // Apply theme visuals
  if app.theme_colors.egui_dark {
    gui_context.set_visuals(egui::Visuals::dark());
  } else {
    gui_context.set_visuals(egui::Visuals::light());
  }

  // Bottom panel: camera controls and view presets (rendered first so it
  // claims the bottom edge before a potential bottom editor panel).
  egui::Panel::bottom("controls").show(root_ui, |ui| {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
      ui.label("Projection:");
      if ui
        .selectable_label(app.orthogonal_view, "Orthogonal")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.set_orthogonal_view(true);
      }
      if ui
        .selectable_label(!app.orthogonal_view, "Perspective")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.set_orthogonal_view(false);
      }

      ui.separator();
      ui.label("Views:");

      let (az, el) = (app.camera_azimuth, app.camera_elevation);
      let is = |a: f32, e: f32| (az - a).abs() < 1.0 && (el - e).abs() < 1.0;
      let m = modifier_label();

      if ui
        .selectable_label(is(-30.0, 30.0), "Diagonal")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 0"))
        .clicked()
      {
        app.camera_azimuth = -30.0;
        app.camera_elevation = 30.0;
        app.camera_target = [0.0; 3];
      }
      if ui
        .selectable_label(is(-90.0, 89.0), "Top")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 4"))
        .clicked()
      {
        app.camera_azimuth = -90.0;
        app.camera_elevation = 89.0;
      }
      if ui
        .selectable_label(is(-90.0, -89.0), "Bottom")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 5"))
        .clicked()
      {
        app.camera_azimuth = -90.0;
        app.camera_elevation = -89.0;
      }
      if ui
        .selectable_label(is(180.0, 0.0), "Left")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 6"))
        .clicked()
      {
        app.camera_azimuth = 180.0;
        app.camera_elevation = 0.0;
      }
      if ui
        .selectable_label(is(0.0, 0.0), "Right")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 7"))
        .clicked()
      {
        app.camera_azimuth = 0.0;
        app.camera_elevation = 0.0;
      }
      if ui
        .selectable_label(is(-90.0, 0.0), "Front")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 8"))
        .clicked()
      {
        app.camera_azimuth = -90.0;
        app.camera_elevation = 0.0;
      }
      if ui
        .selectable_label(is(90.0, 0.0), "Back")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{m} 9"))
        .clicked()
      {
        app.camera_azimuth = 90.0;
        app.camera_elevation = 0.0;
      }
      ui.separator();
      // The raytrace runs in the background; the button stays disabled
      // until the still is dismissed (camera move, scene change, or its
      // close button), since re-rendering the same view is pointless.
      let can_raytrace = !app.geometries.is_empty()
        && !app.is_raytracing()
        && app.raytrace_texture.is_none()
        && app.raytrace_image.is_none();
      if ui
        .add_enabled(can_raytrace, egui::Button::new("Raytrace"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(
          "Path-trace the current view (an orthographic projection is \
           rendered as the equivalent perspective)",
        )
        .clicked()
      {
        app.pending_raytrace = true;
      }
      ui.separator();
      ui.label("Theme:");
      if ui
        .selectable_label(app.theme_mode == ThemeMode::System, "Auto")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.theme_mode = ThemeMode::System;
        app.theme_colors = app.resolve_theme();
      }
      if ui
        .selectable_label(app.theme_mode == ThemeMode::Light, "Light")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.theme_mode = ThemeMode::Light;
        app.theme_colors = app.resolve_theme();
      }
      if ui
        .selectable_label(app.theme_mode == ThemeMode::Dark, "Dark")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.theme_mode = ThemeMode::Dark;
        app.theme_colors = app.resolve_theme();
      }
      ui.separator();
      // This toggle stays reachable while the editor panel (and with it the
      // Settings button) is hidden.
      if ui
        .selectable_label(app.editor_visible, "Editor")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("Show or hide the code editor ({m} E)"))
        .clicked()
      {
        app.editor_visible = !app.editor_visible;
      }
      // With the editor panel hidden, its Run and Reload buttons move here so
      // an externally edited file can still be re-run.
      if !app.editor_visible {
        ui.separator();
        if ui
          .button("Run")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .on_hover_text(format!("Run code ({m} ↵)"))
          .clicked()
        {
          app.execute_source();
        }
        if ui
          .add_enabled(app.current_file.is_some(), egui::Button::new("Reload"))
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .on_hover_text("Load the latest version from disk and run it")
          .clicked()
        {
          app.pending_file_action = Some(FileAction::Reload);
        }
        // The About button lives in the editor panel, so version information
        // needs a way in here as well.
        if ui
          .button("ℹ About")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .on_hover_text(format!("LuaCAD Studio {}", version::VERSION))
          .clicked()
        {
          app.show_settings = true;
          app.settings_tab = ABOUT_TAB;
        }
      }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
      ui.label(format!(
        "Azimuth: {:.1}  Elevation: {:.1}  Distance: {:.1}",
        app.camera_azimuth, app.camera_elevation, app.camera_distance
      ));
      if !app.geometries.is_empty() {
        // Count what the viewport actually renders: the triangles of all
        // flattened CSG leaves (3 vertices per triangle).
        let total_tris: usize = app
          .csg_groups
          .iter()
          .flat_map(|g| &g.primitives)
          .map(|p| p.vertices.len() / 3)
          .sum();
        ui.separator();
        let num_objects = app.geometries.len();
        ui.label(format!(
          "{} {}, {} Preview {}",
          num_objects,
          if num_objects == 1 {
            "Object"
          } else {
            "Objects"
          },
          total_tris,
          if total_tris == 1 {
            "Triangle"
          } else {
            "Triangles"
          },
        ))
        .on_hover_text(
          "Triangles drawn by the viewport preview (CSG inputs, \
           booleans not applied). The exported mesh can differ — \
           check it with Export or `luacad info`.",
        );
      }
    });
    // With the editor panel hidden, errors and file status messages have no
    // other place to appear.
    if !app.editor_visible {
      if let Some(error) = &app.lua_error {
        ui.colored_label(egui::Color32::RED, format!("Error: {error}"));
      }
      if let Some((msg, is_error)) = &app.export_status {
        let color = if *is_error {
          egui::Color32::RED
        } else {
          egui::Color32::from_rgb(0, 180, 0)
        };
        ui.colored_label(color, msg.as_str());
      }
    }
    ui.add_space(4.0);
  });

  // Editor panel (position depends on settings)
  let screen_rect = gui_context.input(|i| i.viewport_rect());
  let screen_width = screen_rect.width();
  let screen_height = screen_rect.height();

  // Capture position before borrowing app in the closure
  let editor_position = app.editor_position;
  let editor_visible = app.editor_visible;
  if !editor_visible {
    // A hidden editor cannot keep the keyboard focus, and an editor action
    // queued now must not fire once the panel is shown again
    app.editor_focused = false;
    app.pending_editor_action = None;
  }

  // Closure that renders the editor panel contents.
  let mut render_editor_panel = |ui: &mut egui::Ui| {
    // Ensure content fills the panel height so TopBottomPanel persists
    // its resized size correctly (egui stores the frame's rendered rect).
    ui.set_min_height(ui.available_height());
    ui.heading("Code Editor");

    ui.horizontal(|ui| {
      if ui
        .button("New")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.pending_file_action = Some(FileAction::New);
      }
      if ui
        .button("Open")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.pending_file_action = Some(FileAction::Open);
      }
      // Unsaved changes are flagged with an asterisk and an orange button
      let unsaved = app.has_unsaved_changes();
      let save_button = if unsaved {
        egui::Button::new(
          egui::RichText::new("Save *").color(egui::Color32::BLACK),
        )
        .fill(UNSAVED_ORANGE)
      } else {
        egui::Button::new("Save")
      };
      let save_response = ui
        .add(save_button)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
      let save_response = if unsaved {
        save_response.on_hover_text("The file has unsaved changes")
      } else {
        save_response
      };
      if save_response.clicked() {
        app.pending_file_action = Some(FileAction::Save);
      }
      if ui
        .button("Save As")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.pending_file_action = Some(FileAction::SaveAs);
      }
      if ui
        .add_enabled(app.current_file.is_some(), egui::Button::new("Reload"))
        .on_hover_text("Load the latest version from disk")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.pending_file_action = Some(FileAction::Reload);
      }
      ui.separator();
      if ui
        .button("Clear")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.pending_file_action = Some(FileAction::New);
      }
    });
    ui.add_space(8.0);

    // --- Find / Replace bar ---
    if app.search.open {
      // Recompute matches when query, case sensitivity, or text changes
      if !app.search.query.is_empty() {
        let key = (
          app.search.query.clone(),
          app.search.case_sensitive,
          app.text_content.clone(),
        );
        if key != app.search.last_computed {
          // Only a changed query (or case toggle) may move the caret. When the
          // text itself changed the user is typing in the editor, so matches
          // are refreshed without touching their cursor.
          let query_changed = key.0 != app.search.last_computed.0
            || key.1 != app.search.last_computed.1;
          app.search.matches = find_matches(
            &app.text_content,
            &app.search.query,
            app.search.case_sensitive,
          );

          // Set current_match to first match at or after cursor
          if app.search.matches.is_empty() {
            app.search.current_match = None;
          } else {
            // The current match follows the caret, so the counter and the
            // highlight stay meaningful while the user edits the text.
            let cursor_byte =
              byte_index_of(&app.text_content, app.editor_cursor_pos);
            app.search.current_match = Some(
              app
                .search
                .matches
                .iter()
                .position(|m| m.end >= cursor_byte)
                .unwrap_or(0),
            );
            // Only a new query may pull the caret to the match; on a text
            // edit the caret stays where the user put it.
            if query_changed {
              app.search.needs_cursor_update = true;
            }
          }
          app.search.last_computed = key;
        }
      } else {
        app.search.matches.clear();
        app.search.current_match = None;
        app.search.last_computed = Default::default();
      }

      let search_field_id = egui::Id::new("search_query_field");

      ui.horizontal(|ui| {
        let response = ui.add(
          egui::TextEdit::singleline(&mut app.search.query)
            .id(search_field_id)
            .desired_width(200.0)
            .hint_text("Find"),
        );

        // Focus management
        if app.search.focus_search_field {
          response.request_focus();
          if !app.search.query.is_empty()
            && let Some(mut state) =
              egui::TextEdit::load_state(ui.ctx(), search_field_id)
          {
            use egui::text::CCursor;
            use egui::text_selection::CCursorRange;
            state.cursor.set_char_range(Some(CCursorRange::two(
              CCursor::new(0),
              CCursor::new(app.search.query.chars().count()),
            )));
            state.store(ui.ctx(), search_field_id);
          }
          app.search.focus_search_field = false;
        }

        // Enter/Shift+Enter in search field → next/prev match
        if response.lost_focus()
          && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
          if ui.input(|i| i.modifiers.shift) {
            navigate_match(&mut app.search, -1);
          } else {
            navigate_match(&mut app.search, 1);
          }
          response.request_focus();
        }

        // Match count display
        if !app.search.query.is_empty() {
          let count = app.search.matches.len();
          let display = if count == 0 {
            "No results".to_string()
          } else if let Some(idx) = app.search.current_match {
            format!("{} of {}", idx + 1, count)
          } else {
            format!("{} matches", count)
          };
          ui.label(&display);
        }

        if icon_button(ui, LucideIcon::ChevronUp)
          .on_hover_text("Previous (Shift+Enter)")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          navigate_match(&mut app.search, -1);
        }
        if icon_button(ui, LucideIcon::ChevronDown)
          .on_hover_text("Next (Enter)")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          navigate_match(&mut app.search, 1);
        }

        let case_label = if app.search.case_sensitive {
          "Aa ✓"
        } else {
          "Aa"
        };
        if ui
          .selectable_label(app.search.case_sensitive, case_label)
          .on_hover_text("Case sensitive")
          .clicked()
        {
          app.search.case_sensitive = !app.search.case_sensitive;
          app.search.last_computed = Default::default();
        }

        let replace_icon = if app.search.show_replace {
          LucideIcon::ChevronDown
        } else {
          LucideIcon::ChevronRight
        };
        if icon_button(ui, replace_icon)
          .on_hover_text("Toggle replace")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.search.show_replace = !app.search.show_replace;
        }

        if icon_button(ui, LucideIcon::X)
          .on_hover_text("Close (Esc)")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.search.open = false;
          app.search.matches.clear();
          app.search.current_match = None;
          app.search.last_computed = Default::default();
        }
      });

      // Replace row
      if app.search.show_replace {
        ui.horizontal(|ui| {
          ui.add(
            egui::TextEdit::singleline(&mut app.search.replace)
              .desired_width(200.0)
              .hint_text("Replace"),
          );

          let has_match = app.search.current_match.is_some();
          if ui
            .add_enabled(has_match, egui::Button::new("Replace"))
            .clicked()
            && let Some(idx) = app.search.current_match
            && idx < app.search.matches.len()
          {
            let range = app.search.matches[idx].clone();
            let replacement = app.search.replace.clone();
            app.text_content.replace_range(range, &replacement);
            app.search.last_computed = Default::default();
          }

          let has_matches = !app.search.matches.is_empty();
          if ui
            .add_enabled(has_matches, egui::Button::new("Replace All"))
            .clicked()
          {
            let replacement = app.search.replace.clone();
            for range in app.search.matches.iter().rev() {
              app.text_content.replace_range(range.clone(), &replacement);
            }
            app.search.last_computed = Default::default();
          }
        });
      }

      ui.add_space(4.0);
    }

    let mut cursor_line: usize = 1;
    let mut cursor_col: usize = 1;
    let mut selection_len: usize = 0;

    // Reserve space for bottom content, let editor fill the rest.
    // When lint diagnostics are present, reserve extra space so the
    // diagnostic list can grow (up to 200px additional).
    let lint_count = app.lint_diagnostics.len();
    let lint_extra = if lint_count > 0 {
      (lint_count as f32 * 18.0).min(200.0)
    } else {
      0.0
    };
    let bottom_reserve = 160.0 + lint_extra;
    let editor_height = (ui.available_height() - bottom_reserve).max(100.0);

    // Collect lint underline ranges before entering the layouter closure
    let lint_underlines: Vec<(std::ops::Range<usize>, LintSeverity)> = app
      .lint_diagnostics
      .iter()
      .map(|d| (d.byte_start..d.byte_end, d.severity))
      .collect();

    let search_matches: Vec<std::ops::Range<usize>> = if app.search.open {
      app.search.matches.clone()
    } else {
      vec![]
    };
    let current_search_match: Option<usize> = if app.search.open {
      app.search.current_match
    } else {
      None
    };

    egui::ScrollArea::vertical()
      .min_scrolled_height(editor_height)
      .max_height(editor_height)
      .show(ui, |ui| {
        let lint_underlines = &lint_underlines;
        let search_matches = &search_matches;
        // Read before the closure: it outlives the mutable borrow of the
        // buffer that TextEdit takes.
        let scad_syntax = app.is_scad();
        let mut layouter =
          |ui: &egui::Ui, string: &dyn egui::TextBuffer, wrap_width: f32| {
            let string = string.as_str();
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
              // syntect has no OpenSCAD definition; C shares its comments,
              // braces, numbers and strings, so it is the closest fit.
              if scad_syntax { "c" } else { "lua" },
            );
            layout_job.wrap.max_width = wrap_width;

            // Add underlines for lint diagnostics
            for (range, severity) in lint_underlines {
              let color = match severity {
                LintSeverity::Error => egui::Color32::RED,
                LintSeverity::Warning => egui::Color32::from_rgb(220, 120, 0),
              };
              for section in &mut layout_job.sections {
                let s_start = section.byte_range.start.0;
                let s_end = section.byte_range.end.0;
                // If this section overlaps the diagnostic range, underline it
                if s_start < range.end && s_end > range.start {
                  section.format.underline = egui::Stroke::new(1.5_f32, color);
                }
              }
            }

            // Highlight search matches by splitting sections at match boundaries
            if !search_matches.is_empty() {
              // The current match has to stand out from the other matches
              // without the text selection backing it up, so it gets an opaque
              // background with a forced-contrast glyph colour.
              let match_bg =
                egui::Color32::from_rgba_unmultiplied(255, 235, 0, 80);
              let current_bg = egui::Color32::from_rgb(255, 150, 0);
              let current_fg = egui::Color32::BLACK;

              let mut new_sections: Vec<egui::text::LayoutSection> = Vec::new();
              for section in layout_job.sections.drain(..) {
                let s_start = section.byte_range.start.0;
                let s_end = section.byte_range.end.0;

                let overlapping: Vec<(usize, &std::ops::Range<usize>)> =
                  search_matches
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.start < s_end && m.end > s_start)
                    .collect();

                if overlapping.is_empty() {
                  new_sections.push(section);
                  continue;
                }

                let mut cursor = s_start;
                let mut is_first = true;
                for (match_idx, m) in &overlapping {
                  let overlap_start = m.start.max(s_start);
                  let overlap_end = m.end.min(s_end);

                  // Non-match portion before this overlap
                  if cursor < overlap_start {
                    new_sections.push(egui::text::LayoutSection {
                      leading_space: if is_first {
                        section.leading_space
                      } else {
                        0.0
                      },
                      byte_range: cursor.into()..overlap_start.into(),
                      format: section.format.clone(),
                    });
                    is_first = false;
                  }

                  // Match portion with background highlight
                  let is_current = current_search_match == Some(*match_idx);
                  let mut fmt = section.format.clone();
                  if is_current {
                    fmt.background = current_bg;
                    fmt.color = current_fg;
                  } else {
                    fmt.background = match_bg;
                  }
                  new_sections.push(egui::text::LayoutSection {
                    leading_space: if is_first {
                      section.leading_space
                    } else {
                      0.0
                    },
                    byte_range: overlap_start.into()..overlap_end.into(),
                    format: fmt,
                  });
                  is_first = false;
                  cursor = overlap_end;
                }

                // Remainder after last overlap
                if cursor < s_end {
                  new_sections.push(egui::text::LayoutSection {
                    leading_space: 0.0,
                    byte_range: cursor.into()..s_end.into(),
                    format: section.format.clone(),
                  });
                }
              }
              layout_job.sections = new_sections;
            }

            ui.ctx().fonts_mut(|f| f.layout_job(layout_job))
          };

        let te_output = egui::TextEdit::multiline(&mut app.text_content)
          .desired_width(ui.available_width())
          .min_size(egui::vec2(0.0, editor_height))
          .font(egui::TextStyle::Monospace)
          .code_editor()
          .layouter(&mut layouter)
          .show(ui);

        app.editor_focused = te_output.response.has_focus();

        // Handle double/triple clicks in the editor ourselves: egui only acts
        // on the *release* of the second click, and it re-scans the whole
        // buffer to find the word, which takes seconds in a large file.
        let press = ui.input(|i| {
          if i.pointer.button_pressed(egui::PointerButton::Primary) {
            i.pointer.interact_pos().map(|pos| (i.time, pos))
          } else {
            None
          }
        });
        let pointer_in_editor = te_output.response.contains_pointer();
        if let Some((time, pos)) = press
          && pointer_in_editor
        {
          let max_dist = ui.ctx().options(|o| o.input_options.max_click_dist);
          let count = match app.editor_click {
            Some(prev)
              if time - prev.time < DOUBLE_CLICK_SECS
                && egui::pos2(prev.pos.0, prev.pos.1).distance(pos)
                  <= max_dist =>
            {
              prev.count + 1
            }
            _ => 1,
          };
          app.editor_click = Some(EditorClick {
            time,
            pos: (pos.x, pos.y),
            count,
          });

          if count >= 2
            && let Some(range) = te_output.cursor_range
          {
            // The TextEdit has already moved the caret to the press position
            let caret = range.as_sorted_char_range().start.0;
            let (start, end) = if count == 2 {
              double_click_range(&app.text_content, caret)
            } else {
              triple_click_range(&app.text_content, caret)
            };

            let mut state = te_output.state.clone();
            use egui::text::CCursor;
            use egui::text_selection::CCursorRange;
            state.cursor.set_char_range(Some(CCursorRange::two(
              CCursor::new(start),
              CCursor::new(end),
            )));
            state.store(ui.ctx(), te_output.response.id);
          }
        }

        // Keep egui from counting double clicks of its own while the pointer
        // is over the editor — its word lookup walks the entire buffer, so it
        // would stall for seconds on the release of every double click. The
        // option is read at the start of the next frame; short text fields
        // elsewhere keep egui's own handling.
        ui.ctx().options_mut(|o| {
          o.input_options.max_double_click_delay = if pointer_in_editor {
            0.0
          } else {
            DOUBLE_CLICK_SECS
          };
        });

        // Extract cursor position for status line
        if let Some(range) = te_output.cursor_range {
          let sorted = range.as_sorted_char_range();
          let cursor_pos = sorted.end.0;
          selection_len = sorted.end.0 - sorted.start.0;
          app.editor_cursor_pos = cursor_pos;
          app.editor_selection_len = selection_len;

          // Calculate line and column from character offset. The caret
          // counts characters, so it has to be turned into a byte offset
          // before the text is sliced — slicing at the character index cuts
          // multi-byte characters like `ß` in half and panics.
          let cursor_byte = byte_index_of(&app.text_content, cursor_pos);
          let text_before_cursor = &app.text_content[..cursor_byte];
          cursor_line = text_before_cursor.lines().count().max(1);
          // If cursor is right after a newline, it's on the next line
          if text_before_cursor.ends_with('\n') {
            cursor_line += 1;
            cursor_col = 1;
          } else {
            cursor_col = text_before_cursor
              .rsplit_once('\n')
              .map(|(_, after)| after.chars().count() + 1)
              .unwrap_or(cursor_pos + 1);
          }
        }

        // Apply pending editor action (Cmd+D, Cmd+L, Cmd+/)
        if let Some(action) = app.pending_editor_action.take() {
          let (cursor_start, cursor_end) =
            if let Some(range) = te_output.cursor_range {
              let sorted = range.as_sorted_char_range();
              (sorted.start.0, sorted.end.0)
            } else {
              (0, 0)
            };

          let (new_start, new_end) = apply_editor_action(
            &action,
            &mut app.text_content,
            cursor_start,
            cursor_end,
          );

          // Update cursor/selection state
          let mut state = te_output.state.clone();
          use egui::text::CCursor;
          use egui::text_selection::CCursorRange;
          state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(new_start),
            CCursor::new(new_end),
          )));
          state.store(ui.ctx(), te_output.response.id);
        }

        // Navigate cursor to current search match
        if app.search.open && app.search.needs_cursor_update {
          app.search.needs_cursor_update = false;
          if let Some(idx) = app.search.current_match
            && idx < app.search.matches.len()
          {
            let match_range = &app.search.matches[idx];
            let char_start =
              char_index_of(&app.text_content, match_range.start);
            let char_end = char_index_of(&app.text_content, match_range.end);

            let mut state = te_output.state.clone();
            use egui::text::CCursor;
            use egui::text_selection::CCursorRange;
            state.cursor.set_char_range(Some(CCursorRange::two(
              CCursor::new(char_start),
              CCursor::new(char_end),
            )));
            state.store(ui.ctx(), te_output.response.id);

            app.editor_cursor_pos = char_end;
            app.editor_selection_len = char_end - char_start;

            // Center the match in the editor, unless it is already
            // comfortably visible — then the view stays where it is.
            let galley = &te_output.galley;
            let match_rect = galley
              .pos_from_cursor(CCursor::new(char_start))
              .union(galley.pos_from_cursor(CCursor::new(char_end)))
              .translate(te_output.galley_pos.to_vec2());
            let margin = match_rect.height().max(1.0) * 2.0;
            let visible = ui.clip_rect().shrink2(egui::vec2(0.0, margin));
            if match_rect.top() < visible.top()
              || match_rect.bottom() > visible.bottom()
            {
              ui.scroll_to_rect(match_rect, Some(egui::Align::Center));
            }
          }
        }
      });

    ui.add_space(6.0);
    // Status line: cursor position + lines/chars + Run button
    ui.horizontal(|ui| {
      let mut status = format!("Ln {}, Col {}", cursor_line, cursor_col);
      if selection_len > 0 {
        status.push_str(&format!(" ({} selected)", selection_len));
      }
      ui.label(&status);
      ui.separator();
      ui.label(format!(
        "Lines: {}  Chars: {}",
        app.text_content.lines().count(),
        app.text_content.chars().count()
      ));

      let remaining = ui.available_width();
      ui.add_space(remaining - 60.0);
      let run_btn = egui::Button::new(egui::RichText::new("Run").size(18.0))
        .min_size(egui::vec2(60.0, 30.0));
      if ui
        .add(run_btn)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.execute_source();
      }
    });

    ui.separator();

    ui.horizontal(|ui| {
      let has_geometry = !app.geometries.is_empty();

      #[cfg(feature = "csgrs")]
      {
        let csgrs_btn = ui
          .add_enabled(
            has_geometry,
            egui::Button::new(egui::RichText::new("Export via csgrs   ")),
          )
          .on_hover_cursor(egui::CursorIcon::PointingHand);
        paint_dropdown_arrow(ui, &csgrs_btn);
        egui::Popup::from_toggle_button_response(&csgrs_btn)
          .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
          .show(|ui| {
            for &fmt in ExportFormat::ALL {
              if ui
                .button(fmt.label())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
              {
                app.pending_export = Some(fmt);
              }
            }
          });
      }

      let has_scad = app.geometries.iter().any(|g| g.scad.is_some());
      if ui
        .add_enabled(has_scad, egui::Button::new("Export SCAD"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.pending_scad_export = true;
      }

      let export_btn = ui
        .add_enabled(
          has_geometry,
          egui::Button::new(egui::RichText::new("Export   ")),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
      paint_dropdown_arrow(ui, &export_btn);
      egui::Popup::from_toggle_button_response(&export_btn)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
          for &fmt in ManifoldFormat::DROPDOWN {
            if ui
              .button(fmt.label())
              .on_hover_cursor(egui::CursorIcon::PointingHand)
              .clicked()
            {
              app.pending_manifold_export = Some(fmt);
            }
          }
        });

      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let threemf_btn =
          egui::Button::new(egui::RichText::new("Export 3MF").size(18.0))
            .min_size(egui::vec2(100.0, 30.0));
        if ui
          .add_enabled(has_geometry, threemf_btn)
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.pending_manifold_export = Some(ManifoldFormat::ThreeMF);
        }
      });
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
      if ui
        .button("⚙ Settings")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.show_settings = true;
        app.settings_tab = 0;
      }
      if ui
        .button("ℹ About")
        .on_hover_text(format!("LuaCAD Studio {}", version::VERSION))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.show_settings = true;
        app.settings_tab = ABOUT_TAB;
      }
    });

    if let Some(error) = &app.lua_error {
      ui.separator();
      ui.colored_label(egui::Color32::RED, format!("Error: {error}"));
    }

    if !app.lint_diagnostics.is_empty() {
      ui.separator();
      egui::ScrollArea::vertical()
        .id_salt("lint_diagnostics")
        .max_height(ui.available_height())
        .show(ui, |ui| {
          for d in &app.lint_diagnostics {
            let (color, prefix) = match d.severity {
              LintSeverity::Error => (egui::Color32::RED, "error"),
              LintSeverity::Warning => {
                (egui::Color32::from_rgb(220, 120, 0), "warning")
              }
            };
            ui.colored_label(
              color,
              format!(
                "Ln {}, Col {}: {prefix}[{}]: {}",
                d.line, d.column, d.code, d.message,
              ),
            );
          }
        });
    }

    if let Some((msg, is_error)) = &app.export_status {
      ui.separator();
      let color = if *is_error {
        egui::Color32::RED
      } else {
        egui::Color32::from_rgb(0, 180, 0)
      };
      ui.colored_label(color, msg.as_str());
    }
  };

  // Show the editor panel in the configured position (unless it is hidden).
  // Each position uses a distinct ID so egui's persisted size state doesn't
  // conflict across sides (a side panel stores a width, a top/bottom one a
  // height, and both live under the same `Panel` type).
  if editor_visible {
    match editor_position {
      EditorPosition::Right => {
        egui::Panel::right("editor_right")
          .default_size(screen_width * 0.4)
          .min_size(screen_width * 0.2)
          .show(root_ui, |ui| render_editor_panel(ui));
      }
      EditorPosition::Left => {
        egui::Panel::left("editor_left")
          .default_size(screen_width * 0.4)
          .min_size(screen_width * 0.2)
          .show(root_ui, |ui| render_editor_panel(ui));
      }
      EditorPosition::Top => {
        egui::Panel::top("editor_top")
          .default_size(screen_height * 0.4)
          .size_range(100.0..=screen_height * 0.8)
          .resizable(true)
          .show(root_ui, |ui| render_editor_panel(ui));
      }
      EditorPosition::Bottom => {
        egui::Panel::bottom("editor_bottom")
          .default_size(screen_height * 0.4)
          .size_range(100.0..=screen_height * 0.8)
          .resizable(true)
          .show(root_ui, |ui| render_editor_panel(ui));
      }
    }
  }

  // Settings dialog
  if app.show_settings {
    let mut open = true;
    egui::Window::new("Settings")
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
      .show(gui_context, |ui| {
        // Tab bar
        ui.horizontal(|ui| {
          let tabs = ["General", "Shortcuts", "About"];
          for (i, label) in tabs.iter().enumerate() {
            if ui
              .selectable_label(app.settings_tab == i, *label)
              .on_hover_cursor(egui::CursorIcon::PointingHand)
              .clicked()
            {
              app.settings_tab = i;
            }
          }
        });
        ui.separator();

        match app.settings_tab {
          0 => render_settings_general(ui, app),
          1 => render_settings_shortcuts(ui),
          ABOUT_TAB => render_settings_about(ui),
          _ => {}
        }
      });
    if !open {
      app.show_settings = false;
    }
  }

  // Save confirmation when the file was changed on disk by another program
  if app.show_overwrite_confirm {
    egui::Window::new("File Changed on Disk")
      .collapsible(false)
      .resizable(false)
      .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
      .show(gui_context, |ui| {
        ui.label(
          "The file was modified by another program \
           since it was opened in the editor.",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
          if ui
            .button("Overwrite")
            .on_hover_text(
              "Save the editor content, discarding the changes on disk",
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            app.pending_file_action = Some(FileAction::ForceSave);
            app.show_overwrite_confirm = false;
          }
          if ui
            .button("Reload")
            .on_hover_text(
              "Load the file from disk, discarding the editor content",
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            app.pending_file_action = Some(FileAction::Reload);
            app.show_overwrite_confirm = false;
            app.quit_after_save = false;
          }
          if ui
            .button("Cancel")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            app.show_overwrite_confirm = false;
            app.quit_after_save = false;
          }
        });
      });
  }

  // Confirmation when closing the window with unsaved editor changes
  if app.show_close_confirm {
    egui::Window::new("Unsaved Changes")
      .collapsible(false)
      .resizable(false)
      .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
      .show(gui_context, |ui| {
        let file_label = match &app.current_file {
          Some(path) => path.file_name().unwrap_or_default().to_string_lossy(),
          None => "The unsaved file".into(),
        };
        ui.label(format!("{file_label} has unsaved changes."));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
          if ui
            .add(
              egui::Button::new(
                egui::RichText::new("Save & Quit").color(egui::Color32::BLACK),
              )
              .fill(UNSAVED_ORANGE),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            app.pending_file_action = Some(FileAction::Save);
            app.quit_after_save = true;
            app.show_close_confirm = false;
          }
          if ui
            .button("Discard & Quit")
            .on_hover_text("Quit without saving the changes")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            app.should_exit = true;
            app.show_close_confirm = false;
          }
          if ui
            .button("Cancel")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            app.show_close_confirm = false;
          }
        });
      });
  }

  // The scene rect is the remaining area after all panels.
  // We intentionally do NOT use CentralPanel here — that would cause egui
  // to mark mouse events in this area as handled, preventing 3D interaction.
  let scene_rect = root_ui.available_rect_before_wrap();

  // Loading indicator over the render view while the model builds in the
  // background. The previous scene (or the empty startup view) stays visible
  // underneath, and the editor remains fully interactive.
  if app.is_lua_executing() {
    egui::Area::new(egui::Id::new("scene_loading"))
      .order(egui::Order::Foreground)
      .pivot(egui::Align2::CENTER_CENTER)
      .fixed_pos(scene_rect.center())
      .show(gui_context, |ui| {
        ui.vertical_centered(|ui| {
          ui.add(egui::Spinner::new().size(40.0));
          ui.add_space(8.0);
          ui.label(egui::RichText::new("Rendering model…").size(14.0));
        });
      });
  }

  render_raytrace_overlay(gui_context, app, scene_rect);

  PanelLayout { scene_rect }
}

/// Cap for the raytrace resolution: the CLI's fixed render width, so a
/// large or high-DPI viewport doesn't multiply the (already long) render
/// time any further.
const RAYTRACE_MAX_DIM: f32 = 2048.0;

/// Start a requested raytrace, upload a finished one as a texture, and draw
/// the still (or a progress spinner) over the viewport.
fn render_raytrace_overlay(
  gui_context: &egui::Context,
  app: &mut AppState,
  scene_rect: egui::Rect,
) {
  // Start the job here rather than at the button, where the viewport size
  // (needed for the render resolution) is not known yet.
  if app.pending_raytrace {
    app.pending_raytrace = false;
    let size = scene_rect.size() * gui_context.pixels_per_point();
    let scale = (RAYTRACE_MAX_DIM / size.max_elem()).min(1.0);
    let width = (size.x * scale).round().max(1.0) as usize;
    let height = (size.y * scale).round().max(1.0) as usize;
    app.start_raytrace(width, height);
  }

  // Upload a finished raytrace as an egui texture
  if let Some(image) = app.raytrace_image.take() {
    // Letterbox bars continue the image's background color seamlessly
    app.raytrace_bg = [image.rgb[0], image.rgb[1], image.rgb[2]];
    let color_image =
      egui::ColorImage::from_rgb([image.width, image.height], &image.rgb);
    app.raytrace_texture = Some(gui_context.load_texture(
      "raytrace",
      color_image,
      egui::TextureOptions::LINEAR,
    ));
  }

  // Show the still over the viewport, letterboxed to keep its aspect if the
  // window was resized since the render started
  if let Some(texture) = &app.raytrace_texture {
    let painter = gui_context.layer_painter(egui::LayerId::new(
      egui::Order::Middle,
      egui::Id::new("raytrace_view"),
    ));
    let [r, g, b] = app.raytrace_bg;
    painter.rect_filled(
      scene_rect,
      egui::CornerRadius::ZERO,
      egui::Color32::from_rgb(r, g, b),
    );
    let size = texture.size_vec2();
    let scale = (scene_rect.width() / size.x).min(scene_rect.height() / size.y);
    let fitted =
      egui::Rect::from_center_size(scene_rect.center(), size * scale);
    painter.image(
      texture.id(),
      fitted,
      egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
      egui::Color32::WHITE,
    );

    egui::Area::new(egui::Id::new("raytrace_close"))
      .order(egui::Order::Foreground)
      .pivot(egui::Align2::RIGHT_TOP)
      .fixed_pos(scene_rect.right_top() + egui::vec2(-8.0, 8.0))
      .show(gui_context, |ui| {
        if ui
          .button("× Close")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .on_hover_text("Back to the live preview")
          .clicked()
        {
          app.clear_raytrace();
        }
      });
  }

  // Spinner with a scanline progress readout while the raytrace runs. The
  // live preview stays visible (dimmed) and interactive underneath.
  if app.is_raytracing() {
    let painter = gui_context.layer_painter(egui::LayerId::new(
      egui::Order::Middle,
      egui::Id::new("raytrace_progress_dim"),
    ));
    painter.rect_filled(
      scene_rect,
      egui::CornerRadius::ZERO,
      egui::Color32::from_black_alpha(64),
    );
    egui::Area::new(egui::Id::new("raytrace_progress"))
      .order(egui::Order::Foreground)
      .pivot(egui::Align2::CENTER_CENTER)
      .fixed_pos(scene_rect.center())
      .show(gui_context, |ui| {
        // A popup-style frame keeps the readout legible over the model
        egui::Frame::popup(ui.style()).show(ui, |ui| {
          ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new().size(40.0));
            ui.add_space(8.0);
            ui.add(
              egui::Label::new(
                egui::RichText::new(format!(
                  "Raytracing… {:.0} %",
                  100.0 * app.raytrace_progress()
                ))
                .size(14.0),
              )
              .extend(),
            );
          });
        });
      });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::app::AppState;

  /// Drive `render_ui` headlessly and return the app state after the pass.
  struct Harness {
    ctx: egui::Context,
    app: AppState,
    time: f64,
    /// Scene rect returned by the most recent `render_ui` pass
    scene_rect: egui::Rect,
    /// Text painted by the most recent `render_ui` pass
    painted_text: Vec<String>,
  }

  /// Collect the text of every glyph run in a shape tree.
  fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
      egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
      egui::Shape::Vec(shapes) => {
        for shape in shapes {
          collect_text(shape, out);
        }
      }
      _ => {}
    }
  }

  impl Harness {
    fn new(text: &str) -> Self {
      let ctx = egui::Context::default();
      ctx.options_mut(|o| {
        o.input_options.max_double_click_delay = 0.5;
        o.input_options.max_click_dist = 10.0;
      });
      let mut app = AppState::new(None);
      app.text_content = text.to_string();
      Self {
        ctx,
        app,
        time: 0.0,
        scene_rect: egui::Rect::NOTHING,
        painted_text: vec![],
      }
    }

    /// Run one UI pass with the given events, advancing time by `dt` seconds.
    fn pass(&mut self, dt: f64, events: Vec<egui::Event>) {
      self.pass_at_width(dt, events, 1200.0);
    }

    /// Run one UI pass in a window of the given width.
    fn pass_at_width(&mut self, dt: f64, events: Vec<egui::Event>, width: f32) {
      self.time += dt;
      let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
          egui::Pos2::ZERO,
          egui::vec2(width, 800.0),
        )),
        time: Some(self.time),
        events,
        ..Default::default()
      };
      let app = &mut self.app;
      let scene_rect = &mut self.scene_rect;
      let mut output = self.ctx.run_ui(input, |ui| {
        *scene_rect = render_ui(ui, app).scene_rect;
      });
      self.painted_text.clear();
      for clipped in &output.shapes {
        collect_text(&clipped.shape, &mut self.painted_text);
      }
      // Nothing paints in these tests, and epaint asserts that a
      // `TexturesDelta` is not dropped with deltas still pending.
      output.textures_delta.clear();
    }

    fn click_event(pos: egui::Pos2, pressed: bool) -> egui::Event {
      egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
      }
    }

    /// Press the primary button at `pos` (without releasing it).
    fn press(&mut self, dt: f64, pos: egui::Pos2) {
      self.pass(
        dt,
        vec![egui::Event::PointerMoved(pos), Self::click_event(pos, true)],
      );
    }

    fn release(&mut self, dt: f64, pos: egui::Pos2) {
      self.pass(dt, vec![Self::click_event(pos, false)]);
    }

    /// Whether the last pass painted a label containing `needle`.
    fn painted(&self, needle: &str) -> bool {
      self.painted_text.iter().any(|t| t.contains(needle))
    }

    fn selected_text(&self) -> String {
      let end = self.app.editor_cursor_pos;
      let start = end.saturating_sub(self.app.editor_selection_len);
      self
        .app
        .text_content
        .chars()
        .skip(start)
        .take(end - start)
        .collect()
    }
  }

  /// A point inside the word `width` on the first line of the editor.
  const IN_WORD: egui::Pos2 = egui::Pos2 { x: 800.0, y: 65.0 };

  /// Double clicking must not get slower as the file grows: both the word
  /// lookup and egui's own handling used to walk the whole buffer, which made
  /// this take tens of seconds for a few thousand lines.
  #[test]
  fn double_click_stays_fast_in_a_large_file() {
    let text: String = (0..3200)
      .map(|i| format!("local width_{i} = 10 + some_value * factor_{i}\n"))
      .collect();
    let mut h = Harness::new(&text);
    h.pass(0.016, vec![]);
    h.press(0.016, IN_WORD);
    h.release(0.05, IN_WORD);

    let before = std::time::Instant::now();
    h.press(0.2, IN_WORD);
    let press = before.elapsed();
    let before = std::time::Instant::now();
    h.release(0.05, IN_WORD);
    let release = before.elapsed();

    // A pass over a file this size costs a few hundred ms in a debug build;
    // the double click must not add another order of magnitude on top.
    let idle = {
      let before = std::time::Instant::now();
      h.pass(0.016, vec![]);
      before.elapsed()
    };
    let budget = 4 * idle.max(std::time::Duration::from_millis(50));
    assert!(
      press < budget && release < budget,
      "double click cost press={press:?} release={release:?}, idle pass={idle:?}"
    );
    assert_eq!(h.selected_text(), "width_0");
  }

  /// An open `.scad` file goes through the OpenSCAD front end instead of the
  /// Lua one, and everything past that point is the same — the geometry lands
  /// in the viewport exactly as a Lua model's does.
  #[test]
  fn an_open_scad_file_builds_through_the_openscad_front_end() {
    let mut h = Harness::new("difference() { cube(10); sphere(6); }\n");
    assert!(!h.app.is_scad(), "an unsaved buffer is Lua");
    h.app.current_file = Some(std::path::PathBuf::from("model.scad"));
    assert!(h.app.is_scad());

    h.app.execute_source();
    while h.app.is_lua_executing() {
      h.app.poll_lua_job();
      std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(h.app.lua_error, None);
    assert_eq!(h.app.geometries.len(), 1);
    assert!(!h.app.csg_groups.is_empty(), "nothing reached the viewport");

    // The same source is not valid Lua, so the dispatch is what made it work.
    h.app.current_file = Some(std::path::PathBuf::from("model.lua"));
    h.app.execute_source();
    while h.app.is_lua_executing() {
      h.app.poll_lua_job();
      std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
      h.app.lua_error.is_some(),
      "Lua should reject OpenSCAD source"
    );
  }

  /// selene only knows Lua, so an OpenSCAD buffer is left unlinted rather
  /// than reported as one long syntax error.
  #[test]
  fn an_open_scad_file_is_not_linted_as_lua() {
    // Valid OpenSCAD, but `{ … }` after a call is not a Lua block.
    let mut h = Harness::new("difference() { cube(10); }\n");
    h.app.update_lint();
    assert!(
      !h.app.lint_diagnostics.is_empty(),
      "this is not valid Lua, so selene should complain"
    );

    h.app.current_file = Some(std::path::PathBuf::from("model.scad"));
    h.app.update_lint();
    assert!(
      h.app.lint_diagnostics.is_empty(),
      "OpenSCAD must not be linted"
    );
  }

  /// Hiding the editor panel hands its space to the 3D scene, so an external
  /// editor can be used next to a window that is all viewport.
  #[test]
  fn hiding_the_editor_expands_the_scene() {
    let mut h = Harness::new("local width = 10\n");
    h.pass(0.016, vec![]);
    let with_editor = h.scene_rect.width();
    assert!(
      with_editor < 1000.0,
      "editor panel did not claim space: scene is {with_editor} wide"
    );

    h.app.editor_visible = false;
    h.pass(0.016, vec![]);
    assert!(
      h.scene_rect.width() >= 1199.0,
      "hidden editor still claims space: scene is {} wide",
      h.scene_rect.width()
    );
    assert!(!h.app.editor_focused);

    h.app.editor_visible = true;
    h.pass(0.016, vec![]);
    assert_eq!(h.scene_rect.width(), with_editor, "panel did not come back");
  }

  /// The raytrace runs in the background and its result reaches the
  /// viewport as an egui texture: spinner state while it runs, still (with
  /// its background color captured for the letterbox) once it is done, and
  /// the pose snapshot that dismisses it on the next camera move.
  #[test]
  fn a_raytrace_ends_as_a_texture_with_a_matching_pose_snapshot() {
    let mut h = Harness::new("render(cube({size = {10, 10, 10}}))");
    h.app.execute_source();
    while h.app.is_lua_executing() {
      h.app.poll_lua_job();
      std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!h.app.geometries.is_empty(), "Lua produced no geometry");

    h.app.start_raytrace(64, 48);
    assert!(h.app.is_raytracing());
    h.pass(0.016, vec![]);
    assert!(
      h.app.raytrace_texture.is_none(),
      "texture before completion"
    );

    while h.app.is_raytracing() {
      h.app.poll_raytrace_job();
      std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(h.app.raytrace_image.is_some(), "raytrace produced no image");
    assert_eq!(h.app.raytrace_snapshot, Some(h.app.raytrace_view()));

    // The next UI pass uploads the image as a texture and captures the
    // letterbox color from its top-left (background) pixel
    h.pass(0.016, vec![]);
    assert!(h.app.raytrace_image.is_none());
    assert!(h.app.raytrace_texture.is_some());
    assert_ne!(h.app.raytrace_bg, [0; 3]);

    // A camera move invalidates the snapshot — the render loop then
    // dismisses the still
    h.app.camera_azimuth += 10.0;
    assert_ne!(h.app.raytrace_snapshot, Some(h.app.raytrace_view()));
    h.app.clear_raytrace();
    assert!(h.app.raytrace_texture.is_none());
    assert!(h.app.raytrace_snapshot.is_none());
  }

  /// In a narrow window the bottom bar wraps its controls onto more lines
  /// instead of clipping them at the right edge.
  #[test]
  fn narrow_windows_wrap_the_bottom_bar() {
    let mut h = Harness::new("local width = 10\n");
    h.pass(0.016, vec![]);
    let wide_panel_top = h.scene_rect.bottom();
    h.pass_at_width(0.016, vec![], 400.0);
    let narrow_panel_top = h.scene_rect.bottom();
    assert!(
      narrow_panel_top < wide_panel_top - 10.0,
      "bottom bar did not grow at 400px width: its top edge moved from \
       {wide_panel_top} to {narrow_panel_top}"
    );
  }

  #[test]
  fn single_click_does_not_select() {
    let mut h = Harness::new("local width = 10\nlocal height = 20\n");
    h.pass(0.016, vec![]);
    h.press(0.016, IN_WORD);
    h.release(0.016, IN_WORD);
    h.pass(0.016, vec![]);
    assert_eq!(h.app.editor_selection_len, 0);
  }

  #[test]
  fn double_click_selects_word_while_button_is_still_down() {
    let mut h = Harness::new("local width = 10\nlocal height = 20\n");
    h.pass(0.016, vec![]);
    h.press(0.016, IN_WORD);
    h.release(0.05, IN_WORD);
    // Second press only — the button is still held down
    h.press(0.2, IN_WORD);
    // One more pass so the stored selection is picked up by the TextEdit
    h.pass(0.016, vec![]);
    assert_eq!(h.selected_text(), "width");
  }

  #[test]
  fn double_click_at_native_pace_still_selects() {
    let mut h = Harness::new("local width = 10\nlocal height = 20\n");
    h.pass(0.016, vec![]);
    h.press(0.016, IN_WORD);
    h.release(0.06, IN_WORD);
    // 410 ms between the two presses: within the 500 ms the OS considers a
    // double click, but beyond the 300 ms egui counts by default.
    h.press(0.35, IN_WORD);
    h.release(0.06, IN_WORD);
    h.pass(0.016, vec![]);
    // The selection also survives the release of the second click
    assert_eq!(h.selected_text(), "width");
  }

  #[test]
  fn triple_click_selects_line() {
    let mut h = Harness::new("local width = 10\nlocal height = 20\n");
    h.pass(0.016, vec![]);
    h.press(0.016, IN_WORD);
    h.release(0.05, IN_WORD);
    h.press(0.2, IN_WORD);
    h.release(0.05, IN_WORD);
    h.press(0.2, IN_WORD);
    h.pass(0.016, vec![]);
    assert_eq!(h.selected_text(), "local width = 10");
  }

  /// The word picked while the button is held must be the one egui settles on
  /// when it is released — otherwise the selection would visibly jump.
  #[test]
  fn selection_does_not_change_on_release() {
    // x = 770 lands on the space right after `local`
    for pos in [IN_WORD, egui::pos2(770.0, 65.0)] {
      let mut h = Harness::new("local width = 10\nlocal height = 20\n");
      h.pass(0.016, vec![]);
      h.press(0.016, pos);
      h.release(0.05, pos);
      h.press(0.2, pos);
      h.pass(0.016, vec![]);
      let while_pressed = h.selected_text();
      h.release(0.05, pos);
      h.pass(0.016, vec![]);
      assert!(!while_pressed.is_empty(), "nothing selected at {pos:?}");
      assert_eq!(while_pressed, h.selected_text(), "at {pos:?}");
    }
  }

  /// With the find bar open, typing in the editor must insert at the caret —
  /// the match recompute must not pull the caret back to a search hit.
  #[test]
  fn typing_with_the_find_bar_open_does_not_jump_to_a_match() {
    let mut h = Harness::new(
      "local width = 10\nlocal height = 20\nlocal depth = 30\nlocal width2 = 40\n",
    );
    h.app.search.open = true;
    h.app.search.query = "width".to_string();
    // First pass computes the matches and moves the caret to the first one,
    // second pass lets the TextEdit pick that cursor state up.
    h.pass(0.016, vec![]);
    h.pass(0.016, vec![]);
    assert_eq!(
      h.selected_text(),
      "width",
      "search did not select the match"
    );

    // Click on a later line, away from any match
    let pos = egui::pos2(800.0, 130.0);
    h.press(0.016, pos);
    h.release(0.05, pos);
    h.pass(0.016, vec![]);
    let cursor = h.app.editor_cursor_pos;
    assert!(cursor > 0, "click did not place the caret in the editor");

    h.pass(0.016, vec![egui::Event::Text("X".into())]);
    h.pass(0.016, vec![]);
    assert_eq!(
      h.app.editor_cursor_pos,
      cursor + 1,
      "caret moved away from where the character was typed"
    );
    assert_eq!(
      h.app.text_content.chars().nth(cursor),
      Some('X'),
      "character was not inserted at the caret"
    );
    // The find bar keeps tracking a match so the counter stays meaningful
    assert!(h.app.search.current_match.is_some());
  }

  /// A match outside the visible area has to be scrolled to the middle of the
  /// editor — including when it is reached from the find field, where the
  /// editor has no focus and egui's own scroll-to-cursor does not kick in.
  #[test]
  fn navigating_to_a_match_scrolls_it_to_the_middle() {
    // ~14 characters per line, with the match halfway down the file so that
    // centering it is not clamped by either end of the document
    let mut text: String =
      (0..200).map(|i| format!("local a{i} = {i}\n")).collect();
    let needle_at = text.chars().count();
    text.push_str("local needle = 1\n");
    text.push_str(
      &(0..200)
        .map(|i| format!("local b{i} = {i}\n"))
        .collect::<String>(),
    );

    let mut h = Harness::new(&text);
    h.ctx.all_styles_mut(|s| {
      s.scroll_animation = egui::style::ScrollAnimation::none();
    });
    // Two points inside the editor, near its top and its bottom
    let top_probe = egui::pos2(800.0, 100.0);
    let bottom_probe = egui::pos2(800.0, 300.0);

    // Read back the character offset the given point sits on
    let probe = |h: &mut Harness, pos| {
      h.press(0.3, pos);
      h.release(0.05, pos);
      h.pass(0.016, vec![]);
      h.app.editor_cursor_pos
    };

    // Before searching, the editor shows the top of the file
    h.pass(0.016, vec![]);
    let before = probe(&mut h, bottom_probe);
    assert!(
      before < needle_at / 4,
      "editor did not start at the top of the file (at {before})"
    );

    // Search from the find field, so the editor loses focus
    h.app.search.open = true;
    h.app.search.query = "needle".to_string();
    h.app.search.focus_search_field = true;
    for _ in 0..3 {
      h.pass(0.016, vec![]);
    }
    assert!(!h.app.editor_focused, "the editor kept the focus");
    assert_eq!(h.selected_text(), "needle", "match was not selected");

    // The match is now in view with several lines of context above and below,
    // i.e. it sits around the middle rather than against an edge
    let top = probe(&mut h, top_probe);
    let bottom = probe(&mut h, bottom_probe);
    let lines_above = (needle_at as i64 - top as i64) / 14;
    let lines_below = (bottom as i64 - needle_at as i64) / 14;
    assert!(
      lines_above >= 4 && lines_below >= 4,
      "match at {needle_at} is not centered: {lines_above} lines above, \
       {lines_below} lines below (probes hit {top} and {bottom})"
    );
  }

  /// A caret behind a multi-byte character used to crash the studio: the
  /// caret counts characters, and the status line sliced the text at that
  /// offset, which cuts characters like `ß` or an emoji in half.
  #[test]
  fn multi_byte_characters_do_not_crash_the_status_line() {
    let mut h = Harness::new("");
    h.pass(0.016, vec![]);
    // Focus the editor, then type text with multi-byte characters in it
    h.press(0.016, IN_WORD);
    h.release(0.05, IN_WORD);
    // Typed one character at a time, as the status line is recomputed after
    // every keystroke — the crash needs a caret that falls inside a character
    for ch in "straße 🙂 größe".chars() {
      h.pass(0.016, vec![egui::Event::Text(ch.to_string())]);
    }
    h.pass(0.016, vec![]);
    assert_eq!(h.app.text_content, "straße 🙂 größe");
    assert_eq!(
      h.app.editor_cursor_pos,
      h.app.text_content.chars().count(),
      "caret is not behind the typed text"
    );
  }

  /// Searching used to scan a lowercased copy of the text, whose byte offsets
  /// drift apart from the original as soon as lowercasing changes a
  /// character's length — the highlighter then sliced the text mid-character.
  #[test]
  fn find_bar_matches_multi_byte_text_case_insensitively() {
    let mut h = Harness::new("local Größe = 10\nlocal größe = 20\n");
    h.app.search.open = true;
    h.app.search.query = "GRÖßE".to_string();
    h.pass(0.016, vec![]);
    h.pass(0.016, vec![]);
    assert_eq!(h.app.search.matches.len(), 2, "both casings have to match");
    assert_eq!(h.selected_text(), "Größe");
  }

  #[test]
  fn slow_clicks_stay_separate() {
    let mut h = Harness::new("local width = 10\nlocal height = 20\n");
    h.pass(0.016, vec![]);
    h.press(0.016, IN_WORD);
    h.release(0.05, IN_WORD);
    // 1 s apart — two independent single clicks
    h.press(1.0, IN_WORD);
    h.release(0.05, IN_WORD);
    h.pass(0.016, vec![]);
    assert_eq!(h.app.editor_selection_len, 0);
  }

  /// Settings → About is the only place in the GUI that names the running
  /// binary, so it has to show the version.
  #[test]
  fn about_tab_shows_the_version() {
    let mut h = Harness::new("local width = 10\n");
    h.app.show_settings = true;
    h.app.settings_tab = ABOUT_TAB;
    // Two passes: egui gives an `Area` its size on the first one.
    h.pass(0.016, vec![]);
    h.pass(0.016, vec![]);
    assert!(
      h.painted(version::CRATE_VERSION),
      "About tab painted: {:?}",
      h.painted_text
    );
  }

  /// The About button in the bottom bar keeps the version reachable while
  /// the editor panel (which holds the other one) is hidden.
  #[test]
  fn about_button_is_reachable_without_the_editor() {
    let mut h = Harness::new("local width = 10\n");
    h.app.editor_visible = false;
    h.pass(0.016, vec![]);
    assert!(
      h.painted("About"),
      "bottom bar painted: {:?}",
      h.painted_text
    );
  }
}
