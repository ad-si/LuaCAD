use egui_extras::syntax_highlighting;
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
use luacad::export::ManifoldFormat;
use luacad::linter::LintSeverity;

use crate::app::{
  AppState, EditorClick, EditorPosition, FileAction, SearchState,
};
use crate::editor::{
  apply_editor_action, double_click_range, triple_click_range,
};
use crate::theme::ThemeMode;

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
      ],
    ),
    (
      "Editing",
      vec![
        (format!("{m} C"), "Copy"),
        (format!("{m} X"), "Cut"),
        (format!("{m} V"), "Paste"),
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
    ui.horizontal(|ui| {
      ui.label("Projection:");
      let ratio = 1.0 / (22.5_f32).to_radians().tan();
      if ui
        .selectable_label(app.orthogonal_view, "Orthogonal")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
        && !app.orthogonal_view
      {
        app.camera_distance /= ratio;
        app.orthogonal_view = true;
      }
      if ui
        .selectable_label(!app.orthogonal_view, "Perspective")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
        && app.orthogonal_view
      {
        app.camera_distance *= ratio;
        app.orthogonal_view = false;
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
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
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
          "{} {}, {} {}",
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
        ));
      }
    });
    ui.add_space(4.0);
  });

  // Editor panel (position depends on settings)
  let screen_rect = gui_context.input(|i| i.viewport_rect());
  let screen_width = screen_rect.width();
  let screen_height = screen_rect.height();

  // Capture position before borrowing app in the closure
  let editor_position = app.editor_position;

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
          app.search.matches.clear();
          let query = &app.search.query;
          let text = &app.text_content;

          if app.search.case_sensitive {
            let mut start = 0;
            while let Some(pos) = text[start..].find(query.as_str()) {
              let abs = start + pos;
              app.search.matches.push(abs..abs + query.len());
              start = abs + query.len().max(1);
            }
          } else {
            let query_lower = query.to_lowercase();
            let text_lower = text.to_lowercase();
            let mut start = 0;
            while let Some(pos) = text_lower[start..].find(query_lower.as_str())
            {
              let abs = start + pos;
              app.search.matches.push(abs..abs + query_lower.len());
              start = abs + query_lower.len().max(1);
            }
          }

          // Set current_match to first match at or after cursor
          if app.search.matches.is_empty() {
            app.search.current_match = None;
          } else {
            // The current match follows the caret, so the counter and the
            // highlight stay meaningful while the user edits the text.
            let cursor_byte: usize = app
              .text_content
              .chars()
              .take(app.editor_cursor_pos)
              .collect::<String>()
              .len();
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
              "lua",
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
              let match_bg =
                egui::Color32::from_rgba_premultiplied(255, 255, 0, 60);
              let current_bg =
                egui::Color32::from_rgba_premultiplied(255, 165, 0, 100);

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
                  let bg = if current_search_match == Some(*match_idx) {
                    current_bg
                  } else {
                    match_bg
                  };
                  let mut fmt = section.format.clone();
                  fmt.background = bg;
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

          // Calculate line and column from character offset
          let text_before_cursor =
            &app.text_content[..cursor_pos.min(app.text_content.len())];
          cursor_line = text_before_cursor.lines().count().max(1);
          // If cursor is right after a newline, it's on the next line
          if text_before_cursor.ends_with('\n') {
            cursor_line += 1;
            cursor_col = 1;
          } else {
            cursor_col = text_before_cursor
              .rsplit_once('\n')
              .map(|(_, after)| after.len() + 1)
              .unwrap_or(text_before_cursor.len() + 1);
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
              app.text_content[..match_range.start].chars().count();
            let char_end = app.text_content[..match_range.end].chars().count();

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
        app.text_content.len()
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
        app.execute_lua_code();
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

  // Show the editor panel in the configured position.
  // Each position uses a distinct ID so egui's persisted size state doesn't
  // conflict across sides (a side panel stores a width, a top/bottom one a
  // height, and both live under the same `Panel` type).
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
          let tabs = ["General", "Shortcuts"];
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

  PanelLayout { scene_rect }
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
      }
    }

    /// Run one UI pass with the given events, advancing time by `dt` seconds.
    fn pass(&mut self, dt: f64, events: Vec<egui::Event>) {
      self.time += dt;
      let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
          egui::Pos2::ZERO,
          egui::vec2(1200.0, 800.0),
        )),
        time: Some(self.time),
        events,
        ..Default::default()
      };
      let app = &mut self.app;
      let mut output = self.ctx.run_ui(input, |ui| {
        render_ui(ui, app);
      });
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
}
