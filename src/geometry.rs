use csgrs::mesh::Mesh as CsgMesh;
use csgrs::traits::CSG;
use mlua::{UserData, UserDataMethods};
use three_d::*;

#[derive(Clone, Debug)]
pub struct CsgGeometry {
  pub mesh: CsgMesh<()>,
}

impl UserData for CsgGeometry {
  fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    methods.add_method("translate", |_, this, (x, y, z): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.translate(x, y, z),
      })
    });

    methods.add_method("rotate", |_, this, (x, y, z): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.rotate(x, y, z),
      })
    });

    methods.add_method("scale", |_, this, (sx, sy, sz): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.scale(sx, sy, sz),
      })
    });

    // CSG difference: a - b
    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.difference(&other_ref.mesh),
        })
      },
    );

    // CSG union: a + b
    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.union(&other_ref.mesh),
        })
      },
    );

    // CSG intersection: a * b
    methods.add_meta_method(
      mlua::MetaMethod::Mul,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.intersection(&other_ref.mesh),
        })
      },
    );

    methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
      Ok(format!(
        "CsgGeometry(polygons: {})",
        this.mesh.polygons.len()
      ))
    });
  }
}

/// Convert a csgrs mesh to a three-d CpuMesh.
/// Applies CAD Z-up → GL Y-up coordinate swap: (x,y,z) → (y,z,x)
pub fn csg_to_cpu_mesh(csg: &CsgMesh<()>) -> CpuMesh {
  let tri_mesh = csg.triangulate();

  let mut positions: Vec<Vec3> = Vec::new();
  let mut normals: Vec<Vec3> = Vec::new();
  let mut indices: Vec<u32> = Vec::new();

  for polygon in &tri_mesh.polygons {
    let base_idx = positions.len() as u32;
    for vertex in &polygon.vertices {
      let p = &vertex.pos;
      let n = &vertex.normal;
      // CAD (x,y,z) → GL (y,z,x)
      positions.push(vec3(p.y, p.z, p.x));
      normals.push(vec3(n.y, n.z, n.x));
    }
    // Fan-triangulate polygons with more than 3 vertices
    for i in 1..polygon.vertices.len().saturating_sub(1) {
      indices.push(base_idx);
      indices.push(base_idx + i as u32);
      indices.push(base_idx + i as u32 + 1);
    }
  }

  CpuMesh {
    positions: Positions::F32(positions),
    indices: Indices::U32(indices),
    normals: Some(normals),
    ..Default::default()
  }
}
