use std::path::Path;
use std::process::ExitCode;
use std::time::SystemTime;

const FORMATS: &[&str] = &["stl", "obj", "ply", "3mf", "scad"];

fn print_help() {
  let version = env!("CARGO_PKG_VERSION");
  println!("luacad {version} — Execute LuaCAD code from the command line");
  println!();
  println!("Usage:");
  println!("  luacad run <file.lua>                     Run a LuaCAD file");
  println!(
    "  luacad info <file.lua>                    Print geometry metadata"
  );
  println!(
    "  luacad lint <file.lua|dir>...             Lint Lua files with selene"
  );
  println!(
    "  luacad convert <input.lua> <output.stl>   Convert to a mesh format"
  );
  println!(
    "  luacad watch <input.lua> <output.stl>     Rebuild on file changes"
  );
  println!();
  println!("Options:");
  println!("  --help, -h       Show this help message");
  println!("  --version, -v    Show version");
  println!();
  println!("Convert / watch options:");
  println!(
    "  --format <fmt>   Override output format (default: infer from extension)"
  );
  println!(
    "  --via-openscad   Use OpenSCAD to generate the output instead of csgrs"
  );
  println!();
  println!("Supported formats: {}", FORMATS.join(", "));
}

fn print_version() {
  println!("luacad {}", env!("CARGO_PKG_VERSION"));
}

fn infer_format(path: &Path) -> Option<&'static str> {
  let ext = path.extension()?.to_str()?.to_lowercase();
  match ext.as_str() {
    "stl" => Some("stl"),
    "obj" => Some("obj"),
    "ply" => Some("ply"),
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
    "stl" => luacad::export::export_stl(geometries, output),
    "obj" => luacad::export::export_obj(geometries, output),
    "ply" => luacad::export::export_ply(geometries, output),
    "3mf" => luacad::export::export_3mf(geometries, output),
    "scad" => {
      let nodes: Vec<_> =
        geometries.iter().filter_map(|g| g.scad.clone()).collect();
      luacad::scad_export::export_scad(&nodes, output)
    }
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
  let tmp_dir = std::env::temp_dir().join("luacad_openscad");
  std::fs::create_dir_all(&tmp_dir)
    .map_err(|e| format!("Failed to create temp dir: {e}"))?;
  let tmp_scad = tmp_dir.join("export_temp.scad");
  std::fs::write(&tmp_scad, &scad_source)
    .map_err(|e| format!("Failed to write temp SCAD file: {e}"))?;

  let result = std::process::Command::new("openscad")
    .arg("-o")
    .arg(output)
    .arg(&tmp_scad)
    .output()
    .map_err(|e| {
      format!("Failed to run OpenSCAD: {e}. Is OpenSCAD installed and in PATH?")
    })?;

  if result.status.success() {
    Ok(())
  } else {
    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(format!("OpenSCAD failed: {}", stderr.trim()))
  }
}

/// Shared options for convert and watch subcommands.
struct ConvertOpts<'a> {
  input: &'a str,
  output: &'a Path,
  format: &'a str,
  via_openscad: bool,
}

/// Run a single convert cycle: execute Lua, then export.
/// Returns Ok(object_count) on success, Err(message) on failure.
fn do_convert(opts: &ConvertOpts) -> Result<usize, String> {
  let code = std::fs::read_to_string(opts.input)
    .map_err(|e| format!("Error reading {}: {e}", opts.input))?;

  let geometries = luacad::lua_engine::execute_lua(&code)?;
  let count = geometries.len();

  if opts.via_openscad {
    export_via_openscad(&geometries, opts.output)?;
  } else {
    export(&geometries, opts.format, opts.output)?;
  }

  Ok(count)
}

fn cmd_info(args: &[String]) -> ExitCode {
  if args.is_empty() {
    eprintln!("Missing input file. Usage: luacad info <file.lua>");
    return ExitCode::FAILURE;
  }

  let input = &args[0];
  let code = match std::fs::read_to_string(input) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {input}: {e}");
      return ExitCode::FAILURE;
    }
  };

  let mut geometries = match luacad::lua_engine::execute_lua(&code) {
    Ok(g) => g,
    Err(e) => {
      eprintln!("{e}");
      return ExitCode::FAILURE;
    }
  };

  if geometries.is_empty() {
    println!("File:       {input}");
    println!("Objects:    0");
    return ExitCode::SUCCESS;
  }

  use csgrs::traits::CSG;

  let mut total_polygons: usize = 0;
  let mut total_triangles: usize = 0;
  let mut overall_min = [f32::MAX; 3];
  let mut overall_max = [f32::MIN; 3];
  let mut per_object: Vec<(usize, usize)> = Vec::new();

  for geom in &mut geometries {
    geom.materialize();
    let mesh = geom.mesh.as_ref().unwrap();
    let poly_count = mesh.polygons.len();
    let tri_count: usize = mesh
      .polygons
      .iter()
      .map(|p| p.vertices.len().saturating_sub(2))
      .sum();
    total_polygons += poly_count;
    total_triangles += tri_count;
    per_object.push((poly_count, tri_count));

    if !mesh.polygons.is_empty() {
      let bb = mesh.bounding_box();
      overall_min[0] = overall_min[0].min(bb.mins.x);
      overall_min[1] = overall_min[1].min(bb.mins.y);
      overall_min[2] = overall_min[2].min(bb.mins.z);
      overall_max[0] = overall_max[0].max(bb.maxs.x);
      overall_max[1] = overall_max[1].max(bb.maxs.y);
      overall_max[2] = overall_max[2].max(bb.maxs.z);
    }
  }

  println!("File:       {input}");
  println!("Objects:    {}", geometries.len());
  println!("Polygons:   {total_polygons}");
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
    for (i, (polys, tris)) in per_object.iter().enumerate() {
      println!("Object {}: {polys} polygons, {tris} triangles", i + 1);
    }
  }

  ExitCode::SUCCESS
}

