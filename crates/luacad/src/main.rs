use std::process::ExitCode;

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().collect();

  if args.len() < 2 {
    eprintln!("Usage: luacad <file.lua> [--export <format> <output>]");
    eprintln!();
    eprintln!("Execute LuaCAD code and optionally export the result.");
    eprintln!();
    eprintln!("Formats: stl, obj, ply, 3mf, scad");
    return ExitCode::FAILURE;
  }

  let lua_path = &args[1];
  let code = match std::fs::read_to_string(lua_path) {
    Ok(c) => c,
    Err(e) => {
      eprintln!("Error reading {lua_path}: {e}");
      return ExitCode::FAILURE;
    }
  };

  let geometries = match luacad::lua_engine::execute_lua(&code) {
    Ok(g) => g,
    Err(e) => {
      eprintln!("{e}");
      return ExitCode::FAILURE;
    }
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

  // Handle --export flag
  if let Some(pos) = args.iter().position(|a| a == "--export") {
    if pos + 2 >= args.len() {
      eprintln!("--export requires <format> <output-path>");
      return ExitCode::FAILURE;
    }
    let format = &args[pos + 1];
    let output = std::path::Path::new(&args[pos + 2]);

    let result = match format.as_str() {
      "stl" => luacad::export::export_stl(&geometries, output),
      "obj" => luacad::export::export_obj(&geometries, output),
      "ply" => luacad::export::export_ply(&geometries, output),
      "3mf" => luacad::export::export_3mf(&geometries, output),
      "scad" => {
        let nodes: Vec<_> = geometries
          .iter()
          .filter_map(|g| g.scad.clone())
          .collect();
        luacad::scad_export::export_scad(&nodes, output)
      }
      other => {
        eprintln!("Unknown format: {other}");
        eprintln!("Supported formats: stl, obj, ply, 3mf, scad");
        return ExitCode::FAILURE;
      }
    };

    match result {
      Ok(()) => println!("Exported to {}", output.display()),
      Err(e) => {
        eprintln!("Export failed: {e}");
        return ExitCode::FAILURE;
      }
    }
  }

  ExitCode::SUCCESS
}
