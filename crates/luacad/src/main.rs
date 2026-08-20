use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::SystemTime;

use clap::{ArgAction, Args, Parser, Subcommand};

const FORMATS: &[&str] = &["stl", "obj", "ply", "off", "amf", "3mf", "scad"];

/// Extensions the renderer writes. `convert` and `watch` produce geometry
/// files, so asking them for one of these is a mistake worth naming instead
/// of reporting as an unknown format.
const RENDER_FORMATS: &[&str] = &["png"];

// Without the `csgrs` feature Manifold already is the default backend, so
// the flag only changes anything in a build that has csgrs compiled in.
#[cfg(feature = "csgrs")]
const VIA_MANIFOLD_HELP: &str =
  "Use Manifold instead of csgrs (3mf, stl, obj, ply, off, amf)";
#[cfg(not(feature = "csgrs"))]
const VIA_MANIFOLD_HELP: &str =
  "No-op; Manifold is already the default backend";

fn after_help_text() -> String {
  format!(
    "Supported formats: {}\n\
     Images ({}) are written by `luacad render`, not by convert / watch.",
    FORMATS.join(", "),
    RENDER_FORMATS.join(", ")
  )
}

/// Point a `convert`/`watch` invocation at `luacad render`.
fn render_format_error(fmt: &str) -> String {
  format!(
    "'{fmt}' is an image format, so it is written by the renderer, \
     not by an exporter.\n\
     Render it instead: luacad render <file.lua> <output.{fmt}>"
  )
}

#[derive(Parser)]
#[command(
  name = "luacad",
  version = luacad::version::VERSION,
  about = "Execute LuaCAD code from the command line",
  after_help = after_help_text(),
  disable_version_flag = true,
  arg_required_else_help = true
)]
struct Cli {
  /// Show version
  #[arg(short = 'v', long = "version", action = ArgAction::Version)]
  version: Option<bool>,

  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Run a LuaCAD file
  Run {
    #[arg(value_name = "file.lua")]
    file: PathBuf,
  },
  /// Print geometry metadata
  Info {
    #[arg(value_name = "file.lua")]
    file: PathBuf,
  },
  /// Lint Lua files with selene
  Lint {
    #[arg(value_name = "file.lua|dir", required = true)]
    paths: Vec<PathBuf>,
  },
  /// Convert to a mesh or SCAD format
  Convert(ConvertArgs),
  /// Render to a PNG image
  Render {
    #[arg(value_name = "input.lua")]
    input: PathBuf,
    /// Output image (default: input with a .png extension)
    #[arg(value_name = "output.png")]
    output: Option<PathBuf>,
    /// Smooth shading for rasterized renders (default: flat, showing
    /// tessellation). Path-traced renders are always smooth.
    #[arg(long)]
    smooth: bool,
    /// Path-traced rendering (soft shadows, ambient occlusion)
    #[arg(long)]
    raytrace: bool,
    /// Samples per pixel for path-traced renders; more samples mean
    /// less noise at proportionally longer render times (default: 128)
    #[arg(long, value_name = "N", requires = "raytrace")]
    samples: Option<std::num::NonZeroUsize>,
    /// Camera angle in degrees as azimuth,elevation: azimuth orbits
    /// around the vertical axis, elevation tilts above the model
    /// (default: -30,30, the studio's initial view)
    #[arg(
      long,
      value_name = "AZ,EL",
      allow_hyphen_values = true,
      value_parser = parse_camera
    )]
    camera: Option<(f32, f32)>,
  },
  /// Rebuild on file changes
  Watch(ConvertArgs),
}

/// Shared arguments for the convert and watch subcommands.
#[derive(Args)]
struct ConvertArgs {
  #[arg(value_name = "input.lua")]
  input: PathBuf,
  #[arg(value_name = "output.stl")]
  output: PathBuf,
  /// Override output format (default: infer from extension)
  #[arg(long, value_name = "fmt")]
  format: Option<String>,
  /// Delegate the export to an installed OpenSCAD binary
  #[arg(long)]
  via_openscad: bool,
  #[arg(long, help = VIA_MANIFOLD_HELP, conflicts_with = "via_openscad")]
  via_manifold: bool,
}

fn infer_format(path: &Path) -> Option<&'static str> {
  let ext = path.extension()?.to_str()?.to_lowercase();
  match ext.as_str() {
    "stl" => Some("stl"),
    "obj" => Some("obj"),
    "ply" => Some("ply"),
    "off" => Some("off"),
    "amf" => Some("amf"),
    "3mf" => Some("3mf"),
    "scad" => Some("scad"),
    _ => None,
  }
}

