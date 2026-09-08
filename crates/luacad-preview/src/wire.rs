//! The flattened scene as a byte buffer.
//!
//! The playground splits the preview across two WebAssembly modules — the
//! engine flattens, the viewer renders — so the scene has to cross a
//! `postMessage` between them. It goes as one buffer of little-endian words
//! rather than as JSON: a model of a few hundred thousand triangles is a
//! handful of megabytes of floats, which JSON would triple in size and make
//! the browser parse a character at a time.
//!
//! ```text
//! u32     magic          "LCSG"
//! u32     version        1
//! u32     group_count
//!   per group:
//!     u32     leaf_count
//!     per leaf:
//!       u32     operation      0 = intersected, 1 = subtracted
//!       u32     convexity
//!       f32[16] transform      column-major, CAD space
//!       f32[3]  color          diffuse, 0..1
//!       f32[4]  specular       color in rgb, shininess in a
//!       f32[3]  emission
//!       u32     vertex_count   vertices, i.e. three times the triangles
//!       f32[]   vertices       vertex_count * 3, GL axes (y, z, x)
//! u32     overlay_count
//!   per overlay:
//!     f32[16] transform
//!     f32[4]  color            RGBA, 0..1
//!     u32     vertex_count
//!     f32[]   vertices
//! ```
//!
//! The transparent view mode's solid meshes are not part of this: they are
//! the Manifold booleans of the whole model, which the playground does not
//! compute.

use crate::tree::{
  CsgGroup, CsgLeaf, CsgScene, Operation, OverlayMesh, Shading,
};

/// First word of a buffer, so a mismatched pair of modules fails loudly
/// instead of decoding noise.
const MAGIC: u32 = u32::from_le_bytes(*b"LCSG");
const VERSION: u32 = 1;

/// Serialize a scene.
pub fn encode(scene: &CsgScene) -> Vec<u8> {
  let mut out = Vec::with_capacity(scene.triangle_count() * 3 * 12 + 64);
  push_u32(&mut out, MAGIC);
  push_u32(&mut out, VERSION);

  push_u32(&mut out, scene.groups.len() as u32);
  for group in &scene.groups {
    push_u32(&mut out, group.primitives.len() as u32);
    for leaf in &group.primitives {
      push_u32(
        &mut out,
        match leaf.operation {
          Operation::Intersection => 0,
          Operation::Subtraction => 1,
        },
      );
      push_u32(&mut out, leaf.convexity);
      push_floats(&mut out, &leaf.transform);
      push_floats(&mut out, &leaf.shading.color);
      push_floats(&mut out, &leaf.shading.specular);
      push_floats(&mut out, &leaf.shading.emission);
      push_vertices(&mut out, &leaf.vertices);
    }
  }

  push_u32(&mut out, scene.overlays.len() as u32);
  for overlay in &scene.overlays {
    push_floats(&mut out, &overlay.transform);
    push_floats(&mut out, &overlay.color);
    push_vertices(&mut out, &overlay.vertices);
  }
  out
}

/// Read back what [`encode`] wrote.
pub fn decode(bytes: &[u8]) -> Result<CsgScene, DecodeError> {
  let mut cursor = Cursor { bytes, at: 0 };
  if cursor.u32()? != MAGIC {
    return Err(DecodeError::NotAScene);
  }
  let version = cursor.u32()?;
  if version != VERSION {
    return Err(DecodeError::Version(version));
  }

  let group_count = cursor.u32()?;
  let mut groups = Vec::with_capacity(group_count as usize);
  for _ in 0..group_count {
    let leaf_count = cursor.u32()?;
    let mut primitives = Vec::with_capacity(leaf_count as usize);
    for _ in 0..leaf_count {
      let operation = match cursor.u32()? {
        0 => Operation::Intersection,
        1 => Operation::Subtraction,
        other => return Err(DecodeError::Operation(other)),
      };
      let convexity = cursor.u32()?;
      let transform = cursor.floats::<16>()?;
      let shading = Shading {
        color: cursor.floats::<3>()?,
        specular: cursor.floats::<4>()?,
        emission: cursor.floats::<3>()?,
      };
      primitives.push(CsgLeaf {
        vertices: cursor.vertices()?,
        transform,
        operation,
        convexity,
        shading,
      });
    }
    groups.push(CsgGroup { primitives });
  }

  let overlay_count = cursor.u32()?;
  let mut overlays = Vec::with_capacity(overlay_count as usize);
  for _ in 0..overlay_count {
    let transform = cursor.floats::<16>()?;
    let color = cursor.floats::<4>()?;
    overlays.push(OverlayMesh {
      vertices: cursor.vertices()?,
      transform,
      color,
    });
  }

  Ok(CsgScene { groups, overlays })
}

/// Why a buffer could not be read as a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
  /// The buffer does not start with the scene magic.
  NotAScene,
  /// Written by a different version of the format.
  Version(u32),
  /// A primitive is neither intersected nor subtracted.
  Operation(u32),
  /// The buffer ends in the middle of a field.
  Truncated,
}

