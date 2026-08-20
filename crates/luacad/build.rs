//! Bake the version string into the crate at build time.
//!
//! A binary installed from crates.io only knows its crate version, but one
//! built from a git checkout also carries
//! `git describe --always --dirty --tags`, so a local build can be traced
//! back to the commit it came from. `--always` keeps that working in a
//! checkout without tags (it falls back to the abbreviated hash), and
//! `--dirty` marks a build made from a modified working tree.

use std::path::Path;
use std::process::Command;

fn main() {
  // Packagers building outside a checkout (or wanting a reproducible string)
  // can set this themselves; an empty value drops the suffix entirely.
  println!("cargo:rerun-if-env-changed=LUACAD_GIT_DESCRIBE");
  let describe = match std::env::var("LUACAD_GIT_DESCRIBE") {
    Ok(value) => value.trim().to_string(),
    Err(_) => git_describe().unwrap_or_default(),
  };

  let crate_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
  // At a clean release tag the description repeats the crate version, so the
  // suffix would only add noise.
  let version = if describe.is_empty()
    || describe == crate_version
    || describe.strip_prefix('v') == Some(crate_version.as_str())
  {
    crate_version
  } else {
    format!("{crate_version} ({describe})")
  };

  println!("cargo:rustc-env=LUACAD_GIT_DESCRIBE={describe}");
  println!("cargo:rustc-env=LUACAD_VERSION={version}");
  println!(
    "cargo:rustc-env=LUACAD_BUILD_TARGET={}",
    std::env::var("TARGET").unwrap_or_default()
  );
}

/// Run `git describe` in the crate directory, returning `None` whenever git
/// is missing or this is not a checkout (an unpacked crates.io tarball).
fn git_describe() -> Option<String> {
  let output = Command::new("git")
    .args(["describe", "--always", "--dirty", "--tags"])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  let describe = String::from_utf8(output.stdout).ok()?.trim().to_string();
  if describe.is_empty() {
    return None;
  }
  watch_git_dir();
  Some(describe)
}

/// Rebuild when the checked-out commit changes. The working tree's state
/// (what `--dirty` reports) is not watchable this way, so `.git/index` stands
/// in for it: git refreshes the index whenever a tracked file is staged, and
/// most commands that notice a change do so too.
fn watch_git_dir() {
  let Some(output) = Command::new("git")
    .args(["rev-parse", "--absolute-git-dir"])
    .output()
    .ok()
    .filter(|out| out.status.success())
  else {
    return;
  };
  let Ok(git_dir) = String::from_utf8(output.stdout) else {
    return;
  };
  let git_dir = Path::new(git_dir.trim());
  for entry in ["HEAD", "index", "refs"] {
    let path = git_dir.join(entry);
    if path.exists() {
      println!("cargo:rerun-if-changed={}", path.display());
    }
  }
}