fn export(
  geometries: &[luacad::geometry::CsgGeometry],
  format: &str,
  output: &Path,
) -> Result<(), String> {
  match format {
    "scad" => {
      let nodes: Vec<_> =
        geometries.iter().filter_map(|g| g.scad.clone()).collect();
      luacad::scad_export::export_scad(&nodes, output)
    }
    #[cfg(feature = "csgrs")]
    fmt @ ("stl" | "obj" | "ply" | "off" | "amf" | "3mf") => match fmt {
      "stl" => luacad::export::export_stl(geometries, output),
      "obj" => luacad::export::export_obj(geometries, output),
      "ply" => luacad::export::export_ply(geometries, output),
      "off" => luacad::export::export_off(geometries, output),
      "amf" => luacad::export::export_amf(geometries, output),
      "3mf" => luacad::export::export_3mf(geometries, output),
      _ => unreachable!(),
    },
    #[cfg(not(feature = "csgrs"))]
    "stl" | "obj" | "ply" | "off" | "amf" | "3mf" => {
      luacad::export::export_manifold(geometries, format, output)
    }
    fmt if RENDER_FORMATS.contains(&fmt) => Err(render_format_error(fmt)),
    other => Err(format!(
      "Unknown format: {other}\nSupported formats: {}",
      FORMATS.join(", ")
    )),
  }
}

fn export_via_openscad(
  geometries: &[luacad::geometry::CsgGeometry],
  output: &Path,
) -> Result<(), String> {
  let nodes: Vec<_> =
    geometries.iter().filter_map(|g| g.scad.clone()).collect();
  if nodes.is_empty() {
    return Err("No SCAD geometry to export".to_string());
  }

  let scad_source = luacad::scad_export::generate_scad(&nodes);

  // The staging directory is unique per run. A fixed name would let two
  // exports running at once — a `watch` in another terminal, a parallel
  // build — overwrite each other's source between the write and OpenSCAD
  // reading it, and the export would quietly produce the wrong model.
  let stamp = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let tmp_dir = std::env::temp_dir()
    .join(format!("luacad_openscad-{}-{stamp}", std::process::id()));
  std::fs::create_dir_all(&tmp_dir)
    .map_err(|e| format!("Failed to create temp dir: {e}"))?;
  let tmp_scad = tmp_dir.join("export.scad");
  std::fs::write(&tmp_scad, &scad_source)
    .map_err(|e| format!("Failed to write temp SCAD file: {e}"))?;

  let result = std::process::Command::new("openscad")
    .arg("-o")
    .arg(output)
    .arg(&tmp_scad)
    .output();
  let _ = std::fs::remove_dir_all(&tmp_dir);

  let result = result.map_err(|e| {
    format!("Failed to run OpenSCAD: {e}. Is OpenSCAD installed and in PATH?")
  })?;

  if result.status.success() {
    Ok(())
  } else {
    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(format!("OpenSCAD failed: {}", stderr.trim()))
  }
}

/// Run a single convert cycle: execute Lua, then export.
/// Returns Ok(object_count) on success, Err(message) on failure.
fn do_convert(args: &ConvertArgs, format: &str) -> Result<usize, String> {
  let code = std::fs::read_to_string(&args.input)
    .map_err(|e| format!("Error reading {}: {e}", args.input.display()))?;

  let geometries =
    luacad::lua_engine::execute_lua_with_path(&code, Some(&args.input))?;
  let count = geometries.len();

  if args.via_manifold {
    luacad::export::export_manifold(&geometries, format, &args.output)?;
  } else if args.via_openscad {
    export_via_openscad(&geometries, &args.output)?;
  } else {
    export(&geometries, format, &args.output)?;
  }

  Ok(count)
}

