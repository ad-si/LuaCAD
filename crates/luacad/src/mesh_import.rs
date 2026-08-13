//! Read a mesh file into vertices and triangles for the Manifold backend.
//!
//! Covers every format LuaCAD can export — STL, OBJ, PLY, OFF, 3MF and AMF —
//! so `import()` round-trips its own output. Only geometry is read: colors,
//! materials, normals and texture coordinates are dropped, because a Manifold
//! carries none of them.

/// A triangle soup, indexed the way `manifold_meshgl` wants it.
#[derive(Debug)]
pub struct ImportedMesh {
  pub vertices: Vec<[f32; 3]>,
  pub triangles: Vec<[u32; 3]>,
}

impl ImportedMesh {
  fn is_empty(&self) -> bool {
    self.vertices.is_empty() || self.triangles.is_empty()
  }
}

/// The mesh formats `import()` understands.
pub const IMPORT_FORMATS: &[&str] = &["stl", "obj", "ply", "off", "3mf", "amf"];

/// Whether `file`'s extension is one [`import_mesh`] can read. Says nothing
/// about the contents — only that a parser exists for it.
pub fn is_mesh_file(file: &str) -> bool {
  IMPORT_FORMATS.contains(&extension(file).as_str())
}

fn extension(file: &str) -> String {
  std::path::Path::new(file)
    .extension()
    .and_then(|e| e.to_str())
    .map(|e| e.to_ascii_lowercase())
    .unwrap_or_default()
}

/// Read `file`, choosing the parser by extension.
pub fn import_mesh(file: &str) -> Result<ImportedMesh, String> {
  let ext = extension(file);

  let mesh = match ext.as_str() {
    "stl" => read_stl(file),
    "obj" => read_obj(file),
    "ply" => read_ply(file),
    "off" => read_off(file),
    "3mf" => read_3mf(file),
    "amf" => read_amf(file),
    "" => Err(format!("import(): {file} has no extension")),
    other => Err(format!(
      "import(): cannot read .{other} files.\n\
       Supported: {}",
      IMPORT_FORMATS.join(", ")
    )),
  }?;

  if mesh.is_empty() {
    return Err(format!("import(): {file} contains no triangles"));
  }
  Ok(mesh)
}

fn read(file: &str) -> Result<Vec<u8>, String> {
  std::fs::read(file).map_err(|e| format!("import(): cannot read {file}: {e}"))
}

fn read_text(file: &str) -> Result<String, String> {
  std::fs::read_to_string(file)
    .map_err(|e| format!("import(): cannot read {file}: {e}"))
}

/// Merge duplicate positions so a triangle soup becomes an indexed mesh.
///
/// STL and AMF store each triangle's corners independently, and Manifold
/// rejects the result as non-manifold unless coincident corners collapse onto
/// one vertex. Positions are hashed bit-for-bit: any two corners that came
/// from the same written value match exactly, which is what these formats
/// guarantee for shared edges.
struct VertexMerger {
  vertices: Vec<[f32; 3]>,
  seen: std::collections::HashMap<[u32; 3], u32>,
}

impl VertexMerger {
  fn new() -> Self {
    Self {
      vertices: Vec::new(),
      seen: std::collections::HashMap::new(),
    }
  }

  fn insert(&mut self, v: [f32; 3]) -> u32 {
    // -0.0 and 0.0 are equal but have different bits; normalize so they
    // cannot split a shared edge into two vertices.
    let key = [
      (v[0] + 0.0).to_bits(),
      (v[1] + 0.0).to_bits(),
      (v[2] + 0.0).to_bits(),
    ];
    *self.seen.entry(key).or_insert_with(|| {
      self.vertices.push(v);
      (self.vertices.len() - 1) as u32
    })
  }
}

// ---------------------------------------------------------------------------
// STL
// ---------------------------------------------------------------------------

