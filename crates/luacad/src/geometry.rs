use csgrs::mesh::Mesh as CsgMesh;
use csgrs::mesh::plane::Plane;
use csgrs::traits::CSG;
use mlua::{UserData, UserDataMethods, Value as LuaValue};
use nalgebra::{Matrix4, Vector3};
use std::sync::OnceLock;

use crate::scad_export::ScadNode;

/// Create an empty csgrs mesh with no polygons.
fn empty_mesh() -> CsgMesh<()> {
  CsgMesh {
    polygons: vec![],
    bounding_box: OnceLock::new(),
    metadata: None,
  }
}

fn table_get_f32(t: &mlua::Table, key: &str) -> Option<f32> {
  t.get::<mlua::Value>(key)
    .ok()
    .and_then(|v| lua_val_to_f32(&v))
}

fn table_get_bool(t: &mlua::Table, key: &str) -> bool {
  t.get::<bool>(key).unwrap_or(false)
}

fn table_get_u32(t: &mlua::Table, key: &str) -> Option<u32> {
  table_get_f32(t, key).map(|v| v as u32)
}

pub fn lua_val_to_f32(v: &mlua::Value) -> Option<f32> {
  match v {
    mlua::Value::Number(n) => Some(*n as f32),
    mlua::Value::Integer(n) => Some(*n as f32),
    _ => None,
  }
}

