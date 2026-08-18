//! Minimal Wavefront OBJ loader.
//!
//! The legacy `ContentLoader` took a `MainGui` in its constructor and logged
//! straight to a Swing text area. This loader has zero UI dependencies: it is a
//! pure function from a path (plus a material id and optional transform) to a
//! list of triangles, returning a typed error.

use crate::geometry::Triangle;
use crate::math::Vec3;
use crate::{Float, MaterialId};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ObjError {
    #[error("failed to read OBJ file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("malformed OBJ at line {line}: {reason}")]
    Parse { line: usize, reason: String },
}

/// A scale → rotate → translate transform applied to loaded vertices.
/// Rotation is Euler angles in degrees, applied extrinsically about the world
/// X, then Y, then Z axis.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub scale: Float,
    pub rotate_deg: Vec3,
    pub translate: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            scale: 1.0,
            rotate_deg: Vec3::ZERO,
            translate: Vec3::ZERO,
        }
    }
}

/// A `Transform` with its rotation matrix precomputed, so per-vertex
/// application does no trigonometry.
#[derive(Clone, Copy)]
struct CompiledTransform {
    scale: Float,
    /// Rows of the rotation matrix `Rz · Ry · Rx`.
    rot: [Vec3; 3],
    translate: Vec3,
}

impl CompiledTransform {
    fn new(t: &Transform) -> Self {
        let (sx, cx) = t.rotate_deg.x.to_radians().sin_cos();
        let (sy, cy) = t.rotate_deg.y.to_radians().sin_cos();
        let (sz, cz) = t.rotate_deg.z.to_radians().sin_cos();
        let rot = [
            Vec3::new(cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx),
            Vec3::new(sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx),
            Vec3::new(-sy, cy * sx, cy * cx),
        ];
        CompiledTransform {
            scale: t.scale,
            rot,
            translate: t.translate,
        }
    }

    #[inline]
    fn rotate(&self, v: Vec3) -> Vec3 {
        Vec3::new(self.rot[0].dot(v), self.rot[1].dot(v), self.rot[2].dot(v))
    }

    #[inline]
    fn point(&self, p: Vec3) -> Vec3 {
        self.rotate(p * self.scale) + self.translate
    }

    /// Normals see only the rotation (uniform scale and translation leave
    /// directions unchanged, so no inverse-transpose is needed).
    #[inline]
    fn normal(&self, n: Vec3) -> Vec3 {
        self.rotate(n)
    }
}

/// Load all faces of an OBJ file as triangles, assigning every triangle the
/// given material. Smooth normals are used when the file provides them.
pub fn load(
    path: impl AsRef<Path>,
    material: MaterialId,
    transform: Transform,
) -> Result<Vec<Triangle>, ObjError> {
    load_filtered(path, None, material, transform)
}

/// Like [`load`], but if `group` is `Some(name)` only faces belonging to the
/// matching `g`/`o` group are loaded. This lets a multi-object OBJ be placed as
/// several scene objects with different materials.
pub fn load_filtered(
    path: impl AsRef<Path>,
    group: Option<&str>,
    material: MaterialId,
    transform: Transform,
) -> Result<Vec<Triangle>, ObjError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| ObjError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let reader = BufReader::new(file);
    let transform = CompiledTransform::new(&transform);

    let mut positions: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut texcoords: Vec<[Float; 2]> = Vec::new();
    let mut triangles: Vec<Triangle> = Vec::new();
    let mut current_group: Option<String> = None;

    for (line_no, line) in reader.lines().enumerate() {
        let line = line.map_err(|source| ObjError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut tokens = line.split_whitespace();
        match tokens.next() {
            Some("v") => positions.push(transform.point(parse_vec3(tokens, line_no)?)),
            Some("vn") => normals.push(transform.normal(parse_vec3(tokens, line_no)?)),
            Some("vt") => texcoords.push(parse_uv(tokens, line_no)?),
            // Track the current group/object name.
            Some("g") | Some("o") => current_group = tokens.next().map(|s| s.to_string()),
            Some("f") => {
                // Skip faces outside the requested group (vertices/normals still
                // accumulate globally, so indices stay valid).
                if group.is_some() && group != current_group.as_deref() {
                    continue;
                }
                let verts: Vec<FaceVert> = tokens
                    .map(|t| {
                        FaceVert::parse(t, positions.len(), texcoords.len(), normals.len(), line_no)
                    })
                    .collect::<Result<_, _>>()?;
                if verts.len() < 3 {
                    return Err(ObjError::Parse {
                        line: line_no + 1,
                        reason: "face with fewer than 3 vertices".into(),
                    });
                }
                // Fan-triangulate the (possibly n-gon) face.
                for i in 1..verts.len() - 1 {
                    let tri = make_triangle(
                        &positions,
                        &normals,
                        &texcoords,
                        verts[0],
                        verts[i],
                        verts[i + 1],
                        material,
                    )?;
                    triangles.push(tri);
                }
            }
            // Smoothing, materials, texcoords: ignored for now.
            _ => {}
        }
    }

    Ok(triangles)
}