/// Read binary or ASCII STL.
///
/// The format has no reliable marker: an ASCII file starts with "solid", but
/// so do binary files written by some tools. The length check is what actually
/// decides, because a binary file's size is fixed by its triangle count.
fn read_stl(file: &str) -> Result<ImportedMesh, String> {
  let data = read(file)?;

  if data.len() >= 84 {
    let count =
      u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;
    if data.len() == 84 + count * 50 {
      return Ok(read_stl_binary(&data, count));
    }
  }

  let text = String::from_utf8_lossy(&data);
  if text.trim_start().starts_with("solid") {
    return read_stl_ascii(&text);
  }

  Err(format!(
    "import(): {file} is not a valid STL file (neither binary nor ASCII)"
  ))
}

fn read_stl_binary(data: &[u8], count: usize) -> ImportedMesh {
  let mut merger = VertexMerger::new();
  let mut triangles = Vec::with_capacity(count);

  for i in 0..count {
    // 80-byte header, 4-byte count, then 50 bytes per triangle: a normal and
    // three corners as 3×f32 each, plus a 2-byte attribute word.
    let base = 84 + i * 50 + 12;
    let corner = |c: usize| {
      let o = base + c * 12;
      let f = |k: usize| {
        f32::from_le_bytes([
          data[o + k * 4],
          data[o + k * 4 + 1],
          data[o + k * 4 + 2],
          data[o + k * 4 + 3],
        ])
      };
      [f(0), f(1), f(2)]
    };
    let tri = [
      merger.insert(corner(0)),
      merger.insert(corner(1)),
      merger.insert(corner(2)),
    ];
    push_triangle(&mut triangles, tri);
  }

  ImportedMesh {
    vertices: merger.vertices,
    triangles,
  }
}

fn read_stl_ascii(text: &str) -> Result<ImportedMesh, String> {
  let mut merger = VertexMerger::new();
  let mut triangles = Vec::new();
  let mut corners: Vec<u32> = Vec::new();

  for line in text.lines() {
    let mut parts = line.split_whitespace();
    match parts.next() {
      Some("vertex") => {
        let v = parse_vec3(&mut parts)
          .ok_or_else(|| format!("import(): malformed STL vertex: {line}"))?;
        corners.push(merger.insert(v));
      }
      Some("endloop") => {
        // Fan-triangulate: the spec says three corners, but files with more
        // do occur and a fan is the only sane reading of them.
        for i in 1..corners.len().saturating_sub(1) {
          push_triangle(
            &mut triangles,
            [corners[0], corners[i], corners[i + 1]],
          );
        }
        corners.clear();
      }
      _ => {}
    }
  }

  Ok(ImportedMesh {
    vertices: merger.vertices,
    triangles,
  })
}

// ---------------------------------------------------------------------------
// OBJ
// ---------------------------------------------------------------------------

/// Read the geometry of a Wavefront OBJ: `v` positions and `f` faces.
///
/// Faces may be any polygon and are fan-triangulated. Only the position index
/// of each `v/vt/vn` triplet is used; negative indices count back from the
/// last vertex read, as the format specifies.
fn read_obj(file: &str) -> Result<ImportedMesh, String> {
  let text = read_text(file)?;
  let mut vertices: Vec<[f32; 3]> = Vec::new();
  let mut triangles = Vec::new();

  for line in text.lines() {
    let mut parts = line.split_whitespace();
    match parts.next() {
      Some("v") => {
        let v = parse_vec3(&mut parts)
          .ok_or_else(|| format!("import(): malformed OBJ vertex: {line}"))?;
        vertices.push(v);
      }
      Some("f") => {
        let mut corners: Vec<u32> = Vec::new();
        for token in parts {
          let index_str = token.split('/').next().unwrap_or_default();
          let index: i64 = index_str.parse().map_err(|_| {
            format!("import(): malformed OBJ face index: {token}")
          })?;
          let resolved = if index > 0 {
            index - 1
          } else if index < 0 {
            vertices.len() as i64 + index
          } else {
            return Err("import(): OBJ face index 0 is not valid".to_string());
          };
          if resolved < 0 || resolved >= vertices.len() as i64 {
            return Err(format!(
              "import(): OBJ face index {index} is out of range"
            ));
          }
          corners.push(resolved as u32);
        }
        for i in 1..corners.len().saturating_sub(1) {
          push_triangle(
            &mut triangles,
            [corners[0], corners[i], corners[i + 1]],
          );
        }
      }
      _ => {}
    }
  }

  Ok(ImportedMesh {
    vertices,
    triangles,
  })
}