/// Resolve a CSS3/SVG named color (the same 147 colors supported by OpenSCAD).
fn named_color(name: &str) -> Option<[f32; 3]> {
  let c = |r: u8, g: u8, b: u8| {
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
  };
  match name.to_lowercase().as_str() {
    "aliceblue" => c(240, 248, 255),
    "antiquewhite" => c(250, 235, 215),
    "aqua" => c(0, 255, 255),
    "aquamarine" => c(127, 255, 212),
    "azure" => c(240, 255, 255),
    "beige" => c(245, 245, 220),
    "bisque" => c(255, 228, 196),
    "black" => c(0, 0, 0),
    "blanchedalmond" => c(255, 235, 205),
    "blue" => c(0, 0, 255),
    "blueviolet" => c(138, 43, 226),
    "brown" => c(165, 42, 42),
    "burlywood" => c(222, 184, 135),
    "cadetblue" => c(95, 158, 160),
    "chartreuse" => c(127, 255, 0),
    "chocolate" => c(210, 105, 30),
    "coral" => c(255, 127, 80),
    "cornflowerblue" => c(100, 149, 237),
    "cornsilk" => c(255, 248, 220),
    "crimson" => c(220, 20, 60),
    "cyan" => c(0, 255, 255),
    "darkblue" => c(0, 0, 139),
    "darkcyan" => c(0, 139, 139),
    "darkgoldenrod" => c(184, 134, 11),
    "darkgray" | "darkgrey" => c(169, 169, 169),
    "darkgreen" => c(0, 100, 0),
    "darkkhaki" => c(189, 183, 107),
    "darkmagenta" => c(139, 0, 139),
    "darkolivegreen" => c(85, 107, 47),
    "darkorange" => c(255, 140, 0),
    "darkorchid" => c(153, 50, 204),
    "darkred" => c(139, 0, 0),
    "darksalmon" => c(233, 150, 122),
    "darkseagreen" => c(143, 188, 143),
    "darkslateblue" => c(72, 61, 139),
    "darkslategray" | "darkslategrey" => c(47, 79, 79),
    "darkturquoise" => c(0, 206, 209),
    "darkviolet" => c(148, 0, 211),
    "deeppink" => c(255, 20, 147),
    "deepskyblue" => c(0, 191, 255),
    "dimgray" | "dimgrey" => c(105, 105, 105),
    "dodgerblue" => c(30, 144, 255),
    "firebrick" => c(178, 34, 34),
    "floralwhite" => c(255, 250, 240),
    "forestgreen" => c(34, 139, 34),
    "fuchsia" => c(255, 0, 255),
    "gainsboro" => c(220, 220, 220),
    "ghostwhite" => c(248, 248, 255),
    "gold" => c(255, 215, 0),
    "goldenrod" => c(218, 165, 32),
    "gray" | "grey" => c(128, 128, 128),
    "green" => c(0, 128, 0),
    "greenyellow" => c(173, 255, 47),
    "honeydew" => c(240, 255, 240),
    "hotpink" => c(255, 105, 180),
    "indianred" => c(205, 92, 92),
    "indigo" => c(75, 0, 130),
    "ivory" => c(255, 255, 240),
    "khaki" => c(240, 230, 140),
    "lavender" => c(230, 230, 250),
    "lavenderblush" => c(255, 240, 245),
    "lawngreen" => c(124, 252, 0),
    "lemonchiffon" => c(255, 250, 205),
    "lightblue" => c(173, 216, 230),
    "lightcoral" => c(240, 128, 128),
    "lightcyan" => c(224, 255, 255),
    "lightgoldenrodyellow" => c(250, 250, 210),
    "lightgray" | "lightgrey" => c(211, 211, 211),
    "lightgreen" => c(144, 238, 144),
    "lightpink" => c(255, 182, 193),
    "lightsalmon" => c(255, 160, 122),
    "lightseagreen" => c(32, 178, 170),
    "lightskyblue" => c(135, 206, 250),
    "lightslategray" | "lightslategrey" => c(119, 136, 153),
    "lightsteelblue" => c(176, 196, 222),
    "lightyellow" => c(255, 255, 224),
    "lime" => c(0, 255, 0),
    "limegreen" => c(50, 205, 50),
    "linen" => c(250, 240, 230),
    "magenta" => c(255, 0, 255),
    "maroon" => c(128, 0, 0),
    "mediumaquamarine" => c(102, 205, 170),
    "mediumblue" => c(0, 0, 205),
    "mediumorchid" => c(186, 85, 211),
    "mediumpurple" => c(147, 112, 219),
    "mediumseagreen" => c(60, 179, 113),
    "mediumslateblue" => c(123, 104, 238),
    "mediumspringgreen" => c(0, 250, 154),
    "mediumturquoise" => c(72, 209, 204),
    "mediumvioletred" => c(199, 21, 133),
    "midnightblue" => c(25, 25, 112),
    "mintcream" => c(245, 255, 250),
    "mistyrose" => c(255, 228, 225),
    "moccasin" => c(255, 228, 181),
    "navajowhite" => c(255, 222, 173),
    "navy" => c(0, 0, 128),
    "oldlace" => c(253, 245, 230),
    "olive" => c(128, 128, 0),
    "olivedrab" => c(107, 142, 35),
    "orange" => c(255, 165, 0),
    "orangered" => c(255, 69, 0),
    "orchid" => c(218, 112, 214),
    "palegoldenrod" => c(238, 232, 170),
    "palegreen" => c(152, 251, 152),
    "paleturquoise" => c(175, 238, 238),
    "palevioletred" => c(219, 112, 147),
    "papayawhip" => c(255, 239, 213),
    "peachpuff" => c(255, 218, 185),
    "peru" => c(205, 133, 63),
    "pink" => c(255, 192, 203),
    "plum" => c(221, 160, 221),
    "powderblue" => c(176, 224, 230),
    "purple" => c(128, 0, 128),
    "red" => c(255, 0, 0),
    "rosybrown" => c(188, 143, 143),
    "royalblue" => c(65, 105, 225),
    "saddlebrown" => c(139, 69, 19),
    "salmon" => c(250, 128, 114),
    "sandybrown" => c(244, 164, 96),
    "seagreen" => c(46, 139, 87),
    "seashell" => c(255, 245, 238),
    "sienna" => c(160, 82, 45),
    "silver" => c(192, 192, 192),
    "skyblue" => c(135, 206, 235),
    "slateblue" => c(106, 90, 205),
    "slategray" | "slategrey" => c(112, 128, 144),
    "snow" => c(255, 250, 250),
    "springgreen" => c(0, 255, 127),
    "steelblue" => c(70, 130, 180),
    "tan" => c(210, 180, 140),
    "teal" => c(0, 128, 128),
    "thistle" => c(216, 191, 216),
    "tomato" => c(255, 99, 71),
    "turquoise" => c(64, 224, 208),
    "violet" => c(238, 130, 238),
    "wheat" => c(245, 222, 179),
    "white" => c(255, 255, 255),
    "whitesmoke" => c(245, 245, 245),
    "yellow" => c(255, 255, 0),
    "yellowgreen" => c(154, 205, 50),
    _ => None,
  }
}

#[derive(Clone, Debug)]
pub struct CsgGeometry {
  /// The materialized mesh. `None` when the geometry was produced by a CSG
  /// boolean and we're deferring the expensive mesh computation until export.
  /// Call [`CsgGeometry::materialize`] to force evaluation.
  pub mesh: Option<CsgMesh<()>>,
  pub color: Option<[f32; 3]>,
  pub scad: Option<ScadNode>,
}

