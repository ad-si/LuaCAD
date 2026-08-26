//! A minimal PDF writer: one RGB image, stroked vector marks on top of it,
//! and Helvetica text. That is exactly what the annotated screenshot export
//! needs, and it keeps a full PDF library out of the dependency tree.
//!
//! All coordinates are in PDF user space: 1/72 inch, origin in the lower left
//! corner of the page.

use std::io::Write;
use std::path::Path;

/// An 8-bit RGB image, interleaved, top row first.
pub struct Image {
  pub width: usize,
  pub height: usize,
  pub rgb: Vec<u8>,
}

/// A stroked mark drawn over the page.
pub enum Mark {
  /// An open path through the given points
  Polyline {
    points: Vec<(f32, f32)>,
    color: [u8; 3],
    width: f32,
  },
  /// An axis-aligned rectangle, given by its lower left corner
  Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [u8; 3],
    stroke: f32,
  },
  Ellipse {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    color: [u8; 3],
    stroke: f32,
  },
}

/// A single line of text, positioned at the left end of its baseline.
pub struct Text {
  pub x: f32,
  pub y: f32,
  pub size: f32,
  pub color: [u8; 3],
  pub text: String,
}

#[derive(Default)]
pub struct Page {
  /// Where the document's image goes on this page, as
  /// (lower left x, lower left y, width, height)
  pub image: Option<(f32, f32, f32, f32)>,
  pub marks: Vec<Mark>,
  pub text: Vec<Text>,
}

pub struct Document {
  pub page_width: f32,
  pub page_height: f32,
  /// The one image the pages can place. A page that does not place it simply
  /// leaves `Page::image` empty.
  pub image: Option<Image>,
  pub pages: Vec<Page>,
  /// What ends up in the PDF's `/Producer` field
  pub producer: String,
}

/// Advance widths of Helvetica for printable ASCII, in 1/1000 em. Used for
/// line breaking, so that a wrapped paragraph fills the page width instead of
/// being guessed at with an average character width.
#[rustfmt::skip]
const HELVETICA_WIDTHS: [u16; 95] = [
  278, 278, 355, 556, 556, 889, 667, 191, 333, 333, // ' ' .. ')'
  389, 584, 278, 333, 278, 278, 556, 556, 556, 556, // '*' .. '3'
  556, 556, 556, 556, 556, 556, 278, 278, 584, 584, // '4' .. '='
  584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, // '>' .. 'G'
  722, 278, 500, 667, 556, 833, 722, 778, 667, 778, // 'H' .. 'Q'
  722, 667, 611, 722, 667, 944, 667, 667, 611, 278, // 'R' .. '['
  278, 278, 469, 556, 333, 556, 556, 500, 556, 556, // '\\' .. 'e'
  278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // 'f' .. 'o'
  556, 556, 333, 500, 278, 556, 500, 722, 500, 500, // 'p' .. 'y'
  500, 334, 260, 334, 584,                          // 'z' .. '~'
];

/// Width of a character set in Helvetica at `size`.
fn char_width(c: char, size: f32) -> f32 {
  let per_mille = if (' '..='~').contains(&c) {
    HELVETICA_WIDTHS[c as usize - 0x20]
  } else {
    // Close enough for the accented letters WinAnsi covers
    556
  };
  per_mille as f32 / 1000.0 * size
}

/// Width of a string set in Helvetica at `size`.
pub fn text_width(text: &str, size: f32) -> f32 {
  text.chars().map(|c| char_width(c, size)).sum()
}

/// Break `text` into lines no wider than `max_width`, at spaces where
/// possible and inside a word only when the word alone is too long. Existing
/// line breaks are kept.
pub fn wrap(text: &str, size: f32, max_width: f32) -> Vec<String> {
  let mut lines = Vec::new();
  for paragraph in text.replace('\t', "    ").lines() {
    let mut line = String::new();
    for word in paragraph.split(' ') {
      let candidate = if line.is_empty() {
        word.to_string()
      } else {
        format!("{line} {word}")
      };
      if text_width(&candidate, size) <= max_width {
        line = candidate;
        continue;
      }
      if !line.is_empty() {
        lines.push(std::mem::take(&mut line));
      }
      // A single word wider than the line has to be split mid-word
      let mut rest = word;
      while text_width(rest, size) > max_width {
        let mut split = 0;
        let mut width = 0.0;
        for (i, c) in rest.char_indices() {
          let next = width + char_width(c, size);
          if next > max_width && i > 0 {
            break;
          }
          width = next;
          split = i + c.len_utf8();
        }
        lines.push(rest[..split].to_string());
        rest = &rest[split..];
      }
      line = rest.to_string();
    }
    lines.push(line);
  }
  lines
}

