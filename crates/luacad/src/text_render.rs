//! Turn `text()` into closed contours, so the Manifold backend can extrude it
//! like any other sketch.
//!
//! Glyph outlines come straight from the font file. Layout is a single line of
//! left-to-right glyph advances plus kerning pairs — enough for the labels and
//! engravings text() is used for, but not a substitute for a shaping engine:
//! ligatures, cursive joining and bidirectional runs are not applied. OpenSCAD
//! runs the same text through HarfBuzz, so complex scripts render there and
//! not here.

use kurbo::PathEl;
use std::sync::OnceLock;

/// Curve flattening tolerance, in font units scaled to the requested size.
/// 0.01 mm keeps the facets well below the resolution of any printer.
const TOLERANCE: f64 = 0.01;

/// Horizontal placement of the text relative to the origin.
enum HAlign {
  Left,
  Center,
  Right,
}

/// Vertical placement of the text relative to the origin.
enum VAlign {
  Baseline,
  Bottom,
  Top,
  Center,
}

/// The system font database, loaded once. Scanning the system font
/// directories takes long enough to be worth doing a single time, and a
/// script with many `text()` calls would otherwise pay it repeatedly.
fn font_database() -> &'static fontdb::Database {
  static DB: OnceLock<fontdb::Database> = OnceLock::new();
  DB.get_or_init(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db
  })
}

/// Resolve an OpenSCAD font spec to a face in the database.
///
/// The spec is `"Family"` or `"Family:style=Style"`, e.g. `"Arial:style=Bold"`.
/// An unknown family falls back to the default sans-serif face, matching
/// OpenSCAD's behaviour of rendering *something* rather than nothing.
fn resolve_face(spec: &str) -> Option<fontdb::ID> {
  let db = font_database();

  let (family, style) = match spec.split_once(':') {
    Some((family, rest)) => {
      let style = rest
        .split(':')
        .find_map(|part| part.trim().strip_prefix("style="))
        .map(|s| s.trim().to_ascii_lowercase());
      (family.trim(), style)
    }
    None => (spec.trim(), None),
  };

  // OpenSCAD style names combine weight and slant in one string.
  let style = style.unwrap_or_default();
  let bold = style.contains("bold");
  let italic = style.contains("italic") || style.contains("oblique");

  let query = fontdb::Query {
    families: &[fontdb::Family::Name(family), fontdb::Family::SansSerif],
    weight: if bold {
      fontdb::Weight::BOLD
    } else {
      fontdb::Weight::NORMAL
    },
    stretch: fontdb::Stretch::Normal,
    style: if italic {
      fontdb::Style::Italic
    } else {
      fontdb::Style::Normal
    },
  };

  db.query(&query).or_else(|| {
    // Nothing matched the family at all — take any face rather than failing.
    db.query(&fontdb::Query {
      families: &[fontdb::Family::SansSerif],
      ..query
    })
  })
}

/// Whether the machine has any font at all to draw with.
///
/// Text geometry is only as available as the system font database, and a bare
/// container has none. Tests that assert on glyph outlines use this to skip
/// rather than fail there; CI installs a font so they actually run.
#[cfg(test)]
pub(crate) fn has_system_font() -> bool {
  resolve_face("sans-serif").is_some()
}

/// Flatten `text` into closed contours, in mm, ready for
/// `CrossSection::from_contours`.
///
/// Contours keep the font's winding directions, so counters (the hole in an
/// `o`) come out as reversed contours and the non-zero fill rule cuts them
/// away.
pub fn text_to_polygons(
  text: &str,
  size: f32,
  font: &str,
  halign: &str,
  valign: &str,
) -> Result<Vec<Vec<[f32; 2]>>, String> {
  if text.is_empty() {
    return Ok(Vec::new());
  }

  let halign = match halign.trim().to_ascii_lowercase().as_str() {
    "" | "left" => HAlign::Left,
    "center" | "centre" => HAlign::Center,
    "right" => HAlign::Right,
    other => {
      return Err(format!(
        "text(): unknown halign '{other}'. Valid: left, center, right"
      ));
    }
  };
  let valign = match valign.trim().to_ascii_lowercase().as_str() {
    "" | "baseline" => VAlign::Baseline,
    "bottom" => VAlign::Bottom,
    "top" => VAlign::Top,
    "center" | "centre" => VAlign::Center,
    other => {
      return Err(format!(
        "text(): unknown valign '{other}'. \
         Valid: baseline, bottom, top, center"
      ));
    }
  };

  let id = resolve_face(font).ok_or_else(|| {
    format!("text(): no font found for '{font}', and no fallback is installed")
  })?;

  // The outer Option is "no such face in the database", the inner Result is
  // "the face is there but unreadable".
  font_database()
    .with_face_data(id, |data, index| {
      let face = ttf_parser::Face::parse(data, index)
        .map_err(|e| format!("text(): cannot parse font '{font}': {e}"))?;
      Ok(layout(text, size, &face, &halign, &valign))
    })
    .ok_or_else(|| format!("text(): cannot read font data for '{font}'"))?
}

