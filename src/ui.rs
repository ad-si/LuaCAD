use egui_extras::syntax_highlighting;
use three_d::egui;

use crate::app::{AppState, FileAction};
use crate::editor::apply_editor_action;
use crate::export::{ExportFormat, OpenScadFormat};
use crate::theme::ThemeMode;

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
      let title = match &app.current_file {
        Some(path) => format!(
          "Code Editor - {}",
          path.file_name().unwrap_or_default().to_string_lossy()
        ),
        None => "Code Editor".to_string(),
      };
      ui.heading(title);

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

      // Reserve space for bottom content, let editor fill the rest
      let bottom_reserve = 160.0; // status line + separator + export buttons + shortcuts + errors
      let editor_height = (ui.available_height() - bottom_reserve).max(100.0);

      egui::ScrollArea::vertical()
        .min_scrolled_height(editor_height)
        .max_height(editor_height)
        .show(ui, |ui| {
          let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
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
        let has_scad = app.geometries.iter().any(|g| g.scad.is_some());

        let csgrs_popup_id = ui.make_persistent_id("csgrs_export_popup");
        let csgrs_btn = ui
          .add_enabled(
            has_geometry,
            egui::Button::new(egui::RichText::new("Export via csgrs   ")),
          )
          .on_hover_cursor(egui::CursorIcon::PointingHand);
        paint_dropdown_arrow(ui, &csgrs_btn);
        if csgrs_btn.clicked() {
          ui.memory_mut(|mem| mem.toggle_popup(csgrs_popup_id));
        }
        egui::popup_below_widget(
          ui,
          csgrs_popup_id,
          &csgrs_btn,
          egui::PopupCloseBehavior::CloseOnClick,
          |ui| {
            for &fmt in ExportFormat::ALL {
              if ui
                .button(fmt.label())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
              {
                app.pending_export = Some(fmt);
              }
            }
          },
        );

        let openscad_popup_id = ui.make_persistent_id("openscad_export_popup");
        let openscad_btn = ui
          .add_enabled(
            has_scad,
            egui::Button::new(egui::RichText::new("Export via OpenSCAD   ")),
          )
          .on_hover_cursor(egui::CursorIcon::PointingHand);
        paint_dropdown_arrow(ui, &openscad_btn);
        if openscad_btn.clicked() {
          ui.memory_mut(|mem| mem.toggle_popup(openscad_popup_id));
        }
        egui::popup_below_widget(
          ui,
          openscad_popup_id,
          &openscad_btn,
          egui::PopupCloseBehavior::CloseOnClick,
          |ui| {
            for &fmt in OpenScadFormat::ALL {
              if ui
                .button(fmt.label())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
              {
                app.pending_openscad_export = Some(fmt);
              }
            }
          },
        );

        if ui
          .add_enabled(has_scad, egui::Button::new("Export SCAD"))
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          app.pending_export = Some(ExportFormat::OpenSCAD);
        }
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

      if let Some((msg, is_error)) = &app.export_status {
        ui.separator();
        let color = if *is_error {
          egui::Color32::RED
        } else {
          egui::Color32::GREEN
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
        let sections: &[(&str, &[(&str, &str)])] = &[
          (
            "File",
            &[
              ("⌘ O", "Open file"),
              ("⌘ S", "Save file"),
              ("⌘ ↵", "Run code"),
            ],
          ),
          (
            "Editing",
            &[
              ("⌘ C", "Copy"),
              ("⌘ X", "Cut"),
              ("⌘ V", "Paste"),
              ("⌘ D", "Select word / next occurrence"),
              ("⌘ L", "Select line"),
              ("⌘ G", "Toggle comment"),
              ("Tab", "Indent selection"),
              ("⇧ Tab", "Unindent selection"),
            ],
          ),
          (
            "Viewport",
            &[("Drag", "Rotate camera"), ("Scroll", "Zoom in / out")],
          ),
        ];
        for (i, &(section, shortcuts)) in sections.iter().enumerate() {
          if i > 0 {
            ui.add_space(6.0);
          }
          ui.label(egui::RichText::new(section).strong().size(14.0));
          egui::Grid::new(format!("shortcuts_{section}"))
            .num_columns(2)
            .spacing([20.0, 4.0])
            .show(ui, |ui| {
              for &(key, desc) in shortcuts {
                ui.label(egui::RichText::new(key).monospace());
                ui.label(desc);
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

      if ui
        .selectable_label(is(-30.0, 30.0), "Default")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.camera_azimuth = -30.0;
        app.camera_elevation = 30.0;
      }
      if ui
        .selectable_label(is(0.0, 90.0), "Top")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.camera_azimuth = 0.0;
        app.camera_elevation = 89.0;
      }
      if ui
        .selectable_label(is(0.0, -90.0), "Bottom")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.camera_azimuth = 0.0;
        app.camera_elevation = -89.0;
      }
      if ui
        .selectable_label(is(0.0, 0.0), "Front")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.camera_azimuth = 0.0;
        app.camera_elevation = 0.0;
      }
      if ui
        .selectable_label(is(180.0, 0.0), "Back")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.camera_azimuth = 180.0;
        app.camera_elevation = 0.0;
      }
      if ui
        .selectable_label(is(-90.0, 0.0), "Left")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.camera_azimuth = -90.0;
        app.camera_elevation = 0.0;
      }
      if ui
        .selectable_label(is(90.0, 0.0), "Right")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
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
        let total_polys: usize =
          app.geometries.iter().map(|g| g.mesh.polygons.len()).sum();
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
