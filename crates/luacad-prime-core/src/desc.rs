//! Serializable scene description.
//!
//! This replaces the legacy approach of Java-`Serializable` scenes saved with
//! `ObjectOutputStream` — a brittle, version-fragile, and security-sensitive
//! format. A `SceneDesc` is plain data that round-trips through any Serde
//! format (the CLI uses RON), and [`SceneDesc::build`] turns it into a runnable
//! [`Scene`], resolving mesh paths relative to a base directory.

use crate::camera::CameraConfig;
use crate::geometry::{Primitive, Sphere, Triangle};
use crate::material::Material;
use crate::math::Vec3;
use crate::obj::{self, ObjError, Transform};
use crate::scene::{Background, Scene};
use crate::texture::ImageData;
use crate::{Float, MaterialId};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error(transparent)]
    Obj(#[from] ObjError),
    #[error("object references material #{index}, but only {count} materials are defined")]
    BadMaterial { index: usize, count: usize },
    #[error("scene defines no materials")]
    NoMaterials,
    #[error("texture error: {0}")]
    Texture(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SceneDesc {
    #[serde(default)]
    pub camera: CameraConfig,
    #[serde(default)]
    pub background: Background,
    pub materials: Vec<Material>,
    pub objects: Vec<ObjectDesc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ObjectDesc {
    Sphere {
        center: Vec3,
        radius: Float,
        material: MaterialId,
    },
    Triangle {
        v0: Vec3,
        v1: Vec3,
        v2: Vec3,
        material: MaterialId,
    },
    /// A quad given by four corners in order; expands to two triangles.
    Quad {
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        material: MaterialId,
    },
    /// A mesh loaded from an OBJ file relative to the scene file's directory.
    Mesh {
        path: String,
        material: MaterialId,
        #[serde(default)]
        transform: Option<TransformDesc>,
        /// If set, load only the faces of this OBJ `g`/`o` group.
        #[serde(default)]
        group: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TransformDesc {
    #[serde(default = "one")]
    pub scale: Float,
    /// Euler rotation in degrees, applied about the world X, then Y, then Z
    /// axis (after scaling, before translation).
    #[serde(default)]
    pub rotate: Vec3,
    #[serde(default)]
    pub translate: Vec3,
}

fn one() -> Float {
    1.0
}

impl Default for TransformDesc {
    fn default() -> Self {
        TransformDesc {
            scale: 1.0,
            rotate: Vec3::ZERO,
            translate: Vec3::ZERO,
        }
    }
}

impl From<TransformDesc> for Transform {
    fn from(t: TransformDesc) -> Self {
        Transform {
            scale: t.scale,
            rotate_deg: t.rotate,
            translate: t.translate,
        }
    }
}

impl SceneDesc {
    /// Build a runnable [`Scene`]. Mesh paths and image textures are resolved
    /// relative to `base_dir` (typically the directory containing the scene
    /// file); `decoder` decodes image files into pixels (kept out of the
    /// codec-free core).
    pub fn build<F>(mut self, base_dir: &Path, decoder: &mut F) -> Result<Scene, SceneError>
    where
        F: FnMut(&Path) -> Result<ImageData, String>,
    {
        if self.materials.is_empty() {
            return Err(SceneError::NoMaterials);
        }
        // Resolve any image textures up front.
        for m in &mut self.materials {
            m.resolve_textures(base_dir, decoder)
                .map_err(SceneError::Texture)?;
        }
        let count = self.materials.len();
        let check = |index: MaterialId| -> Result<MaterialId, SceneError> {
            if index < count {
                Ok(index)
            } else {
                Err(SceneError::BadMaterial { index, count })
            }
        };

        let mut prims: Vec<Primitive> = Vec::new();
        for obj in self.objects {
            match obj {
                ObjectDesc::Sphere {
                    center,
                    radius,
                    material,
                } => prims.push(Sphere::new(center, radius, check(material)?).into()),
                ObjectDesc::Triangle {
                    v0,
                    v1,
                    v2,
                    material,
                } => prims.push(Triangle::new(v0, v1, v2, check(material)?).into()),
                ObjectDesc::Quad {
                    a,
                    b,
                    c,
                    d,
                    material,
                } => {
                    let m = check(material)?;
                    prims.push(Triangle::new(a, b, c, m).into());
                    prims.push(Triangle::new(a, c, d, m).into());
                }
                ObjectDesc::Mesh {
                    path,
                    material,
                    transform,
                    group,
                } => {
                    let m = check(material)?;
                    let full = base_dir.join(&path);
                    let t = transform.map(Transform::from).unwrap_or_default();
                    let tris = obj::load_filtered(&full, group.as_deref(), m, t)?;
                    prims.extend(tris.into_iter().map(Primitive::from));
                }
            }
        }

        Ok(Scene::new(
            self.materials,
            prims,
            self.camera,
            self.background,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    /// A decoder that should never be called (the test scenes use no images).
    fn no_decoder(_: &Path) -> Result<ImageData, String> {
        Err("no image textures expected".into())
    }

    #[test]
    fn round_trips_through_ron() {
        let desc = SceneDesc {
            camera: CameraConfig::default(),
            background: Background::default(),
            materials: vec![Material::Lambertian {
                albedo: Color::new(0.5, 0.5, 0.5).into(),
                normal: None,
            }],
            objects: vec![ObjectDesc::Sphere {
                center: Vec3::new(0.0, 0.0, -1.0),
                radius: 0.5,
                material: 0,
            }],
        };
        let text = ron::ser::to_string(&desc).unwrap();
        let back: SceneDesc = ron::from_str(&text).unwrap();
        let scene = back.build(Path::new("."), &mut no_decoder).unwrap();
        assert_eq!(scene.primitive_count(), 1);
    }

    #[test]
    fn transform_rotate_and_absorption_are_optional() {
        // Older scene files (no `rotate`, no `absorption`) must keep parsing.
        let t: TransformDesc = ron::from_str("(scale: 2.0)").unwrap();
        assert_eq!(t.rotate.length(), 0.0);
        let t: TransformDesc = ron::from_str("(rotate: (x: 0.0, y: 90.0, z: 0.0))").unwrap();
        assert!((t.rotate.y - 90.0).abs() < 1e-6);
        assert_eq!(t.scale, 1.0);

        let m: Material = ron::from_str("Dielectric(ior: 1.5)").unwrap();
        assert!(
            matches!(m, Material::Dielectric { absorption, .. } if absorption.max_component() == 0.0)
        );
        let m: Material =
            ron::from_str("Dielectric(ior: 1.5, absorption: (x: 0.1, y: 0.0, z: 0.4))").unwrap();
        assert!(
            matches!(m, Material::Dielectric { absorption, .. } if (absorption.z - 0.4).abs() < 1e-6)
        );
        let m: Material = ron::from_str("Dielectric(ior: 1.5, roughness: 0.3)").unwrap();
        assert!(
            matches!(m, Material::Dielectric { roughness, absorption, .. }
                if (roughness - 0.3).abs() < 1e-6 && absorption.max_component() == 0.0)
        );
    }

    #[test]
    fn bad_material_index_is_rejected() {
        let desc = SceneDesc {
            camera: CameraConfig::default(),
            background: Background::default(),
            materials: vec![Material::Lambertian {
                albedo: Color::ONE.into(),
                normal: None,
            }],
            objects: vec![ObjectDesc::Sphere {
                center: Vec3::ZERO,
                radius: 1.0,
                material: 5,
            }],
        };
        assert!(matches!(
            desc.build(Path::new("."), &mut no_decoder),
            Err(SceneError::BadMaterial { index: 5, count: 1 })
        ));
    }
}
