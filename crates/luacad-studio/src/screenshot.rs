//! Screenshots of the Studio window: pick an area, mark it up, save it as a
//! PDF next to the model file.
//!
//! The area is dragged out over the live window, then read back from the
//! frame buffer once the selection overlay is gone again (see the read-back
//! in the event loop). The marks are kept as vectors in image-relative
//! coordinates, so they scale with the dialog and go into the PDF as paths
//! rather than as pixels.

use std::path::{Path, PathBuf};

use crate::pdf;

/// The mark-up tools of the screenshot dialog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
  Pen,
  Arrow,
  Rectangle,
  Ellipse,
}

impl Tool {
  pub const ALL: &'static [Tool] =
    &[Tool::Pen, Tool::Arrow, Tool::Rectangle, Tool::Ellipse];

  pub fn label(self) -> &'static str {
    match self {
      Self::Pen => "Pen",
      Self::Arrow => "Arrow",
      Self::Rectangle => "Rectangle",
      Self::Ellipse => "Ellipse",
    }
  }
}

/// The colors the tools can draw in.
const COLORS: [[u8; 3]; 6] = [
  [220, 50, 40],
  [240, 160, 20],
  [40, 160, 80],
  [40, 110, 230],
  [20, 20, 20],
  [250, 250, 250],
];

/// Smallest selection that still counts as an area rather than a stray click,
/// in screen points.
const MIN_SELECTION: f32 = 8.0;

/// One drawn mark. The points are relative to the captured image (0…1 across
/// its width and height), so they survive a differently sized dialog and map
/// onto the PDF page without a second coordinate system.
#[derive(Clone, Debug)]
pub struct Mark {
  pub tool: Tool,
  pub points: Vec<egui::Pos2>,
  pub color: [u8; 3],
  /// Stroke width in captured-image pixels
  pub width: f32,
}

/// Pixels read back from the window, top row first.
pub struct Capture {
  pub width: usize,
  pub height: usize,
  pub rgb: Vec<u8>,
}

pub struct ScreenshotState {
  /// True while the user drags the area out over the window
  pub selecting: bool,
  /// Where the current selection drag started, in screen points
  drag_start: Option<egui::Pos2>,
  /// Window region to read back at the end of this frame, in screen points
  pub pending_capture: Option<egui::Rect>,
  /// The captured pixels; kept while the dialog is open because the PDF
  /// export reads them again
  pub capture: Option<Capture>,
  texture: Option<egui::TextureHandle>,
  pub marks: Vec<Mark>,
  /// The mark currently being dragged out
  drawing: Option<Mark>,
  pub tool: Tool,
  pub color: [u8; 3],
  /// Stroke width in screen points, converted to image pixels per mark
  pub thickness: f32,
  /// The text annotation shown under the image in the PDF
  pub note: String,
  /// One-shot flag: write the PDF. The event loop resolves the path, since
  /// it may have to ask for one.
  pub pending_save: bool,
}

impl Default for ScreenshotState {
  fn default() -> Self {
    Self {
      selecting: false,
      drag_start: None,
      pending_capture: None,
      capture: None,
      texture: None,
      marks: vec![],
      drawing: None,
      tool: Tool::Pen,
      color: COLORS[0],
      thickness: 3.0,
      note: String::new(),
      pending_save: false,
    }
  }
}

impl ScreenshotState {
  /// Start dragging out the area to capture.
  pub fn begin_selection(&mut self) {
    self.close();
    self.selecting = true;
  }

  /// Whether a selection or the dialog is currently on screen.
  pub fn is_active(&self) -> bool {
    self.selecting || self.capture.is_some()
  }

  /// Drop the screenshot and everything drawn on it.
  pub fn close(&mut self) {
    *self = Self {
      tool: self.tool,
      color: self.color,
      thickness: self.thickness,
      ..Self::default()
    };
  }
}

/// Flip an image's rows, converting between OpenGL's bottom-up read-back and
/// the top-down order everything else uses.
pub fn flip_rows(rgb: &[u8], width: usize, height: usize) -> Vec<u8> {
  let stride = width * 3;
  let mut out = Vec::with_capacity(rgb.len());
  for row in (0..height).rev() {
    out.extend_from_slice(&rgb[row * stride..(row + 1) * stride]);
  }
  out
}

