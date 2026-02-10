/// OpenSCAD AST and code generation.
///
/// Each geometry operation records a `ScadNode` that mirrors the Lua call.
/// When the user exports as `.scad`, the tree is serialized to valid OpenSCAD source.
use std::fmt::Write;

/// A node in the OpenSCAD AST.
#[derive(Clone, Debug)]
pub enum ScadNode {
  // 3D primitives
  Cube {
    w: f32,
    d: f32,
    h: f32,
    center: bool,
  },
  Sphere {
    r: f32,
    segments: u32,
  },
  Cylinder {
    r1: f32,
    r2: f32,
    h: f32,
    segments: u32,
    center: bool,
  },
  Polyhedron {
    points: Vec<[f32; 3]>,
    faces: Vec<Vec<usize>>,
  },

  // 2D primitives
  Circle {
    r: f32,
    segments: u32,
  },
  Square {
    w: f32,
    h: f32,
    center: bool,
  },
  Polygon {
    points: Vec<[f32; 2]>,
  },
  Text {
    text: String,
    size: f32,
    font: String,
    halign: String,
    valign: String,
  },

  // Extrusions
  LinearExtrude {
    height: f32,
    center: bool,
    twist: f32,
    slices: u32,
    scale: f32,
    child: Box<ScadNode>,
  },
  RotateExtrude {
    angle: f32,
    segments: u32,
    child: Box<ScadNode>,
  },

  // Transforms
  Translate {
    x: f32,
    y: f32,
    z: f32,
    child: Box<ScadNode>,
  },
  Rotate {
    x: f32,
    y: f32,
    z: f32,
    child: Box<ScadNode>,
  },
  Scale {
    x: f32,
    y: f32,
    z: f32,
    child: Box<ScadNode>,
  },
  Mirror {
    x: f32,
    y: f32,
    z: f32,
    child: Box<ScadNode>,
  },
  Multmatrix {
    matrix: [f32; 16],
    child: Box<ScadNode>,
  },
  Resize {
    x: f32,
    y: f32,
    z: f32,
    child: Box<ScadNode>,
  },
  Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    child: Box<ScadNode>,
  },
  Offset {
    delta: Option<f32>,
    r: Option<f32>,
    chamfer: bool,
    child: Box<ScadNode>,
  },
  Projection {
    cut: bool,
    child: Box<ScadNode>,
  },
  Render {
    convexity: u32,
    child: Box<ScadNode>,
  },

  // CSG booleans
  Union(Vec<ScadNode>),
  Difference(Vec<ScadNode>),
  Intersection(Vec<ScadNode>),
  Hull(Box<ScadNode>),
  Minkowski(Vec<ScadNode>),

  // File operations (ScadNode-only, no mesh)
  Import {
    file: String,
    convexity: u32,
  },
  Surface {
    file: String,
    center: bool,
    convexity: u32,
  },

  // Modifier characters
  Modifier {
    kind: ModifierKind,
    child: Box<ScadNode>,
  },
}

/// OpenSCAD modifier characters: *, !, #, %
#[derive(Clone, Debug)]
pub enum ModifierKind {
  /// `*` — disable/skip this subtree
  Skip,
  /// `!` — show only this subtree
  Only,
  /// `#` — highlight/debug
  Debug,
  /// `%` — transparent/background
  Transparent,
}

fn fmt_f32(v: f32) -> String {
  // Trim trailing zeros but keep at least one decimal
  let s = format!("{:.6}", v);
  let s = s.trim_end_matches('0');
  let s = s.trim_end_matches('.');
  s.to_string()
}

fn write_indent(out: &mut String, depth: usize) {
  for _ in 0..depth {
    out.push_str("  ");
  }
}

impl ScadNode {
  /// Serialize this node to OpenSCAD source code.
  #[allow(dead_code)]
  pub fn to_scad(&self) -> String {
    let mut out = String::new();
    self.write_to(&mut out, 0);
    out
  }