fn cmd_info(input: &Path) -> ExitCode {
  let code = match std::fs::read_to_string(input) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {}: {e}", input.display());
      return ExitCode::FAILURE;
    }
  };

  let geometries =
    match luacad::lua_engine::execute_lua_with_path(&code, Some(input)) {
      Ok(g) => g,
      Err(e) => {
        eprintln!("{e}");
        return ExitCode::FAILURE;
      }
    };

  if geometries.is_empty() {
    println!("File:       {}", input.display());
    println!("Objects:    0");
    return ExitCode::SUCCESS;
  }

  let mut total_triangles: usize = 0;
  let mut overall_min = [f32::MAX; 3];
  let mut overall_max = [f32::MIN; 3];
  let mut per_object: Vec<usize> = Vec::new();

  for geom in &geometries {
    if let Some(ref scad) = geom.scad {
      // Dimension-aware, so an outline is reported by the triangles it
      // tessellates into rather than as an empty object.
      let mesh = luacad::export::materialize_scad_display_mesh(scad);
      let tri_count = mesh.triangles.len();
      total_triangles += tri_count;
      per_object.push(tri_count);

      if tri_count > 0 {
        let (bb_min, bb_max) = mesh.bounding_box();
        overall_min[0] = overall_min[0].min(bb_min[0]);
        overall_min[1] = overall_min[1].min(bb_min[1]);
        overall_min[2] = overall_min[2].min(bb_min[2]);
        overall_max[0] = overall_max[0].max(bb_max[0]);
        overall_max[1] = overall_max[1].max(bb_max[1]);
        overall_max[2] = overall_max[2].max(bb_max[2]);
      }
    } else {
      per_object.push(0);
    }
  }

  println!("File:       {}", input.display());
  println!("Objects:    {}", geometries.len());
  println!("Triangles:  {total_triangles}");

  if overall_min[0] <= overall_max[0] {
    println!("Bounding box:");
    println!(
      "  X: {:.3} .. {:.3}  (W: {:.3})",
      overall_min[0],
      overall_max[0],
      overall_max[0] - overall_min[0]
    );
    println!(
      "  Y: {:.3} .. {:.3}  (D: {:.3})",
      overall_min[1],
      overall_max[1],
      overall_max[1] - overall_min[1]
    );
    println!(
      "  Z: {:.3} .. {:.3}  (H: {:.3})",
      overall_min[2],
      overall_max[2],
      overall_max[2] - overall_min[2]
    );
  }

  if geometries.len() > 1 {
    for (i, tris) in per_object.iter().enumerate() {
      println!("Object {}: {tris} triangles", i + 1);
    }
  }

  // `info` reports on the model rather than producing one, so an unsupported
  // construct is a warning here — the triangle counts above just exclude it.
  let blockers =
    luacad::export::geometries_unsupported_for_display(&geometries);
  if !blockers.is_empty() {
    println!();
    println!("Warning: some constructs contribute no triangles:");
    for item in &blockers {
      println!("  - {item}");
    }
  }

  ExitCode::SUCCESS
}

fn collect_lua_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
  fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
      .map_err(|e| format!("Error reading directory {}: {e}", dir.display()))?
    {
      let entry = entry.map_err(|e| format!("Error reading entry: {e}"))?;
      let p = entry.path();
      if p.is_dir() {
        walk(&p, files)?;
      } else if p.extension().is_some_and(|ext| ext == "lua") {
        files.push(p);
      }
    }
    Ok(())
  }

  let mut files = Vec::new();
  for path in paths {
    if path.is_dir() {
      walk(path, &mut files)?;
    } else {
      files.push(path.clone());
    }
  }
  files.sort();
  Ok(files)
}

fn cmd_lint(paths: &[PathBuf]) -> ExitCode {
  let files = match collect_lua_files(paths) {
    Ok(f) => f,
    Err(e) => {
      eprintln!("{e}");
      return ExitCode::FAILURE;
    }
  };

  if files.is_empty() {
    eprintln!("No .lua files found");
    return ExitCode::FAILURE;
  }

  let mut total_warnings = 0usize;
  let mut total_errors = 0usize;
  let mut had_parse_error = false;

  for file in &files {
    let code = match std::fs::read_to_string(file) {
      Ok(c) => c,
      Err(e) => {
        eprintln!("Error reading {}: {e}", file.display());
        had_parse_error = true;
        continue;
      }
    };

    match luacad::linter::lint(&code) {
      Ok(diagnostics) => {
        for d in &diagnostics {
          let severity_str = match d.severity {
            luacad::linter::LintSeverity::Warning => {
              total_warnings += 1;
              "warning"
            }
            luacad::linter::LintSeverity::Error => {
              total_errors += 1;
              "error"
            }
          };
          eprintln!(
            "{}:{}:{}: {severity_str}[{}]: {}",
            file.display(),
            d.line,
            d.column,
            d.code,
            d.message,
          );
          for note in &d.notes {
            eprintln!("  note: {note}");
          }
        }
      }
      Err(e) => {
        eprintln!("{}:  parse error: {e}", file.display());
        had_parse_error = true;
      }
    }
  }

  if total_errors > 0 || total_warnings > 0 || had_parse_error {
    eprintln!();
    eprintln!(
      "Checked {} files: {total_errors} errors, {total_warnings} warnings",
      files.len(),
    );
  } else {
    eprintln!("Checked {} files: no issues found", files.len());
  }

  if total_errors > 0 || had_parse_error {
    ExitCode::FAILURE
  } else {
    ExitCode::SUCCESS
  }
}