/// Read a region of the window's back buffer, given in screen points.
///
/// Has to run after the frame is fully drawn and before the buffers are
/// swapped — and in a frame that no longer paints the selection overlay,
/// which would otherwise end up in the image.
pub fn capture_region(
  gl: &glow::Context,
  region: egui::Rect,
  pixels_per_point: f32,
  frame_width: u32,
  frame_height: u32,
) -> Option<Capture> {
  let left = (region.left() * pixels_per_point).round() as i32;
  let top = (region.top() * pixels_per_point).round() as i32;
  let left = left.clamp(0, frame_width as i32);
  let top = top.clamp(0, frame_height as i32);
  let width = ((region.width() * pixels_per_point).round() as i32)
    .min(frame_width as i32 - left);
  let height = ((region.height() * pixels_per_point).round() as i32)
    .min(frame_height as i32 - top);
  if width < 1 || height < 1 {
    return None;
  }
  // OpenGL counts rows from the bottom of the frame buffer
  let bottom = frame_height as i32 - (top + height);

  let mut rgb = vec![0u8; (width * height * 3) as usize];
  unsafe {
    use glow::HasContext as _;
    gl.read_buffer(glow::BACK);
    // Rows of an RGB image are not multiples of the default 4-byte alignment
    gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
    gl.read_pixels(
      left,
      bottom,
      width,
      height,
      glow::RGB,
      glow::UNSIGNED_BYTE,
      glow::PixelPackData::Slice(Some(&mut rgb)),
    );
  }

  let (width, height) = (width as usize, height as usize);
  Some(Capture {
    width,
    height,
    rgb: flip_rows(&rgb, width, height),
  })
}

/// Draw the area selection over the window and, once the drag ends, queue the
/// selected region for read-back.
pub fn render_selection(
  gui_context: &egui::Context,
  state: &mut ScreenshotState,
  screen_rect: egui::Rect,
) {
  if !state.selecting {
    return;
  }

  // A full-window interactive area keeps the drag away from the 3D view,
  // which would otherwise rotate the camera underneath the selection: with
  // the pointer over an area, egui reports the events as handled.
  egui::Area::new(egui::Id::new("screenshot_selection"))
    .order(egui::Order::Foreground)
    .fixed_pos(screen_rect.min)
    .show(gui_context, |ui| {
      ui.allocate_rect(screen_rect, egui::Sense::click_and_drag());
    });
  gui_context.set_cursor_icon(egui::CursorIcon::Crosshair);

  if gui_context.input(|i| i.key_pressed(egui::Key::Escape)) {
    state.selecting = false;
    state.drag_start = None;
    return;
  }

  // The drag is tracked through the raw pointer state rather than the area's
  // response, so that a press landing in the very frame the overlay appears
  // in still starts a selection.
  let (pressed, released, pointer) = gui_context.input(|i| {
    (
      i.pointer.primary_pressed(),
      i.pointer.primary_released(),
      i.pointer.latest_pos(),
    )
  });
  if pressed {
    state.drag_start = pointer;
  }

  // A release without a press of its own belongs to the click that started
  // the selection — the button reports that click on release, in this very
  // pass. Acting on it would end the selection before it began.
  if released && let Some(start) = state.drag_start {
    if let Some(end) = pointer {
      let region = egui::Rect::from_two_pos(start, end).intersect(screen_rect);
      if region.width() >= MIN_SELECTION && region.height() >= MIN_SELECTION {
        state.pending_capture = Some(region);
      }
    }
    state.selecting = false;
    state.drag_start = None;
    // Returning before anything is painted is what makes the capture at the
    // end of this frame show the window without the overlay on top.
    return;
  }

  let painter = gui_context.layer_painter(egui::LayerId::new(
    egui::Order::Foreground,
    egui::Id::new("screenshot_overlay"),
  ));
  let dim = egui::Color32::from_black_alpha(96);
  let selection = match (state.drag_start, pointer) {
    (Some(start), Some(end)) => Some(egui::Rect::from_two_pos(start, end)),
    _ => None,
  };

  match selection {
    Some(selection) => {
      // Dim everything around the selection, so the area being taken stays
      // at full brightness
      let selection = selection.intersect(screen_rect);
      for around in [
        egui::Rect::from_min_max(
          screen_rect.min,
          egui::pos2(screen_rect.right(), selection.top()),
        ),
        egui::Rect::from_min_max(
          egui::pos2(screen_rect.left(), selection.bottom()),
          screen_rect.max,
        ),
        egui::Rect::from_min_max(
          egui::pos2(screen_rect.left(), selection.top()),
          egui::pos2(selection.left(), selection.bottom()),
        ),
        egui::Rect::from_min_max(
          egui::pos2(selection.right(), selection.top()),
          egui::pos2(screen_rect.right(), selection.bottom()),
        ),
      ] {
        painter.rect_filled(around, egui::CornerRadius::ZERO, dim);
      }
      painter.rect_stroke(
        selection,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.0, egui::Color32::WHITE),
        egui::StrokeKind::Outside,
      );
      painter.text(
        selection.left_top() - egui::vec2(0.0, 6.0),
        egui::Align2::LEFT_BOTTOM,
        format!(
          "{} × {}",
          selection.width().round(),
          selection.height().round()
        ),
        egui::FontId::monospace(12.0),
        egui::Color32::WHITE,
      );
    }
    None => {
      painter.rect_filled(screen_rect, egui::CornerRadius::ZERO, dim);
      painter.text(
        egui::pos2(screen_rect.center().x, screen_rect.top() + 40.0),
        egui::Align2::CENTER_CENTER,
        "Drag to select the area to capture — Esc to cancel",
        egui::FontId::proportional(16.0),
        egui::Color32::WHITE,
      );
    }
  }
}