  fn write_to(&self, out: &mut String, depth: usize) {
    match self {
      ScadNode::Cube { w, d, h, center } => {
        write_indent(out, depth);
        if *center {
          let _ = writeln!(
            out,
            "cube([{}, {}, {}], center = true);",
            fmt_f32(*w),
            fmt_f32(*d),
            fmt_f32(*h)
          );
        } else {
          let _ = writeln!(
            out,
            "cube([{}, {}, {}]);",
            fmt_f32(*w),
            fmt_f32(*d),
            fmt_f32(*h)
          );
        }
      }

      ScadNode::Sphere { r, segments } => {
        write_indent(out, depth);
        let _ =
          writeln!(out, "sphere(r = {}, $fn = {});", fmt_f32(*r), segments);
      }

      ScadNode::Cylinder {
        r1,
        r2,
        h,
        segments,
        center,
      } => {
        write_indent(out, depth);
        let center_str = if *center { ", center = true" } else { "" };
        let _ = writeln!(
          out,
          "cylinder(h = {}, r1 = {}, r2 = {}, $fn = {}{});",
          fmt_f32(*h),
          fmt_f32(*r1),
          fmt_f32(*r2),
          segments,
          center_str
        );
      }

      ScadNode::Polyhedron { points, faces } => {
        write_indent(out, depth);
        out.push_str("polyhedron(\n");
        write_indent(out, depth + 1);
        out.push_str("points = [\n");
        for (i, p) in points.iter().enumerate() {
          write_indent(out, depth + 2);
          let _ = write!(
            out,
            "[{}, {}, {}]{}",
            fmt_f32(p[0]),
            fmt_f32(p[1]),
            fmt_f32(p[2]),
            if i + 1 < points.len() { ",\n" } else { "\n" }
          );
        }
        write_indent(out, depth + 1);
        out.push_str("],\n");
        write_indent(out, depth + 1);
        out.push_str("faces = [\n");
        for (i, f) in faces.iter().enumerate() {
          write_indent(out, depth + 2);
          let indices: Vec<String> =
            f.iter().map(|idx| idx.to_string()).collect();
          let _ = write!(
            out,
            "[{}]{}",
            indices.join(", "),
            if i + 1 < faces.len() { ",\n" } else { "\n" }
          );
        }
        write_indent(out, depth + 1);
        out.push_str("]\n");
        write_indent(out, depth);
        out.push_str(");\n");
      }

      ScadNode::Circle { r, segments } => {
        write_indent(out, depth);
        let _ =
          writeln!(out, "circle(r = {}, $fn = {});", fmt_f32(*r), segments);
      }

      ScadNode::Square { w, h, center } => {
        write_indent(out, depth);
        if *center {
          let _ = writeln!(
            out,
            "square([{}, {}], center = true);",
            fmt_f32(*w),
            fmt_f32(*h)
          );
        } else {
          let _ = writeln!(out, "square([{}, {}]);", fmt_f32(*w), fmt_f32(*h));
        }
      }

      ScadNode::Polygon { points } => {
        write_indent(out, depth);
        out.push_str("polygon(points = [\n");
        for (i, p) in points.iter().enumerate() {
          write_indent(out, depth + 1);
          let _ = write!(
            out,
            "[{}, {}]{}",
            fmt_f32(p[0]),
            fmt_f32(p[1]),
            if i + 1 < points.len() { ",\n" } else { "\n" }
          );
        }
        write_indent(out, depth);
        out.push_str("]);\n");
      }

      ScadNode::Text {
        text,
        size,
        font,
        halign,
        valign,
      } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "text(\"{}\", size = {}, font = \"{}\", halign = \"{}\", valign = \"{}\");",
          text,
          fmt_f32(*size),
          font,
          halign,
          valign
        );
      }

