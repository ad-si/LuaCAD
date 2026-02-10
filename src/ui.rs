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

  // Right panel: code editor
  let panel_response = egui::SidePanel::right("code_editor")
        .default_width(400.0)
        .show(gui_context, |ui| {
            let title = match &app.current_file {
                Some(path) => format!("Code Editor - {}", path.file_name().unwrap_or_default().to_string_lossy()),
                None => "Code Editor".to_string(),
            };
            ui.heading(title);

            let editor_height = ui.available_height() - 100.0;
            egui::ScrollArea::vertical()
                .max_height(editor_height)
                .show(ui, |ui| {
                    let mut layouter =
                        |ui: &egui::Ui, string: &str, wrap_width: f32| {
                            let theme = if ui.style().visuals.dark_mode {
                                syntax_highlighting::CodeTheme::dark(14.0)
                            } else {
                                syntax_highlighting::CodeTheme::light(14.0)
                            };

                            let mut layout_job =
                                syntax_highlighting::highlight(
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
                            .desired_rows(30)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .layouter(&mut layouter)
                            .show(ui);

                    // Apply pending editor action (Cmd+D, Cmd+L, Cmd+G)
                    if let Some(action) = app.pending_editor_action.take() {
                        let (cursor_start, cursor_end) = if let Some(range) = te_output.cursor_range {
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

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("Open").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.pending_file_action = Some(FileAction::Open);
                }
                if ui.button("Save").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.pending_file_action = Some(FileAction::Save);
                }
                if ui.button("Save As").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.pending_file_action = Some(FileAction::SaveAs);
                }
                ui.separator();
                if ui.button("Clear").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.text_content.clear();
                    app.geometries.clear();
                    app.lua_error = None;
                    app.scene_dirty = true;
                    app.current_file = None;
                }

                if ui.button("Load Example").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.text_content = "-- CSG Boolean Operations Demo\n\n-- Create a hollow box\nlocal outer = cube(30, 20, 15)\nlocal inner = cube(26, 16, 15):translate(2, 2, 2)\nlocal box = outer - inner\n\n-- Cut a window in the front\nlocal window = cube(10, 4, 8):translate(10, -1, 4)\nlocal result = box - window\n\nrender(result)".to_string();
                    app.current_file = None;
                }

                let remaining = ui.available_width();
                ui.add_space(remaining - 60.0);
                let run_btn = egui::Button::new(
                    egui::RichText::new("Run").size(18.0),
                ).min_size(egui::vec2(60.0, 30.0));
                if ui.add(run_btn).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.execute_lua_code();
                }
            });

            ui.horizontal(|ui| {
                let has_geometry = !app.geometries.is_empty();
                let has_scad = app.geometries.iter().any(|g| g.scad.is_some());

                let csgrs_popup_id = ui.make_persistent_id("csgrs_export_popup");
                let csgrs_btn = ui.add_enabled(has_geometry,
                    egui::Button::new(egui::RichText::new("Export via csgrs   ")))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                paint_dropdown_arrow(ui, &csgrs_btn);
                if csgrs_btn.clicked() {
                    ui.memory_mut(|mem| mem.toggle_popup(csgrs_popup_id));
                }
                egui::popup_below_widget(ui, csgrs_popup_id, &csgrs_btn, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    for &fmt in ExportFormat::ALL {
                        if ui.button(fmt.label()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            app.pending_export = Some(fmt);
                        }
                    }
                });

                let openscad_popup_id = ui.make_persistent_id("openscad_export_popup");
                let openscad_btn = ui.add_enabled(has_scad,
                    egui::Button::new(egui::RichText::new("Export via OpenSCAD   ")))
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                paint_dropdown_arrow(ui, &openscad_btn);
                if openscad_btn.clicked() {
                    ui.memory_mut(|mem| mem.toggle_popup(openscad_popup_id));
                }
                egui::popup_below_widget(ui, openscad_popup_id, &openscad_btn, egui::PopupCloseBehavior::CloseOnClick, |ui| {
                    for &fmt in OpenScadFormat::ALL {
                        if ui.button(fmt.label()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            app.pending_openscad_export = Some(fmt);
                        }
                    }
                });

                if ui.add_enabled(has_scad, egui::Button::new("Export SCAD")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.pending_export = Some(ExportFormat::OpenSCAD);
                }
            });

            ui.add_space(6.0);
            ui.label(format!("Lines: {}  Chars: {}", app.text_content.lines().count(), app.text_content.len()));
            ui.add_space(4.0);
            ui.label("⌘O Open  ⌘S Save  ⌘D Select Word/Next  ⌘L Select Line  ⌘G Toggle Comment");

            if let Some(error) = &app.lua_error {
                ui.separator();
                ui.colored_label(
                    egui::Color32::RED,
                    format!("Error: {error}"),
                );
            }

            if !app.geometries.is_empty() {
                let total_polys: usize = app.geometries.iter()
                    .map(|g| g.mesh.polygons.len()).sum();
                ui.separator();
                ui.colored_label(
                    egui::Color32::GREEN,
                    format!("{} object(s), {} polygons", app.geometries.len(), total_polys),
                );
            }

            if let Some((msg, is_error)) = &app.export_status {
                ui.separator();
                let color = if *is_error { egui::Color32::RED } else { egui::Color32::GREEN };
                ui.colored_label(color, msg.as_str());
            }
        });
  let right_panel_width = panel_response.response.rect.width();

  // Bottom panel: camera controls and view presets
  egui::TopBottomPanel::bottom("controls").show(gui_context, |ui| {
    ui.horizontal(|ui| {
      ui.label(format!(
        "Az: {:.1} El: {:.1} Dist: {:.1}",
        app.camera_azimuth, app.camera_elevation, app.camera_distance
      ));
      ui.separator();

      ui.label("Projection:");
      if ui
        .selectable_label(app.orthogonal_view, "Orthogonal")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
        app.orthogonal_view = true;
      }
      if ui
        .selectable_label(!app.orthogonal_view, "Perspective")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
      {
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

    ui.label("Mouse: Drag to rotate, Scroll to zoom");
  });

  right_panel_width
}
