//! Import SVG files as 2D polygons, matching OpenSCAD's conventions:
//! physical units (mm, cm, in, …) are honored, unitless coordinates are
//! interpreted at 96 dpi, and the y-axis is flipped across the viewBox
//! so the drawing keeps its visual orientation with y pointing up.

use kurbo::PathEl;
use usvg::tiny_skia_path::PathSegment;

const PX_TO_MM: f64 = 25.4 / 96.0;

/// Curve flattening tolerance in px (≈ 13 µm after px→mm conversion).
const TOLERANCE: f64 = 0.05;

/// Parse an SVG file into closed contours in mm.
pub fn svg_to_polygons(file: &str) -> Result<Vec<Vec<[f32; 2]>>, String> {
  let data = std::fs::read(file)
    .map_err(|e| format!("Cannot read SVG file {file}: {e}"))?;
  let tree = usvg::Tree::from_data(&data, &usvg::Options::default())
    .map_err(|e| format!("Cannot parse SVG file {file}: {e}"))?;
  let height_px = tree.size().height() as f64;
  let mut contours = Vec::new();
  collect_group(tree.root(), height_px, &mut contours);
  Ok(contours)
}

fn collect_group(
  group: &usvg::Group,
  height_px: f64,
  out: &mut Vec<Vec<[f32; 2]>>,
) {
  for node in group.children() {
    match node {
      usvg::Node::Group(g) => collect_group(g, height_px, out),
      usvg::Node::Path(p) => collect_path(p, height_px, out),
      _ => {}
    }
  }
}

fn collect_path(
  path: &usvg::Path,
  height_px: f64,
  out: &mut Vec<Vec<[f32; 2]>>,
) {
  let Some(data) = path.data().clone().transform(path.abs_transform()) else {
    return;
  };

  let mut bez = kurbo::BezPath::new();
  for seg in data.segments() {
    match seg {
      PathSegment::MoveTo(p) => bez.move_to(pt(p)),
      PathSegment::LineTo(p) => bez.line_to(pt(p)),
      PathSegment::QuadTo(p1, p2) => bez.quad_to(pt(p1), pt(p2)),
      PathSegment::CubicTo(p1, p2, p3) => bez.curve_to(pt(p1), pt(p2), pt(p3)),
      PathSegment::Close => bez.close_path(),
    }
  }

  let to_mm = |p: kurbo::Point| {
    [
      (p.x * PX_TO_MM) as f32,
      ((height_px - p.y) * PX_TO_MM) as f32,
    ]
  };

  // Both explicit `Z` and an unclosed trailing subpath produce a contour,
  // like SVG fills (subpaths are implicitly closed when filling)
  let mut contour: Vec<[f32; 2]> = Vec::new();
  kurbo::flatten(bez, TOLERANCE, |el| match el {
    PathEl::MoveTo(p) => {
      flush(&mut contour, out);
      contour.push(to_mm(p));
    }
    PathEl::LineTo(p) => contour.push(to_mm(p)),
    PathEl::ClosePath => flush(&mut contour, out),
    // flatten() only emits MoveTo/LineTo/ClosePath
    _ => {}
  });
  flush(&mut contour, out);
}

fn pt(p: usvg::tiny_skia_path::Point) -> kurbo::Point {
  kurbo::Point::new(p.x as f64, p.y as f64)
}

fn flush(contour: &mut Vec<[f32; 2]>, out: &mut Vec<Vec<[f32; 2]>>) {
  if contour.len() >= 3 {
    out.push(std::mem::take(contour));
  } else {
    contour.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;

  fn parse(svg: &str) -> Vec<Vec<[f32; 2]>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
      "luacad_svg_test_{}_{}.svg",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = std::fs::File::create(&path).unwrap();
    write!(file, "{svg}").unwrap();
    drop(file);
    let result = svg_to_polygons(path.to_str().unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    result
  }

  fn bbox(contours: &[Vec<[f32; 2]>]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for c in contours {
      for p in c {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
      }
    }
    b
  }

  #[test]
  fn mm_units_map_one_to_one() {
    // A 10×10 mm square filling a 10×10 viewBox with mm dimensions
    let contours = parse(
      r#"<svg xmlns="http://www.w3.org/2000/svg"
           width="10mm" height="10mm" viewBox="0 0 10 10">
           <path d="M 0,0 L 10,0 L 10,10 L 0,10 Z"/>
         </svg>"#,
    );
    assert_eq!(contours.len(), 1);
    let b = bbox(&contours);
    assert!((b[0]).abs() < 1e-3 && (b[1]).abs() < 1e-3);
    assert!((b[2] - 10.0).abs() < 1e-3 && (b[3] - 10.0).abs() < 1e-3);
  }

  #[test]
  fn y_axis_is_flipped() {
    // A rectangle in the top half of the viewBox ends up in
    // the upper y range of the output (OpenSCAD convention)
    let contours = parse(
      r#"<svg xmlns="http://www.w3.org/2000/svg"
           width="10mm" height="10mm" viewBox="0 0 10 10">
           <path d="M 0,0 L 10,0 L 10,2 L 0,2 Z"/>
         </svg>"#,
    );
    let b = bbox(&contours);
    assert!((b[1] - 8.0).abs() < 1e-3 && (b[3] - 10.0).abs() < 1e-3);
  }

  #[test]
  fn unitless_is_96_dpi() {
    // 96 unitless px = 1 inch = 25.4 mm
    let contours = parse(
      r#"<svg xmlns="http://www.w3.org/2000/svg"
           width="96" height="96" viewBox="0 0 96 96">
           <path d="M 0,0 L 96,0 L 96,96 L 0,96 Z"/>
         </svg>"#,
    );
    let b = bbox(&contours);
    assert!((b[2] - 25.4).abs() < 1e-3 && (b[3] - 25.4).abs() < 1e-3);
  }

  #[test]
  fn beziers_are_flattened() {
    // A circle approximated with cubic béziers
    let contours = parse(
      r#"<svg xmlns="http://www.w3.org/2000/svg"
           width="10mm" height="10mm" viewBox="0 0 10 10">
           <circle cx="5" cy="5" r="5"/>
         </svg>"#,
    );
    assert_eq!(contours.len(), 1);
    assert!(contours[0].len() > 16, "expected a smooth polygon");
    let b = bbox(&contours);
    assert!((b[0]).abs() < 0.05 && (b[2] - 10.0).abs() < 0.05);
  }
}
