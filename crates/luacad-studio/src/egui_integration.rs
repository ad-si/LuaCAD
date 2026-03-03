use crate::camera::Viewport;
use crate::input::*;
use std::sync::Arc;

pub struct EguiIntegration {
  painter: egui_glow::Painter,
  egui_context: egui::Context,
  output: Option<egui::FullOutput>,
  viewport: Viewport,
  modifiers: Modifiers,
}

impl EguiIntegration {
  pub fn new(gl: Arc<glow::Context>) -> Self {
    Self {
      egui_context: egui::Context::default(),
      painter: egui_glow::Painter::new(gl, "", None, true).unwrap(),
      output: None,
      viewport: Viewport::new_at_origo(1, 1),
      modifiers: Modifiers::default(),
    }
  }

  /// Process events and run the egui UI callback.
  /// Returns whether egui wants input (pointer or keyboard).
  pub fn update(
    &mut self,
    events: &mut [Event],
    accumulated_time_in_ms: f64,
    viewport: Viewport,
    device_pixel_ratio: f32,
    callback: impl FnOnce(&egui::Context),
  ) -> bool {
    self.egui_context.set_pixels_per_point(device_pixel_ratio);
    self.viewport = viewport;

    let egui_input = egui::RawInput {
      screen_rect: Some(egui::Rect {
        min: egui::Pos2 {
          x: viewport.x as f32 / device_pixel_ratio,
          y: viewport.y as f32 / device_pixel_ratio,
        },
        max: egui::Pos2 {
          x: viewport.x as f32 / device_pixel_ratio
            + viewport.width as f32 / device_pixel_ratio,
          y: viewport.y as f32 / device_pixel_ratio
            + viewport.height as f32 / device_pixel_ratio,
        },
      }),
      time: Some(accumulated_time_in_ms * 0.001),
      modifiers: egui_modifiers(&self.modifiers),
      events: events
        .iter()
        .filter_map(|event| {
          translate_event(event, viewport, device_pixel_ratio)
        })
        .collect(),
      ..Default::default()
    };

    self.egui_context.begin_pass(egui_input);
    callback(&self.egui_context);
    self.output = Some(self.egui_context.end_pass());

    // Mark events as handled if egui wants the input
    for event in events.iter_mut() {
      if let Event::ModifiersChange { modifiers } = event {
        self.modifiers = *modifiers;
      }
      if self.egui_context.wants_pointer_input() {
        match event {
          Event::MousePress { handled, .. }
          | Event::MouseRelease { handled, .. }
          | Event::MouseWheel { handled, .. }
          | Event::MouseMotion { handled, .. } => {
            *handled = true;
          }
          _ => {}
        }
      }
      if self.egui_context.wants_keyboard_input() {
        match event {
          Event::KeyPress { handled, .. }
          | Event::KeyRelease { handled, .. } => {
            *handled = true;
          }
          _ => {}
        }
      }
    }

    self.egui_context.wants_pointer_input()
      || self.egui_context.wants_keyboard_input()
  }

  /// Render the egui output. Call after update().
  pub fn render(&mut self) {
    let output = self
      .output
      .take()
      .expect("call EguiIntegration::update before render");
    let scale = self.egui_context.pixels_per_point();
    let clipped_meshes = self.egui_context.tessellate(output.shapes, scale);
    self.painter.paint_and_update_textures(
      [self.viewport.width, self.viewport.height],
      scale,
      &clipped_meshes,
      &output.textures_delta,
    );
    unsafe {
      use glow::HasContext as _;
      self.painter.gl().disable(glow::FRAMEBUFFER_SRGB);
    }
  }
}

impl Drop for EguiIntegration {
  fn drop(&mut self) {
    self.painter.destroy();
  }
}