fn collect_lua_files(
  paths: &[String],
) -> Result<Vec<std::path::PathBuf>, String> {
  let mut files = Vec::new();
  for path_str in paths {
    let path = Path::new(path_str);
    if path.is_dir() {
      for entry in std::fs::read_dir(path)
        .map_err(|e| format!("Error reading directory {path_str}: {e}"))?
      {
        let entry = entry.map_err(|e| format!("Error reading entry: {e}"))?;
        let p = entry.path();
        if p.extension().is_some_and(|ext| ext == "lua") {
          files.push(p);
        }
      }
    } else {
      files.push(path.to_path_buf());
    }
  }
  files.sort();
  Ok(files)
}

fn cmd_lint(args: &[String]) -> ExitCode {
  if args.is_empty() {
    eprintln!(
      "Missing file or directory. Usage: luacad lint <file.lua|dir>..."
    );
    return ExitCode::FAILURE;
  }

  let files = match collect_lua_files(args) {
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

fn cmd_run(args: &[String]) -> ExitCode {
  if args.is_empty() {
    eprintln!("Missing input file. Usage: luacad run <file.lua>");
    return ExitCode::FAILURE;
  }

  let code = match std::fs::read_to_string(&args[0]) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {}: {e}", args[0]);
      return ExitCode::FAILURE;
    }
  };

  match luacad::lua_engine::execute_lua(&code) {
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

/// Parse convert/watch args into (input, output_str, format_override, via_openscad).
fn parse_convert_args(
  args: &[String],
) -> Result<(&str, &str, Option<&str>, bool), ExitCode> {
  let mut input: Option<&str> = None;
  let mut output: Option<&str> = None;
  let mut format_override: Option<&str> = None;
  let mut via_openscad = false;
  let mut i = 0;

  while i < args.len() {
    match args[i].as_str() {
      "--format" => {
        i += 1;
        if i >= args.len() {
          eprintln!("--format requires a value");
          return Err(ExitCode::FAILURE);
        }
        format_override = Some(&args[i]);
      }
      "--via-openscad" => {
        via_openscad = true;
      }
      arg if arg.starts_with('-') => {
        eprintln!("Unknown option: {arg}");
        return Err(ExitCode::FAILURE);
      }
      arg => {
        if input.is_none() {
          input = Some(arg);
        } else if output.is_none() {
          output = Some(arg);
        } else {
          eprintln!("Unexpected argument: {arg}");
          return Err(ExitCode::FAILURE);
        }
      }
    }
    i += 1;
  }

  let Some(input) = input else {
    eprintln!("Missing input file");
    return Err(ExitCode::FAILURE);
  };
  let Some(output) = output else {
    eprintln!("Missing output file");
    return Err(ExitCode::FAILURE);
  };

  Ok((input, output, format_override, via_openscad))
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

fn cmd_convert(args: &[String]) -> ExitCode {
  let (input, output_str, format_override, via_openscad) =
    match parse_convert_args(args) {
      Ok(v) => v,
      Err(code) => return code,
    };

  let output_path = Path::new(output_str);
  let format = match resolve_format(format_override, output_path, output_str) {
    Ok(f) => f,
    Err(code) => return code,
  };

  let opts = ConvertOpts {
    input,
    output: output_path,
    format,
    via_openscad,
  };

  match do_convert(&opts) {
    Ok(count) => {
      let label = if count == 1 { "object" } else { "objects" };
      println!("OK: {count} {label}");
      println!("Exported to {}", output_path.display());
      ExitCode::SUCCESS
    }
    Err(e) => {
      eprintln!("{e}");
      ExitCode::FAILURE
    }
  }
}

fn cmd_watch(args: &[String]) -> ExitCode {
  let (input, output_str, format_override, via_openscad) =
    match parse_convert_args(args) {
      Ok(v) => v,
      Err(code) => return code,
    };

  let output_path = Path::new(output_str);
  let format = match resolve_format(format_override, output_path, output_str) {
    Ok(f) => f,
    Err(code) => return code,
  };

  let input_path = Path::new(input);
  if !input_path.exists() {
    eprintln!("Input file not found: {input}");
    return ExitCode::FAILURE;
  }

  let opts = ConvertOpts {
    input,
    output: output_path,
    format,
    via_openscad,
  };

  println!("Watching {input} for changes (Ctrl+C to stop)");

  // Initial build
  let mut last_modified = SystemTime::UNIX_EPOCH;
  let mut build_count: u64 = 0;

  loop {
    let modified = std::fs::metadata(input_path)
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
      match do_convert(&opts) {
        Ok(count) => {
          let elapsed = start.elapsed();
          let label = if count == 1 { "object" } else { "objects" };
          println!(
            "OK: {count} {label}, exported to {} ({:.1}s)",
            output_path.display(),
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
  let args: Vec<String> = std::env::args().skip(1).collect();

  if args.is_empty() {
    print_help();
    return ExitCode::FAILURE;
  }

  match args[0].as_str() {
    "--help" | "-h" | "help" => {
      print_help();
      ExitCode::SUCCESS
    }
    "--version" | "-v" => {
      print_version();
      ExitCode::SUCCESS
    }
    "run" => cmd_run(&args[1..]),
    "info" => cmd_info(&args[1..]),
    "lint" => cmd_lint(&args[1..]),
    "convert" => cmd_convert(&args[1..]),
    "watch" => cmd_watch(&args[1..]),
    _ => cmd_run(&args),
  }
}