#[derive(Clone, Copy)]
struct FaceVert {
    pos: usize,
    tex: Option<usize>,
    normal: Option<usize>,
}

impl FaceVert {
    /// Parse a `v`, `v/t`, `v//n`, or `v/t/n` token, resolving 1-based and
    /// negative (relative) indices into 0-based ones.
    fn parse(
        token: &str,
        n_pos: usize,
        n_tex: usize,
        n_norm: usize,
        line_no: usize,
    ) -> Result<FaceVert, ObjError> {
        let mut parts = token.split('/');
        let pos = resolve_index(parts.next().unwrap_or(""), n_pos, line_no)?;
        let tex = match parts.next() {
            Some(s) if !s.is_empty() => Some(resolve_index(s, n_tex, line_no)?),
            _ => None,
        };
        let normal = match parts.next() {
            Some(s) if !s.is_empty() => Some(resolve_index(s, n_norm, line_no)?),
            _ => None,
        };
        Ok(FaceVert { pos, tex, normal })
    }
}

fn resolve_index(s: &str, count: usize, line_no: usize) -> Result<usize, ObjError> {
    let idx: i64 = s.parse().map_err(|_| ObjError::Parse {
        line: line_no + 1,
        reason: format!("invalid index '{s}'"),
    })?;
    let resolved = if idx > 0 {
        idx - 1
    } else if idx < 0 {
        count as i64 + idx
    } else {
        return Err(ObjError::Parse {
            line: line_no + 1,
            reason: "index 0 is not valid in OBJ".into(),
        });
    };
    if resolved < 0 || resolved as usize >= count {
        return Err(ObjError::Parse {
            line: line_no + 1,
            reason: format!("index {idx} out of range"),
        });
    }
    Ok(resolved as usize)
}

#[allow(clippy::too_many_arguments)]
fn make_triangle(
    positions: &[Vec3],
    normals: &[Vec3],
    texcoords: &[[Float; 2]],
    a: FaceVert,
    b: FaceVert,
    c: FaceVert,
    material: MaterialId,
) -> Result<Triangle, ObjError> {
    let mut tri = Triangle::new(
        positions[a.pos],
        positions[b.pos],
        positions[c.pos],
        material,
    );
    if let (Some(na), Some(nb), Some(nc)) = (a.normal, b.normal, c.normal) {
        tri = tri.with_normals([normals[na], normals[nb], normals[nc]]);
    }
    if let (Some(ta), Some(tb), Some(tc)) = (a.tex, b.tex, c.tex) {
        tri = tri.with_uvs([texcoords[ta], texcoords[tb], texcoords[tc]]);
    }
    Ok(tri)
}

/// Parse the first two floats of a `vt` line as `(u, v)`. The `v` axis is
/// flipped (OBJ origin is bottom-left, image origin is top-left).
fn parse_uv<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
    line_no: usize,
) -> Result<[Float; 2], ObjError> {
    let mut next = || -> Result<Float, ObjError> {
        tokens
            .next()
            .ok_or_else(|| ObjError::Parse {
                line: line_no + 1,
                reason: "expected 2 texture coordinates".into(),
            })?
            .parse::<Float>()
            .map_err(|_| ObjError::Parse {
                line: line_no + 1,
                reason: "invalid texture coordinate".into(),
            })
    };
    let u = next()?;
    let v = next()?;
    Ok([u, 1.0 - v])
}