fn cmd_run(input: &Path) -> ExitCode {
  let code = match std::fs::read_to_string(input) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {}: {e}", input.display());
      return ExitCode::FAILURE;
    }
  };

  match luacad::lua_engine::execute_lua_with_path(&code, Some(input)) {
    Ok(geometries) => {
      if geometries.is_empty() {
        println!("OK");
      } else {
        let label = if geometries.len() == 1 {
          "object"
        } else {
          "objects"
        };
        println!("OK: {} {label}", geometries.len());
      }
      ExitCode::SUCCESS
    }
    Err(e) => {
      eprintln!("{e}");
      ExitCode::FAILURE
    }
  }
}

/// Resolve the output format from override or file extension.
fn resolve_format<'a>(
  format_override: Option<&'a str>,
  output_path: &Path,
  output_str: &str,
) -> Result<&'a str, ExitCode>
where
  'static: 'a,
{
  // The renderer's formats never reach `infer_format`, so catch them from
  // both the override and the extension before the generic errors below.
  let requested = format_override.map(str::to_ascii_lowercase).or_else(|| {
    output_path
      .extension()
      .and_then(|e| e.to_str())
      .map(|e| e.to_ascii_lowercase())
  });
  if let Some(fmt) = requested.filter(|f| RENDER_FORMATS.contains(&f.as_str()))
  {
    eprintln!("{}", render_format_error(&fmt));
    return Err(ExitCode::FAILURE);
  }

  if let Some(fmt) = format_override {
    Ok(fmt)
  } else if let Some(fmt) = infer_format(output_path) {
    Ok(fmt)
  } else {
    eprintln!(
      "Cannot infer format from extension of '{output_str}'. \
       Use --format to specify one."
    );
    eprintln!("Supported formats: {}", FORMATS.join(", "));
    Err(ExitCode::FAILURE)
  }
}

/// Parse a `--camera` value: two comma-separated angles in degrees.
fn parse_camera(s: &str) -> Result<(f32, f32), String> {
  let err =
    || format!("expected two comma-separated angles (e.g. -30,30), got '{s}'");
  let (az, el) = s.split_once(',').ok_or_else(err)?;
  Ok((
    az.trim().parse().map_err(|_| err())?,
    el.trim().parse().map_err(|_| err())?,
  ))
}

fn cmd_render(
  input: &Path,
  output: Option<&Path>,
  smooth: bool,
  raytrace: bool,
  samples: Option<std::num::NonZeroUsize>,
  camera: Option<(f32, f32)>,
) -> ExitCode {
  let output = output
    .map(Path::to_path_buf)
    .unwrap_or_else(|| input.with_extension("png"));

  let code = match std::fs::read_to_string(input) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {}: {e}", input.display());
      return ExitCode::FAILURE;
    }
  };

  let geometries =
    match luacad::lua_engine::execute_lua_with_path(&code, Some(input)) {
      Ok(g) => g,
      Err(e) => {
        eprintln!("{e}");
        return ExitCode::FAILURE;
      }
    };

  let result = if raytrace {
    #[cfg(feature = "raytrace")]
    {
      luacad::raytrace::render_to_png(
        &geometries,
        &output,
        samples.map(std::num::NonZeroUsize::get),
        camera,
      )
    }
    #[cfg(not(feature = "raytrace"))]
    {
      let _ = samples;
      Err(
        "This build of luacad has the `raytrace` feature disabled".to_string(),
      )
    }
  } else {
    luacad::render::render_to_png(&geometries, &output, smooth, camera)
  };
  match result {
    Ok(()) => {
      let label = if geometries.len() == 1 {
        "object"
      } else {
        "objects"
      };
      println!("OK: {} {label}", geometries.len());
      println!("Rendered to {}", output.display());
      ExitCode::SUCCESS
    }
    Err(e) => {
      eprintln!("{e}");
      ExitCode::FAILURE
    }
  }
}