// ---------------------------------------------------------------------------
// OFF
// ---------------------------------------------------------------------------

/// Read an OFF file: a counts line, then positions, then faces prefixed by
/// their corner count. Comments (`#`) and blank lines are skipped.
fn read_off(file: &str) -> Result<ImportedMesh, String> {
  let text = read_text(file)?;
  let mut tokens = text
    .lines()
    .map(|l| l.split('#').next().unwrap_or_default())
    .flat_map(|l| l.split_whitespace());

  let header = tokens
    .next()
    .ok_or_else(|| format!("import(): {file} is empty"))?;
  if header != "OFF" {
    return Err(format!("import(): {file} does not start with OFF"));
  }

  let mut next_usize = |what: &str| -> Result<usize, String> {
    tokens
      .next()
      .and_then(|t| t.parse().ok())
      .ok_or_else(|| format!("import(): {file} has no {what} count"))
  };
  let vertex_count = next_usize("vertex")?;
  let face_count = next_usize("face")?;
  let _edge_count = next_usize("edge")?;

  let mut vertices = Vec::with_capacity(vertex_count);
  for _ in 0..vertex_count {
    let v = parse_vec3(&mut tokens)
      .ok_or_else(|| format!("import(): {file} has a malformed vertex"))?;
    vertices.push(v);
  }

  let mut triangles = Vec::new();
  for _ in 0..face_count {
    let corner_count: usize = tokens
      .next()
      .and_then(|t| t.parse().ok())
      .ok_or_else(|| format!("import(): {file} has a malformed face"))?;
    let mut corners = Vec::with_capacity(corner_count);
    for _ in 0..corner_count {
      let index: usize = tokens
        .next()
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| format!("import(): {file} has a malformed face"))?;
      if index >= vertices.len() {
        return Err(format!(
          "import(): OFF face index {index} is out of range"
        ));
      }
      corners.push(index as u32);
    }
    for i in 1..corners.len().saturating_sub(1) {
      push_triangle(&mut triangles, [corners[0], corners[i], corners[i + 1]]);
    }
  }

  Ok(ImportedMesh {
    vertices,
    triangles,
  })
}

// ---------------------------------------------------------------------------
// PLY
// ---------------------------------------------------------------------------

/// Scalar widths PLY uses, in bytes, for skipping properties we don't read.
fn ply_type_size(name: &str) -> Option<usize> {
  match name {
    "char" | "uchar" | "int8" | "uint8" => Some(1),
    "short" | "ushort" | "int16" | "uint16" => Some(2),
    "int" | "uint" | "int32" | "uint32" | "float" | "float32" => Some(4),
    "double" | "float64" => Some(8),
    _ => None,
  }
}

/// One property of a PLY element: either a scalar or a variable-length list.
enum PlyProperty {
  Scalar { name: String, ty: String },
  List { count_ty: String, item_ty: String },
}

