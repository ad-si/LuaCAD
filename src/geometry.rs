use csgrs::mesh::Mesh as CsgMesh;
use csgrs::mesh::plane::Plane;
use csgrs::traits::CSG;
use mlua::{UserData, UserDataMethods};
use nalgebra::{Matrix4, Vector3};
use three_d::*;

pub fn lua_val_to_f32(v: &mlua::Value) -> Option<f32> {
  match v {
    mlua::Value::Number(n) => Some(*n as f32),
    mlua::Value::Integer(n) => Some(*n as f32),
    _ => None,
  }
}

fn named_color(name: &str) -> Option<[f32; 3]> {
  match name.to_lowercase().as_str() {
    "white" => Some([1.0, 1.0, 1.0]),
    "black" => Some([0.0, 0.0, 0.0]),
    "red" => Some([1.0, 0.0, 0.0]),
    "green" => Some([0.0, 1.0, 0.0]),
    "blue" => Some([0.0, 0.0, 1.0]),
    "cyan" => Some([0.0, 1.0, 1.0]),
    "magenta" => Some([1.0, 0.0, 1.0]),
    "yellow" => Some([1.0, 1.0, 0.0]),
    "orange" => Some([1.0, 0.5, 0.0]),
    "gray" | "grey" => Some([0.5, 0.5, 0.5]),
    _ => None,
  }
}

#[derive(Clone, Debug)]
pub struct CsgGeometry {
  pub mesh: CsgMesh<()>,
  pub color: Option<[f32; 3]>,
}

impl UserData for CsgGeometry {
  fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    // --- Transformations ---

    methods.add_method(
      "translate",
      |_, this, (x, y, z): (f32, f32, Option<f32>)| {
        let z = z.unwrap_or(0.0);
        Ok(CsgGeometry {
          mesh: this.mesh.translate(x, y, z),
          color: this.color,
        })
      },
    );

    methods.add_method("rotate", |_, this, args: mlua::MultiValue| {
      if args.len() >= 6 {
        // rotate(cx, cy, cz, rx, ry, rz) — rotate around center point
        let cx = lua_val_to_f32(&args[0]).unwrap_or(0.0);
        let cy = lua_val_to_f32(&args[1]).unwrap_or(0.0);
        let cz = lua_val_to_f32(&args[2]).unwrap_or(0.0);
        let rx = lua_val_to_f32(&args[3]).unwrap_or(0.0);
        let ry = lua_val_to_f32(&args[4]).unwrap_or(0.0);
        let rz = lua_val_to_f32(&args[5]).unwrap_or(0.0);
        let mesh = this
          .mesh
          .translate(-cx, -cy, -cz)
          .rotate(rx, ry, rz)
          .translate(cx, cy, cz);
        Ok(CsgGeometry {
          mesh,
          color: this.color,
        })
      } else {
        // rotate(rx, ry, rz)
        let rx = lua_val_to_f32(args.front().unwrap_or(&mlua::Value::Nil))
          .unwrap_or(0.0);
        let ry = lua_val_to_f32(args.get(1).unwrap_or(&mlua::Value::Nil))
          .unwrap_or(0.0);
        let rz = lua_val_to_f32(args.get(2).unwrap_or(&mlua::Value::Nil))
          .unwrap_or(0.0);
        Ok(CsgGeometry {
          mesh: this.mesh.rotate(rx, ry, rz),
          color: this.color,
        })
      }
    });

    methods.add_method(
      "rotate2d",
      |_, this, (x, y, angle): (f32, f32, f32)| {
        let mesh = this
          .mesh
          .translate(-x, -y, 0.0)
          .rotate(0.0, 0.0, angle)
          .translate(x, y, 0.0);
        Ok(CsgGeometry {
          mesh,
          color: this.color,
        })
      },
    );

    methods.add_method("scale", |_, this, (sx, sy, sz): (f32, f32, f32)| {
      Ok(CsgGeometry {
        mesh: this.mesh.scale(sx, sy, sz),
        color: this.color,
      })
    });

    methods.add_method("resize", |_, this, (x, y, z): (f32, f32, f32)| {
      let bb = this.mesh.bounding_box();
      let cur_w = bb.maxs.x - bb.mins.x;
      let cur_d = bb.maxs.y - bb.mins.y;
      let cur_h = bb.maxs.z - bb.mins.z;

      let sx = if x > 0.0 && cur_w > 1e-9 {
        x / cur_w
      } else {
        1.0
      };
      let sy = if y > 0.0 && cur_d > 1e-9 {
        y / cur_d
      } else {
        1.0
      };
      let sz = if z > 0.0 && cur_h > 1e-9 {
        z / cur_h
      } else {
        1.0
      };

      Ok(CsgGeometry {
        mesh: this.mesh.scale(sx, sy, sz),
        color: this.color,
      })
    });

