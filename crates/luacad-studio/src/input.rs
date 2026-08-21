use crate::camera::Viewport;
use std::time::Instant;
use winit::event::{TouchPhase, WindowEvent};

/// A pixel coordinate in physical pixels (x from left, y from bottom).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PhysicalPoint {
  pub x: f32,
  pub y: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum MouseButton {
  Left,
  Right,
  Middle,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
  pub alt: bool,
  pub ctrl: bool,
  pub shift: bool,
  pub command: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum Key {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Escape,
  Tab,
  Backspace,
  Enter,
  Space,
  Insert,
  Delete,
  Home,
  End,
  PageUp,
  PageDown,
  Num0,
  Num1,
  Num2,
  Num3,
  Num4,
  Num5,
  Num6,
  Num7,
  Num8,
  Num9,
  A,
  B,
  C,
  D,
  E,
  F,
  G,
  H,
  I,
  J,
  K,
  L,
  M,
  N,
  O,
  P,
  Q,
  R,
  S,
  T,
  U,
  V,
  W,
  X,
  Y,
  Z,
  Slash,
}

#[derive(Clone, Debug)]
pub enum Event {
  MousePress {
    button: MouseButton,
    position: PhysicalPoint,
    modifiers: Modifiers,
    handled: bool,
  },
  MouseRelease {
    button: MouseButton,
    position: PhysicalPoint,
    modifiers: Modifiers,
    handled: bool,
  },
  MouseMotion {
    button: Option<MouseButton>,
    delta: (f32, f32),
    position: PhysicalPoint,
    handled: bool,
  },
  MouseWheel {
    delta: (f32, f32),
    position: PhysicalPoint,
    modifiers: Modifiers,
    handled: bool,
  },
  MouseEnter,
  MouseLeave,
  KeyPress {
    kind: Key,
    modifiers: Modifiers,
    handled: bool,
  },
  KeyRelease {
    kind: Key,
    modifiers: Modifiers,
    handled: bool,
  },
  ModifiersChange {
    modifiers: Modifiers,
  },
  Text(String),
}

pub struct FrameInput {
  pub events: Vec<Event>,
  pub accumulated_time: f64,
  pub viewport: Viewport,
  pub device_pixel_ratio: f32,
}

/// Internal logical point for coordinate conversion.
#[derive(Debug, Copy, Clone, PartialEq)]
struct LogicalPoint {
  x: f32,
  y: f32,
  device_pixel_ratio: f32,
  height: f32,
}

impl From<LogicalPoint> for PhysicalPoint {
  fn from(value: LogicalPoint) -> Self {
    Self {
      x: value.x * value.device_pixel_ratio,
      y: value.height - value.y * value.device_pixel_ratio,
    }
  }
}

pub struct FrameInputGenerator {
  last_time: Instant,
  first_frame: bool,
  events: Vec<Event>,
  accumulated_time: f64,
  viewport: Viewport,
  device_pixel_ratio: f64,
  cursor_pos: Option<LogicalPoint>,
  finger_id: Option<u64>,
  secondary_cursor_pos: Option<LogicalPoint>,
  secondary_finger_id: Option<u64>,
  modifiers: Modifiers,
  mouse_pressed: Option<MouseButton>,
}

impl FrameInputGenerator {
  pub fn from_winit_window(window: &winit::window::Window) -> Self {
    let size = window.inner_size();
    let device_pixel_ratio = window.scale_factor();
    Self {
      events: Vec::new(),
      accumulated_time: 0.0,
      viewport: Viewport::new_at_origo(size.width, size.height),
      device_pixel_ratio,
      first_frame: true,
      last_time: Instant::now(),
      cursor_pos: None,
      finger_id: None,
      secondary_cursor_pos: None,
      secondary_finger_id: None,
      modifiers: Modifiers::default(),
      mouse_pressed: None,
    }
  }

  pub fn generate(&mut self) -> FrameInput {
    let now = Instant::now();
    let duration = now.duration_since(self.last_time);
    let elapsed_ms = duration.as_secs() as f64 * 1000.0
      + duration.subsec_nanos() as f64 * 1e-6;
    self.accumulated_time += elapsed_ms;
    self.last_time = now;

    let frame_input = FrameInput {
      // Hands over the buffer and leaves an empty one behind, exactly as
      // draining did, but without reallocating for the copy.
      events: std::mem::take(&mut self.events),
      accumulated_time: self.accumulated_time,
      viewport: self.viewport,
      device_pixel_ratio: self.device_pixel_ratio as f32,
    };
    self.first_frame = false;
    frame_input
  }

  pub fn handle_winit_window_event(&mut self, event: &WindowEvent) {
    match event {
      WindowEvent::Resized(physical_size) => {
        self.viewport =
          Viewport::new_at_origo(physical_size.width, physical_size.height);
      }
      WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
        // The matching `Resized` event carries the new size.
        self.device_pixel_ratio = *scale_factor;
      }
      WindowEvent::ModifiersChanged(modifiers) => {
        let state = modifiers.state();
        self.modifiers = Modifiers {
          alt: state.alt_key(),
          ctrl: state.control_key(),
          shift: state.shift_key(),
          // ⌘ on macOS, Ctrl everywhere else
          command: if cfg!(target_os = "macos") {
            state.super_key()
          } else {
            state.control_key()
          },
        };
        self.events.push(Event::ModifiersChange {
          modifiers: self.modifiers,
        });
      }
      WindowEvent::KeyboardInput {
        event,
        is_synthetic,
        ..
      } => {
        let pressed = event.state == winit::event::ElementState::Pressed;
        if let winit::keyboard::PhysicalKey::Code(keycode) = event.physical_key
          && let Some(kind) = translate_key_code(keycode)
        {
          self.events.push(if pressed {
            Event::KeyPress {
              kind,
              modifiers: self.modifiers,
              handled: false,
            }
          } else {
            Event::KeyRelease {
              kind,
              modifiers: self.modifiers,
              handled: false,
            }
          });
        }

        // winit 0.29 dropped `ReceivedCharacter`; the text a key press
        // produces now rides along with the key event itself. Synthetic
        // presses are replayed for every key held while the window regains
        // focus, and must not be typed into the editor a second time.
        if pressed
          && !is_synthetic
          && !self.modifiers.ctrl
          && !self.modifiers.command
          && let Some(text) = &event.text
        {
          let text: String =
            text.chars().filter(|c| is_printable_char(*c)).collect();
          if !text.is_empty() {
            self.events.push(Event::Text(text));
          }
        }
      }
      WindowEvent::Focused(false) => {
        // Reset all modifier state when the window loses focus.
        // Without this, holding Cmd and switching apps (Cmd+Tab) causes
        // the Cmd key to appear permanently pressed.
        self.modifiers = Modifiers::default();
        self.events.push(Event::ModifiersChange {
          modifiers: self.modifiers,
        });
      }
      WindowEvent::MouseWheel { delta, .. } => {
        if let Some(position) = self.cursor_pos {
          match delta {
            winit::event::MouseScrollDelta::LineDelta(x, y) => {
              let line_height = 24.0;
              self.events.push(Event::MouseWheel {
                delta: (*x * line_height, *y * line_height),
                position: position.into(),
                modifiers: self.modifiers,
                handled: false,
              });
            }
            winit::event::MouseScrollDelta::PixelDelta(delta) => {
              let d = delta.to_logical(self.device_pixel_ratio);
              self.events.push(Event::MouseWheel {
                delta: (d.x, d.y),
                position: position.into(),
                modifiers: self.modifiers,
                handled: false,
              });
            }
          }
        }
      }
      WindowEvent::MouseInput { state, button, .. } => {
        if let Some(position) = self.cursor_pos {
          let button = match button {
            winit::event::MouseButton::Left => Some(MouseButton::Left),
            winit::event::MouseButton::Middle => Some(MouseButton::Middle),
            winit::event::MouseButton::Right => Some(MouseButton::Right),
            _ => None,
          };
          if let Some(b) = button {
            self.events.push(
              if *state == winit::event::ElementState::Pressed {
                self.mouse_pressed = Some(b);
                Event::MousePress {
                  button: b,
                  position: position.into(),
                  modifiers: self.modifiers,
                  handled: false,
                }
              } else {
                self.mouse_pressed = None;
                Event::MouseRelease {
                  button: b,
                  position: position.into(),
                  modifiers: self.modifiers,
                  handled: false,
                }
              },
            );
          }
        }
      }
      WindowEvent::CursorMoved { position, .. } => {
        let p = position.to_logical(self.device_pixel_ratio);
        let delta = if let Some(last_pos) = self.cursor_pos {
          (p.x - last_pos.x, p.y - last_pos.y)
        } else {
          (0.0, 0.0)
        };
        let position = LogicalPoint {
          x: p.x,
          y: p.y,
          device_pixel_ratio: self.device_pixel_ratio as f32,
          height: self.viewport.height as f32,
        };
        self.events.push(Event::MouseMotion {
          button: self.mouse_pressed,
          delta,
          position: position.into(),
          handled: false,
        });
        self.cursor_pos = Some(position);
      }
      WindowEvent::CursorEntered { .. } => {
        self.events.push(Event::MouseEnter);
      }
      WindowEvent::CursorLeft { .. } => {
        self.mouse_pressed = None;
        self.events.push(Event::MouseLeave);
      }
      WindowEvent::Touch(touch) => {
        let position =
          touch.location.to_logical::<f32>(self.device_pixel_ratio);
        let position = LogicalPoint {
          x: position.x,
          y: position.y,
          device_pixel_ratio: self.device_pixel_ratio as f32,
          height: self.viewport.height as f32,
        };
        match touch.phase {
          TouchPhase::Started => {
            if self.finger_id.is_none() {
              self.events.push(Event::MousePress {
                button: MouseButton::Left,
                position: position.into(),
                modifiers: self.modifiers,
                handled: false,
              });
              self.cursor_pos = Some(position);
              self.finger_id = Some(touch.id);
            } else if self.secondary_finger_id.is_none() {
              self.secondary_cursor_pos = Some(position);
              self.secondary_finger_id = Some(touch.id);
            }
          }
          TouchPhase::Ended | TouchPhase::Cancelled => {
            if self.finger_id.map(|id| id == touch.id).unwrap_or(false) {
              self.events.push(Event::MouseRelease {
                button: MouseButton::Left,
                position: position.into(),
                modifiers: self.modifiers,
                handled: false,
              });
              self.cursor_pos = None;
              self.finger_id = None;
            } else if self
              .secondary_finger_id
              .map(|id| id == touch.id)
              .unwrap_or(false)
            {
              self.secondary_cursor_pos = None;
              self.secondary_finger_id = None;
            }
          }
          TouchPhase::Moved => {
            if self.finger_id.map(|id| id == touch.id).unwrap_or(false) {
              let last_pos = self.cursor_pos.unwrap();
              if let Some(p) = self.secondary_cursor_pos {
                self.events.push(Event::MouseWheel {
                  position: position.into(),
                  modifiers: self.modifiers,
                  handled: false,
                  delta: (
                    (position.x - p.x).abs() - (last_pos.x - p.x).abs(),
                    (position.y - p.y).abs() - (last_pos.y - p.y).abs(),
                  ),
                });
              } else {
                self.events.push(Event::MouseMotion {
                  button: Some(MouseButton::Left),
                  position: position.into(),
                  handled: false,
                  delta: (position.x - last_pos.x, position.y - last_pos.y),
                });
              }
              self.cursor_pos = Some(position);
            } else if self
              .secondary_finger_id
              .map(|id| id == touch.id)
              .unwrap_or(false)
            {
              let last_pos = self.secondary_cursor_pos.unwrap();
              if let Some(p) = self.cursor_pos {
                self.events.push(Event::MouseWheel {
                  position: p.into(),
                  modifiers: self.modifiers,
                  handled: false,
                  delta: (
                    (position.x - p.x).abs() - (last_pos.x - p.x).abs(),
                    (position.y - p.y).abs() - (last_pos.y - p.y).abs(),
                  ),
                });
              }
              self.secondary_cursor_pos = Some(position);
            }
          }
        }
      }
      _ => {}
    }
  }
}