/// Walk the string, outlining each glyph at the current pen position.
fn layout(
  text: &str,
  size: f32,
  face: &ttf_parser::Face,
  halign: &HAlign,
  valign: &VAlign,
) -> Vec<Vec<[f32; 2]>> {
  // Font units are relative to the em square; `size` is the em size in mm,
  // which is how OpenSCAD interprets it.
  let scale = size as f64 / face.units_per_em() as f64;

  let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
  let mut pen: f64 = 0.0;
  let mut previous: Option<ttf_parser::GlyphId> = None;

  for ch in text.chars() {
    // A character the font has no glyph for still advances by the width of
    // `.notdef`, so missing glyphs leave a gap instead of silently colliding.
    let glyph = face.glyph_index(ch).unwrap_or(ttf_parser::GlyphId(0));

    if let Some(prev) = previous {
      pen += kerning(face, prev, glyph);
    }

    let mut builder = OutlineBuilder::default();
    // Whitespace has no outline; only the advance matters.
    if face.outline_glyph(glyph, &mut builder).is_some() {
      builder.flush();
      for path in builder.paths {
        flatten_into(path, pen, scale, &mut contours);
      }
    }

    pen += face.glyph_hor_advance(glyph).unwrap_or(0) as f64;
    previous = Some(glyph);
  }

  let width = pen * scale;
  let dx = match halign {
    HAlign::Left => 0.0,
    HAlign::Center => -width / 2.0,
    HAlign::Right => -width,
  };

  // Ascender is positive above the baseline, descender negative below it.
  let ascender = face.ascender() as f64 * scale;
  let descender = face.descender() as f64 * scale;
  let dy = match valign {
    VAlign::Baseline => 0.0,
    VAlign::Bottom => -descender,
    VAlign::Top => -ascender,
    VAlign::Center => -(ascender + descender) / 2.0,
  };

  if dx != 0.0 || dy != 0.0 {
    for contour in &mut contours {
      for p in contour.iter_mut() {
        p[0] += dx as f32;
        p[1] += dy as f32;
      }
    }
  }

  contours
}

/// Kerning adjustment between two glyphs, in font units.
///
/// Only the legacy `kern` table is consulted. Fonts that carry their kerning
/// in `GPOS` instead need a shaping engine to apply it, so their text comes
/// out with default advances.
fn kerning(
  face: &ttf_parser::Face,
  left: ttf_parser::GlyphId,
  right: ttf_parser::GlyphId,
) -> f64 {
  let Some(kern) = face.tables().kern else {
    return 0.0;
  };
  kern
    .subtables
    .into_iter()
    .filter(|s| s.horizontal && !s.variable)
    .find_map(|s| s.glyphs_kerning(left, right))
    .unwrap_or(0) as f64
}