    methods.add_method("mirror", |_, this, (x, y, z): (f32, f32, f32)| {
      let len_sq = x * x + y * y + z * z;
      if len_sq < 1e-12 {
        return Ok(this.clone());
      }
      let plane = Plane::from_normal(Vector3::new(x, y, z), 0.0);
      Ok(CsgGeometry {
        mesh: this.mesh.mirror(plane),
        color: this.color,
      })
    });

    methods.add_method("multmatrix", |_, this, matrix: mlua::Table| {
      let vals: Vec<f32> = (1..=16)
        .map(|i| matrix.get::<f32>(i).unwrap_or(0.0))
        .collect();
      if vals.len() != 16 {
        return Err(mlua::Error::RuntimeError(
          "multmatrix requires a table with 16 elements".to_string(),
        ));
      }
      // Row-major to nalgebra column-major
      #[rustfmt::skip]
      let mat = Matrix4::new(
        vals[0],  vals[1],  vals[2],  vals[3],
        vals[4],  vals[5],  vals[6],  vals[7],
        vals[8],  vals[9],  vals[10], vals[11],
        vals[12], vals[13], vals[14], vals[15],
      );
      Ok(CsgGeometry {
        mesh: this.mesh.transform(&mat),
        color: this.color,
      })
    });

    // --- Clone/Copy ---

    methods.add_method("clone", |_, this, ()| Ok(this.clone()));
    methods.add_method("copy", |_, this, ()| Ok(this.clone()));

    // --- Color ---

    methods.add_method("setcolor", |_, this, args: mlua::MultiValue| {
      let color = if args.len() >= 3 {
        // setcolor(r, g, b)
        let r = lua_val_to_f32(&args[0]).unwrap_or(1.0);
        let g = lua_val_to_f32(&args[1]).unwrap_or(1.0);
        let b = lua_val_to_f32(&args[2]).unwrap_or(1.0);
        [r, g, b]
      } else if let mlua::Value::Table(t) = &args[0] {
        // setcolor({r, g, b})
        let r: f32 = t.get::<f32>(1).unwrap_or(1.0);
        let g: f32 = t.get::<f32>(2).unwrap_or(1.0);
        let b: f32 = t.get::<f32>(3).unwrap_or(1.0);
        [r, g, b]
      } else if let mlua::Value::String(s) = &args[0] {
        // setcolor("red")
        match s.to_str() {
          Ok(name) => named_color(&name).unwrap_or([1.0, 1.0, 1.0]),
          Err(_) => [1.0, 1.0, 1.0],
        }
      } else {
        [1.0, 1.0, 1.0]
      };
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: Some(color),
      })
    });

    // --- Multi-object booleans ---

    methods.add_method(
      "add",
      |_, this, args: mlua::Variadic<mlua::AnyUserData>| {
        let mut result = this.mesh.clone();
        for ud in args.iter() {
          let other = ud.borrow::<CsgGeometry>()?;
          result = result.union(&other.mesh);
        }
        Ok(CsgGeometry {
          mesh: result,
          color: this.color,
        })
      },
    );

    methods.add_method(
      "sub",
      |_, this, args: mlua::Variadic<mlua::AnyUserData>| {
        let mut result = this.mesh.clone();
        for ud in args.iter() {
          let other = ud.borrow::<CsgGeometry>()?;
          result = result.difference(&other.mesh);
        }
        Ok(CsgGeometry {
          mesh: result,
          color: this.color,
        })
      },
    );

    methods.add_method(
      "intersect",
      |_, this, args: mlua::Variadic<mlua::AnyUserData>| {
        let mut result = this.mesh.clone();
        for ud in args.iter() {
          let other = ud.borrow::<CsgGeometry>()?;
          result = result.intersection(&other.mesh);
        }
        Ok(CsgGeometry {
          mesh: result,
          color: this.color,
        })
      },
    );

    // --- Hull + Minkowski ---

    methods.add_method("hull", |_, this, ()| {
      Ok(CsgGeometry {
        mesh: this.mesh.convex_hull(),
        color: this.color,
      })
    });

    methods.add_method("minkowski", |_, this, other: mlua::AnyUserData| {
      let other_ref = other.borrow::<CsgGeometry>()?;
      Ok(CsgGeometry {
        mesh: this.mesh.minkowski_sum(&other_ref.mesh),
        color: this.color,
      })
    });

    // --- CSG operators ---

    // CSG union: a + b
    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.union(&other_ref.mesh),
          color: this.color,
        })
      },
    );

    // CSG difference: a - b
    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        Ok(CsgGeometry {
          mesh: this.mesh.difference(&other_ref.mesh),
          color: this.color,
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
          color: this.color,
        })
      },
    );

    methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
      let color_str = this
        .color
        .map(|c| format!(", color: [{:.2},{:.2},{:.2}]", c[0], c[1], c[2]))
        .unwrap_or_default();
      Ok(format!(
        "CsgGeometry(polygons: {}{})",
        this.mesh.polygons.len(),
        color_str
      ))
    });
  }
}