/// Read an ASCII or binary-little-endian PLY.
///
/// Big-endian files are rejected rather than mis-read; nothing LuaCAD writes
/// produces them, and guessing would corrupt the geometry silently.
fn read_ply(file: &str) -> Result<ImportedMesh, String> {
  let data = read(file)?;

  // The header is ASCII regardless of the body's encoding.
  let header_end = find_subslice(&data, b"end_header")
    .ok_or_else(|| format!("import(): {file} has no PLY header"))?;
  let header = String::from_utf8_lossy(&data[..header_end]).to_string();
  let after = data[header_end..]
    .iter()
    .position(|&b| b == b'\n')
    .map(|i| header_end + i + 1)
    .ok_or_else(|| format!("import(): {file} has a truncated PLY header"))?;

  let mut format = String::new();
  let mut elements: Vec<(String, usize, Vec<PlyProperty>)> = Vec::new();

  for line in header.lines() {
    let mut parts = line.split_whitespace();
    match parts.next() {
      Some("format") => {
        format = parts.next().unwrap_or_default().to_string();
      }
      Some("element") => {
        let name = parts.next().unwrap_or_default().to_string();
        let count: usize = parts
          .next()
          .and_then(|t| t.parse().ok())
          .ok_or_else(|| format!("import(): malformed PLY element: {line}"))?;
        elements.push((name, count, Vec::new()));
      }
      Some("property") => {
        let Some((_, _, props)) = elements.last_mut() else {
          continue;
        };
        match parts.next() {
          Some("list") => {
            let count_ty = parts.next().unwrap_or_default().to_string();
            let item_ty = parts.next().unwrap_or_default().to_string();
            props.push(PlyProperty::List { count_ty, item_ty });
          }
          Some(ty) => props.push(PlyProperty::Scalar {
            name: parts.next().unwrap_or_default().to_string(),
            ty: ty.to_string(),
          }),
          None => {}
        }
      }
      _ => {}
    }
  }

  match format.as_str() {
    "ascii" => {
      read_ply_ascii(&String::from_utf8_lossy(&data[after..]), &elements)
    }
    "binary_little_endian" => read_ply_binary(&data[after..], &elements),
    "binary_big_endian" => Err(format!(
      "import(): {file} is big-endian PLY, which is not supported"
    )),
    other => Err(format!("import(): unknown PLY format '{other}'")),
  }
}

fn read_ply_ascii(
  body: &str,
  elements: &[(String, usize, Vec<PlyProperty>)],
) -> Result<ImportedMesh, String> {
  let mut tokens = body.split_whitespace();
  let mut vertices = Vec::new();
  let mut triangles = Vec::new();

  for (name, count, props) in elements {
    for _ in 0..*count {
      let mut position = [0.0f32; 3];
      let mut corners: Vec<u32> = Vec::new();

      for prop in props {
        match prop {
          PlyProperty::Scalar {
            name: prop_name, ..
          } => {
            let value: f32 = tokens
              .next()
              .and_then(|t| t.parse().ok())
              .ok_or_else(|| "import(): truncated PLY body".to_string())?;
            match prop_name.as_str() {
              "x" => position[0] = value,
              "y" => position[1] = value,
              "z" => position[2] = value,
              _ => {}
            }
          }
          PlyProperty::List { .. } => {
            let n: usize = tokens
              .next()
              .and_then(|t| t.parse().ok())
              .ok_or_else(|| "import(): truncated PLY body".to_string())?;
            for _ in 0..n {
              let index: u32 = tokens
                .next()
                .and_then(|t| t.parse().ok())
                .ok_or_else(|| "import(): truncated PLY body".to_string())?;
              corners.push(index);
            }
          }
        }
      }

      match name.as_str() {
        "vertex" => vertices.push(position),
        "face" => fan(&mut triangles, &corners),
        _ => {}
      }
    }
  }

  validate_indices(&vertices, &triangles)?;
  Ok(ImportedMesh {
    vertices,
    triangles,
  })
}