impl std::fmt::Display for DecodeError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NotAScene => write!(f, "not a LuaCAD scene buffer"),
      Self::Version(version) => {
        write!(f, "scene buffer version {version}, expected {VERSION}")
      }
      Self::Operation(value) => write!(f, "unknown CSG operation {value}"),
      Self::Truncated => write!(f, "scene buffer ends mid-field"),
    }
  }
}

impl std::error::Error for DecodeError {}

// ---------------------------------------------------------------------------
// Reading and writing

fn push_u32(out: &mut Vec<u8>, value: u32) {
  out.extend_from_slice(&value.to_le_bytes());
}

fn push_floats(out: &mut Vec<u8>, values: &[f32]) {
  for value in values {
    out.extend_from_slice(&value.to_le_bytes());
  }
}

fn push_vertices(out: &mut Vec<u8>, vertices: &[[f32; 3]]) {
  push_u32(out, vertices.len() as u32);
  for vertex in vertices {
    push_floats(out, vertex);
  }
}

struct Cursor<'a> {
  bytes: &'a [u8],
  at: usize,
}

impl Cursor<'_> {
  fn take(&mut self, count: usize) -> Result<&[u8], DecodeError> {
    let end = self.at.checked_add(count).ok_or(DecodeError::Truncated)?;
    let slice = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated)?;
    self.at = end;
    Ok(slice)
  }

  fn u32(&mut self) -> Result<u32, DecodeError> {
    let bytes = self.take(4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }

  fn f32(&mut self) -> Result<f32, DecodeError> {
    let bytes = self.take(4)?;
    Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
  }

  fn floats<const N: usize>(&mut self) -> Result<[f32; N], DecodeError> {
    let mut out = [0.0; N];
    for value in &mut out {
      *value = self.f32()?;
    }
    Ok(out)
  }

  fn vertices(&mut self) -> Result<Vec<[f32; 3]>, DecodeError> {
    let count = self.u32()? as usize;
    // The count comes off the wire, so the allocation it asks for is capped:
    // a corrupt buffer would otherwise reserve gigabytes before the first
    // field is even read.
    let mut vertices = Vec::with_capacity(count.min(1 << 16));
    for _ in 0..count {
      vertices.push(self.floats::<3>()?);
    }
    Ok(vertices)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn scene() -> CsgScene {
    let transform = [
      1.0, 0.0, 0.0, 0.0, //
      0.0, 1.0, 0.0, 0.0, //
      0.0, 0.0, 1.0, 0.0, //
      1.0, 2.0, 3.0, 1.0,
    ];
    CsgScene {
      groups: vec![CsgGroup {
        primitives: vec![
          CsgLeaf {
            vertices: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            transform,
            operation: Operation::Intersection,
            convexity: 1,
            shading: Shading {
              color: [0.1, 0.2, 0.3],
              specular: [0.4, 0.5, 0.6, 32.0],
              emission: [0.0, 0.0, 0.7],
            },
          },
          CsgLeaf {
            vertices: vec![],
            transform,
            operation: Operation::Subtraction,
            convexity: 4,
            shading: Shading::plain([1.0, 0.0, 0.0]),
          },
        ],
      }],
      overlays: vec![OverlayMesh {
        vertices: vec![[2.0, 3.0, 4.0]],
        transform,
        color: [1.0, 0.32, 0.32, 0.5],
      }],
    }
  }

  #[test]
  fn a_scene_survives_the_round_trip() {
    let decoded = decode(&encode(&scene())).unwrap();
    let original = scene();
    assert_eq!(decoded.groups.len(), original.groups.len());
    let leaves = &decoded.groups[0].primitives;
    assert_eq!(leaves.len(), 2);
    assert_eq!(
      leaves[0].vertices,
      original.groups[0].primitives[0].vertices
    );
    assert_eq!(leaves[0].shading, original.groups[0].primitives[0].shading);
    assert_eq!(
      leaves[0].transform,
      original.groups[0].primitives[0].transform
    );
    assert_eq!(leaves[1].operation, Operation::Subtraction);
    assert_eq!(leaves[1].convexity, 4);
    assert!(leaves[1].vertices.is_empty());
    assert_eq!(decoded.overlays.len(), 1);
    assert_eq!(decoded.overlays[0].color, original.overlays[0].color);
    assert_eq!(decoded.overlays[0].vertices, original.overlays[0].vertices);
  }

  /// The viewer is handed whatever the page passes it, so a buffer that is
  /// not a scene has to come back as an error rather than as garbage.
  #[test]
  fn a_foreign_buffer_is_rejected() {
    assert_eq!(decode(b"not a scene").unwrap_err(), DecodeError::NotAScene);
    assert_eq!(decode(&[]).unwrap_err(), DecodeError::Truncated);

    let mut truncated = encode(&scene());
    truncated.truncate(truncated.len() - 4);
    assert_eq!(decode(&truncated).unwrap_err(), DecodeError::Truncated);
  }
}