/// Show the captured screenshot with the mark-up tools, the note field and
/// the save button.
pub fn render_dialog(
  gui_context: &egui::Context,
  state: &mut ScreenshotState,
  current_file: Option<&Path>,
  screen_rect: egui::Rect,
) {
  if state.capture.is_none() {
    return;
  }
  if state.texture.is_none() {
    let uploaded = state.capture.as_ref().map(|capture| {
      gui_context.load_texture(
        "screenshot",
        egui::ColorImage::from_rgb(
          [capture.width, capture.height],
          &capture.rgb,
        ),
        egui::TextureOptions::LINEAR,
      )
    });
    state.texture = uploaded;
  }
  let (Some(capture), Some(texture)) = (&state.capture, &state.texture) else {
    return;
  };

  // Show the image at the size it was taken at, shrunk to leave room for the
  // toolbar, the note field and the buttons.
  let natural = egui::vec2(capture.width as f32, capture.height as f32)
    / gui_context.pixels_per_point();
  let scale = ((screen_rect.width() - 80.0) / natural.x)
    .min((screen_rect.height() - 260.0) / natural.y)
    .clamp(0.1, 1.0);
  let display_size = natural * scale;
  let image_pixels_per_point = capture.width as f32 / display_size.x;
  let texture_id = texture.id();

  let mut close = false;
  let mut open = true;
  egui::Window::new("Screenshot")
    .open(&mut open)
    .collapsible(false)
    .resizable(false)
    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
    .show(gui_context, |ui| {
      ui.horizontal(|ui| {
        for &tool in Tool::ALL {
          if ui
            .selectable_label(state.tool == tool, tool.label())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
          {
            state.tool = tool;
          }
        }
        ui.separator();
        for color in COLORS {
          if color_swatch(ui, color, state.color == color).clicked() {
            state.color = color;
          }
        }
        ui.separator();
        ui.add(
          egui::Slider::new(&mut state.thickness, 1.0..=12.0)
            .show_value(false)
            .text("Thickness"),
        );
        ui.separator();
        if ui
          .add_enabled(!state.marks.is_empty(), egui::Button::new("Undo"))
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          state.marks.pop();
        }
        if ui
          .add_enabled(!state.marks.is_empty(), egui::Button::new("Clear"))
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          state.marks.clear();
        }
      });

      ui.add_space(6.0);
      let (image_rect, response) =
        ui.allocate_exact_size(display_size, egui::Sense::drag());
      let painter = ui.painter_at(image_rect);
      painter.image(
        texture_id,
        image_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
      );
      handle_drawing(state, &response, image_rect, image_pixels_per_point);
      for mark in state.marks.iter().chain(state.drawing.iter()) {
        paint_mark(&painter, image_rect, mark, image_pixels_per_point);
      }
      if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
      }

      ui.add_space(8.0);
      ui.add(
        egui::TextEdit::multiline(&mut state.note)
          .desired_rows(3)
          .desired_width(display_size.x)
          .hint_text("Note — printed under the image"),
      );

      ui.add_space(8.0);
      ui.horizontal(|ui| {
        if ui
          .button("Save PDF")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          state.pending_save = true;
        }
        if ui
          .button("Cancel")
          .on_hover_cursor(egui::CursorIcon::PointingHand)
          .clicked()
        {
          close = true;
        }
        match output_path(current_file) {
          Some(path) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            ui.label(format!("Saves as {name}")).on_hover_text(format!(
              "Next to the model file, named after the current time:\n{}",
              path.display()
            ));
          }
          None => {
            ui.label("Save the model first, or pick a location");
          }
        }
      });
    });

  if close || !open {
    state.close();
  }
}