fn read_ply_binary(
  body: &[u8],
  elements: &[(String, usize, Vec<PlyProperty>)],
) -> Result<ImportedMesh, String> {
  let mut cursor = 0usize;
  let mut vertices = Vec::new();
  let mut triangles = Vec::new();

  let truncated = || "import(): truncated PLY body".to_string();

  for (name, count, props) in elements {
    for _ in 0..*count {
      let mut position = [0.0f32; 3];
      let mut corners: Vec<u32> = Vec::new();

      for prop in props {
        match prop {
          PlyProperty::Scalar {
            name: prop_name,
            ty,
          } => {
            let size = ply_type_size(ty)
              .ok_or_else(|| format!("import(): unknown PLY type '{ty}'"))?;
            let bytes =
              body.get(cursor..cursor + size).ok_or_else(truncated)?;
            cursor += size;
            match prop_name.as_str() {
              "x" => position[0] = ply_scalar(bytes, ty),
              "y" => position[1] = ply_scalar(bytes, ty),
              "z" => position[2] = ply_scalar(bytes, ty),
              _ => {}
            }
          }
          PlyProperty::List { count_ty, item_ty } => {
            let count_size = ply_type_size(count_ty).ok_or_else(|| {
              format!("import(): unknown PLY type '{count_ty}'")
            })?;
            let item_size = ply_type_size(item_ty).ok_or_else(|| {
              format!("import(): unknown PLY type '{item_ty}'")
            })?;
            let bytes = body
              .get(cursor..cursor + count_size)
              .ok_or_else(truncated)?;
            cursor += count_size;
            let n = ply_scalar(bytes, count_ty) as usize;
            for _ in 0..n {
              let bytes =
                body.get(cursor..cursor + item_size).ok_or_else(truncated)?;
              cursor += item_size;
              corners.push(ply_scalar(bytes, item_ty) as u32);
            }
          }
        }
      }

      match name.as_str() {
        "vertex" => vertices.push(position),
        "face" => fan(&mut triangles, &corners),
        _ => {}
      }
    }
  }

  validate_indices(&vertices, &triangles)?;
  Ok(ImportedMesh {
    vertices,
    triangles,
  })
}