/// Write `document` to `path`.
pub fn write(document: &Document, path: &Path) -> std::io::Result<()> {
  std::fs::write(path, to_bytes(document))
}

/// Serialize `document` into the bytes of a PDF file.
pub fn to_bytes(document: &Document) -> Vec<u8> {
  // Fixed object numbers; the pages follow them, two objects each (the page
  // and its content stream).
  const PAGES: usize = 2;
  const FONT: usize = 3;
  const INFO: usize = 4;
  const IMAGE: usize = 5;
  let first_page = if document.image.is_some() { 6 } else { 5 };
  let page_ids: Vec<usize> = (0..document.pages.len())
    .map(|i| first_page + 2 * i)
    .collect();

  let mut objects: Vec<Vec<u8>> = Vec::new();

  objects.push(format!("<< /Type /Catalog /Pages {PAGES} 0 R >>").into_bytes());

  let kids: Vec<String> =
    page_ids.iter().map(|id| format!("{id} 0 R")).collect();
  objects.push(
    format!(
      "<< /Type /Pages /Kids [{}] /Count {} >>",
      kids.join(" "),
      page_ids.len()
    )
    .into_bytes(),
  );

  objects.push(
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
      /Encoding /WinAnsiEncoding >>"
      .to_vec(),
  );

  let mut info = b"<< /Producer ".to_vec();
  info.extend_from_slice(&pdf_string(&document.producer));
  info.extend_from_slice(b" >>");
  objects.push(info);

  if let Some(image) = &document.image {
    let data = deflate(&image.rgb);
    let mut object = format!(
      "<< /Type /XObject /Subtype /Image /Width {} /Height {} \
       /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode \
       /Length {} >>\nstream\n",
      image.width,
      image.height,
      data.len()
    )
    .into_bytes();
    object.extend_from_slice(&data);
    object.extend_from_slice(b"\nendstream");
    objects.push(object);
  }

  for (index, page) in document.pages.iter().enumerate() {
    let contents_id = page_ids[index] + 1;
    let image_resource = if document.image.is_some() {
      format!("/XObject << /Im0 {IMAGE} 0 R >> ")
    } else {
      String::new()
    };
    objects.push(
      format!(
        "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [0 0 {} {}] \
         /Resources << {image_resource}/Font << /F1 {FONT} 0 R >> >> \
         /Contents {contents_id} 0 R >>",
        number(document.page_width),
        number(document.page_height),
      )
      .into_bytes(),
    );

    let data = deflate(&page_content(document, page));
    let mut object = format!(
      "<< /Length {} /Filter /FlateDecode >>\nstream\n",
      data.len()
    )
    .into_bytes();
    object.extend_from_slice(&data);
    object.extend_from_slice(b"\nendstream");
    objects.push(object);
  }

  let mut out = Vec::new();
  // The binary comment marks the file as containing binary data, so that
  // tools transferring it do not mangle line endings.
  out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
  let mut offsets = Vec::with_capacity(objects.len());
  for (index, body) in objects.iter().enumerate() {
    offsets.push(out.len());
    out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendobj\n");
  }

  let xref_offset = out.len();
  out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
  out.extend_from_slice(b"0000000000 65535 f \n");
  for offset in &offsets {
    out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
  }
  out.extend_from_slice(
    format!(
      "trailer\n<< /Size {} /Root 1 0 R /Info {INFO} 0 R >>\n\
       startxref\n{xref_offset}\n%%EOF\n",
      objects.len() + 1
    )
    .as_bytes(),
  );
  out
}

