use egui_extras::syntax_highlighting;
#[cfg(feature = "csgrs")]
use luacad::export::ExportFormat;
use luacad::export::ManifoldFormat;
use luacad::linter::LintSeverity;
use three_d::egui;

use crate::app::{AppState, FileAction};
use crate::editor::apply_editor_action;
use crate::theme::ThemeMode;

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

pub fn render_ui(gui_context: &egui::Context, app: &mut AppState) -> f32 {
  // Re-lint whenever the editor text changes
  app.update_lint();

  // Apply theme visuals
  if app.theme_colors.egui_dark {
    gui_context.set_visuals(egui::Visuals::dark());
  } else {
    gui_context.set_visuals(egui::Visuals::light());
  }

  // Right panel: code editor (~1/3 of screen)
  let screen_width = gui_context.screen_rect().width();
  // Force panel to 30% of screen width for the first few frames while the
  // window size settles (default_width alone is unreliable on frame 0 with
  // maximized windows because screen_rect may not reflect the final size yet).
  let panel_id = egui::Id::new("code_editor");
  let frame_count: u32 = gui_context.data_mut(|d| {
    let c = d.get_temp_mut_or_default::<u32>(panel_id.with("_frames"));
    *c += 1;
    *c
  });
  if frame_count <= 3 {
    let target_width = screen_width * 0.4;
    let screen = gui_context.screen_rect();
    let rect = egui::Rect::from_min_max(
      egui::pos2(screen.right() - target_width, screen.top()),
      screen.right_bottom(),
    );
    gui_context.data_mut(|d| {
      d.insert_persisted(
        panel_id,
        egui::containers::panel::PanelState { rect },
      );
    });
  }
  let panel_response = egui::SidePanel::right("code_editor")
    .default_width(screen_width * 0.4)
    .min_width(screen_width * 0.2)
    .show(gui_context, |ui| {
      ui.heading("Code Editor");

      ui.horizontal(|ui| {
        if ui
          .button("Open")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.pending_file_action = Some(FileAction::Open);
        }
        if ui
          .button("Save")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.pending_file_action = Some(FileAction::Save);
        }
        if ui
          .button("Save As")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.pending_file_action = Some(FileAction::SaveAs);
        }
        ui.separator();
        if ui
          .button("Clear")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.text_content.clear();
          app.geometries.clear();
          app.lua_error = None;
          app.scene_dirty = true;
          app.current_file = None;
        }
      });
      ui.add_space(8.0);

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

      egui::ScrollArea::vertical()
        .min_scrolled_height(editor_height)
        .max_height(editor_height)
        .show(ui, |ui| {
          let lint_underlines = &lint_underlines;
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
                  let s_start = section.byte_range.start;
                  let s_end = section.byte_range.end;
                  // If this section overlaps the diagnostic range, underline it
                  if s_start < range.end && s_end > range.start {
                    section.format.underline = egui::Stroke::new(1.5, color);
                  }
                }
              }

              ui.fonts(|f| f.layout_job(layout_job))
            };

          let te_output = egui::TextEdit::multiline(&mut app.text_content)
            .desired_width(ui.available_width())
            .min_size(egui::vec2(0.0, editor_height))
            .font(egui::TextStyle::Monospace)
            .code_editor()
            .layouter(&mut layouter)
            .show(ui);

          // Extract cursor position for status line
          if let Some(range) = te_output.cursor_range {
            let sorted = range.as_sorted_char_range();
            let cursor_pos = sorted.end;
            selection_len = sorted.end - sorted.start;
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

          // Apply pending editor action (Cmd+D, Cmd+L, Cmd+G)
          if let Some(action) = app.pending_editor_action.take() {
            let (cursor_start, cursor_end) =
              if let Some(range) = te_output.cursor_range {
                let sorted = range.as_sorted_char_range();
                (sorted.start, sorted.end)
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

        ui.with_layout(
          egui::Layout::right_to_left(egui::Align::Center),
          |ui| {
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
          },
        );
      });

      ui.add_space(6.0);
      ui.horizontal(|ui| {
        if ui
          .button("Shortcuts")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.show_shortcuts = true;
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
    });
  let right_panel_width = panel_response.response.rect.width();

  // Keyboard shortcuts modal
  if app.show_shortcuts {
    let mut open = true;
    egui::Window::new("Keyboard Shortcuts")
      .open(&mut open)
      .collapsible(false)
      .resizable(false)
      .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
      .show(gui_context, |ui| {
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
              (format!("{m} D"), "Select word / next occurrence"),
              (format!("{m} L"), "Select line"),
              (format!("{m} G"), "Toggle comment"),
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
      });
    if !open {
      app.show_shortcuts = false;
    }
  }

  // Bottom panel: camera controls and view presets
  egui::TopBottomPanel::bottom("controls").show(gui_context, |ui| {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
      ui.label("Projection:");
      // Ortho visible half-height = camera_distance (height_param=2.0, zoom=distance).
      // Perspective visible half-height at target = camera_distance * tan(22.5°).
      // Scale distance on toggle to keep same visible extent.
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
        let total_polys: usize = {
          #[cfg(feature = "csgrs")]
          {
            app
              .geometries
              .iter()
              .map(|g| g.mesh.as_ref().map_or(0, |m| m.polygons.len()))
              .sum()
          }
          #[cfg(not(feature = "csgrs"))]
          {
            0
          }
        };
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
          total_polys,
          if total_polys == 1 {
            "Polygon"
          } else {
            "Polygons"
          },
        ));
      }
    });
    ui.add_space(4.0);
  });

  right_panel_width
}