/// Decode one little-endian PLY scalar as f32. Integer types are read as
/// integers first so index values survive exactly.
fn ply_scalar(bytes: &[u8], ty: &str) -> f32 {
  let u = |n: usize| -> u64 {
    let mut buf = [0u8; 8];
    buf[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(buf)
  };
  match ty {
    "char" | "int8" => bytes[0] as i8 as f32,
    "uchar" | "uint8" => bytes[0] as f32,
    "short" | "int16" => (u(2) as u16) as i16 as f32,
    "ushort" | "uint16" => (u(2) as u16) as f32,
    "int" | "int32" => (u(4) as u32) as i32 as f32,
    "uint" | "uint32" => (u(4) as u32) as f32,
    "float" | "float32" => f32::from_bits(u(4) as u32),
    "double" | "float64" => f64::from_bits(u(8)) as f32,
    _ => 0.0,
  }
}

// ---------------------------------------------------------------------------
// 3MF
// ---------------------------------------------------------------------------

/// Read a 3MF archive. Every object in every model is merged into one mesh,
/// because `import()` yields a single solid.
fn read_3mf(file: &str) -> Result<ImportedMesh, String> {
  let handle = std::fs::File::open(file)
    .map_err(|e| format!("import(): cannot read {file}: {e}"))?;
  let models = threemf::read(std::io::BufReader::new(handle))
    .map_err(|e| format!("import(): cannot parse {file}: {e}"))?;

  let mut vertices: Vec<[f32; 3]> = Vec::new();
  let mut triangles = Vec::new();

  for model in models {
    for object in model.resources.object {
      let Some(mesh) = object.mesh else { continue };
      // Each object indexes its own vertex list, so shift by what is already
      // there when concatenating.
      let base = vertices.len() as u32;
      for v in &mesh.vertices.vertex {
        vertices.push([v.x as f32, v.y as f32, v.z as f32]);
      }
      for t in &mesh.triangles.triangle {
        push_triangle(
          &mut triangles,
          [base + t.v1 as u32, base + t.v2 as u32, base + t.v3 as u32],
        );
      }
    }
  }

  validate_indices(&vertices, &triangles)?;
  Ok(ImportedMesh {
    vertices,
    triangles,
  })
}

// ---------------------------------------------------------------------------
// AMF
// ---------------------------------------------------------------------------

/// Read an uncompressed AMF file.
///
/// AMF may also be a zip archive holding the same XML. LuaCAD writes the plain
/// form, and a zipped one is reported rather than mis-parsed.
fn read_amf(file: &str) -> Result<ImportedMesh, String> {
  let data = read(file)?;
  if data.starts_with(b"PK") {
    return Err(format!(
      "import(): {file} is a compressed AMF archive, which is not supported.\n\
       Unzip it first, or export the model as 3MF instead."
    ));
  }

  let text = String::from_utf8_lossy(&data);
  let doc = roxmltree::Document::parse(&text)
    .map_err(|e| format!("import(): cannot parse {file}: {e}"))?;

  let mut vertices: Vec<[f32; 3]> = Vec::new();
  let mut triangles = Vec::new();

  let number = |node: roxmltree::Node, tag: &str| -> Option<f32> {
    node
      .children()
      .find(|c| c.has_tag_name(tag))
      .and_then(|c| c.text())
      .and_then(|t| t.trim().parse().ok())
  };

  for mesh in doc.descendants().filter(|n| n.has_tag_name("mesh")) {
    // Volumes index into the vertex list of their own mesh.
    let base = vertices.len() as u32;

    for vertex in mesh
      .children()
      .filter(|n| n.has_tag_name("vertices"))
      .flat_map(|n| n.children())
      .filter(|n| n.has_tag_name("vertex"))
    {
      let Some(coords) =
        vertex.children().find(|c| c.has_tag_name("coordinates"))
      else {
        continue;
      };
      vertices.push([
        number(coords, "x").unwrap_or(0.0),
        number(coords, "y").unwrap_or(0.0),
        number(coords, "z").unwrap_or(0.0),
      ]);
    }

    for triangle in mesh
      .children()
      .filter(|n| n.has_tag_name("volume"))
      .flat_map(|n| n.children())
      .filter(|n| n.has_tag_name("triangle"))
    {
      let corner =
        |tag: &str| -> Option<u32> { number(triangle, tag).map(|v| v as u32) };
      if let (Some(v1), Some(v2), Some(v3)) =
        (corner("v1"), corner("v2"), corner("v3"))
      {
        push_triangle(&mut triangles, [base + v1, base + v2, base + v3]);
      }
    }
  }

  validate_indices(&vertices, &triangles)?;
  Ok(ImportedMesh {
    vertices,
    triangles,
  })
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Append a triangle, dropping degenerate ones. A triangle that repeats a
/// vertex has no area and makes Manifold reject the whole mesh.
fn push_triangle(triangles: &mut Vec<[u32; 3]>, tri: [u32; 3]) {
  if tri[0] != tri[1] && tri[1] != tri[2] && tri[0] != tri[2] {
    triangles.push(tri);
  }
}

/// Fan-triangulate a polygon given as corner indices.
fn fan(triangles: &mut Vec<[u32; 3]>, corners: &[u32]) {
  for i in 1..corners.len().saturating_sub(1) {
    push_triangle(triangles, [corners[0], corners[i], corners[i + 1]]);
  }
}

fn validate_indices(
  vertices: &[[f32; 3]],
  triangles: &[[u32; 3]],
) -> Result<(), String> {
  let limit = vertices.len() as u32;
  if triangles.iter().flatten().any(|&i| i >= limit) {
    return Err(
      "import(): the file references a vertex that does not exist".to_string(),
    );
  }
  Ok(())
}

fn parse_vec3<'a>(
  parts: &mut impl Iterator<Item = &'a str>,
) -> Option<[f32; 3]> {
  let mut v = [0.0f32; 3];
  for slot in &mut v {
    *slot = parts.next()?.parse().ok()?;
  }
  Some(v)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
  haystack
    .windows(needle.len())
    .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;

  /// A unit tetrahedron, the smallest closed solid worth round-tripping.
  const TETRA_VERTS: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
  ];
  const TETRA_TRIS: [[u32; 3]; 4] =
    [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];

  fn write_temp(name: &str, contents: &[u8]) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
      "luacad_import_{}_{}_{name}",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = std::fs::File::create(&path).unwrap();
    file.write_all(contents).unwrap();
    path.to_str().unwrap().to_string()
  }

  fn assert_tetra(mesh: &ImportedMesh) {
    assert_eq!(mesh.vertices.len(), 4, "expected 4 merged vertices");
    assert_eq!(mesh.triangles.len(), 4);
  }

  #[test]
  fn ascii_stl_corners_are_merged_into_shared_vertices() {
    let mut stl = String::from("solid tetra\n");
    for tri in TETRA_TRIS {
      stl.push_str("facet normal 0 0 0\n  outer loop\n");
      for i in tri {
        let v = TETRA_VERTS[i as usize];
        stl.push_str(&format!("    vertex {} {} {}\n", v[0], v[1], v[2]));
      }
      stl.push_str("  endloop\nendfacet\n");
    }
    stl.push_str("endsolid tetra\n");

    let mesh = import_mesh(&write_temp("t.stl", stl.as_bytes())).unwrap();
    assert_tetra(&mesh);
  }

  #[test]
  fn binary_stl_is_detected_by_its_length() {
    let mut data = vec![0u8; 80];
    data.extend_from_slice(&(TETRA_TRIS.len() as u32).to_le_bytes());
    for tri in TETRA_TRIS {
      data.extend_from_slice(&[0u8; 12]); // normal
      for i in tri {
        for c in TETRA_VERTS[i as usize] {
          data.extend_from_slice(&c.to_le_bytes());
        }
      }
      data.extend_from_slice(&[0u8; 2]); // attribute byte count
    }

    let mesh = import_mesh(&write_temp("t.stl", &data)).unwrap();
    assert_tetra(&mesh);
  }

  #[test]
  fn obj_faces_are_one_based() {
    let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\n\
               f 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n";
    let mesh = import_mesh(&write_temp("t.obj", obj.as_bytes())).unwrap();
    assert_tetra(&mesh);
  }

  #[test]
  fn obj_negative_indices_count_back_from_the_last_vertex() {
    let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -1 -2\n";
    let mesh = import_mesh(&write_temp("t.obj", obj.as_bytes())).unwrap();
    assert_eq!(mesh.triangles, vec![[0, 2, 1]]);
  }

  #[test]
  fn obj_face_indices_ignore_texture_and_normal_parts() {
    let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1/1/1 2/2/2 3/3/3\n";
    let mesh = import_mesh(&write_temp("t.obj", obj.as_bytes())).unwrap();
    assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
  }

  #[test]
  fn obj_quads_are_fan_triangulated() {
    let obj = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";
    let mesh = import_mesh(&write_temp("t.obj", obj.as_bytes())).unwrap();
    assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 2, 3]]);
  }

  #[test]
  fn an_out_of_range_obj_index_is_an_error() {
    let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 9\n";
    let err = import_mesh(&write_temp("t.obj", obj.as_bytes())).unwrap_err();
    assert!(err.contains("out of range"), "{err}");
  }

  #[test]
  fn off_reads_counts_then_vertices_then_faces() {
    let off = "OFF\n4 4 0\n0 0 0\n1 0 0\n0 1 0\n0 0 1\n\
               3 0 2 1\n3 0 1 3\n3 0 3 2\n3 1 2 3\n";
    let mesh = import_mesh(&write_temp("t.off", off.as_bytes())).unwrap();
    assert_tetra(&mesh);
  }

  #[test]
  fn off_comments_are_skipped() {
    let off = "OFF\n# a comment\n3 1 0\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
    let mesh = import_mesh(&write_temp("t.off", off.as_bytes())).unwrap();
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.triangles.len(), 1);
  }

  #[test]
  fn ascii_ply_reads_vertices_and_faces() {
    let ply = "ply\nformat ascii 1.0\nelement vertex 4\n\
               property float x\nproperty float y\nproperty float z\n\
               element face 4\nproperty list uchar int vertex_indices\n\
               end_header\n\
               0 0 0\n1 0 0\n0 1 0\n0 0 1\n\
               3 0 2 1\n3 0 1 3\n3 0 3 2\n3 1 2 3\n";
    let mesh = import_mesh(&write_temp("t.ply", ply.as_bytes())).unwrap();
    assert_tetra(&mesh);
  }

  #[test]
  fn ascii_ply_skips_properties_it_does_not_need() {
    // Colors sit between the coordinates and the face list.
    let ply = "ply\nformat ascii 1.0\nelement vertex 3\n\
               property float x\nproperty float y\nproperty float z\n\
               property uchar red\nproperty uchar green\nproperty uchar blue\n\
               element face 1\nproperty list uchar int vertex_indices\n\
               end_header\n\
               0 0 0 255 0 0\n1 0 0 0 255 0\n0 1 0 0 0 255\n3 0 1 2\n";
    let mesh = import_mesh(&write_temp("t.ply", ply.as_bytes())).unwrap();
    assert_eq!(mesh.vertices[1], [1.0, 0.0, 0.0]);
    assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
  }

  #[test]
  fn binary_little_endian_ply_reads_the_same_geometry() {
    let mut data = Vec::new();
    data.extend_from_slice(
      b"ply\nformat binary_little_endian 1.0\nelement vertex 4\n\
        property float x\nproperty float y\nproperty float z\n\
        element face 4\nproperty list uchar int vertex_indices\n\
        end_header\n",
    );
    for v in TETRA_VERTS {
      for c in v {
        data.extend_from_slice(&c.to_le_bytes());
      }
    }
    for tri in TETRA_TRIS {
      data.push(3);
      for i in tri {
        data.extend_from_slice(&i.to_le_bytes());
      }
    }

    let mesh = import_mesh(&write_temp("t.ply", &data)).unwrap();
    assert_tetra(&mesh);
    assert_eq!(mesh.vertices[3], [0.0, 0.0, 1.0]);
  }

  #[test]
  fn big_endian_ply_is_rejected_rather_than_misread() {
    let ply = "ply\nformat binary_big_endian 1.0\nelement vertex 0\n\
               end_header\n";
    let err = import_mesh(&write_temp("t.ply", ply.as_bytes())).unwrap_err();
    assert!(err.contains("big-endian"), "{err}");
  }

  #[test]
  fn amf_reads_vertices_and_volumes() {
    let amf = r#"<?xml version="1.0"?>
      <amf unit="millimeter"><object id="0"><mesh>
      <vertices>
        <vertex><coordinates><x>0</x><y>0</y><z>0</z></coordinates></vertex>
        <vertex><coordinates><x>1</x><y>0</y><z>0</z></coordinates></vertex>
        <vertex><coordinates><x>0</x><y>1</y><z>0</z></coordinates></vertex>
      </vertices>
      <volume><triangle><v1>0</v1><v2>1</v2><v3>2</v3></triangle></volume>
      </mesh></object></amf>"#;
    let mesh = import_mesh(&write_temp("t.amf", amf.as_bytes())).unwrap();
    assert_eq!(mesh.vertices.len(), 3);
    assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
  }

  #[test]
  fn a_zipped_amf_says_so_instead_of_failing_to_parse() {
    let err = import_mesh(&write_temp("t.amf", b"PK\x03\x04rest")).unwrap_err();
    assert!(err.contains("compressed"), "{err}");
  }

  #[test]
  fn degenerate_triangles_are_dropped() {
    let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 1 2\nf 1 2 3\n";
    let mesh = import_mesh(&write_temp("t.obj", obj.as_bytes())).unwrap();
    assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
  }

  #[test]
  fn an_unknown_extension_lists_the_supported_ones() {
    let err = import_mesh(&write_temp("t.xyz", b"nonsense")).unwrap_err();
    assert!(err.contains("stl"), "{err}");
  }

  #[test]
  fn a_file_without_triangles_is_an_error() {
    let err = import_mesh(&write_temp("t.obj", b"v 0 0 0\n")).unwrap_err();
    assert!(err.contains("no triangles"), "{err}");
  }

  #[test]
  fn a_missing_file_reports_the_path() {
    let err = import_mesh("/nonexistent/model.stl").unwrap_err();
    assert!(err.contains("/nonexistent/model.stl"), "{err}");
  }
}