/// The content stream of one page.
fn page_content(document: &Document, page: &Page) -> Vec<u8> {
  let mut out = Vec::new();
  if document.image.is_some()
    && let Some((x, y, width, height)) = page.image
  {
    // The image XObject is drawn into the unit square, so the placement is a
    // scale and a translation of the current transformation matrix.
    out.extend_from_slice(
      format!(
        "q\n{} 0 0 {} {} {} cm\n/Im0 Do\nQ\n",
        number(width),
        number(height),
        number(x),
        number(y)
      )
      .as_bytes(),
    );
  }

  if !page.marks.is_empty() {
    // Round caps and joins, so freehand strokes do not look chipped
    out.extend_from_slice(b"q\n1 J\n1 j\n");
    for mark in &page.marks {
      out.extend_from_slice(&mark_content(mark));
    }
    out.extend_from_slice(b"Q\n");
  }

  for text in &page.text {
    let [r, g, b] = text.color;
    out.extend_from_slice(
      format!(
        "BT\n/F1 {} Tf\n{} {} {} rg\n{} {} Td\n",
        number(text.size),
        number(r as f32 / 255.0),
        number(g as f32 / 255.0),
        number(b as f32 / 255.0),
        number(text.x),
        number(text.y),
      )
      .as_bytes(),
    );
    out.extend_from_slice(&pdf_string(&text.text));
    out.extend_from_slice(b" Tj\nET\n");
  }

  out
}

/// The path operators of a single mark, including its color and width.
fn mark_content(mark: &Mark) -> Vec<u8> {
  let (color, width) = match mark {
    Mark::Polyline { color, width, .. } => (color, width),
    Mark::Rect { color, stroke, .. } | Mark::Ellipse { color, stroke, .. } => {
      (color, stroke)
    }
  };
  let [r, g, b] = color;
  let mut out = format!(
    "{} {} {} RG\n{} w\n",
    number(*r as f32 / 255.0),
    number(*g as f32 / 255.0),
    number(*b as f32 / 255.0),
    number(*width),
  );

  match mark {
    Mark::Polyline { points, .. } => {
      for (index, (x, y)) in points.iter().enumerate() {
        let op = if index == 0 { "m" } else { "l" };
        out.push_str(&format!("{} {} {op}\n", number(*x), number(*y)));
      }
    }
    Mark::Rect {
      x,
      y,
      width,
      height,
      ..
    } => {
      out.push_str(&format!(
        "{} {} {} {} re\n",
        number(*x),
        number(*y),
        number(*width),
        number(*height)
      ));
    }
    Mark::Ellipse { cx, cy, rx, ry, .. } => {
      // Four cubic segments approximate the ellipse; the magic constant is
      // the usual circle-to-Bézier factor.
      const K: f32 = 0.552_284_8;
      let (kx, ky) = (rx * K, ry * K);
      out.push_str(&format!("{} {} m\n", number(cx + rx), number(*cy)));
      for (c1, c2, end) in [
        ((cx + rx, cy + ky), (cx + kx, cy + ry), (*cx, cy + ry)),
        ((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, *cy)),
        ((cx - rx, cy - ky), (cx - kx, cy - ry), (*cx, cy - ry)),
        ((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, *cy)),
      ] {
        out.push_str(&format!(
          "{} {} {} {} {} {} c\n",
          number(c1.0),
          number(c1.1),
          number(c2.0),
          number(c2.1),
          number(end.0),
          number(end.1)
        ));
      }
    }
  }
  out.push_str("S\n");
  out.into_bytes()
}

/// Format a number the way a content stream wants it: no exponent, no
/// trailing zeros.
fn number(value: f32) -> String {
  let mut text = format!("{value:.3}");
  if text.contains('.') {
    while text.ends_with('0') {
      text.pop();
    }
    if text.ends_with('.') {
      text.pop();
    }
  }
  if text == "-0" { "0".to_string() } else { text }
}

/// A PDF literal string: WinAnsi bytes in parentheses, with the three
/// characters that would end it escaped.
fn pdf_string(text: &str) -> Vec<u8> {
  let mut out = vec![b'('];
  for c in text.chars() {
    let byte = win_ansi(c);
    if matches!(byte, b'(' | b')' | b'\\') {
      out.push(b'\\');
    }
    out.push(byte);
  }
  out.push(b')');
  out
}

/// Map a character to its WinAnsiEncoding byte, replacing anything the
/// encoding cannot represent with a question mark.
fn win_ansi(c: char) -> u8 {
  match c {
    // WinAnsi agrees with Latin-1 everywhere except 0x80..=0x9F
    ' '..='~' | '\u{a0}'..='\u{ff}' => c as u8,
    '€' => 0x80,
    '…' => 0x85,
    '‘' => 0x91,
    '’' => 0x92,
    '“' => 0x93,
    '”' => 0x94,
    '•' => 0x95,
    '–' => 0x96,
    '—' => 0x97,
    _ => b'?',
  }
}

