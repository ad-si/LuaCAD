#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeMode {
  System,
  Light,
  Dark,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
  pub bg: (f32, f32, f32),
  pub egui_dark: bool,
}

impl ThemeColors {
  pub fn dark() -> Self {
    Self {
      bg: (0.12, 0.12, 0.16),
      egui_dark: true,
    }
  }

  pub fn light() -> Self {
    Self {
      bg: (0.85, 0.85, 0.88),
      egui_dark: false,
    }
  }
}

/// Detect system dark mode. On macOS, checks AppleInterfaceStyle.
pub fn system_is_dark_mode() -> bool {
  #[cfg(target_os = "macos")]
  {
    std::process::Command::new("defaults")
      .args(["read", "-g", "AppleInterfaceStyle"])
      .output()
      .map(|o| String::from_utf8_lossy(&o.stdout).contains("Dark"))
      .unwrap_or(false)
  }
  #[cfg(not(target_os = "macos"))]
  {
    true // default to dark on other platforms
  }
}