fn parse_vec3<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
    line_no: usize,
) -> Result<Vec3, ObjError> {
    let mut next = || -> Result<Float, ObjError> {
        tokens
            .next()
            .ok_or_else(|| ObjError::Parse {
                line: line_no + 1,
                reason: "expected 3 floats".into(),
            })?
            .parse::<Float>()
            .map_err(|_| ObjError::Parse {
                line: line_no + 1,
                reason: "invalid float".into(),
            })
    };
    Ok(Vec3::new(next()?, next()?, next()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_a_quad_into_two_triangles() {
        let tmp = tempfile_path();
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(f, "# a quad").unwrap();
            writeln!(f, "v 0 0 0").unwrap();
            writeln!(f, "v 1 0 0").unwrap();
            writeln!(f, "v 1 1 0").unwrap();
            writeln!(f, "v 0 1 0").unwrap();
            writeln!(f, "vn 0 0 1").unwrap();
            writeln!(f, "f 1//1 2//1 3//1 4//1").unwrap();
        }
        let tris = load(&tmp, 0, Transform::default()).unwrap();
        assert_eq!(tris.len(), 2);
        assert!(tris[0].normals.is_some());
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn loads_texture_coordinates() {
        let tmp = tempfile_path();
        {
            let mut f = File::create(&tmp).unwrap();
            for line in [
                "v 0 0 0",
                "v 1 0 0",
                "v 0 1 0",
                "vt 0 0",
                "vt 1 0",
                "vt 0 1",
                "vn 0 0 1",
                "f 1/1/1 2/2/1 3/3/1",
            ] {
                writeln!(f, "{line}").unwrap();
            }
        }
        let tris = load(&tmp, 0, Transform::default()).unwrap();
        assert_eq!(tris.len(), 1);
        let uvs = tris[0].uvs.expect("uvs should be loaded");
        // v is flipped (1 - obj_v): vt "0 0" -> [0, 1].
        assert_eq!(uvs[0], [0.0, 1.0]);
        assert_eq!(uvs[1], [1.0, 1.0]);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn group_filter_loads_only_matching_faces() {
        let tmp = tempfile_path();
        {
            let mut f = File::create(&tmp).unwrap();
            for line in ["v 0 0 0", "v 1 0 0", "v 0 1 0", "v 0 0 1"] {
                writeln!(f, "{line}").unwrap();
            }
            writeln!(f, "g alpha").unwrap();
            writeln!(f, "f 1 2 3").unwrap();
            writeln!(f, "g beta").unwrap();
            writeln!(f, "f 1 2 4").unwrap();
            writeln!(f, "f 2 3 4").unwrap();
        }
        let all = load(&tmp, 0, Transform::default()).unwrap();
        let alpha = load_filtered(&tmp, Some("alpha"), 0, Transform::default()).unwrap();
        let beta = load_filtered(&tmp, Some("beta"), 0, Transform::default()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(alpha.len(), 1);
        assert_eq!(beta.len(), 2);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn rotation_transform_rotates_positions_and_normals() {
        let tmp = tempfile_path();
        {
            let mut f = File::create(&tmp).unwrap();
            for line in [
                "v 1 0 0",
                "v 0 1 0",
                "v 0 0 0",
                "vn 1 0 0",
                "f 1//1 2//1 3//1",
            ] {
                writeln!(f, "{line}").unwrap();
            }
        }
        let t = Transform {
            scale: 2.0,
            rotate_deg: Vec3::new(0.0, 90.0, 0.0),
            translate: Vec3::new(0.0, 0.0, 1.0),
        };
        let tris = load(&tmp, 0, t).unwrap();
        let tri = &tris[0];
        // (1,0,0) → scale (2,0,0) → rotY 90° (0,0,-2) → translate (0,0,-1).
        assert!((tri.v0 - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4);
        // (0,1,0) → (0,2,0) → (0,2,0) → (0,2,1).
        assert!((tri.v1 - Vec3::new(0.0, 2.0, 1.0)).length() < 1e-4);
        // The normal sees only the rotation: (1,0,0) → (0,0,-1).
        let n = tri.normals.unwrap()[0];
        assert!((n - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-4);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn negative_indices_resolve() {
        let tmp = tempfile_path();
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(f, "v 0 0 0").unwrap();
            writeln!(f, "v 1 0 0").unwrap();
            writeln!(f, "v 0 1 0").unwrap();
            writeln!(f, "f -3 -2 -1").unwrap();
        }
        let tris = load(&tmp, 0, Transform::default()).unwrap();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0].v0, Vec3::ZERO);
        std::fs::remove_file(&tmp).ok();
    }

    fn tempfile_path() -> String {
        let mut p = std::env::temp_dir();
        // Unique-ish name without extra deps.
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("prime_obj_test_{n}.obj"));
        p.display().to_string()
    }
}
