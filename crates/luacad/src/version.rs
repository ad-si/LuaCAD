//! Version information baked in by the build script.
//!
//! Both binaries report the same values: `luacad --version`,
//! `luacad-studio --version`, and Studio's Settings → About all read them
//! from here, so a locally built binary can be identified without hashing it.

/// Version from `Cargo.toml`, shared by every crate in the workspace.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `git describe --always --dirty --tags` at build time. Empty for a build
/// made outside a git checkout, such as an install from crates.io.
pub const GIT_DESCRIBE: &str = env!("LUACAD_GIT_DESCRIBE");

/// Target triple the binary was built for, e.g. `aarch64-apple-darwin`.
pub const BUILD_TARGET: &str = env!("LUACAD_BUILD_TARGET");

/// The version to show users: the crate version, followed by the git
/// description when it adds anything — `1.1.0 (v1.1.0-3-g23a0ea2-dirty)` for
/// a build three commits past the release tag with local modifications.
pub const VERSION: &str = env!("LUACAD_VERSION");

#[cfg(test)]
mod tests {
  use super::{CRATE_VERSION, VERSION};

  #[test]
  fn version_starts_with_the_crate_version() {
    assert!(!CRATE_VERSION.is_empty());
    assert!(
      VERSION.starts_with(CRATE_VERSION),
      "{VERSION} should start with {CRATE_VERSION}"
    );
  }

  #[test]
  fn git_description_is_parenthesized_when_present() {
    let suffix = &VERSION[CRATE_VERSION.len()..];
    assert!(
      suffix.is_empty() || (suffix.starts_with(" (") && suffix.ends_with(')')),
      "unexpected version suffix: {suffix:?}"
    );
  }
}