// ---------------------------------------------------------------------------
// CsgSketch — 2D shapes that can be extruded to 3D
// ---------------------------------------------------------------------------

use csgrs::sketch::Sketch;

#[derive(Clone, Debug)]
pub struct CsgSketch {
  pub sketch: Sketch<()>,
  pub color: Option<[f32; 3]>,
}

impl UserData for CsgSketch {
  fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    // --- Transformations ---

    methods.add_method("translate", |_, this, (x, y): (f32, f32)| {
      Ok(CsgSketch {
        sketch: this.sketch.translate(x, y, 0.0),
        color: this.color,
      })
    });

    methods.add_method("rotate", |_, this, angle: f32| {
      Ok(CsgSketch {
        sketch: this.sketch.rotate(0.0, 0.0, angle),
        color: this.color,
      })
    });

    methods.add_method("scale", |_, this, (sx, sy): (f32, f32)| {
      Ok(CsgSketch {
        sketch: this.sketch.scale(sx, sy, 1.0),
        color: this.color,
      })
    });

    methods.add_method("mirror", |_, this, (x, y): (f32, f32)| {
      let len_sq = x * x + y * y;
      if len_sq < 1e-12 {
        return Ok(this.clone());
      }
      let plane = Plane::from_normal(Vector3::new(x, y, 0.0), 0.0);
      Ok(CsgSketch {
        sketch: this.sketch.mirror(plane),
        color: this.color,
      })
    });

    methods.add_method("offset", |_, this, d: f32| {
      Ok(CsgSketch {
        sketch: this.sketch.offset(d),
        color: this.color,
      })
    });

    // --- Extrusion ---

    methods.add_method("linear_extrude", |_, this, args: mlua::MultiValue| {
      let height = if let Some(first) = args.front() {
        lua_val_to_f32(first).unwrap_or(1.0)
      } else {
        1.0
      };
      let mesh = this.sketch.extrude(height);
      Ok(CsgGeometry {
        mesh,
        color: this.color,
      })
    });

    methods.add_method("rotate_extrude", |_, this, args: mlua::MultiValue| {
      let angle = args.front().and_then(lua_val_to_f32).unwrap_or(360.0);
      let segments = args
        .get(1)
        .and_then(lua_val_to_f32)
        .map(|v| v as usize)
        .unwrap_or(32);
      let mesh = this
        .sketch
        .revolve(angle, segments)
        .map_err(|e| mlua::Error::RuntimeError(format!("{e:?}")))?;
      Ok(CsgGeometry {
        mesh,
        color: this.color,
      })
    });

    // Alias
    methods.add_method("rotateextrude", |_, this, args: mlua::MultiValue| {
      let angle = args.front().and_then(lua_val_to_f32).unwrap_or(360.0);
      let segments = args
        .get(1)
        .and_then(lua_val_to_f32)
        .map(|v| v as usize)
        .unwrap_or(32);
      let mesh = this
        .sketch
        .revolve(angle, segments)
        .map_err(|e| mlua::Error::RuntimeError(format!("{e:?}")))?;
      Ok(CsgGeometry {
        mesh,
        color: this.color,
      })
    });

    // --- Clone ---

    methods.add_method("clone", |_, this, ()| Ok(this.clone()));
    methods.add_method("copy", |_, this, ()| Ok(this.clone()));

    // --- Color ---

    methods.add_method("setcolor", |_, this, args: mlua::MultiValue| {
      let color = if args.len() >= 3 {
        let r = lua_val_to_f32(&args[0]).unwrap_or(1.0);
        let g = lua_val_to_f32(&args[1]).unwrap_or(1.0);
        let b = lua_val_to_f32(&args[2]).unwrap_or(1.0);
        [r, g, b]
      } else if let mlua::Value::String(s) = &args[0] {
        match s.to_str() {
          Ok(name) => named_color(&name).unwrap_or([1.0, 1.0, 1.0]),
          Err(_) => [1.0, 1.0, 1.0],
        }
      } else {
        [1.0, 1.0, 1.0]
      };
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: Some(color),
      })
    });

    // --- 2D Boolean operators ---

    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgSketch>()?;
        Ok(CsgSketch {
          sketch: this.sketch.union(&other_ref.sketch),
          color: this.color,
        })
      },
    );

    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgSketch>()?;
        Ok(CsgSketch {
          sketch: this.sketch.difference(&other_ref.sketch),
          color: this.color,
        })
      },
    );

    methods.add_meta_method(
      mlua::MetaMethod::Mul,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgSketch>()?;
        Ok(CsgSketch {
          sketch: this.sketch.intersection(&other_ref.sketch),
          color: this.color,
        })
      },
    );

    methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
      Ok(format!(
        "CsgSketch(geometries: {})",
        this.sketch.geometry.len()
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