impl CsgGeometry {
  /// Ensure `self.mesh` is populated by evaluating the ScadNode tree if needed.
  /// After this call, `self.mesh` is guaranteed to be `Some`.
  pub fn materialize(&mut self) {
    if self.mesh.is_some() {
      return;
    }
    if let Some(ref scad) = self.scad {
      self.mesh = Some(materialize_scad(scad));
    }
  }

  /// Return a reference to the mesh, materializing it first if necessary.
  pub fn mesh(&mut self) -> &CsgMesh<()> {
    self.materialize();
    self.mesh.as_ref().unwrap()
  }

  /// Return a reference to the mesh if already materialized, without forcing evaluation.
  pub fn mesh_if_materialized(&self) -> Option<&CsgMesh<()>> {
    self.mesh.as_ref()
  }
}

/// Recursively evaluate a ScadNode tree into a csgrs mesh.
pub fn materialize_scad(node: &ScadNode) -> CsgMesh<()> {
  match node {
    // --- Leaf 3D primitives ---
    ScadNode::Cube { w, d, h, center } => {
      let m = CsgMesh::<()>::cuboid(*w, *d, *h, None);
      if *center {
        m.translate(-w / 2.0, -d / 2.0, -h / 2.0)
      } else {
        m
      }
    }
    ScadNode::Sphere { r, segments } => {
      CsgMesh::<()>::sphere(*r, *segments as usize, *segments as usize, None)
    }
    ScadNode::Cylinder {
      r1,
      r2,
      h,
      segments,
      center,
    } => {
      let m = CsgMesh::<()>::frustum(*r1, *r2, *h, *segments as usize, None);
      if *center {
        m.translate(0.0, 0.0, -h / 2.0)
      } else {
        m
      }
    }
    ScadNode::Polyhedron { points, faces } => {
      let face_refs: Vec<&[usize]> =
        faces.iter().map(|f| f.as_slice()).collect();
      CsgMesh::<()>::polyhedron(points, &face_refs, None)
        .unwrap_or_else(|_| empty_mesh())
    }

    // --- CSG booleans ---
    ScadNode::Union(children) => {
      let mut iter = children.iter();
      let first = iter.next().map(materialize_scad).unwrap_or_else(empty_mesh);
      iter.fold(first, |acc, child| acc.union(&materialize_scad(child)))
    }
    ScadNode::Difference(children) => {
      let mut iter = children.iter();
      let first = iter.next().map(materialize_scad).unwrap_or_else(empty_mesh);
      iter.fold(first, |acc, child| acc.difference(&materialize_scad(child)))
    }
    ScadNode::Intersection(children) => {
      let mut iter = children.iter();
      let first = iter.next().map(materialize_scad).unwrap_or_else(empty_mesh);
      iter.fold(first, |acc, child| {
        acc.intersection(&materialize_scad(child))
      })
    }
    ScadNode::Hull(child) => materialize_scad(child).convex_hull(),
    ScadNode::Minkowski(children) => {
      let mut iter = children.iter();
      let first = iter.next().map(materialize_scad).unwrap_or_else(empty_mesh);
      iter.fold(first, |acc, child| {
        acc.minkowski_sum(&materialize_scad(child))
      })
    }

    // --- Transforms ---
    ScadNode::Translate { x, y, z, child } => {
      materialize_scad(child).translate(*x, *y, *z)
    }
    ScadNode::Rotate { x, y, z, child } => {
      materialize_scad(child).rotate(*x, *y, *z)
    }
    ScadNode::Scale { x, y, z, child } => {
      materialize_scad(child).scale(*x, *y, *z)
    }
    ScadNode::Mirror { x, y, z, child } => {
      let plane = Plane::from_normal(Vector3::new(*x, *y, *z), 0.0);
      materialize_scad(child).mirror(plane)
    }
    ScadNode::Multmatrix { matrix, child } => {
      #[rustfmt::skip]
      let mat = Matrix4::new(
        matrix[0],  matrix[1],  matrix[2],  matrix[3],
        matrix[4],  matrix[5],  matrix[6],  matrix[7],
        matrix[8],  matrix[9],  matrix[10], matrix[11],
        matrix[12], matrix[13], matrix[14], matrix[15],
      );
      materialize_scad(child).transform(&mat)
    }
    ScadNode::Resize { x, y, z, child } => {
      let mesh = materialize_scad(child);
      let bb = mesh.bounding_box();
      let cur_w = bb.maxs.x - bb.mins.x;
      let cur_d = bb.maxs.y - bb.mins.y;
      let cur_h = bb.maxs.z - bb.mins.z;
      let sx = if *x > 0.0 && cur_w > 1e-9 {
        x / cur_w
      } else {
        1.0
      };
      let sy = if *y > 0.0 && cur_d > 1e-9 {
        y / cur_d
      } else {
        1.0
      };
      let sz = if *z > 0.0 && cur_h > 1e-9 {
        z / cur_h
      } else {
        1.0
      };
      mesh.scale(sx, sy, sz)
    }

    // --- Color / modifiers / render: pass through ---
    ScadNode::Color { child, .. }
    | ScadNode::Render { child, .. }
    | ScadNode::Modifier { child, .. } => materialize_scad(child),

    // --- Extrusions: these produce geometry but need sketch data ---
    // Cannot be reconstructed from ScadNode alone (we don't store the sketch mesh).
    // These should never appear with mesh: None because extrusions produce a
    // concrete mesh at creation time. Return empty as a safe fallback.
    ScadNode::LinearExtrude { .. }
    | ScadNode::RotateExtrude { .. }
    | ScadNode::Projection { .. } => empty_mesh(),

    // --- 2D / text / file ops: no 3D mesh ---
    _ => empty_mesh(),
  }
}