/// Zlib-compress a stream, as `/FlateDecode` expects it.
fn deflate(data: &[u8]) -> Vec<u8> {
  let mut encoder =
    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
  encoder.write_all(data).expect("write to a Vec cannot fail");
  encoder.finish().expect("write to a Vec cannot fail")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn document() -> Document {
    Document {
      page_width: 595.276,
      page_height: 841.89,
      image: Some(Image {
        width: 2,
        height: 1,
        rgb: vec![255, 0, 0, 0, 0, 255],
      }),
      pages: vec![Page {
        image: Some((40.0, 600.0, 200.0, 100.0)),
        marks: vec![Mark::Polyline {
          points: vec![(40.0, 600.0), (240.0, 700.0)],
          color: [255, 0, 0],
          width: 2.0,
        }],
        text: vec![Text {
          x: 40.0,
          y: 560.0,
          size: 11.0,
          color: [0, 0, 0],
          text: "A note (with parens)".to_string(),
        }],
      }],
      producer: "LuaCAD Studio".to_string(),
    }
  }

  #[test]
  fn a_document_is_a_well_formed_pdf_file() {
    let bytes = to_bytes(&document());
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));

    let text = String::from_utf8_lossy(&bytes);
    // 4 fixed objects + the image + the page and its content stream
    assert!(text.contains("7 0 obj"), "expected seven objects");
    assert!(!text.contains("8 0 obj"));
    assert!(text.contains("/Count 1"));
    assert!(text.contains("/Kids [6 0 R]"));
  }

  /// Byte offset of `needle` in `haystack`. The compressed streams make the
  /// file invalid UTF-8, so the checks below work on bytes.
  fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
      .windows(needle.len())
      .position(|window| window == needle)
  }

  /// Readers seek to `startxref` and then index into the table, so every
  /// offset in it has to point at the `N 0 obj` line of that object.
  #[test]
  fn the_xref_table_points_at_the_objects() {
    let bytes = to_bytes(&document());
    let tail = find(&bytes, b"startxref\n").expect("startxref") + 10;
    let digits: Vec<u8> = bytes[tail..]
      .iter()
      .copied()
      .take_while(u8::is_ascii_digit)
      .collect();
    let xref_offset: usize =
      String::from_utf8(digits).unwrap().parse().unwrap();
    assert!(bytes[xref_offset..].starts_with(b"xref\n0 8\n"));

    // One 20-byte entry per object, after the header and the free entry
    let entries = xref_offset + b"xref\n0 8\n".len() + 20;
    for index in 0..7 {
      let entry = &bytes[entries + index * 20..][..10];
      let offset: usize =
        String::from_utf8(entry.to_vec()).unwrap().parse().unwrap();
      assert!(
        bytes[offset..].starts_with(format!("{} 0 obj", index + 1).as_bytes()),
        "object {} is not at its xref offset",
        index + 1
      );
    }
  }

  #[test]
  fn parentheses_in_text_are_escaped() {
    assert_eq!(pdf_string("a (b) \\c"), b"(a \\(b\\) \\\\c)".to_vec());
  }

  #[test]
  fn text_outside_win_ansi_becomes_a_question_mark() {
    assert_eq!(pdf_string("a—b☃"), b"(a\x97b?)".to_vec());
    assert_eq!(pdf_string("Grün"), b"(Gr\xfcn)".to_vec());
  }

  #[test]
  fn wrapping_fills_the_line_without_overflowing_it() {
    let text = "The quick brown fox jumps over the lazy dog again and again";
    let lines = wrap(text, 11.0, 120.0);
    assert!(lines.len() > 1);
    for line in &lines {
      assert!(text_width(line, 11.0) <= 120.0, "line {line:?} is too wide");
    }
    assert_eq!(lines.join(" "), text);
  }

  #[test]
  fn wrapping_keeps_explicit_line_breaks() {
    assert_eq!(wrap("one\ntwo", 11.0, 500.0), vec!["one", "two"]);
  }

  /// A word longer than the line still has to end up on the page.
  #[test]
  fn a_single_long_word_is_split() {
    let lines = wrap(&"x".repeat(200), 11.0, 100.0);
    assert!(lines.len() > 1);
    for line in &lines {
      assert!(text_width(line, 11.0) <= 100.0);
    }
    assert_eq!(lines.concat().len(), 200);
  }

  #[test]
  fn numbers_are_written_without_trailing_zeros() {
    assert_eq!(number(1.0), "1");
    assert_eq!(number(1.5), "1.5");
    assert_eq!(number(0.0), "0");
  }
}