/// Where the PDF of an unmodified document goes: next to the model file,
/// timestamped like every other export. `None` for a document that has no
/// file yet — the event loop then asks for a location.
pub fn output_path(current_file: Option<&Path>) -> Option<PathBuf> {
  let directory = current_file?.parent()?;
  Some(directory.join(crate::timestamped_filename(current_file, "pdf")))
}

/// A small filled square that picks a mark-up color.
fn color_swatch(
  ui: &mut egui::Ui,
  color: [u8; 3],
  selected: bool,
) -> egui::Response {
  let (rect, response) =
    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
  if ui.is_rect_visible(rect) {
    painter_swatch(ui, rect, color, selected, response.hovered());
  }
  response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn painter_swatch(
  ui: &egui::Ui,
  rect: egui::Rect,
  color: [u8; 3],
  selected: bool,
  hovered: bool,
) {
  let [r, g, b] = color;
  ui.painter().rect_filled(
    rect.shrink(2.0),
    egui::CornerRadius::same(2),
    egui::Color32::from_rgb(r, g, b),
  );
  let outline = if selected {
    egui::Stroke::new(2.0, ui.visuals().strong_text_color())
  } else if hovered {
    egui::Stroke::new(1.0, ui.visuals().text_color())
  } else {
    egui::Stroke::new(1.0, ui.visuals().weak_text_color())
  };
  ui.painter().rect_stroke(
    rect.shrink(1.0),
    egui::CornerRadius::same(3),
    outline,
    egui::StrokeKind::Middle,
  );
}

/// Turn a drag over the image into a mark.
fn handle_drawing(
  state: &mut ScreenshotState,
  response: &egui::Response,
  image_rect: egui::Rect,
  image_pixels_per_point: f32,
) {
  let relative = |pos: egui::Pos2| {
    egui::pos2(
      ((pos.x - image_rect.left()) / image_rect.width()).clamp(0.0, 1.0),
      ((pos.y - image_rect.top()) / image_rect.height()).clamp(0.0, 1.0),
    )
  };

  if response.drag_started()
    && let Some(pos) = response.interact_pointer_pos()
  {
    state.drawing = Some(Mark {
      tool: state.tool,
      points: vec![relative(pos)],
      color: state.color,
      width: state.thickness * image_pixels_per_point,
    });
  }

  if response.dragged()
    && let Some(pos) = response.interact_pointer_pos()
    && let Some(mark) = state.drawing.as_mut()
  {
    let point = relative(pos);
    match mark.tool {
      // Freehand keeps every step, thinned out so a slow drag does not pile
      // up thousands of points
      Tool::Pen => {
        let far_enough = mark
          .points
          .last()
          .is_none_or(|last| last.distance(point) > 0.002);
        if far_enough {
          mark.points.push(point);
        }
      }
      // The other tools are defined by where the drag started and where it
      // is now
      _ => {
        if mark.points.len() < 2 {
          mark.points.push(point);
        } else {
          mark.points[1] = point;
        }
      }
    }
  }

  if response.drag_stopped()
    && let Some(mark) = state.drawing.take()
    && mark.points.len() >= 2
  {
    state.marks.push(mark);
  }
}

/// The two barbs of an arrow head, given the tip, the point the shaft comes
/// from and the barb length. Works in any coordinate system, so the dialog
/// and the PDF draw the same arrow.
fn arrow_barbs(
  tip: (f32, f32),
  from: (f32, f32),
  length: f32,
) -> [(f32, f32); 2] {
  let (dx, dy) = (from.0 - tip.0, from.1 - tip.1);
  let magnitude = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
  let (dx, dy) = (dx / magnitude * length, dy / magnitude * length);
  // ±25° off the shaft
  let (sin, cos) = 25_f32.to_radians().sin_cos();
  [
    (tip.0 + dx * cos - dy * sin, tip.1 + dx * sin + dy * cos),
    (tip.0 + dx * cos + dy * sin, tip.1 - dx * sin + dy * cos),
  ]
}

/// How long an arrow head is, for a shaft of the given width.
fn arrow_head_length(width: f32) -> f32 {
  width * 5.0
}

fn paint_mark(
  painter: &egui::Painter,
  image_rect: egui::Rect,
  mark: &Mark,
  image_pixels_per_point: f32,
) {
  let [r, g, b] = mark.color;
  let stroke = egui::Stroke::new(
    (mark.width / image_pixels_per_point).max(1.0),
    egui::Color32::from_rgb(r, g, b),
  );
  let at = |point: &egui::Pos2| {
    egui::pos2(
      image_rect.left() + point.x * image_rect.width(),
      image_rect.top() + point.y * image_rect.height(),
    )
  };
  let (Some(first), Some(last)) = (mark.points.first(), mark.points.last())
  else {
    return;
  };
  let (start, end) = (at(first), at(last));

  match mark.tool {
    Tool::Pen => {
      let points: Vec<egui::Pos2> = mark.points.iter().map(at).collect();
      painter.add(egui::Shape::line(points, stroke));
    }
    Tool::Arrow => {
      painter.line_segment([start, end], stroke);
      for barb in arrow_barbs(
        (end.x, end.y),
        (start.x, start.y),
        arrow_head_length(stroke.width),
      ) {
        painter.line_segment([end, egui::pos2(barb.0, barb.1)], stroke);
      }
    }
    Tool::Rectangle => {
      painter.rect_stroke(
        egui::Rect::from_two_pos(start, end),
        egui::CornerRadius::ZERO,
        stroke,
        egui::StrokeKind::Middle,
      );
    }
    Tool::Ellipse => {
      let rect = egui::Rect::from_two_pos(start, end);
      painter.add(egui::Shape::ellipse_stroke(
        rect.center(),
        rect.size() / 2.0,
        stroke,
      ));
    }
  }
}

/// A4 in PDF units, with a margin wide enough to print without clipping.
const PAGE_WIDTH: f32 = 595.276;
const PAGE_HEIGHT: f32 = 841.89;
const MARGIN: f32 = 40.0;
const NOTE_SIZE: f32 = 11.0;
const NOTE_LEADING: f32 = 15.0;
/// Space between the image and the note
const NOTE_GAP: f32 = 20.0;

/// Lay the screenshot, its marks and the note out on A4 pages and write them
/// to `path`.
pub fn save_pdf(state: &ScreenshotState, path: &Path) -> Result<(), String> {
  let capture = state
    .capture
    .as_ref()
    .ok_or_else(|| "no screenshot to save".to_string())?;
  let document = build_document(capture, &state.marks, &state.note);
  pdf::write(&document, path).map_err(|e| e.to_string())
}

/// Build the PDF: the image with its marks on the first page, the note below
/// it, and any note that does not fit on further pages.
fn build_document(
  capture: &Capture,
  marks: &[Mark],
  note: &str,
) -> pdf::Document {
  let content_width = PAGE_WIDTH - 2.0 * MARGIN;
  let content_height = PAGE_HEIGHT - 2.0 * MARGIN;

  let note = note.trim_end();
  let lines = if note.is_empty() {
    vec![]
  } else {
    pdf::wrap(note, NOTE_SIZE, content_width)
  };

  // Leave room for the note, but never shrink the image below 40 % of the
  // page — a long note continues on the next page instead.
  let note_height = if lines.is_empty() {
    0.0
  } else {
    NOTE_GAP + lines.len() as f32 * NOTE_LEADING
  };
  let image_budget = (content_height - note_height).max(content_height * 0.4);
  let scale = (content_width / capture.width as f32)
    .min(image_budget / capture.height as f32);
  let image_width = capture.width as f32 * scale;
  let image_height = capture.height as f32 * scale;
  let image_x = MARGIN + (content_width - image_width) / 2.0;
  let image_y = PAGE_HEIGHT - MARGIN - image_height;

  let mut pages = vec![pdf::Page {
    image: Some((image_x, image_y, image_width, image_height)),
    marks: marks
      .iter()
      .flat_map(|mark| {
        pdf_marks(mark, image_x, image_y, image_width, image_height, scale)
      })
      .collect(),
    text: vec![],
  }];

  // The note starts under the image and runs on to further pages if it has
  // to, so that nothing is silently dropped.
  let mut baseline = image_y - NOTE_GAP - NOTE_SIZE;
  for line in lines {
    if baseline < MARGIN {
      pages.push(pdf::Page::default());
      baseline = PAGE_HEIGHT - MARGIN - NOTE_SIZE;
    }
    pages
      .last_mut()
      .expect("at least one page")
      .text
      .push(pdf::Text {
        x: MARGIN,
        y: baseline,
        size: NOTE_SIZE,
        color: [20, 20, 20],
        text: line,
      });
    baseline -= NOTE_LEADING;
  }

  pdf::Document {
    page_width: PAGE_WIDTH,
    page_height: PAGE_HEIGHT,
    image: Some(pdf::Image {
      width: capture.width,
      height: capture.height,
      rgb: capture.rgb.clone(),
    }),
    pages,
    producer: format!("LuaCAD Studio {}", luacad::version::CRATE_VERSION),
  }
}

/// Convert one mark from image-relative coordinates into PDF page space.
fn pdf_marks(
  mark: &Mark,
  image_x: f32,
  image_y: f32,
  image_width: f32,
  image_height: f32,
  scale: f32,
) -> Vec<pdf::Mark> {
  // PDF counts upwards from the bottom of the page
  let at = |point: &egui::Pos2| {
    (
      image_x + point.x * image_width,
      image_y + (1.0 - point.y) * image_height,
    )
  };
  let width = (mark.width * scale).max(0.4);
  let color = mark.color;
  let (Some(first), Some(last)) = (mark.points.first(), mark.points.last())
  else {
    return vec![];
  };
  let (start, end) = (at(first), at(last));

  match mark.tool {
    Tool::Pen => vec![pdf::Mark::Polyline {
      points: mark.points.iter().map(at).collect(),
      color,
      width,
    }],
    Tool::Arrow => {
      let mut out = vec![pdf::Mark::Polyline {
        points: vec![start, end],
        color,
        width,
      }];
      for barb in arrow_barbs(end, start, arrow_head_length(width)) {
        out.push(pdf::Mark::Polyline {
          points: vec![end, barb],
          color,
          width,
        });
      }
      out
    }
    Tool::Rectangle => vec![pdf::Mark::Rect {
      x: start.0.min(end.0),
      y: start.1.min(end.1),
      width: (end.0 - start.0).abs(),
      height: (end.1 - start.1).abs(),
      color,
      stroke: width,
    }],
    Tool::Ellipse => vec![pdf::Mark::Ellipse {
      cx: (start.0 + end.0) / 2.0,
      cy: (start.1 + end.1) / 2.0,
      rx: (end.0 - start.0).abs() / 2.0,
      ry: (end.1 - start.1).abs() / 2.0,
      color,
      stroke: width,
    }],
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn capture(width: usize, height: usize) -> Capture {
    Capture {
      width,
      height,
      rgb: vec![128; width * height * 3],
    }
  }

  #[test]
  fn read_back_rows_are_turned_right_side_up() {
    let rgb = vec![
      1, 1, 1, 2, 2, 2, // bottom row as OpenGL returns it
      3, 3, 3, 4, 4, 4, // top row
    ];
    assert_eq!(
      flip_rows(&rgb, 2, 2),
      vec![3, 3, 3, 4, 4, 4, 1, 1, 1, 2, 2, 2]
    );
  }

  #[test]
  fn the_image_fills_the_page_width_and_keeps_its_aspect() {
    let document = build_document(&capture(1600, 900), &[], "");
    let (x, _, width, height) =
      document.pages[0].image.expect("the image is placed");
    assert_eq!(document.pages.len(), 1);
    assert!((width - (PAGE_WIDTH - 2.0 * MARGIN)).abs() < 0.01);
    assert!((x - MARGIN).abs() < 0.01);
    assert!((width / height - 1600.0 / 900.0).abs() < 0.01);
  }

  /// A portrait screenshot is limited by the page height, not its width.
  #[test]
  fn a_tall_image_is_bounded_by_the_page_height() {
    let document = build_document(&capture(400, 2000), &[], "");
    let (_, y, _, height) =
      document.pages[0].image.expect("the image is placed");
    assert!(height <= PAGE_HEIGHT - 2.0 * MARGIN + 0.01);
    assert!(y >= MARGIN - 0.01);
  }

  #[test]
  fn the_note_goes_under_the_image() {
    let document = build_document(&capture(800, 600), &[], "Check this edge");
    let (_, image_y, _, _) = document.pages[0].image.expect("image");
    let text = &document.pages[0].text;
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].text, "Check this edge");
    assert!(text[0].y < image_y, "the note overlaps the image");
    assert!(text[0].y >= MARGIN);
  }

  /// A note too long for the first page continues on the next one rather
  /// than being cut off.
  #[test]
  fn a_long_note_continues_on_further_pages() {
    let note = "word ".repeat(2000);
    let document = build_document(&capture(800, 600), &[], &note);
    assert!(document.pages.len() > 1, "the note was truncated");
    assert!(document.pages[0].image.is_some());
    assert!(document.pages[1].image.is_none());
    let lines: usize = document.pages.iter().map(|p| p.text.len()).sum();
    assert_eq!(
      lines,
      pdf::wrap(note.trim_end(), NOTE_SIZE, PAGE_WIDTH - 2.0 * MARGIN).len()
    );
    for page in &document.pages {
      for text in &page.text {
        assert!(text.y >= MARGIN, "a line fell off the page");
      }
    }
  }

  /// Marks are stored relative to the image, so they have to land on the
  /// image in the PDF — including the y flip.
  #[test]
  fn marks_land_on_the_image_in_the_pdf() {
    let mark = Mark {
      tool: Tool::Rectangle,
      // Upper left quarter of the image
      points: vec![egui::pos2(0.0, 0.0), egui::pos2(0.5, 0.5)],
      color: [255, 0, 0],
      width: 4.0,
    };
    let document = build_document(&capture(800, 600), &[mark], "");
    let (image_x, image_y, image_width, image_height) =
      document.pages[0].image.expect("image");
    let pdf::Mark::Rect {
      x,
      y,
      width,
      height,
      ..
    } = &document.pages[0].marks[0]
    else {
      panic!("expected a rectangle");
    };
    assert!((x - image_x).abs() < 0.01);
    assert!((width - image_width / 2.0).abs() < 0.01);
    assert!((height - image_height / 2.0).abs() < 0.01);
    // The top half of the image is the upper half of the placed rectangle
    assert!((y + height - (image_y + image_height)).abs() < 0.01);
  }

  #[test]
  fn an_arrow_becomes_a_shaft_and_two_barbs() {
    let mark = Mark {
      tool: Tool::Arrow,
      points: vec![egui::pos2(0.1, 0.1), egui::pos2(0.9, 0.9)],
      color: [0, 0, 0],
      width: 4.0,
    };
    let document = build_document(&capture(800, 600), &[mark], "");
    assert_eq!(document.pages[0].marks.len(), 3);
  }

  #[test]
  fn the_output_lands_next_to_the_model_file() {
    let path = output_path(Some(Path::new("/tmp/parts/bracket.lua")))
      .expect("a saved file has a directory");
    assert_eq!(path.parent().unwrap(), Path::new("/tmp/parts"));
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    assert!(name.ends_with("_bracket.pdf"), "unexpected name {name}");
    assert!(output_path(None).is_none());
  }

  #[test]
  fn saving_writes_a_pdf_file() {
    let directory = std::env::temp_dir()
      .join(format!("luacad-screenshot-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp directory");
    let model = directory.join("bracket.lua");
    let state = ScreenshotState {
      capture: Some(capture(120, 80)),
      note: "Off by 0.2 mm".to_string(),
      ..Default::default()
    };

    let path = output_path(Some(&model)).expect("a path next to the model");
    save_pdf(&state, &path).expect("the PDF is written");
    let written = std::fs::read(&path).expect("the file exists");
    assert!(written.starts_with(b"%PDF"));

    std::fs::remove_dir_all(&directory).ok();
  }

  #[test]
  fn closing_keeps_the_tool_settings_but_drops_the_image() {
    let mut state = ScreenshotState {
      tool: Tool::Arrow,
      thickness: 7.0,
      capture: Some(capture(4, 4)),
      note: "gone".to_string(),
      marks: vec![Mark {
        tool: Tool::Pen,
        points: vec![egui::pos2(0.0, 0.0)],
        color: [0, 0, 0],
        width: 1.0,
      }],
      ..Default::default()
    };
    state.close();
    assert!(!state.is_active());
    assert!(state.capture.is_none());
    assert!(state.marks.is_empty());
    assert!(state.note.is_empty());
    assert_eq!(state.tool, Tool::Arrow);
    assert_eq!(state.thickness, 7.0);
  }
}