impl UserData for CsgGeometry {
  fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    // --- Transformations ---

    methods.add_method(
      "translate",
      |_, this, (x, y, z): (f32, f32, Option<f32>)| {
        let z = z.unwrap_or(0.0);
        let scad = this.scad.as_ref().map(|s| ScadNode::Translate {
          x,
          y,
          z,
          child: Box::new(s.clone()),
        });
        Ok(CsgGeometry {
          mesh: this.mesh.as_ref().map(|m| m.translate(x, y, z)),
          color: this.color,
          scad,
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
        let mesh = this.mesh.as_ref().map(|m| {
          m.translate(-cx, -cy, -cz)
            .rotate(rx, ry, rz)
            .translate(cx, cy, cz)
        });
        let scad = this.scad.as_ref().map(|s| ScadNode::Translate {
          x: cx,
          y: cy,
          z: cz,
          child: Box::new(ScadNode::Rotate {
            x: rx,
            y: ry,
            z: rz,
            child: Box::new(ScadNode::Translate {
              x: -cx,
              y: -cy,
              z: -cz,
              child: Box::new(s.clone()),
            }),
          }),
        });
        Ok(CsgGeometry {
          mesh,
          color: this.color,
          scad,
        })
      } else {
        // rotate(rx, ry, rz)
        let rx = lua_val_to_f32(args.front().unwrap_or(&mlua::Value::Nil))
          .unwrap_or(0.0);
        let ry = lua_val_to_f32(args.get(1).unwrap_or(&mlua::Value::Nil))
          .unwrap_or(0.0);
        let rz = lua_val_to_f32(args.get(2).unwrap_or(&mlua::Value::Nil))
          .unwrap_or(0.0);
        let scad = this.scad.as_ref().map(|s| ScadNode::Rotate {
          x: rx,
          y: ry,
          z: rz,
          child: Box::new(s.clone()),
        });
        Ok(CsgGeometry {
          mesh: this.mesh.as_ref().map(|m| m.rotate(rx, ry, rz)),
          color: this.color,
          scad,
        })
      }
    });

    methods.add_method(
      "rotate2d",
      |_, this, (x, y, angle): (f32, f32, f32)| {
        let mesh = this.mesh.as_ref().map(|m| {
          m.translate(-x, -y, 0.0)
            .rotate(0.0, 0.0, angle)
            .translate(x, y, 0.0)
        });
        let scad = this.scad.as_ref().map(|s| ScadNode::Translate {
          x,
          y,
          z: 0.0,
          child: Box::new(ScadNode::Rotate {
            x: 0.0,
            y: 0.0,
            z: angle,
            child: Box::new(ScadNode::Translate {
              x: -x,
              y: -y,
              z: 0.0,
              child: Box::new(s.clone()),
            }),
          }),
        });
        Ok(CsgGeometry {
          mesh,
          color: this.color,
          scad,
        })
      },
    );

    methods.add_method("scale", |_, this, (sx, sy, sz): (f32, f32, f32)| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Scale {
        x: sx,
        y: sy,
        z: sz,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.as_ref().map(|m| m.scale(sx, sy, sz)),
        color: this.color,
        scad,
      })
    });

    methods.add_method_mut("resize", |_, this, (x, y, z): (f32, f32, f32)| {
      // resize() needs the bounding box, so materialize if needed
      let mesh = this.mesh();
      let bb = mesh.bounding_box();
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

      let scad = this.scad.as_ref().map(|s| ScadNode::Resize {
        x,
        y,
        z,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.as_ref().map(|m| m.scale(sx, sy, sz)),
        color: this.color,
        scad,
      })
    });

    methods.add_method("mirror", |_, this, (x, y, z): (f32, f32, f32)| {
      let len_sq = x * x + y * y + z * z;
      if len_sq < 1e-12 {
        return Ok(this.clone());
      }
      let plane = Plane::from_normal(Vector3::new(x, y, z), 0.0);
      let scad = this.scad.as_ref().map(|s| ScadNode::Mirror {
        x,
        y,
        z,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.as_ref().map(|m| m.mirror(plane)),
        color: this.color,
        scad,
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
      let scad = this.scad.as_ref().map(|s| {
        let mut arr = [0.0f32; 16];
        arr.copy_from_slice(&vals);
        ScadNode::Multmatrix {
          matrix: arr,
          child: Box::new(s.clone()),
        }
      });
      Ok(CsgGeometry {
        mesh: this.mesh.as_ref().map(|m| m.transform(&mat)),
        color: this.color,
        scad,
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
      let scad = this.scad.as_ref().map(|s| ScadNode::Color {
        r: color[0],
        g: color[1],
        b: color[2],
        a: 1.0,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: Some(color),
        scad,
      })
    });

    // --- color() alias for setcolor() with alpha support ---

    methods.add_method("color", |_, this, args: mlua::MultiValue| {
      let (color, alpha) = if args.len() >= 3 {
        let r = lua_val_to_f32(&args[0]).unwrap_or(1.0);
        let g = lua_val_to_f32(&args[1]).unwrap_or(1.0);
        let b = lua_val_to_f32(&args[2]).unwrap_or(1.0);
        let a = args.get(3).and_then(lua_val_to_f32).unwrap_or(1.0);
        ([r, g, b], a)
      } else if let mlua::Value::Table(t) = &args[0] {
        let r: f32 = t.get::<f32>(1).unwrap_or(1.0);
        let g: f32 = t.get::<f32>(2).unwrap_or(1.0);
        let b: f32 = t.get::<f32>(3).unwrap_or(1.0);
        let a: f32 = t.get::<f32>(4).unwrap_or(1.0);
        ([r, g, b], a)
      } else if let mlua::Value::String(s) = &args[0] {
        let color = match s.to_str() {
          Ok(name) => named_color(&name).unwrap_or([1.0, 1.0, 1.0]),
          Err(_) => [1.0, 1.0, 1.0],
        };
        let a = args.get(1).and_then(lua_val_to_f32).unwrap_or(1.0);
        (color, a)
      } else {
        ([1.0, 1.0, 1.0], 1.0)
      };
      let scad = this.scad.as_ref().map(|s| ScadNode::Color {
        r: color[0],
        g: color[1],
        b: color[2],
        a: alpha,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: Some(color),
        scad,
      })
    });

    // --- Projection (3D → 2D, ScadNode-only) ---

    methods.add_method("projection", |_, this, cut: Option<bool>| {
      let cut = cut.unwrap_or(false);
      let scad = this.scad.as_ref().map(|s| ScadNode::Projection {
        cut,
        child: Box::new(s.clone()),
      });
      // Projection produces a 2D result; keep mesh as-is for viewport
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: this.color,
        scad,
      })
    });

    // --- Render with convexity (ScadNode wrapper) ---

    methods.add_method("render_node", |_, this, convexity: Option<u32>| {
      let convexity = convexity.unwrap_or(0);
      let scad = this.scad.as_ref().map(|s| ScadNode::Render {
        convexity,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: this.color,
        scad,
      })
    });

    // --- Modifier methods ---

    methods.add_method("skip", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Skip,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: this.color,
        scad,
      })
    });

    methods.add_method("only", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Only,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: this.color,
        scad,
      })
    });

    methods.add_method("debug", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Debug,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: this.color,
        scad,
      })
    });

    methods.add_method("transparent", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Transparent,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: this.mesh.clone(),
        color: this.color,
        scad,
      })
    });

    // --- Multi-object booleans (lazy: only build ScadNode tree) ---

    methods.add_method(
      "add",
      |_, this, args: mlua::Variadic<mlua::AnyUserData>| {
        let mut scad_children: Vec<ScadNode> = Vec::new();
        if let Some(s) = &this.scad {
          scad_children.push(s.clone());
        }
        for ud in args.iter() {
          let other = ud.borrow::<CsgGeometry>()?;
          if let Some(s) = &other.scad {
            scad_children.push(s.clone());
          }
        }
        let scad = if scad_children.is_empty() {
          None
        } else {
          Some(ScadNode::Union(scad_children))
        };
        Ok(CsgGeometry {
          mesh: None,
          color: this.color,
          scad,
        })
      },
    );

    methods.add_method(
      "sub",
      |_, this, args: mlua::Variadic<mlua::AnyUserData>| {
        let mut scad_children: Vec<ScadNode> = Vec::new();
        if let Some(s) = &this.scad {
          scad_children.push(s.clone());
        }
        for ud in args.iter() {
          let other = ud.borrow::<CsgGeometry>()?;
          if let Some(s) = &other.scad {
            scad_children.push(s.clone());
          }
        }
        let scad = if scad_children.is_empty() {
          None
        } else {
          Some(ScadNode::Difference(scad_children))
        };
        Ok(CsgGeometry {
          mesh: None,
          color: this.color,
          scad,
        })
      },
    );

    methods.add_method(
      "intersect",
      |_, this, args: mlua::Variadic<mlua::AnyUserData>| {
        let mut scad_children: Vec<ScadNode> = Vec::new();
        if let Some(s) = &this.scad {
          scad_children.push(s.clone());
        }
        for ud in args.iter() {
          let other = ud.borrow::<CsgGeometry>()?;
          if let Some(s) = &other.scad {
            scad_children.push(s.clone());
          }
        }
        let scad = if scad_children.is_empty() {
          None
        } else {
          Some(ScadNode::Intersection(scad_children))
        };
        Ok(CsgGeometry {
          mesh: None,
          color: this.color,
          scad,
        })
      },
    );

    // --- Hull + Minkowski (must materialize: inherently need mesh) ---

    methods.add_method_mut("hull", |_, this, ()| {
      let scad = this
        .scad
        .as_ref()
        .map(|s| ScadNode::Hull(Box::new(s.clone())));
      let mesh = this.mesh();
      Ok(CsgGeometry {
        mesh: Some(mesh.convex_hull()),
        color: this.color,
        scad,
      })
    });

    methods.add_method_mut("minkowski", |_, this, other: mlua::AnyUserData| {
      let mut other_ref = other.borrow_mut::<CsgGeometry>()?;
      let mut children = Vec::new();
      if let Some(s) = &this.scad {
        children.push(s.clone());
      }
      if let Some(s) = &other_ref.scad {
        children.push(s.clone());
      }
      let scad = if children.is_empty() {
        None
      } else {
        Some(ScadNode::Minkowski(children))
      };
      let this_mesh = this.mesh();
      let other_mesh = other_ref.mesh();
      Ok(CsgGeometry {
        mesh: Some(this_mesh.minkowski_sum(other_mesh)),
        color: this.color,
        scad,
      })
    });

    // --- CSG operators (lazy: only build ScadNode tree) ---

    // CSG union: a + b
    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        let scad = match (&this.scad, &other_ref.scad) {
          (Some(a), Some(b)) => {
            Some(ScadNode::Union(vec![a.clone(), b.clone()]))
          }
          _ => None,
        };
        Ok(CsgGeometry {
          mesh: None,
          color: this.color,
          scad,
        })
      },
    );

    // CSG difference: a - b
    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        let scad = match (&this.scad, &other_ref.scad) {
          (Some(a), Some(b)) => {
            Some(ScadNode::Difference(vec![a.clone(), b.clone()]))
          }
          _ => None,
        };
        Ok(CsgGeometry {
          mesh: None,
          color: this.color,
          scad,
        })
      },
    );

    // CSG intersection: a * b
    methods.add_meta_method(
      mlua::MetaMethod::Mul,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgGeometry>()?;
        let scad = match (&this.scad, &other_ref.scad) {
          (Some(a), Some(b)) => {
            Some(ScadNode::Intersection(vec![a.clone(), b.clone()]))
          }
          _ => None,
        };
        Ok(CsgGeometry {
          mesh: None,
          color: this.color,
          scad,
        })
      },
    );

    methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
      let color_str = this
        .color
        .map(|c| format!(", color: [{:.2},{:.2},{:.2}]", c[0], c[1], c[2]))
        .unwrap_or_default();
      let poly_count =
        this.mesh.as_ref().map(|m| m.polygons.len()).unwrap_or(0);
      Ok(format!(
        "CsgGeometry(polygons: {}{}, lazy: {})",
        poly_count,
        color_str,
        this.mesh.is_none()
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
  pub scad: Option<ScadNode>,
}

impl UserData for CsgSketch {
  fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    // --- Transformations ---

    methods.add_method("translate", |_, this, (x, y): (f32, f32)| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Translate {
        x,
        y,
        z: 0.0,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.translate(x, y, 0.0),
        color: this.color,
        scad,
      })
    });

    methods.add_method("rotate", |_, this, angle: f32| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Rotate {
        x: 0.0,
        y: 0.0,
        z: angle,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.rotate(0.0, 0.0, angle),
        color: this.color,
        scad,
      })
    });

    methods.add_method("scale", |_, this, (sx, sy): (f32, f32)| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Scale {
        x: sx,
        y: sy,
        z: 1.0,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.scale(sx, sy, 1.0),
        color: this.color,
        scad,
      })
    });

    methods.add_method("mirror", |_, this, (x, y): (f32, f32)| {
      let len_sq = x * x + y * y;
      if len_sq < 1e-12 {
        return Ok(this.clone());
      }
      let plane = Plane::from_normal(Vector3::new(x, y, 0.0), 0.0);
      let scad = this.scad.as_ref().map(|s| ScadNode::Mirror {
        x,
        y,
        z: 0.0,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.mirror(plane),
        color: this.color,
        scad,
      })
    });

    methods.add_method(
      "offset",
      |_, this, (d, chamfer): (f32, Option<bool>)| {
        let chamfer = chamfer.unwrap_or(false);
        let scad = this.scad.as_ref().map(|s| ScadNode::Offset {
          delta: Some(d),
          r: None,
          chamfer,
          child: Box::new(s.clone()),
        });
        Ok(CsgSketch {
          sketch: this.sketch.offset(d),
          color: this.color,
          scad,
        })
      },
    );

    methods.add_method(
      "offsetradius",
      |_, this, (r, chamfer): (f32, Option<bool>)| {
        let chamfer = chamfer.unwrap_or(false);
        let scad = this.scad.as_ref().map(|s| ScadNode::Offset {
          delta: None,
          r: Some(r),
          chamfer,
          child: Box::new(s.clone()),
        });
        Ok(CsgSketch {
          sketch: this.sketch.offset(r),
          color: this.color,
          scad,
        })
      },
    );

    // --- Hull + Minkowski ---

    // hull() — ScadNode-only on 2D sketches (no native convex hull for Sketch)
    methods.add_method("hull", |_, this, ()| {
      let scad = this
        .scad
        .as_ref()
        .map(|s| ScadNode::Hull(Box::new(s.clone())));
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: this.color,
        scad,
      })
    });

    // minkowski() — ScadNode-only on 2D sketches
    methods.add_method("minkowski", |_, this, other: mlua::AnyUserData| {
      let other_ref = other.borrow::<CsgSketch>()?;
      let mut children = Vec::new();
      if let Some(s) = &this.scad {
        children.push(s.clone());
      }
      if let Some(s) = &other_ref.scad {
        children.push(s.clone());
      }
      let scad = if children.is_empty() {
        None
      } else {
        Some(ScadNode::Minkowski(children))
      };
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: this.color,
        scad,
      })
    });

    // --- Extrusion ---

    methods.add_method("linear_extrude", |_, this, args: mlua::MultiValue| {
      let (height, center, twist, slices, scale) =
        if let Some(LuaValue::Table(t)) = args.front() {
          let h = table_get_f32(t, "height")
            .or_else(|| table_get_f32(t, "h"))
            .or_else(|| t.get::<f32>(1).ok())
            .unwrap_or(1.0);
          let center = table_get_bool(t, "center");
          let twist = table_get_f32(t, "twist").unwrap_or(0.0);
          let slices = table_get_u32(t, "slices").unwrap_or(0);
          let scale = table_get_f32(t, "scale").unwrap_or(1.0);
          (h, center, twist, slices, scale)
        } else {
          let h = args.front().and_then(lua_val_to_f32).unwrap_or(1.0);
          (h, false, 0.0, 0, 1.0)
        };
      let mesh = this.sketch.extrude(height);
      let mesh = if center {
        use csgrs::traits::CSG;
        mesh.translate(0.0, 0.0, -height / 2.0)
      } else {
        mesh
      };
      let scad = this.scad.as_ref().map(|s| ScadNode::LinearExtrude {
        height,
        center,
        twist,
        slices,
        scale,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: Some(mesh),
        color: this.color,
        scad,
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
      let scad = this.scad.as_ref().map(|s| ScadNode::RotateExtrude {
        angle,
        segments: segments as u32,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: Some(mesh),
        color: this.color,
        scad,
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
      let scad = this.scad.as_ref().map(|s| ScadNode::RotateExtrude {
        angle,
        segments: segments as u32,
        child: Box::new(s.clone()),
      });
      Ok(CsgGeometry {
        mesh: Some(mesh),
        color: this.color,
        scad,
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
        scad: this.scad.clone(),
      })
    });

    // --- color() alias with alpha ---

    methods.add_method("color", |_, this, args: mlua::MultiValue| {
      let (color, alpha) = if args.len() >= 3 {
        let r = lua_val_to_f32(&args[0]).unwrap_or(1.0);
        let g = lua_val_to_f32(&args[1]).unwrap_or(1.0);
        let b = lua_val_to_f32(&args[2]).unwrap_or(1.0);
        let a = args.get(3).and_then(lua_val_to_f32).unwrap_or(1.0);
        ([r, g, b], a)
      } else if let mlua::Value::String(s) = &args[0] {
        let c = match s.to_str() {
          Ok(name) => named_color(&name).unwrap_or([1.0, 1.0, 1.0]),
          Err(_) => [1.0, 1.0, 1.0],
        };
        let a = args.get(1).and_then(lua_val_to_f32).unwrap_or(1.0);
        (c, a)
      } else {
        ([1.0, 1.0, 1.0], 1.0)
      };
      let scad = this.scad.as_ref().map(|s| ScadNode::Color {
        r: color[0],
        g: color[1],
        b: color[2],
        a: alpha,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: Some(color),
        scad,
      })
    });

    // --- Modifier methods ---

    methods.add_method("skip", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Skip,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: this.color,
        scad,
      })
    });

    methods.add_method("only", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Only,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: this.color,
        scad,
      })
    });

    methods.add_method("debug", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Debug,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: this.color,
        scad,
      })
    });

    methods.add_method("transparent", |_, this, ()| {
      let scad = this.scad.as_ref().map(|s| ScadNode::Modifier {
        kind: crate::scad_export::ModifierKind::Transparent,
        child: Box::new(s.clone()),
      });
      Ok(CsgSketch {
        sketch: this.sketch.clone(),
        color: this.color,
        scad,
      })
    });

    // --- 2D Boolean operators ---

    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgSketch>()?;
        let scad = match (&this.scad, &other_ref.scad) {
          (Some(a), Some(b)) => {
            Some(ScadNode::Union(vec![a.clone(), b.clone()]))
          }
          _ => None,
        };
        Ok(CsgSketch {
          sketch: this.sketch.union(&other_ref.sketch),
          color: this.color,
          scad,
        })
      },
    );

    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgSketch>()?;
        let scad = match (&this.scad, &other_ref.scad) {
          (Some(a), Some(b)) => {
            Some(ScadNode::Difference(vec![a.clone(), b.clone()]))
          }
          _ => None,
        };
        Ok(CsgSketch {
          sketch: this.sketch.difference(&other_ref.sketch),
          color: this.color,
          scad,
        })
      },
    );

    methods.add_meta_method(
      mlua::MetaMethod::Mul,
      |_, this, other: mlua::AnyUserData| {
        let other_ref = other.borrow::<CsgSketch>()?;
        let scad = match (&this.scad, &other_ref.scad) {
          (Some(a), Some(b)) => {
            Some(ScadNode::Intersection(vec![a.clone(), b.clone()]))
          }
          _ => None,
        };
        Ok(CsgSketch {
          sketch: this.sketch.intersection(&other_ref.sketch),
          color: this.color,
          scad,
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