      ScadNode::LinearExtrude {
        height,
        center,
        twist,
        slices,
        scale,
        child,
      } => {
        write_indent(out, depth);
        let mut params = format!("height = {}", fmt_f32(*height));
        if *center {
          params.push_str(", center = true");
        }
        if *twist != 0.0 {
          params.push_str(&format!(", twist = {}", fmt_f32(*twist)));
        }
        if *slices > 0 {
          params.push_str(&format!(", slices = {}", slices));
        }
        if *scale != 1.0 {
          params.push_str(&format!(", scale = {}", fmt_f32(*scale)));
        }
        let _ = writeln!(out, "linear_extrude({}) {{", params);
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::RotateExtrude {
        angle,
        segments,
        child,
      } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "rotate_extrude(angle = {}, $fn = {}) {{",
          fmt_f32(*angle),
          segments
        );
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Translate { x, y, z, child } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "translate([{}, {}, {}]) {{",
          fmt_f32(*x),
          fmt_f32(*y),
          fmt_f32(*z)
        );
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Rotate { x, y, z, child } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "rotate([{}, {}, {}]) {{",
          fmt_f32(*x),
          fmt_f32(*y),
          fmt_f32(*z)
        );
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Scale { x, y, z, child } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "scale([{}, {}, {}]) {{",
          fmt_f32(*x),
          fmt_f32(*y),
          fmt_f32(*z)
        );
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Mirror { x, y, z, child } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "mirror([{}, {}, {}]) {{",
          fmt_f32(*x),
          fmt_f32(*y),
          fmt_f32(*z)
        );
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Multmatrix { matrix, child } => {
        write_indent(out, depth);
        out.push_str("multmatrix([\n");
        for row in 0..4 {
          write_indent(out, depth + 1);
          let _ = writeln!(
            out,
            "[{}, {}, {}, {}]{}",
            fmt_f32(matrix[row * 4]),
            fmt_f32(matrix[row * 4 + 1]),
            fmt_f32(matrix[row * 4 + 2]),
            fmt_f32(matrix[row * 4 + 3]),
            if row < 3 { "," } else { "" }
          );
        }
        write_indent(out, depth);
        out.push_str("]) {\n");
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Resize { x, y, z, child } => {
        write_indent(out, depth);
        let _ = writeln!(
          out,
          "resize([{}, {}, {}]) {{",
          fmt_f32(*x),
          fmt_f32(*y),
          fmt_f32(*z)
        );
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Color { r, g, b, a, child } => {
        write_indent(out, depth);
        if *a < 1.0 {
          let _ = writeln!(
            out,
            "color([{}, {}, {}, {}]) {{",
            fmt_f32(*r),
            fmt_f32(*g),
            fmt_f32(*b),
            fmt_f32(*a)
          );
        } else {
          let _ = writeln!(
            out,
            "color([{}, {}, {}]) {{",
            fmt_f32(*r),
            fmt_f32(*g),
            fmt_f32(*b)
          );
        }
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Offset {
        delta,
        r,
        chamfer,
        child,
      } => {
        write_indent(out, depth);
        let param = if let Some(d) = delta {
          format!("delta = {}", fmt_f32(*d))
        } else if let Some(rv) = r {
          format!("r = {}", fmt_f32(*rv))
        } else {
          "delta = 0".to_string()
        };
        let chamfer_str = if *chamfer { ", chamfer = true" } else { "" };
        let _ = writeln!(out, "offset({}{}) {{", param, chamfer_str);
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Projection { cut, child } => {
        write_indent(out, depth);
        if *cut {
          let _ = writeln!(out, "projection(cut = true) {{");
        } else {
          let _ = writeln!(out, "projection() {{");
        }
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Render { convexity, child } => {
        write_indent(out, depth);
        if *convexity > 0 {
          let _ = writeln!(out, "render(convexity = {}) {{", convexity);
        } else {
          let _ = writeln!(out, "render() {{");
        }
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Union(children) => {
        write_indent(out, depth);
        out.push_str("union() {\n");
        for child in children {
          child.write_to(out, depth + 1);
        }
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Difference(children) => {
        write_indent(out, depth);
        out.push_str("difference() {\n");
        for child in children {
          child.write_to(out, depth + 1);
        }
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Intersection(children) => {
        write_indent(out, depth);
        out.push_str("intersection() {\n");
        for child in children {
          child.write_to(out, depth + 1);
        }
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Hull(child) => {
        write_indent(out, depth);
        out.push_str("hull() {\n");
        child.write_to(out, depth + 1);
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Minkowski(children) => {
        write_indent(out, depth);
        out.push_str("minkowski() {\n");
        for child in children {
          child.write_to(out, depth + 1);
        }
        write_indent(out, depth);
        out.push_str("}\n");
      }

      ScadNode::Import { file, convexity } => {
        write_indent(out, depth);
        if *convexity > 0 {
          let _ =
            writeln!(out, "import(\"{}\", convexity = {});", file, convexity);
        } else {
          let _ = writeln!(out, "import(\"{}\");", file);
        }
      }

      ScadNode::Surface {
        file,
        center,
        convexity,
      } => {
        write_indent(out, depth);
        let center_str = if *center { ", center = true" } else { "" };
        let convexity_str = if *convexity > 0 {
          format!(", convexity = {}", convexity)
        } else {
          String::new()
        };
        let _ = writeln!(
          out,
          "surface(file = \"{}\"{}{}); ",
          file, center_str, convexity_str
        );
      }

      ScadNode::Modifier { kind, child } => {
        write_indent(out, depth);
        let prefix = match kind {
          ModifierKind::Skip => "*",
          ModifierKind::Only => "!",
          ModifierKind::Debug => "#",
          ModifierKind::Transparent => "%",
        };
        // Render child into a temporary buffer, then prepend the modifier char
        // The child output starts with indentation, so we write the prefix
        // directly before the child's first character.
        let mut child_out = String::new();
        child.write_to(&mut child_out, depth);
        // Trim leading whitespace from child output and replace with modifier + indent
        let trimmed = child_out.trim_start();
        out.push_str(prefix);
        out.push_str(trimmed);
      }
    }
  }
}

/// Generate a complete OpenSCAD file from a list of geometry ScadNodes.
/// Multiple objects are wrapped in a union().
pub fn generate_scad(nodes: &[ScadNode]) -> String {
  let mut out = String::new();
  out.push_str("// Generated by LuaCAD Studio\n\n");

  match nodes.len() {
    0 => {}
    1 => nodes[0].write_to(&mut out, 0),
    _ => {
      out.push_str("union() {\n");
      for node in nodes {
        node.write_to(&mut out, 1);
      }
      out.push_str("}\n");
    }
  }

  out
}

/// Export OpenSCAD source to a file.
pub fn export_scad(
  nodes: &[ScadNode],
  path: &std::path::Path,
) -> Result<(), String> {
  if nodes.is_empty() {
    return Err("No geometry to export".to_string());
  }
  let scad = generate_scad(nodes);
  std::fs::write(path, scad)
    .map_err(|e| format!("Failed to write OpenSCAD file: {e}"))?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cube_basic() {
    let node = ScadNode::Cube {
      w: 10.0,
      d: 20.0,
      h: 30.0,
      center: false,
    };
    assert_eq!(node.to_scad(), "cube([10, 20, 30]);\n");
  }

  #[test]
  fn cube_centered() {
    let node = ScadNode::Cube {
      w: 5.0,
      d: 5.0,
      h: 5.0,
      center: true,
    };
    assert_eq!(node.to_scad(), "cube([5, 5, 5], center = true);\n");
  }

  #[test]
  fn difference_cube_minus_cylinder() {
    let node = ScadNode::Difference(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: true,
      },
      ScadNode::Cylinder {
        r1: 3.0,
        r2: 3.0,
        h: 20.0,
        segments: 32,
        center: true,
      },
    ]);
    let scad = node.to_scad();
    assert!(scad.contains("difference()"));
    assert!(scad.contains("cube([10, 10, 10], center = true);"));
    assert!(
      scad
        .contains("cylinder(h = 20, r1 = 3, r2 = 3, $fn = 32, center = true);")
    );
  }

  #[test]
  fn translate_wraps_child() {
    let node = ScadNode::Translate {
      x: 1.0,
      y: 2.0,
      z: 3.0,
      child: Box::new(ScadNode::Sphere {
        r: 5.0,
        segments: 16,
      }),
    };
    let scad = node.to_scad();
    assert!(scad.contains("translate([1, 2, 3])"));
    assert!(scad.contains("sphere(r = 5, $fn = 16);"));
  }

  #[test]
  fn generate_scad_single() {
    let nodes = vec![ScadNode::Cube {
      w: 1.0,
      d: 2.0,
      h: 3.0,
      center: false,
    }];
    let scad = generate_scad(&nodes);
    assert!(scad.starts_with("// Generated by LuaCAD Studio"));
    assert!(scad.contains("cube([1, 2, 3]);"));
    // Single object should NOT be wrapped in union
    assert!(!scad.contains("union()"));
  }

  #[test]
  fn generate_scad_multiple_wraps_in_union() {
    let nodes = vec![
      ScadNode::Cube {
        w: 1.0,
        d: 1.0,
        h: 1.0,
        center: false,
      },
      ScadNode::Sphere {
        r: 2.0,
        segments: 16,
      },
    ];
    let scad = generate_scad(&nodes);
    assert!(scad.contains("union()"));
  }

  #[test]
  fn sphere_with_segments() {
    let node = ScadNode::Sphere {
      r: 3.5,
      segments: 32,
    };
    assert_eq!(node.to_scad(), "sphere(r = 3.5, $fn = 32);\n");
  }

  #[test]
  fn cylinder_no_center() {
    let node = ScadNode::Cylinder {
      r1: 2.0,
      r2: 1.0,
      h: 5.0,
      segments: 16,
      center: false,
    };
    let scad = node.to_scad();
    assert_eq!(scad, "cylinder(h = 5, r1 = 2, r2 = 1, $fn = 16);\n");
  }

  #[test]
  fn linear_extrude_wraps_2d() {
    let node = ScadNode::LinearExtrude {
      height: 10.0,
      center: false,
      twist: 0.0,
      slices: 0,
      scale: 1.0,
      child: Box::new(ScadNode::Circle {
        r: 5.0,
        segments: 32,
      }),
    };
    let scad = node.to_scad();
    assert!(scad.contains("linear_extrude(height = 10)"));
    assert!(scad.contains("  circle(r = 5, $fn = 32);"));
  }

  #[test]
  fn rotate_extrude_wraps_2d() {
    let node = ScadNode::RotateExtrude {
      angle: 360.0,
      segments: 24,
      child: Box::new(ScadNode::Translate {
        x: 5.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Circle {
          r: 1.0,
          segments: 16,
        }),
      }),
    };
    let scad = node.to_scad();
    assert!(scad.contains("rotate_extrude(angle = 360, $fn = 24)"));
    assert!(scad.contains("translate([5, 0, 0])"));
    assert!(scad.contains("circle(r = 1, $fn = 16);"));
  }

  #[test]
  fn nested_transforms() {
    let node = ScadNode::Translate {
      x: 10.0,
      y: 0.0,
      z: 0.0,
      child: Box::new(ScadNode::Rotate {
        x: 0.0,
        y: 0.0,
        z: 45.0,
        child: Box::new(ScadNode::Scale {
          x: 2.0,
          y: 1.0,
          z: 1.0,
          child: Box::new(ScadNode::Cube {
            w: 1.0,
            d: 1.0,
            h: 1.0,
            center: true,
          }),
        }),
      }),
    };
    let scad = node.to_scad();
    // Verify proper nesting/indentation
    assert!(scad.contains("translate([10, 0, 0]) {\n"));
    assert!(scad.contains("  rotate([0, 0, 45]) {\n"));
    assert!(scad.contains("    scale([2, 1, 1]) {\n"));
    assert!(scad.contains("      cube([1, 1, 1], center = true);\n"));
  }

  #[test]
  fn color_wraps_child() {
    let node = ScadNode::Color {
      r: 1.0,
      g: 0.0,
      b: 0.0,
      a: 1.0,
      child: Box::new(ScadNode::Cube {
        w: 5.0,
        d: 5.0,
        h: 5.0,
        center: false,
      }),
    };
    let scad = node.to_scad();
    assert!(scad.contains("color([1, 0, 0])"));
    assert!(scad.contains("cube([5, 5, 5]);"));
  }

  #[test]
  fn hull_wraps_child() {
    let node = ScadNode::Hull(Box::new(ScadNode::Union(vec![
      ScadNode::Sphere {
        r: 1.0,
        segments: 16,
      },
      ScadNode::Translate {
        x: 5.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Sphere {
          r: 1.0,
          segments: 16,
        }),
      },
    ])));
    let scad = node.to_scad();
    assert!(scad.contains("hull()"));
    assert!(scad.contains("union()"));
    assert!(scad.contains("sphere(r = 1, $fn = 16);"));
    assert!(scad.contains("translate([5, 0, 0])"));
  }

  #[test]
  fn minkowski_two_children() {
    let node = ScadNode::Minkowski(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: true,
      },
      ScadNode::Sphere {
        r: 1.0,
        segments: 16,
      },
    ]);
    let scad = node.to_scad();
    assert!(scad.contains("minkowski()"));
    assert!(scad.contains("cube([10, 10, 10], center = true);"));
    assert!(scad.contains("sphere(r = 1, $fn = 16);"));
  }

  #[test]
  fn intersection_output() {
    let node = ScadNode::Intersection(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: true,
      },
      ScadNode::Sphere {
        r: 7.0,
        segments: 32,
      },
    ]);
    let scad = node.to_scad();
    assert!(scad.contains("intersection()"));
  }

  #[test]
  fn mirror_output() {
    let node = ScadNode::Mirror {
      x: 1.0,
      y: 0.0,
      z: 0.0,
      child: Box::new(ScadNode::Cube {
        w: 5.0,
        d: 5.0,
        h: 5.0,
        center: false,
      }),
    };
    let scad = node.to_scad();
    assert!(scad.contains("mirror([1, 0, 0])"));
    assert!(scad.contains("cube([5, 5, 5]);"));
  }

  #[test]
  fn square_2d() {
    let node = ScadNode::Square {
      w: 10.0,
      h: 5.0,
      center: false,
    };
    assert_eq!(node.to_scad(), "square([10, 5]);\n");
  }

  #[test]
  fn square_2d_centered() {
    let node = ScadNode::Square {
      w: 10.0,
      h: 5.0,
      center: true,
    };
    assert_eq!(node.to_scad(), "square([10, 5], center = true);\n");
  }

  #[test]
  fn polygon_2d() {
    let node = ScadNode::Polygon {
      points: vec![[0.0, 0.0], [10.0, 0.0], [5.0, 10.0]],
    };
    let scad = node.to_scad();
    assert!(scad.contains("polygon(points = ["));
    assert!(scad.contains("[0, 0]"));
    assert!(scad.contains("[10, 0]"));
    assert!(scad.contains("[5, 10]"));
  }

  #[test]
  fn polyhedron_3d() {
    let node = ScadNode::Polyhedron {
      points: vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
      ],
      faces: vec![vec![0, 1, 2], vec![0, 3, 1], vec![0, 2, 3], vec![1, 3, 2]],
    };
    let scad = node.to_scad();
    assert!(scad.contains("polyhedron("));
    assert!(scad.contains("points = ["));
    assert!(scad.contains("faces = ["));
    assert!(scad.contains("[0, 1, 2]"));
  }

  #[test]
  fn fmt_f32_trims_trailing_zeros() {
    assert_eq!(super::fmt_f32(1.0), "1");
    assert_eq!(super::fmt_f32(1.5), "1.5");
    assert_eq!(super::fmt_f32(0.123456), "0.123456");
    assert_eq!(super::fmt_f32(0.1), "0.1");
    assert_eq!(super::fmt_f32(100.0), "100");
  }

  #[test]
  fn export_scad_writes_file() {
    let nodes = vec![ScadNode::Difference(vec![
      ScadNode::Cube {
        w: 4.0,
        d: 2.0,
        h: 1.0,
        center: true,
      },
      ScadNode::Cylinder {
        r1: 0.5,
        r2: 0.5,
        h: 3.0,
        segments: 16,
        center: true,
      },
    ])];

    let dir = std::env::temp_dir().join("luacad_scad_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test_output.scad");

    export_scad(&nodes, &path).expect("export_scad should succeed");
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("// Generated by LuaCAD Studio"));
    assert!(content.contains("difference()"));
    assert!(content.contains("cube([4, 2, 1], center = true);"));
    assert!(content.contains(
      "cylinder(h = 3, r1 = 0.5, r2 = 0.5, $fn = 16, center = true);"
    ));

    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn export_scad_empty_returns_error() {
    let dir = std::env::temp_dir().join("luacad_scad_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.scad");

    let result = export_scad(&[], &path);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No geometry"));
  }
}