fn cmd_convert(args: &ConvertArgs) -> ExitCode {
  let format = match resolve_format(
    args.format.as_deref(),
    &args.output,
    &args.output.to_string_lossy(),
  ) {
    Ok(f) => f,
    Err(code) => return code,
  };

  match do_convert(args, format) {
    Ok(count) => {
      let label = if count == 1 { "object" } else { "objects" };
      println!("OK: {count} {label}");
      println!("Exported to {}", args.output.display());
      ExitCode::SUCCESS
    }
    Err(e) => {
      eprintln!("{e}");
      ExitCode::FAILURE
    }
  }
}

fn cmd_watch(args: &ConvertArgs) -> ExitCode {
  let format = match resolve_format(
    args.format.as_deref(),
    &args.output,
    &args.output.to_string_lossy(),
  ) {
    Ok(f) => f,
    Err(code) => return code,
  };

  if !args.input.exists() {
    eprintln!("Input file not found: {}", args.input.display());
    return ExitCode::FAILURE;
  }

  println!(
    "Watching {} for changes (Ctrl+C to stop)",
    args.input.display()
  );

  // Initial build
  let mut last_modified = SystemTime::UNIX_EPOCH;
  let mut build_count: u64 = 0;

  loop {
    let modified = std::fs::metadata(&args.input)
      .and_then(|m| m.modified())
      .unwrap_or(SystemTime::UNIX_EPOCH);

    if modified != last_modified {
      last_modified = modified;
      build_count += 1;

      if build_count > 1 {
        println!();
        println!("File changed, rebuilding...");
      }

      let start = std::time::Instant::now();
      match do_convert(args, format) {
        Ok(count) => {
          let elapsed = start.elapsed();
          let label = if count == 1 { "object" } else { "objects" };
          println!(
            "OK: {count} {label}, exported to {} ({:.1}s)",
            args.output.display(),
            elapsed.as_secs_f64()
          );
        }
        Err(e) => {
          eprintln!("Error: {e}");
        }
      }
    }

    std::thread::sleep(std::time::Duration::from_millis(500));
  }
}

fn main() -> ExitCode {
  // CSG evaluation (BSP trees in csgrs) recurses deeply on models with
  // many nearly-coplanar polygons, so run everything on a big stack
  const STACK_SIZE: usize = 512 * 1024 * 1024;
  std::thread::Builder::new()
    .stack_size(STACK_SIZE)
    .spawn(main_impl)
    .expect("Failed to spawn main thread")
    .join()
    .expect("Main thread panicked")
}

fn main_impl() -> ExitCode {
  let mut argv: Vec<std::ffi::OsString> = std::env::args_os().collect();

  // `luacad model.lua` is shorthand for `luacad run model.lua`, but only
  // when it really names a script — otherwise clap reports it as an unknown
  // subcommand, which beats "No such file or directory".
  const COMMANDS: &[&str] =
    &["run", "info", "lint", "convert", "render", "watch", "help"];
  if let Some(first) = argv.get(1).and_then(|a| a.to_str())
    && !first.starts_with('-')
    && !COMMANDS.contains(&first)
    && Path::new(first).exists()
  {
    argv.insert(1, "run".into());
  }

  let cli = Cli::parse_from(argv);

  match &cli.command {
    Command::Run { file } => cmd_run(file),
    Command::Info { file } => cmd_info(file),
    Command::Lint { paths } => cmd_lint(paths),
    Command::Convert(args) => cmd_convert(args),
    Command::Render {
      input,
      output,
      smooth,
      raytrace,
      samples,
      camera,
    } => cmd_render(
      input,
      output.as_deref(),
      *smooth,
      *raytrace,
      *samples,
      *camera,
    ),
    Command::Watch(args) => cmd_watch(args),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_png_extension_points_at_the_renderer() {
    let out = Path::new("preview.png");
    assert!(resolve_format(None, out, "preview.png").is_err());
  }

  #[test]
  fn a_png_format_override_points_at_the_renderer() {
    let out = Path::new("model.stl");
    assert!(resolve_format(Some("PNG"), out, "model.stl").is_err());
  }

  #[test]
  fn exporting_png_names_the_render_subcommand() {
    let err = export(&[], "png", Path::new("preview.png")).unwrap_err();
    assert!(err.contains("luacad render"), "{err}");
  }

  #[test]
  fn mesh_formats_still_resolve() {
    let out = Path::new("model.stl");
    assert_eq!(resolve_format(None, out, "model.stl"), Ok("stl"));
    assert_eq!(resolve_format(Some("3mf"), out, "model.stl"), Ok("3mf"));
  }

  #[test]
  fn cli_definition_is_consistent() {
    use clap::CommandFactory;
    Cli::command().debug_assert();
  }
}
