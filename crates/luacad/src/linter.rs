use selene_lib::standard_library::StandardLibrary;
use selene_lib::{Checker, CheckerConfig};

/// A single lint diagnostic with location and message.
#[derive(Debug)]
pub struct LintDiagnostic {
  pub code: &'static str,
  pub message: String,
  pub line: usize,
  pub column: usize,
  /// Byte offset range in the source string (start..end).
  pub byte_start: usize,
  pub byte_end: usize,
  pub severity: LintSeverity,
  pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
  Warning,
  Error,
}

/// Lint Lua source code using selene with LuaCAD's custom standard library.
/// Returns a list of diagnostics, or an error if the code cannot be parsed.
pub fn lint(source: &str) -> Result<Vec<LintDiagnostic>, String> {
  let ast = full_moon::parse(source).map_err(|errors| {
    errors
      .iter()
      .map(|e| format!("{e}"))
      .collect::<Vec<_>>()
      .join("\n")
  })?;

  let config: CheckerConfig<toml::value::Value> = toml::from_str(
    r#"
      [lints]
      # LuaCAD uses mixed tables (e.g. { 10, 20, 30, center = true }) as its
      # primary API pattern, so this lint produces false positives.
      mixed_table = "allow"
    "#,
  )
  .expect("embedded config should be valid");
  let std_lib = build_standard_library();
  let checker = Checker::new(config, std_lib)
    .map_err(|e| format!("Failed to create checker: {e}"))?;

  let diagnostics = checker.test_on(&ast);
  let mut results = Vec::new();

  for d in diagnostics {
    let severity = match d.severity {
      selene_lib::lints::Severity::Allow => continue,
      selene_lib::lints::Severity::Warning => LintSeverity::Warning,
      selene_lib::lints::Severity::Error => LintSeverity::Error,
    };

    let byte_start = d.diagnostic.primary_label.range.0 as usize;
    let byte_end = d.diagnostic.primary_label.range.1 as usize;
    let (line, column) = byte_offset_to_line_col(source, byte_start);

    results.push(LintDiagnostic {
      code: d.diagnostic.code,
      message: d.diagnostic.message,
      line,
      column,
      byte_start,
      byte_end,
      severity,
      notes: d.diagnostic.notes,
    });
  }

  results.sort_by_key(|d| (d.line, d.column));
  Ok(results)
}

fn byte_offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
  let mut line = 1;
  let mut col = 1;
  for (i, ch) in source.char_indices() {
    if i >= offset {
      break;
    }
    if ch == '\n' {
      line += 1;
      col = 1;
    } else {
      col += 1;
    }
  }
  (line, col)
}

/// LuaCAD-specific globals in selene YAML format.
/// Merged at runtime with the built-in lua54 standard library.
const LUACAD_STD_YAML: &str = r#"---
name: luacad
globals:
  # 3D primitives — all accept flexible arguments (table or positional)
  cube:
    args:
      - type: "..."
  sphere:
    args:
      - type: "..."
  cylinder:
    args:
      - type: "..."
  polyhedron:
    args:
      - type: "..."
  pyramid:
    args:
      - type: "..."
  torus:
    args:
      - type: "..."
  octahedron:
    args:
      - type: "..."
  icosahedron:
    args:
      - type: "..."
  ellipsoid:
    args:
      - type: "..."

  # 2D primitives
  circle:
    args:
      - type: "..."
  rect:
    args:
      - type: "..."
  square:
    args:
      - type: "..."
  polygon:
    args:
      - type: "..."

  # Rendering / export
  render:
    args:
      - type: "..."
  scad:
    args:
      - type: "..."
  import:
    args:
      - type: "..."
  surface:
    args:
      - type: "..."

  # Utilities
  var:
    args:
      - type: "..."
  cad:
    args:
      - type: "..."
  version:
    args:
      - type: "..."
  sign:
    args:
      - type: "..."
  round:
    args:
      - type: "..."
  rands:
    args:
      - type: "..."
  text:
    args:
      - type: "..."
  text3d:
    args:
      - type: "..."
  lookup:
    args:
      - type: "..."

  # Vector functions
  vector:
    args:
      - type: "..."
  vec:
    args:
      - type: "..."
  cross:
    args:
      - type: "..."
  norm:
    args:
      - type: "..."

  # Math globals (LuaCAD exposes these as bare globals)
  abs:
    args:
      - type: "..."
  sin:
    args:
      - type: "..."
  cos:
    args:
      - type: "..."
  tan:
    args:
      - type: "..."
  asin:
    args:
      - type: "..."
  acos:
    args:
      - type: "..."
  atan:
    args:
      - type: "..."
  atan2:
    args:
      - type: "..."
  floor:
    args:
      - type: "..."
  ceil:
    args:
      - type: "..."
  sqrt:
    args:
      - type: "..."
  pow:
    args:
      - type: "..."
  exp:
    args:
      - type: "..."
  log:
    args:
      - type: "..."
  ln:
    args:
      - type: "..."
  min:
    args:
      - type: "..."
  max:
    args:
      - type: "..."

  # Constants
  PI:
    property: read-only

  # BOSL2 library table (extensible)
  bosl:
    property: new-fields
"#;

fn build_standard_library() -> StandardLibrary {
  let mut base = StandardLibrary::from_name("lua53")
    .expect("lua53 standard library should be built-in");
  let custom: StandardLibrary = serde_yaml::from_str(LUACAD_STD_YAML)
    .expect("embedded LuaCAD standard library YAML should be valid");
  base.extend(custom);
  base
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn clean_code_produces_no_errors() {
    let result = lint("local x = 1\nprint(x)\n").unwrap();
    let errors: Vec<_> = result
      .iter()
      .filter(|d| d.severity == LintSeverity::Error)
      .collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
  }

  #[test]
  fn luacad_globals_not_flagged() {
    let code = r#"
      local c = cube(1)
      local s = sphere(2)
      render(c + s)
      local v = vec(1, 2, 3)
      print(v)
      print(PI)
      local x = sin(45) + cos(0)
      print(x)
    "#;
    let result = lint(code).unwrap();
    let undefined: Vec<_> = result
      .iter()
      .filter(|d| d.code == "undefined_variable")
      .collect();
    assert!(
      undefined.is_empty(),
      "LuaCAD globals should not be flagged as undefined: {undefined:?}"
    );
  }

  #[test]
  fn unused_variable_detected() {
    let result = lint("local unused = 42\n").unwrap();
    assert!(
      result.iter().any(|d| d.code == "unused_variable"),
      "should detect unused variable"
    );
  }

  #[test]
  fn parse_error_returns_err() {
    let result = lint("local x = ");
    assert!(result.is_err(), "should return parse error");
  }
}