/// Flatten one glyph contour to line segments and append it, positioned at
/// `pen` and scaled to millimetres.
fn flatten_into(
  path: kurbo::BezPath,
  pen: f64,
  scale: f64,
  out: &mut Vec<Vec<[f32; 2]>>,
) {
  let to_mm =
    |p: kurbo::Point| [((p.x + pen) * scale) as f32, (p.y * scale) as f32];

  // Flattening happens in font units, so the tolerance has to be expressed
  // there too, or small text would come out coarse and large text needlessly
  // dense.
  let tolerance = TOLERANCE / scale;

  let mut contour: Vec<[f32; 2]> = Vec::new();
  kurbo::flatten(path, tolerance, |el| match el {
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

fn flush(contour: &mut Vec<[f32; 2]>, out: &mut Vec<Vec<[f32; 2]>>) {
  if contour.len() >= 3 {
    out.push(std::mem::take(contour));
  } else {
    contour.clear();
  }
}

/// Collects glyph outlines from ttf-parser into kurbo paths.
///
/// One path per contour: glyphs are made of several closed loops, and keeping
/// them separate lets each be flattened and wound independently.
#[derive(Default)]
struct OutlineBuilder {
  paths: Vec<kurbo::BezPath>,
  current: kurbo::BezPath,
}

impl OutlineBuilder {
  fn flush(&mut self) {
    if !self.current.is_empty() {
      self.paths.push(std::mem::take(&mut self.current));
    }
  }
}

impl ttf_parser::OutlineBuilder for OutlineBuilder {
  fn move_to(&mut self, x: f32, y: f32) {
    self.flush();
    self.current.move_to((x as f64, y as f64));
  }

  fn line_to(&mut self, x: f32, y: f32) {
    self.current.line_to((x as f64, y as f64));
  }

  fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
    self
      .current
      .quad_to((x1 as f64, y1 as f64), (x as f64, y as f64));
  }

  fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
    self.current.curve_to(
      (x1 as f64, y1 as f64),
      (x2 as f64, y2 as f64),
      (x as f64, y as f64),
    );
  }

  fn close(&mut self) {
    self.current.close_path();
    self.flush();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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

  /// Every assertion below needs at least one system font. A machine without
  /// one (a bare container) skips rather than fails.
  fn have_fonts() -> bool {
    super::has_system_font()
  }

  fn render(
    text: &str,
    size: f32,
    halign: &str,
    valign: &str,
  ) -> Vec<Vec<[f32; 2]>> {
    text_to_polygons(text, size, "sans-serif", halign, valign).unwrap()
  }

  #[test]
  fn empty_text_has_no_contours() {
    assert!(
      text_to_polygons("", 10.0, "sans-serif", "left", "baseline")
        .unwrap()
        .is_empty()
    );
  }

  #[test]
  fn a_glyph_produces_closed_contours() {
    if !have_fonts() {
      return;
    }
    let contours = render("H", 10.0, "left", "baseline");
    assert!(!contours.is_empty(), "expected an outline for 'H'");
    assert!(contours.iter().all(|c| c.len() >= 3));
  }

  #[test]
  fn a_letter_with_a_counter_has_more_than_one_contour() {
    if !have_fonts() {
      return;
    }
    // 'o' is an outer ring plus the counter that has to be cut out of it.
    assert!(render("o", 10.0, "left", "baseline").len() >= 2);
  }

  #[test]
  fn size_scales_the_outline_proportionally() {
    if !have_fonts() {
      return;
    }
    let small = bbox(&render("H", 10.0, "left", "baseline"));
    let large = bbox(&render("H", 20.0, "left", "baseline"));
    let ratio = (large[3] - large[1]) / (small[3] - small[1]);
    assert!((ratio - 2.0).abs() < 0.01, "height ratio was {ratio}");
  }

  #[test]
  fn baseline_alignment_puts_the_cap_height_above_zero() {
    if !have_fonts() {
      return;
    }
    let b = bbox(&render("H", 10.0, "left", "baseline"));
    assert!(
      b[1].abs() < 0.01,
      "'H' should sit on the baseline, got {}",
      b[1]
    );
    assert!(b[3] > 0.0);
  }

  #[test]
  fn top_alignment_hangs_the_text_below_zero() {
    if !have_fonts() {
      return;
    }
    assert!(bbox(&render("H", 10.0, "left", "top"))[3] <= 0.001);
  }

  #[test]
  fn bottom_alignment_lifts_the_text_above_zero() {
    if !have_fonts() {
      return;
    }
    assert!(bbox(&render("Hy", 10.0, "left", "bottom"))[1] >= -0.001);
  }

  #[test]
  fn left_alignment_starts_at_the_origin() {
    if !have_fonts() {
      return;
    }
    assert!(bbox(&render("H", 10.0, "left", "baseline"))[0] >= -0.001);
  }

  #[test]
  fn center_alignment_straddles_the_origin() {
    if !have_fonts() {
      return;
    }
    let b = bbox(&render("HHHH", 10.0, "center", "baseline"));
    assert!(b[0] < 0.0 && b[2] > 0.0);
    assert!((b[0] + b[2]).abs() < 0.5, "not centered: {b:?}");
  }

  #[test]
  fn right_alignment_ends_at_the_origin() {
    if !have_fonts() {
      return;
    }
    assert!(bbox(&render("H", 10.0, "right", "baseline"))[2] <= 0.001);
  }

  #[test]
  fn a_longer_string_is_wider() {
    if !have_fonts() {
      return;
    }
    let one = bbox(&render("H", 10.0, "left", "baseline"));
    let many = bbox(&render("HHH", 10.0, "left", "baseline"));
    assert!(many[2] > one[2] * 2.0);
  }

  #[test]
  fn a_space_advances_the_pen_without_drawing() {
    if !have_fonts() {
      return;
    }
    let tight = bbox(&render("HH", 10.0, "left", "baseline"));
    let spaced = bbox(&render("H H", 10.0, "left", "baseline"));
    assert!(spaced[2] > tight[2]);
  }

  #[test]
  fn an_unknown_alignment_is_rejected() {
    let err = text_to_polygons("H", 10.0, "sans-serif", "middle", "baseline")
      .unwrap_err();
    assert!(err.contains("halign"), "{err}");
  }

  #[test]
  fn an_unknown_family_falls_back_instead_of_failing() {
    if !have_fonts() {
      return;
    }
    let contours =
      text_to_polygons("H", 10.0, "NoSuchFont-9000", "left", "baseline")
        .unwrap();
    assert!(!contours.is_empty());
  }

  #[test]
  fn a_style_suffix_is_parsed_off_the_family() {
    if !have_fonts() {
      return;
    }
    let contours =
      text_to_polygons("H", 10.0, "sans-serif:style=Bold", "left", "baseline")
        .unwrap();
    assert!(!contours.is_empty());
  }
}
