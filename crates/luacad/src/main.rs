use std::path::Path;
use std::process::ExitCode;

const FORMATS: &[&str] = &["stl", "obj", "ply", "3mf", "scad"];

fn print_help() {
  let version = env!("CARGO_PKG_VERSION");
  println!("luacad {version} — Execute LuaCAD code from the command line");
  println!();
  println!("Usage:");
  println!("  luacad <file.lua>                         Run a LuaCAD file");
  println!(
    "  luacad convert <input.lua> <output.stl>   Convert to a mesh format"
  );
  println!();
  println!("Options:");
  println!("  --help, -h       Show this help message");
  println!("  --version, -v    Show version");
  println!();
  println!("Convert options:");
  println!("  --format <fmt>   Override output format (default: infer from extension)");
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
      let nodes: Vec<_> = geometries
        .iter()
        .filter_map(|g| g.scad.clone())
        .collect();
      luacad::scad_export::export_scad(&nodes, output)
    }
    other => Err(format!(
      "Unknown format: {other}\nSupported formats: {}",
      FORMATS.join(", ")
    )),
  }
}

fn run_lua(path: &str) -> Result<Vec<luacad::geometry::CsgGeometry>, ExitCode> {
  let code = match std::fs::read_to_string(path) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {path}: {e}");
      return Err(ExitCode::FAILURE);
    }
  };

  match luacad::lua_engine::execute_lua(&code) {
    Ok(g) => Ok(g),
    Err(e) => {
      eprintln!("{e}");
      Err(ExitCode::FAILURE)
    }
  }
}

fn cmd_run(args: &[String]) -> ExitCode {
  if args.is_empty() {
    eprintln!("Missing input file. Run `luacad --help` for usage.");
    return ExitCode::FAILURE;
  }

  let geometries = match run_lua(&args[0]) {
    Ok(g) => g,
    Err(code) => return code,
  };

  println!(
    "OK: {} {}",
    geometries.len(),
    if geometries.len() == 1 {
      "object"
    } else {
      "objects"
    }
  );

  ExitCode::SUCCESS
}

fn cmd_convert(args: &[String]) -> ExitCode {
  let mut input: Option<&str> = None;
  let mut output: Option<&str> = None;
  let mut format_override: Option<&str> = None;
  let mut i = 0;

  while i < args.len() {
    match args[i].as_str() {
      "--format" => {
        i += 1;
        if i >= args.len() {
          eprintln!("--format requires a value");
          return ExitCode::FAILURE;
        }
        format_override = Some(&args[i]);
      }
      arg if arg.starts_with('-') => {
        eprintln!("Unknown option: {arg}");
        return ExitCode::FAILURE;
      }
      arg => {
        if input.is_none() {
          input = Some(arg);
        } else if output.is_none() {
          output = Some(arg);
        } else {
          eprintln!("Unexpected argument: {arg}");
          return ExitCode::FAILURE;
        }
      }
    }
    i += 1;
  }

  let Some(input) = input else {
    eprintln!("Missing input file. Usage: luacad convert <input.lua> <output>");
    return ExitCode::FAILURE;
  };
  let Some(output_str) = output else {
    eprintln!(
      "Missing output file. Usage: luacad convert <input.lua> <output>"
    );
    return ExitCode::FAILURE;
  };

  let output_path = Path::new(output_str);

  let format = if let Some(fmt) = format_override {
    fmt
  } else if let Some(fmt) = infer_format(output_path) {
    fmt
  } else {
    eprintln!(
      "Cannot infer format from extension of '{output_str}'. \
       Use --format to specify one."
    );
    eprintln!("Supported formats: {}", FORMATS.join(", "));
    return ExitCode::FAILURE;
  };

  let geometries = match run_lua(input) {
    Ok(g) => g,
    Err(code) => return code,
  };

  println!(
    "OK: {} {}",
    geometries.len(),
    if geometries.len() == 1 {
      "object"
    } else {
      "objects"
    }
  );

  match export(&geometries, format, output_path) {
    Ok(()) => {
      println!("Exported to {}", output_path.display());
      ExitCode::SUCCESS
    }
    Err(e) => {
      eprintln!("Export failed: {e}");
      ExitCode::FAILURE
    }
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
    "convert" => cmd_convert(&args[1..]),
    _ => cmd_run(&args),
  }
}