fn is_printable_char(chr: char) -> bool {
  let is_in_private_use_area = ('\u{e000}'..='\u{f8ff}').contains(&chr)
    || ('\u{f0000}'..='\u{ffffd}').contains(&chr)
    || ('\u{100000}'..='\u{10fffd}').contains(&chr);

  !is_in_private_use_area && !chr.is_ascii_control()
}

fn translate_key_code(key: winit::keyboard::KeyCode) -> Option<Key> {
  use winit::keyboard::KeyCode::*;

  Some(match key {
    ArrowDown => Key::ArrowDown,
    ArrowLeft => Key::ArrowLeft,
    ArrowRight => Key::ArrowRight,
    ArrowUp => Key::ArrowUp,
    Escape => Key::Escape,
    Tab => Key::Tab,
    Backspace => Key::Backspace,
    Enter | NumpadEnter => Key::Enter,
    Space => Key::Space,
    Insert => Key::Insert,
    Delete => Key::Delete,
    Home => Key::Home,
    End => Key::End,
    PageUp => Key::PageUp,
    PageDown => Key::PageDown,
    Digit0 | Numpad0 => Key::Num0,
    Digit1 | Numpad1 => Key::Num1,
    Digit2 | Numpad2 => Key::Num2,
    Digit3 | Numpad3 => Key::Num3,
    Digit4 | Numpad4 => Key::Num4,
    Digit5 | Numpad5 => Key::Num5,
    Digit6 | Numpad6 => Key::Num6,
    Digit7 | Numpad7 => Key::Num7,
    Digit8 | Numpad8 => Key::Num8,
    Digit9 | Numpad9 => Key::Num9,
    KeyA => Key::A,
    KeyB => Key::B,
    KeyC => Key::C,
    KeyD => Key::D,
    KeyE => Key::E,
    KeyF => Key::F,
    KeyG => Key::G,
    KeyH => Key::H,
    KeyI => Key::I,
    KeyJ => Key::J,
    KeyK => Key::K,
    KeyL => Key::L,
    KeyM => Key::M,
    KeyN => Key::N,
    KeyO => Key::O,
    KeyP => Key::P,
    KeyQ => Key::Q,
    KeyR => Key::R,
    KeyS => Key::S,
    KeyT => Key::T,
    KeyU => Key::U,
    KeyV => Key::V,
    KeyW => Key::W,
    KeyX => Key::X,
    KeyY => Key::Y,
    KeyZ => Key::Z,
    Slash | NumpadDivide => Key::Slash,
    _ => return None,
  })
}