fn translate_event(
  event: &Event,
  viewport: Viewport,
  dpr: f32,
) -> Option<egui::Event> {
  match event {
    Event::KeyPress {
      kind,
      modifiers,
      handled,
    } => {
      if !handled {
        Some(egui::Event::Key {
          key: egui_key(kind),
          pressed: true,
          modifiers: egui_modifiers(modifiers),
          repeat: false,
          physical_key: None,
        })
      } else {
        None
      }
    }
    Event::KeyRelease {
      kind,
      modifiers,
      handled,
    } => {
      if !handled {
        Some(egui::Event::Key {
          key: egui_key(kind),
          pressed: false,
          modifiers: egui_modifiers(modifiers),
          repeat: false,
          physical_key: None,
        })
      } else {
        None
      }
    }
    Event::MousePress {
      button,
      position,
      modifiers,
      handled,
    } => {
      if !handled {
        Some(egui::Event::PointerButton {
          pos: physical_to_egui(*position, viewport, dpr),
          button: egui_button(button),
          pressed: true,
          modifiers: egui_modifiers(modifiers),
        })
      } else {
        None
      }
    }
    Event::MouseRelease {
      button,
      position,
      modifiers,
      handled,
    } => {
      if !handled {
        Some(egui::Event::PointerButton {
          pos: physical_to_egui(*position, viewport, dpr),
          button: egui_button(button),
          pressed: false,
          modifiers: egui_modifiers(modifiers),
        })
      } else {
        None
      }
    }
    Event::MouseMotion {
      position, handled, ..
    } => {
      if !handled {
        Some(egui::Event::PointerMoved(physical_to_egui(
          *position, viewport, dpr,
        )))
      } else {
        None
      }
    }
    Event::Text(text) => Some(egui::Event::Text(text.clone())),
    Event::MouseLeave => Some(egui::Event::PointerGone),
    Event::MouseWheel {
      delta,
      handled,
      modifiers,
      ..
    } => {
      if !handled {
        Some(egui::Event::MouseWheel {
          delta: egui::Vec2::new(delta.0, delta.1),
          unit: egui::MouseWheelUnit::Point,
          modifiers: egui_modifiers(modifiers),
        })
      } else {
        None
      }
    }
    _ => None,
  }
}

/// Convert physical pixel coordinates (y from bottom) to egui logical
/// coordinates (y from top).
fn physical_to_egui(
  pos: PhysicalPoint,
  viewport: Viewport,
  dpr: f32,
) -> egui::Pos2 {
  egui::Pos2 {
    x: pos.x / dpr,
    y: (viewport.height as f32 - pos.y) / dpr,
  }
}

fn egui_key(key: &Key) -> egui::Key {
  match key {
    Key::ArrowDown => egui::Key::ArrowDown,
    Key::ArrowLeft => egui::Key::ArrowLeft,
    Key::ArrowRight => egui::Key::ArrowRight,
    Key::ArrowUp => egui::Key::ArrowUp,
    Key::Escape => egui::Key::Escape,
    Key::Tab => egui::Key::Tab,
    Key::Backspace => egui::Key::Backspace,
    Key::Enter => egui::Key::Enter,
    Key::Space => egui::Key::Space,
    Key::Insert => egui::Key::Insert,
    Key::Delete => egui::Key::Delete,
    Key::Home => egui::Key::Home,
    Key::End => egui::Key::End,
    Key::PageUp => egui::Key::PageUp,
    Key::PageDown => egui::Key::PageDown,
    Key::Num0 => egui::Key::Num0,
    Key::Num1 => egui::Key::Num1,
    Key::Num2 => egui::Key::Num2,
    Key::Num3 => egui::Key::Num3,
    Key::Num4 => egui::Key::Num4,
    Key::Num5 => egui::Key::Num5,
    Key::Num6 => egui::Key::Num6,
    Key::Num7 => egui::Key::Num7,
    Key::Num8 => egui::Key::Num8,
    Key::Num9 => egui::Key::Num9,
    Key::A => egui::Key::A,
    Key::B => egui::Key::B,
    Key::C => egui::Key::C,
    Key::D => egui::Key::D,
    Key::E => egui::Key::E,
    Key::F => egui::Key::F,
    Key::G => egui::Key::G,
    Key::H => egui::Key::H,
    Key::I => egui::Key::I,
    Key::J => egui::Key::J,
    Key::K => egui::Key::K,
    Key::L => egui::Key::L,
    Key::M => egui::Key::M,
    Key::N => egui::Key::N,
    Key::O => egui::Key::O,
    Key::P => egui::Key::P,
    Key::Q => egui::Key::Q,
    Key::R => egui::Key::R,
    Key::S => egui::Key::S,
    Key::T => egui::Key::T,
    Key::U => egui::Key::U,
    Key::V => egui::Key::V,
    Key::W => egui::Key::W,
    Key::X => egui::Key::X,
    Key::Y => egui::Key::Y,
    Key::Z => egui::Key::Z,
    Key::Slash => egui::Key::Slash,
  }
}

fn egui_modifiers(modifiers: &Modifiers) -> egui::Modifiers {
  egui::Modifiers {
    alt: modifiers.alt,
    ctrl: modifiers.ctrl,
    shift: modifiers.shift,
    command: modifiers.command,
    mac_cmd: cfg!(target_os = "macos") && modifiers.command,
  }
}

fn egui_button(button: &MouseButton) -> egui::PointerButton {
  match button {
    MouseButton::Left => egui::PointerButton::Primary,
    MouseButton::Right => egui::PointerButton::Secondary,
    MouseButton::Middle => egui::PointerButton::Middle,
  }
}
