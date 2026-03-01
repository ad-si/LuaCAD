#[cfg(feature = "csgrs")]
use csgrs::mesh::Mesh as CsgMesh;
#[cfg(feature = "csgrs")]
use csgrs::traits::CSG;
use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::geometry::{CsgGeometry, CsgSketch, lua_val_to_f32};
use crate::scad_export::ScadNode;

// ---------------------------------------------------------------------------
// Table helpers
// ---------------------------------------------------------------------------

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

/// Get segments from table: checks "segments" and "fn" keys, returns default otherwise.
fn table_segments(t: &mlua::Table, default: u32) -> u32 {
  table_get_u32(t, "segments")
    .or_else(|| table_get_u32(t, "fn"))
    .unwrap_or(default)
}

// ---------------------------------------------------------------------------
// cube() argument parsing
// ---------------------------------------------------------------------------

/// Parse cube() arguments: supports all LuaCAD forms.
/// Returns (w, d, h, center).
fn parse_cube_args(
  args: &mlua::MultiValue,
) -> mlua::Result<(f32, f32, f32, bool)> {
  if args.is_empty() {
    return Err(mlua::Error::RuntimeError(
      "cube() requires at least 1 argument".to_string(),
    ));
  }

  let first = &args[0];

  if let LuaValue::Table(t) = first {
    // Check for "size" named key: cube { size = {w,d,h}, center = true }
    if let Ok(LuaValue::Table(size_t)) = t.get::<mlua::Value>("size") {
      let w: f32 = size_t.get::<f32>(1).unwrap_or(1.0);
      let d: f32 = size_t.get::<f32>(2).unwrap_or(1.0);
      let h: f32 = size_t.get::<f32>(3).unwrap_or(1.0);
      let center = table_get_bool(t, "center");
      return Ok((w, d, h, center));
    }

    // Check if first element is a nested table: cube { {w,d,h}, center = true }
    if let Ok(LuaValue::Table(inner)) = t.get::<mlua::Value>(1) {
      let w: f32 = inner.get::<f32>(1).unwrap_or(1.0);
      let d: f32 = inner.get::<f32>(2).unwrap_or(1.0);
      let h: f32 = inner.get::<f32>(3).unwrap_or(1.0);
      let center = table_get_bool(t, "center");
      return Ok((w, d, h, center));
    }

    // Array form: cube { w, d, h } (with optional center)
    let w: f32 = t.get::<f32>(1).unwrap_or(1.0);
    let d: f32 = t.get::<f32>(2).unwrap_or(1.0);
    let h: f32 = t.get::<f32>(3).unwrap_or(1.0);
    let center = table_get_bool(t, "center");
    return Ok((w, d, h, center));
  }

  // Number forms
  let s = lua_val_to_f32(first).ok_or_else(|| {
    mlua::Error::RuntimeError(
      "cube() argument must be a number, three numbers, or a table".to_string(),
    )
  })?;

  // cube(w, d, h) — three separate number args
  if args.len() >= 3 {
    let d = lua_val_to_f32(&args[1]).unwrap_or(s);
    let h = lua_val_to_f32(&args[2]).unwrap_or(s);
    return Ok((s, d, h, false));
  }

  // cube(size) — uniform
  Ok((s, s, s, false))
}

// ---------------------------------------------------------------------------
// sphere() argument parsing
// ---------------------------------------------------------------------------

/// Returns (radius, segments).
fn parse_sphere_args(args: &mlua::MultiValue) -> mlua::Result<(f32, u32)> {
  if args.is_empty() {
    return Err(mlua::Error::RuntimeError(
      "sphere() requires at least 1 argument".to_string(),
    ));
  }

  let first = &args[0];

  if let LuaValue::Table(t) = first {
    let radius = if let Some(r) = table_get_f32(t, "r") {
      r
    } else if let Some(d) = table_get_f32(t, "d") {
      d / 2.0
    } else {
      t.get::<f32>(1).unwrap_or(1.0)
    };
    let segments = table_segments(t, 16);
    return Ok((radius, segments));
  }

  let radius = lua_val_to_f32(first).ok_or_else(|| {
    mlua::Error::RuntimeError(
      "sphere() argument must be a number or {r=..} table".to_string(),
    )
  })?;
  let segments = args
    .get(1)
    .and_then(lua_val_to_f32)
    .map(|v| v as u32)
    .unwrap_or(16);
  Ok((radius, segments))
}

// ---------------------------------------------------------------------------
// cylinder() argument parsing
// ---------------------------------------------------------------------------

/// Returns (r1, r2, height, segments, center).
fn parse_cylinder_args(
  args: &mlua::MultiValue,
) -> mlua::Result<(f32, f32, f32, u32, bool)> {
  if args.is_empty() {
    return Err(mlua::Error::RuntimeError(
      "cylinder() requires at least 1 argument".to_string(),
    ));
  }

  let first = &args[0];

  if let LuaValue::Table(t) = first {
    let h = table_get_f32(t, "h")
      .or_else(|| table_get_f32(t, "height"))
      .unwrap_or(1.0);

    let (r1, r2) = if let Some(r1) = table_get_f32(t, "r1") {
      let r2 = table_get_f32(t, "r2").unwrap_or(r1);
      (r1, r2)
    } else if let Some(d1) = table_get_f32(t, "d1") {
      let d2 = table_get_f32(t, "d2").unwrap_or(d1);
      (d1 / 2.0, d2 / 2.0)
    } else if let Some(r) = table_get_f32(t, "r") {
      (r, r)
    } else if let Some(d) = table_get_f32(t, "d") {
      (d / 2.0, d / 2.0)
    } else {
      (0.5, 0.5)
    };

    let segments = table_segments(t, 16);
    let center = table_get_bool(t, "center");
    return Ok((r1, r2, h, segments, center));
  }

  // Positional: cylinder(radius, height [, segments])
  let r = lua_val_to_f32(first).ok_or_else(|| {
    mlua::Error::RuntimeError(
      "cylinder() argument must be a number or {h=.., r=..} table".to_string(),
    )
  })?;
  let h = args.get(1).and_then(lua_val_to_f32).unwrap_or(1.0);
  let segments = args
    .get(2)
    .and_then(lua_val_to_f32)
    .map(|v| v as u32)
    .unwrap_or(16);
  Ok((r, r, h, segments, false))
}

// ---------------------------------------------------------------------------
// Lua environment setup and execution
// ---------------------------------------------------------------------------

/// Execute LuaCAD code and return the resulting geometries.
///
/// Returns `Ok(geometries)` on success, or `Err(message)` on failure.
pub fn execute_lua(code: &str) -> Result<Vec<CsgGeometry>, String> {
  let lua = Lua::new();
  let collector =
    std::rc::Rc::new(std::cell::RefCell::new(Vec::<CsgGeometry>::new()));

  let result: LuaResult<mlua::MultiValue> = (|| {
    // ---- print() ----
    let print_fn =
      lua.create_function(|_, args: mlua::Variadic<mlua::Value>| {
        let output = args
          .iter()
          .map(|v| format!("{v:?}"))
          .collect::<Vec<_>>()
          .join("\t");
        println!("Lua output: {output}");
        Ok(())
      })?;
    lua.globals().set("print", print_fn)?;

    // ==================================================================
    // 3D PRIMITIVES
    // ==================================================================

    // ---- cube() ----
    let cube_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let (w, d, h, center) = parse_cube_args(&args)?;
      let scad = Some(ScadNode::Cube { w, d, h, center });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            let mesh = CsgMesh::<()>::cuboid(w, d, h, None);
            Some(if center {
              mesh.translate(-w / 2.0, -d / 2.0, -h / 2.0)
            } else {
              mesh
            })
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("cube", cube_fn)?;

    // ---- sphere() ----
    let sphere_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let (radius, segments) = parse_sphere_args(&args)?;
      let scad = Some(ScadNode::Sphere {
        r: radius,
        segments,
      });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::sphere(
              radius,
              segments as usize,
              segments as usize,
              None,
            ))
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("sphere", sphere_fn)?;

    // ---- cylinder() ----
    let cylinder_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let (r1, r2, h, segments, center) = parse_cylinder_args(&args)?;
      let scad = Some(ScadNode::Cylinder {
        r1,
        r2,
        h,
        segments,
        center,
      });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            let mesh =
              CsgMesh::<()>::frustum(r1, r2, h, segments as usize, None);
            Some(if center {
              mesh.translate(0.0, 0.0, -h / 2.0)
            } else {
              mesh
            })
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("cylinder", cylinder_fn)?;

    // ---- polyhedron() ----
    let polyhedron_fn = lua.create_function(|_, args: mlua::MultiValue| {
      // polyhedron { points = {...}, faces = {...} }
      // or polyhedron(x, y, z, points, faces)  (legacy)
      let first = args
        .front()
        .ok_or_else(|| {
          mlua::Error::RuntimeError(
            "polyhedron() requires arguments".to_string(),
          )
        })?
        .clone();

      let (tx, ty, tz, points_table, faces_table) =
        if let LuaValue::Table(t) = &first {
          if let Ok(LuaValue::Table(pts)) = t.get::<mlua::Value>("points") {
            let faces: mlua::Table =
              t.get::<mlua::Table>("faces").map_err(|_| {
                mlua::Error::RuntimeError(
                  "polyhedron() requires 'faces' parameter".to_string(),
                )
              })?;
            (0.0, 0.0, 0.0, pts, faces)
          } else {
            return Err(mlua::Error::RuntimeError(
              "polyhedron() table must have 'points' key".to_string(),
            ));
          }
        } else if args.len() >= 5 {
          // Legacy: polyhedron(x, y, z, points, faces)
          let tx = lua_val_to_f32(&args[0]).unwrap_or(0.0);
          let ty = lua_val_to_f32(&args[1]).unwrap_or(0.0);
          let tz = lua_val_to_f32(&args[2]).unwrap_or(0.0);
          let pts = match &args[3] {
            LuaValue::Table(t) => t.clone(),
            _ => {
              return Err(mlua::Error::RuntimeError(
                "polyhedron() 4th arg must be points table".to_string(),
              ));
            }
          };
          let fcs = match &args[4] {
            LuaValue::Table(t) => t.clone(),
            _ => {
              return Err(mlua::Error::RuntimeError(
                "polyhedron() 5th arg must be faces table".to_string(),
              ));
            }
          };
          (tx, ty, tz, pts, fcs)
        } else {
          return Err(mlua::Error::RuntimeError(
          "polyhedron() requires {points=.., faces=..} or (x,y,z,points,faces)"
            .to_string(),
        ));
        };

      // Parse points as [f32; 3] arrays
      let mut points: Vec<[f32; 3]> = Vec::new();
      for i in 1..=points_table.len()? {
        let pt: mlua::Table = points_table.get(i)?;
        let x: f32 = pt.get::<f32>(1).unwrap_or(0.0);
        let y: f32 = pt.get::<f32>(2).unwrap_or(0.0);
        let z: f32 = pt.get::<f32>(3).unwrap_or(0.0);
        points.push([x, y, z]);
      }

      // Parse faces (0-indexed in Lua, matching OpenSCAD convention)
      let mut faces: Vec<Vec<usize>> = Vec::new();
      for i in 1..=faces_table.len()? {
        let face: mlua::Table = faces_table.get(i)?;
        let mut indices: Vec<usize> = Vec::new();
        for j in 1..=face.len()? {
          let idx: usize = face.get::<usize>(j)?;
          indices.push(idx);
        }
        faces.push(indices);
      }

      let face_refs: Vec<&[usize]> =
        faces.iter().map(|f| f.as_slice()).collect();
      let scad_base = ScadNode::Polyhedron {
        points: points.clone(),
        faces: faces.clone(),
      };
      let scad = if tx != 0.0 || ty != 0.0 || tz != 0.0 {
        Some(ScadNode::Translate {
          x: tx,
          y: ty,
          z: tz,
          child: Box::new(scad_base),
        })
      } else {
        Some(scad_base)
      };
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            let mesh = CsgMesh::<()>::polyhedron(&points, &face_refs, None)
              .map_err(|e| {
                mlua::Error::RuntimeError(format!("polyhedron() error: {e:?}"))
              })?;
            Some(if tx != 0.0 || ty != 0.0 || tz != 0.0 {
              mesh.translate(tx, ty, tz)
            } else {
              mesh
            })
          }
          #[cfg(not(feature = "csgrs"))]
          {
            let _ = &face_refs;
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("polyhedron", polyhedron_fn)?;

    // ---- pyramid() ----
    let pyramid_fn = lua.create_function(
      |_, (x, y, z, length, height): (f32, f32, f32, f32, f32)| {
        let points: Vec<[f32; 3]> = vec![
          [0.0, 0.0, 0.0],
          [length, 0.0, 0.0],
          [length, length, 0.0],
          [0.0, length, 0.0],
          [length / 2.0, length / 2.0, height],
        ];
        let faces: Vec<Vec<usize>> = vec![
          vec![0, 1, 4],
          vec![1, 2, 4],
          vec![2, 3, 4],
          vec![3, 0, 4],
          vec![3, 2, 1, 0],
        ];
        let face_refs: Vec<&[usize]> =
          faces.iter().map(|f| f.as_slice()).collect();
        let scad_base = ScadNode::Polyhedron {
          points: points.clone(),
          faces: faces.clone(),
        };
        let scad = if x != 0.0 || y != 0.0 || z != 0.0 {
          Some(ScadNode::Translate {
            x,
            y,
            z,
            child: Box::new(scad_base),
          })
        } else {
          Some(scad_base)
        };
        Ok(CsgGeometry {
          mesh: {
            #[cfg(feature = "csgrs")]
            {
              let mesh = CsgMesh::<()>::polyhedron(&points, &face_refs, None)
                .map_err(|e| {
                mlua::Error::RuntimeError(format!("pyramid() error: {e:?}"))
              })?;
              Some(if x != 0.0 || y != 0.0 || z != 0.0 {
                mesh.translate(x, y, z)
              } else {
                mesh
              })
            }
            #[cfg(not(feature = "csgrs"))]
            {
              let _ = &face_refs;
              None
            }
          },
          color: None,
          scad,
        })
      },
    )?;
    lua.globals().set("pyramid", pyramid_fn)?;

    // ---- torus() ----
    let torus_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("torus() requires arguments".to_string())
      })?;

      if let LuaValue::Table(t) = first {
        let major = table_get_f32(t, "R")
          .or_else(|| table_get_f32(t, "major"))
          .unwrap_or(2.0);
        let minor = table_get_f32(t, "r")
          .or_else(|| table_get_f32(t, "minor"))
          .unwrap_or(0.5);
        let seg_major = table_segments(t, 24) as usize;
        let seg_minor =
          table_get_u32(t, "segments_minor").unwrap_or(16) as usize;
        // Torus via rotate_extrude of a translated circle
        let scad = Some(ScadNode::RotateExtrude {
          angle: 360.0,
          segments: seg_major as u32,
          child: Box::new(ScadNode::Translate {
            x: major,
            y: 0.0,
            z: 0.0,
            child: Box::new(ScadNode::Circle {
              r: minor,
              segments: seg_minor as u32,
            }),
          }),
        });
        Ok(CsgGeometry {
          mesh: {
            #[cfg(feature = "csgrs")]
            {
              Some(CsgMesh::<()>::torus(
                major, minor, seg_major, seg_minor, None,
              ))
            }
            #[cfg(not(feature = "csgrs"))]
            {
              None
            }
          },
          color: None,
          scad,
        })
      } else if args.len() >= 2 {
        let major = lua_val_to_f32(&args[0]).unwrap_or(2.0);
        let minor = lua_val_to_f32(&args[1]).unwrap_or(0.5);
        let scad = Some(ScadNode::RotateExtrude {
          angle: 360.0,
          segments: 24,
          child: Box::new(ScadNode::Translate {
            x: major,
            y: 0.0,
            z: 0.0,
            child: Box::new(ScadNode::Circle {
              r: minor,
              segments: 16,
            }),
          }),
        });
        Ok(CsgGeometry {
          mesh: {
            #[cfg(feature = "csgrs")]
            {
              Some(CsgMesh::<()>::torus(major, minor, 24, 16, None))
            }
            #[cfg(not(feature = "csgrs"))]
            {
              None
            }
          },
          color: None,
          scad,
        })
      } else {
        Err(mlua::Error::RuntimeError(
          "torus() requires {R=.., r=..} or (major, minor)".to_string(),
        ))
      }
    })?;
    lua.globals().set("torus", torus_fn)?;

    // ---- octahedron() ----
    let octahedron_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let radius = if let Some(LuaValue::Table(t)) = args.front() {
        table_get_f32(t, "r")
          .or_else(|| table_get_f32(t, "radius"))
          .unwrap_or(1.0)
      } else {
        args.front().and_then(lua_val_to_f32).unwrap_or(1.0)
      };
      let r = radius;
      let scad = Some(ScadNode::Polyhedron {
        points: vec![
          [r, 0.0, 0.0],
          [-r, 0.0, 0.0],
          [0.0, r, 0.0],
          [0.0, -r, 0.0],
          [0.0, 0.0, r],
          [0.0, 0.0, -r],
        ],
        faces: vec![
          vec![0, 2, 4],
          vec![2, 1, 4],
          vec![1, 3, 4],
          vec![3, 0, 4],
          vec![2, 0, 5],
          vec![1, 2, 5],
          vec![3, 1, 5],
          vec![0, 3, 5],
        ],
      });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::octahedron(radius, None))
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("octahedron", octahedron_fn)?;

    // ---- icosahedron() ----
    let icosahedron_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let radius = if let Some(LuaValue::Table(t)) = args.front() {
        table_get_f32(t, "r")
          .or_else(|| table_get_f32(t, "radius"))
          .unwrap_or(1.0)
      } else {
        args.front().and_then(lua_val_to_f32).unwrap_or(1.0)
      };
      // Generate icosahedron vertices and faces for OpenSCAD polyhedron
      let phi = (1.0 + 5.0_f32.sqrt()) / 2.0;
      let a = radius / (1.0 + phi * phi).sqrt();
      let b = a * phi;
      let scad = Some(ScadNode::Polyhedron {
        points: vec![
          [-a, b, 0.0],
          [a, b, 0.0],
          [-a, -b, 0.0],
          [a, -b, 0.0],
          [0.0, -a, b],
          [0.0, a, b],
          [0.0, -a, -b],
          [0.0, a, -b],
          [b, 0.0, -a],
          [b, 0.0, a],
          [-b, 0.0, -a],
          [-b, 0.0, a],
        ],
        faces: vec![
          vec![0, 11, 5],
          vec![0, 5, 1],
          vec![0, 1, 7],
          vec![0, 7, 10],
          vec![0, 10, 11],
          vec![1, 5, 9],
          vec![5, 11, 4],
          vec![11, 10, 2],
          vec![10, 7, 6],
          vec![7, 1, 8],
          vec![3, 9, 4],
          vec![3, 4, 2],
          vec![3, 2, 6],
          vec![3, 6, 8],
          vec![3, 8, 9],
          vec![4, 9, 5],
          vec![2, 4, 11],
          vec![6, 2, 10],
          vec![8, 6, 7],
          vec![9, 8, 1],
        ],
      });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::icosahedron(radius, None))
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("icosahedron", icosahedron_fn)?;

    // ---- ellipsoid() ----
    let ellipsoid_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("ellipsoid() requires arguments".to_string())
      })?;

      if let LuaValue::Table(t) = first {
        let rx = table_get_f32(t, "rx").unwrap_or(1.0);
        let ry = table_get_f32(t, "ry").unwrap_or(1.0);
        let rz = table_get_f32(t, "rz").unwrap_or(1.0);
        let segs = table_segments(t, 16) as usize;
        let scad = Some(ScadNode::Scale {
          x: rx,
          y: ry,
          z: rz,
          child: Box::new(ScadNode::Sphere {
            r: 1.0,
            segments: segs as u32,
          }),
        });
        Ok(CsgGeometry {
          mesh: {
            #[cfg(feature = "csgrs")]
            {
              Some(CsgMesh::<()>::ellipsoid(rx, ry, rz, segs, segs, None))
            }
            #[cfg(not(feature = "csgrs"))]
            {
              None
            }
          },
          color: None,
          scad,
        })
      } else if args.len() >= 3 {
        let rx = lua_val_to_f32(&args[0]).unwrap_or(1.0);
        let ry = lua_val_to_f32(&args[1]).unwrap_or(1.0);
        let rz = lua_val_to_f32(&args[2]).unwrap_or(1.0);
        let scad = Some(ScadNode::Scale {
          x: rx,
          y: ry,
          z: rz,
          child: Box::new(ScadNode::Sphere {
            r: 1.0,
            segments: 16,
          }),
        });
        Ok(CsgGeometry {
          mesh: {
            #[cfg(feature = "csgrs")]
            {
              Some(CsgMesh::<()>::ellipsoid(rx, ry, rz, 16, 16, None))
            }
            #[cfg(not(feature = "csgrs"))]
            {
              None
            }
          },
          color: None,
          scad,
        })
      } else {
        Err(mlua::Error::RuntimeError(
          "ellipsoid() requires {rx=.., ry=.., rz=..} or (rx, ry, rz)"
            .to_string(),
        ))
      }
    })?;
    lua.globals().set("ellipsoid", ellipsoid_fn)?;

    // ==================================================================
    // 2D PRIMITIVES (return CsgSketch)
    // ==================================================================

    // ---- circle() ----
    let circle_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("circle() requires arguments".to_string())
      })?;

      if let LuaValue::Table(t) = first {
        let radius = if let Some(r) = table_get_f32(t, "r") {
          r
        } else if let Some(d) = table_get_f32(t, "d") {
          d / 2.0
        } else {
          t.get::<f32>(1).unwrap_or(1.0)
        };
        let segments = table_segments(t, 32) as usize;
        let scad = Some(ScadNode::Circle {
          r: radius,
          segments: segments as u32,
        });
        Ok(CsgSketch {
          sketch: {
            #[cfg(feature = "csgrs")]
            {
              csgrs::sketch::Sketch::<()>::circle(radius, segments, None)
            }
            #[cfg(not(feature = "csgrs"))]
            {}
          },
          color: None,
          scad,
        })
      } else {
        let radius = lua_val_to_f32(first).unwrap_or(1.0);
        let scad = Some(ScadNode::Circle {
          r: radius,
          segments: 32,
        });
        Ok(CsgSketch {
          sketch: {
            #[cfg(feature = "csgrs")]
            {
              csgrs::sketch::Sketch::<()>::circle(radius, 32, None)
            }
            #[cfg(not(feature = "csgrs"))]
            {}
          },
          color: None,
          scad,
        })
      }
    })?;
    lua.globals().set("circle", circle_fn)?;

    // ---- rect() / square() ----
    let rect_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("rect() requires arguments".to_string())
      })?;

      if let LuaValue::Table(t) = first {
        let (w, h) =
          if let Ok(LuaValue::Table(size_t)) = t.get::<mlua::Value>("size") {
            let w: f32 = size_t.get::<f32>(1).unwrap_or(1.0);
            let h: f32 = size_t.get::<f32>(2).unwrap_or(w);
            (w, h)
          } else {
            let w: f32 = t.get::<f32>(1).unwrap_or(1.0);
            let h: f32 = t.get::<f32>(2).unwrap_or(w);
            (w, h)
          };
        let center = table_get_bool(t, "center");
        let scad = Some(ScadNode::Square { w, h, center });
        Ok(CsgSketch {
          sketch: {
            #[cfg(feature = "csgrs")]
            {
              let mut sketch =
                csgrs::sketch::Sketch::<()>::rectangle(w, h, None);
              if center {
                use csgrs::traits::CSG;
                sketch = sketch.translate(-w / 2.0, -h / 2.0, 0.0);
              }
              sketch
            }
            #[cfg(not(feature = "csgrs"))]
            {}
          },
          color: None,
          scad,
        })
      } else {
        let w = lua_val_to_f32(first).unwrap_or(1.0);
        let h = args.get(1).and_then(lua_val_to_f32).unwrap_or(w);
        let scad = Some(ScadNode::Square {
          w,
          h,
          center: false,
        });
        Ok(CsgSketch {
          sketch: {
            #[cfg(feature = "csgrs")]
            {
              csgrs::sketch::Sketch::<()>::rectangle(w, h, None)
            }
            #[cfg(not(feature = "csgrs"))]
            {}
          },
          color: None,
          scad,
        })
      }
    })?;
    lua.globals().set("rect", rect_fn.clone())?;
    lua.globals().set("square", rect_fn)?;

    // ---- polygon() ----
    let polygon_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("polygon() requires arguments".to_string())
      })?;

      let points_table = if let LuaValue::Table(t) = first {
        if let Ok(LuaValue::Table(pts)) = t.get::<mlua::Value>("points") {
          pts
        } else {
          // Assume the table itself is the points array
          t.clone()
        }
      } else {
        return Err(mlua::Error::RuntimeError(
          "polygon() requires a table of points".to_string(),
        ));
      };

      let mut points: Vec<[f32; 2]> = Vec::new();
      for i in 1..=points_table.len()? {
        let pt: mlua::Table = points_table.get(i)?;
        let x: f32 = pt.get::<f32>(1).unwrap_or(0.0);
        let y: f32 = pt.get::<f32>(2).unwrap_or(0.0);
        points.push([x, y]);
      }

      let scad = Some(ScadNode::Polygon {
        points: points.clone(),
      });
      Ok(CsgSketch {
        sketch: {
          #[cfg(feature = "csgrs")]
          {
            csgrs::sketch::Sketch::<()>::polygon(&points, None)
          }
          #[cfg(not(feature = "csgrs"))]
          {}
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("polygon", polygon_fn)?;

    // ==================================================================
    // RENDERING
    // ==================================================================

    // ---- render() ----
    let collector_clone = collector.clone();
    let render_fn = lua.create_function(move |_, ud: mlua::AnyUserData| {
      let geom = ud.borrow::<CsgGeometry>()?.clone();
      collector_clone.borrow_mut().push(geom);
      Ok(())
    })?;
    lua.globals().set("render", render_fn)?;

    // ==================================================================
    // SCAD INTEROP
    // ==================================================================

    // ---- scad() ----
    // Insert raw OpenSCAD code. Returns a CsgGeometry with an empty mesh
    // but a Literal ScadNode so it participates in SCAD export.
    let scad_fn = lua.create_function(|_, code: String| {
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::new())
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad: Some(ScadNode::Literal { code }),
      })
    })?;
    lua.globals().set("scad", scad_fn)?;

    // ---- var() ----
    // OpenSCAD customizer variable. Returns the numeric default value so
    // Lua computation works. The variable name is unused in the Rust engine
    // but kept for API compatibility with LuaCAD.
    let var_fn =
      lua.create_function(|_, (_name, value): (String, f64)| Ok(value))?;
    lua.globals().set("var", var_fn)?;

    // ---- cad() ----
    // Returns an empty CsgGeometry. In LuaCAD this is the base CAD object
    // constructor; here it's mainly for API compatibility.
    let cad_fn = lua.create_function(|_, ()| {
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::new())
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad: None,
      })
    })?;
    lua.globals().set("cad", cad_fn)?;

    // ==================================================================
    // MATH GLOBALS
    // ==================================================================

    // Expose math functions as bare globals
    lua
      .load(
        r#"
        abs = math.abs
        sin = function(x) return math.sin(math.rad(x)) end
        cos = function(x) return math.cos(math.rad(x)) end
        tan = function(x) return math.tan(math.rad(x)) end
        asin = function(x) return math.deg(math.asin(x)) end
        acos = function(x) return math.deg(math.acos(x)) end
        atan = function(x) return math.deg(math.atan(x)) end
        atan2 = function(y, x) return math.deg(math.atan(y, x)) end
        floor = math.floor
        ceil = math.ceil
        sqrt = math.sqrt
        pow = function(x, y) return x ^ y end
        exp = math.exp
        log = math.log
        ln = math.log
        min = math.min
        max = math.max
        PI = math.pi
        "#,
      )
      .exec()?;

    // ---- sign() ----
    let sign_fn = lua.create_function(|_, x: f64| {
      Ok(if x > 0.0 {
        1.0
      } else if x < 0.0 {
        -1.0
      } else {
        0.0
      })
    })?;
    lua.globals().set("sign", sign_fn)?;

    // ---- round() ----
    let round_fn = lua.create_function(|_, x: f64| Ok(x.round() as i64))?;
    lua.globals().set("round", round_fn)?;

    // ---- rands() ----
    let rands_fn = lua.create_function(
      |lua, (min_val, max_val, count, seed): (f64, f64, u32, Option<u64>)| {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        seed.unwrap_or(42).hash(&mut hasher);
        let mut state = hasher.finish();
        let range = max_val - min_val;
        let t = lua.create_table()?;
        for i in 1..=count {
          // Simple xorshift64
          state ^= state << 13;
          state ^= state >> 7;
          state ^= state << 17;
          let frac = (state as f64) / (u64::MAX as f64);
          t.set(i, min_val + frac * range)?;
        }
        Ok(t)
      },
    )?;
    lua.globals().set("rands", rands_fn)?;

    // ---- type-checking functions ----
    lua
      .load(
        r#"
        function is_bool(v) return type(v) == "boolean" end
        function is_num(v) return type(v) == "number" end
        function is_str(v) return type(v) == "string" end
        function is_table(v) return type(v) == "table" end
        function is_list(v) return type(v) == "table" end
        function is_func(v) return type(v) == "function" end
        function concat(a, b)
          local result = {}
          for _, v in ipairs(a) do result[#result + 1] = v end
          for _, v in ipairs(b) do result[#result + 1] = v end
          return result
        end
        "#,
      )
      .exec()?;

    // ==================================================================
    // VECTOR MODULE
    // ==================================================================

    register_vector_type(&lua)?;

    // ==================================================================
    // TEXT (ScadNode-only, no viewport rendering)
    // ==================================================================

    let text_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("text() requires arguments".to_string())
      })?;

      let (text_str, size, font, halign, valign) =
        if let LuaValue::String(s) = first {
          let text = s.to_str().map(|s| s.to_string()).unwrap_or_default();
          // Optional second arg: table with options
          let (size, font, halign, valign) =
            if let Some(LuaValue::Table(t)) = args.get(1) {
              (
                table_get_f32(t, "size").unwrap_or(10.0),
                t.get::<String>("font")
                  .unwrap_or_else(|_| "Arial".to_string()),
                t.get::<String>("halign")
                  .unwrap_or_else(|_| "left".to_string()),
                t.get::<String>("valign")
                  .unwrap_or_else(|_| "baseline".to_string()),
              )
            } else {
              (
                10.0,
                "Arial".to_string(),
                "left".to_string(),
                "baseline".to_string(),
              )
            };
          (text, size, font, halign, valign)
        } else {
          return Err(mlua::Error::RuntimeError(
            "text() first argument must be a string".to_string(),
          ));
        };

      let scad = Some(ScadNode::Text {
        text: text_str,
        size,
        font,
        halign,
        valign,
      });
      // Return a minimal 2D sketch (point) — text can't be rendered in viewport
      Ok(CsgSketch {
        sketch: {
          #[cfg(feature = "csgrs")]
          {
            csgrs::sketch::Sketch::<()>::rectangle(0.001, 0.001, None)
          }
          #[cfg(not(feature = "csgrs"))]
          {}
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("text", text_fn)?;

    // text3d() — text with linear_extrude in ScadNode
    let text3d_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let first = args.front().ok_or_else(|| {
        mlua::Error::RuntimeError("text3d() requires arguments".to_string())
      })?;

      let text_str = if let LuaValue::String(s) = first {
        s.to_str().map(|s| s.to_string()).unwrap_or_default()
      } else {
        return Err(mlua::Error::RuntimeError(
          "text3d() first argument must be a string".to_string(),
        ));
      };

      let (size, depth, font, halign, valign) =
        if let Some(LuaValue::Table(t)) = args.get(1) {
          (
            table_get_f32(t, "size").unwrap_or(10.0),
            table_get_f32(t, "depth").unwrap_or(1.0),
            t.get::<String>("font")
              .unwrap_or_else(|_| "Arial".to_string()),
            t.get::<String>("halign")
              .unwrap_or_else(|_| "left".to_string()),
            t.get::<String>("valign")
              .unwrap_or_else(|_| "baseline".to_string()),
          )
        } else {
          (
            10.0,
            1.0,
            "Arial".to_string(),
            "left".to_string(),
            "baseline".to_string(),
          )
        };

      let text_node = ScadNode::Text {
        text: text_str,
        size,
        font,
        halign,
        valign,
      };
      let scad = Some(ScadNode::LinearExtrude {
        height: depth,
        center: false,
        twist: 0.0,
        slices: 0,
        scale: 1.0,
        child: Box::new(text_node),
      });
      // Minimal mesh placeholder
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::cuboid(0.001, 0.001, 0.001, None))
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("text3d", text3d_fn)?;

    // ==================================================================
    // FILE OPERATIONS (ScadNode-only)
    // ==================================================================

    // ---- import() ----
    let import_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let file = if let Some(LuaValue::String(s)) = args.front() {
        s.to_str().map(|s| s.to_string()).unwrap_or_default()
      } else {
        return Err(mlua::Error::RuntimeError(
          "import() requires a filename string".to_string(),
        ));
      };
      let convexity = args
        .get(1)
        .and_then(lua_val_to_f32)
        .map(|v| v as u32)
        .unwrap_or(0);
      let scad = Some(ScadNode::Import { file, convexity });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::cuboid(0.001, 0.001, 0.001, None))
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("import", import_fn)?;

    // ---- surface() ----
    let surface_fn = lua.create_function(|_, args: mlua::MultiValue| {
      let file = if let Some(LuaValue::String(s)) = args.front() {
        s.to_str().map(|s| s.to_string()).unwrap_or_default()
      } else {
        return Err(mlua::Error::RuntimeError(
          "surface() requires a filename string".to_string(),
        ));
      };
      let center = args
        .get(1)
        .and_then(|v| {
          if let LuaValue::Boolean(b) = v {
            Some(*b)
          } else {
            None
          }
        })
        .unwrap_or(false);
      let convexity = args
        .get(2)
        .and_then(lua_val_to_f32)
        .map(|v| v as u32)
        .unwrap_or(0);
      let scad = Some(ScadNode::Surface {
        file,
        center,
        convexity,
      });
      Ok(CsgGeometry {
        mesh: {
          #[cfg(feature = "csgrs")]
          {
            Some(CsgMesh::<()>::cuboid(0.001, 0.001, 0.001, None))
          }
          #[cfg(not(feature = "csgrs"))]
          {
            None
          }
        },
        color: None,
        scad,
      })
    })?;
    lua.globals().set("surface", surface_fn)?;

    // ==================================================================
    // MODIFIER FUNCTIONS (global wrappers)
    // ==================================================================

    lua
      .load(
        r#"
        function s(obj) return obj:skip() end
        function o(obj) return obj:only() end
        function d(obj) return obj:debug() end
        function t(obj) return obj:transparent() end
        "#,
      )
      .exec()?;

    // ==================================================================
    // SETTINGS OBJECT
    // ==================================================================

    lua
      .load(
        r#"
        settings = {
          fa = 12,
          fs = 2,
          fn = 0,
          t = 0,
          vpr = {55, 0, 25},
          vpt = {0, 0, 0},
          vpd = 140,
          vpf = 22.5,
          children = {},
          preview = true,
        }
        "#,
      )
      .exec()?;

    // ==================================================================
    // UTILITY FUNCTIONS
    // ==================================================================

    // ---- lookup() ----
    let lookup_fn =
      lua.create_function(|_, (key, table): (f64, mlua::Table)| {
        // lookup(key, [[k1,v1], [k2,v2], ...]) — linear interpolation
        let mut pairs: Vec<(f64, f64)> = Vec::new();
        for i in 1..=table.len().unwrap_or(0) {
          if let Ok(entry) = table.get::<mlua::Table>(i) {
            let k: f64 = entry.get::<f64>(1).unwrap_or(0.0);
            let v: f64 = entry.get::<f64>(2).unwrap_or(0.0);
            pairs.push((k, v));
          }
        }
        if pairs.is_empty() {
          return Ok(0.0);
        }
        pairs.sort_by(|a, b| {
          a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Clamp to range
        if key <= pairs[0].0 {
          return Ok(pairs[0].1);
        }
        if key >= pairs[pairs.len() - 1].0 {
          return Ok(pairs[pairs.len() - 1].1);
        }
        // Linear interpolation
        for i in 0..pairs.len() - 1 {
          if key >= pairs[i].0 && key <= pairs[i + 1].0 {
            let t = (key - pairs[i].0) / (pairs[i + 1].0 - pairs[i].0);
            return Ok(pairs[i].1 + t * (pairs[i + 1].1 - pairs[i].1));
          }
        }
        Ok(pairs[pairs.len() - 1].1)
      })?;
    lua.globals().set("lookup", lookup_fn)?;

    // ---- version() ----
    let version_fn = lua.create_function(|lua, ()| {
      let t = lua.create_table()?;
      t.set(
        1,
        env!("CARGO_PKG_VERSION_MAJOR").parse::<i32>().unwrap_or(0),
      )?;
      t.set(
        2,
        env!("CARGO_PKG_VERSION_MINOR").parse::<i32>().unwrap_or(0),
      )?;
      t.set(
        3,
        env!("CARGO_PKG_VERSION_PATCH").parse::<i32>().unwrap_or(0),
      )?;
      Ok(t)
    })?;
    lua.globals().set("version", version_fn)?;

    // ==================================================================
    // BOSL LIBRARY
    // ==================================================================

    crate::bosl::register_bosl(&lua)?;

    lua.load(code).eval::<mlua::MultiValue>()
  })();

  match result {
    Ok(returns) => {
      // Auto-render any CsgGeometry returned from top-level
      for val in returns.iter() {
        if let LuaValue::UserData(ud) = val
          && let Ok(geom) = ud.borrow::<CsgGeometry>()
        {
          collector.borrow_mut().push(geom.clone());
        }
      }
      Ok(collector.borrow().clone())
    }
    Err(e) => Err(format!("Lua error: {e}")),
  }
}

// ---------------------------------------------------------------------------
// Vector userdata type
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct LuaVector {
  x: f64,
  y: f64,
  z: f64,
}

impl mlua::UserData for LuaVector {
  fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
    methods.add_method("getx", |_, this, ()| Ok(this.x));
    methods.add_method("gety", |_, this, ()| Ok(this.y));
    methods.add_method("getz", |_, this, ()| Ok(this.z));

    methods.add_method("len", |_, this, ()| {
      Ok((this.x * this.x + this.y * this.y + this.z * this.z).sqrt())
    });

    methods.add_method("unit", |_, this, ()| {
      let len = (this.x * this.x + this.y * this.y + this.z * this.z).sqrt();
      if len < 1e-12 {
        Ok(LuaVector {
          x: 0.0,
          y: 0.0,
          z: 0.0,
        })
      } else {
        Ok(LuaVector {
          x: this.x / len,
          y: this.y / len,
          z: this.z / len,
        })
      }
    });

    methods.add_method("cross", |_, this, other: mlua::AnyUserData| {
      let o = other.borrow::<LuaVector>()?;
      Ok(LuaVector {
        x: this.y * o.z - this.z * o.y,
        y: this.z * o.x - this.x * o.z,
        z: this.x * o.y - this.y * o.x,
      })
    });

    methods.add_method("scalar", |_, this, other: mlua::AnyUserData| {
      let o = other.borrow::<LuaVector>()?;
      Ok(this.x * o.x + this.y * o.y + this.z * o.z)
    });

    methods.add_method("normal", |_, this, direction: Option<f64>| {
      let dir = direction.unwrap_or(1.0);
      // 2D normal (rotate 90 degrees in XY plane)
      Ok(LuaVector {
        x: -this.y * dir,
        y: this.x * dir,
        z: 0.0,
      })
    });

    methods.add_method(
      "rot",
      |_, this, (angle, axis): (f64, Option<String>)| {
        let rad = angle.to_radians();
        let (cos_a, sin_a) = (rad.cos(), rad.sin());
        let axis = axis.unwrap_or_else(|| "z".to_string());
        let (x, y, z) = match axis.as_str() {
          "x" => (
            this.x,
            this.y * cos_a - this.z * sin_a,
            this.y * sin_a + this.z * cos_a,
          ),
          "y" => (
            this.x * cos_a + this.z * sin_a,
            this.y,
            -this.x * sin_a + this.z * cos_a,
          ),
          _ => (
            this.x * cos_a - this.y * sin_a,
            this.x * sin_a + this.y * cos_a,
            this.z,
          ),
        };
        Ok(LuaVector { x, y, z })
      },
    );

    // Operators
    methods.add_meta_method(
      mlua::MetaMethod::Add,
      |_, this, other: mlua::AnyUserData| {
        let o = other.borrow::<LuaVector>()?;
        Ok(LuaVector {
          x: this.x + o.x,
          y: this.y + o.y,
          z: this.z + o.z,
        })
      },
    );

    methods.add_meta_method(
      mlua::MetaMethod::Sub,
      |_, this, other: mlua::AnyUserData| {
        let o = other.borrow::<LuaVector>()?;
        Ok(LuaVector {
          x: this.x - o.x,
          y: this.y - o.y,
          z: this.z - o.z,
        })
      },
    );

    methods.add_meta_method(
      mlua::MetaMethod::Mul,
      |_, this, val: mlua::Value| {
        if let Some(n) = lua_val_to_f32(&val) {
          let n = n as f64;
          Ok(LuaVector {
            x: this.x * n,
            y: this.y * n,
            z: this.z * n,
          })
        } else if let mlua::Value::UserData(ud) = val {
          // Dot product
          let o = ud.borrow::<LuaVector>()?;
          // Return as LuaVector with result in x, 0, 0 — but actually
          // dot product should return number. Use a workaround by returning
          // a scalar-like vector. However, the reference uses this for dot.
          // For proper behavior, we'd need different return types.
          // Return the dot product via a number.
          // Unfortunately mlua needs a single return type.
          // Let's just return vector with the scalar in all components.
          let dot = this.x * o.x + this.y * o.y + this.z * o.z;
          Ok(LuaVector {
            x: dot,
            y: dot,
            z: dot,
          })
        } else {
          Err(mlua::Error::RuntimeError(
            "vector * requires a number or vector".to_string(),
          ))
        }
      },
    );

    methods.add_meta_method(mlua::MetaMethod::Unm, |_, this, ()| {
      Ok(LuaVector {
        x: -this.x,
        y: -this.y,
        z: -this.z,
      })
    });

    methods.add_meta_method(mlua::MetaMethod::Len, |_, this, ()| {
      Ok((this.x * this.x + this.y * this.y + this.z * this.z).sqrt())
    });

    methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
      Ok(format!("vec({}, {}, {})", this.x, this.y, this.z))
    });
  }
}

fn register_vector_type(lua: &Lua) -> LuaResult<()> {
  let vector_fn =
    lua.create_function(|_, (x, y, z): (f64, f64, Option<f64>)| {
      Ok(LuaVector {
        x,
        y,
        z: z.unwrap_or(0.0),
      })
    })?;
  lua.globals().set("vector", vector_fn.clone())?;
  lua.globals().set("vec", vector_fn)?;

  // Global vector utility functions
  let cross_fn = lua.create_function(
    |_, (a, b): (mlua::AnyUserData, mlua::AnyUserData)| {
      let va = a.borrow::<LuaVector>()?;
      let vb = b.borrow::<LuaVector>()?;
      Ok(LuaVector {
        x: va.y * vb.z - va.z * vb.y,
        y: va.z * vb.x - va.x * vb.z,
        z: va.x * vb.y - va.y * vb.x,
      })
    },
  )?;
  lua.globals().set("cross", cross_fn)?;

  let norm_fn = lua.create_function(|_, v: mlua::AnyUserData| {
    let vv = v.borrow::<LuaVector>()?;
    Ok((vv.x * vv.x + vv.y * vv.y + vv.z * vv.z).sqrt())
  })?;
  lua.globals().set("norm", norm_fn)?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scad_export::generate_scad;

  /// Helper: run Lua code and return the collected ScadNodes.
  fn run_lua_scad(code: &str) -> Vec<ScadNode> {
    let geometries = execute_lua(code).expect("Lua execution failed");
    geometries.iter().filter_map(|g| g.scad.clone()).collect()
  }

  #[test]
  fn e2e_cube_produces_scad() {
    let nodes = run_lua_scad("render(cube(5, 10, 15))");
    assert_eq!(nodes.len(), 1);
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([5, 10, 15]);"));
  }

  #[test]
  fn e2e_centered_cube() {
    let nodes = run_lua_scad("render(cube { 4, 2, 1, center = true })");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([4, 2, 1], center = true);"));
  }

  #[test]
  fn e2e_difference_operator() {
    let nodes = run_lua_scad(
      "local a = cube(10, 10, 10)\n\
       local b = sphere(5)\n\
       render(a - b)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("difference()"));
    assert!(scad.contains("cube([10, 10, 10]);"));
    assert!(scad.contains("sphere("));
  }

  #[test]
  fn e2e_union_operator() {
    let nodes = run_lua_scad(
      "local a = cube(5, 5, 5)\n\
       local b = cube(5, 5, 5):translate(3, 0, 0)\n\
       render(a + b)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("union()"));
    assert!(scad.contains("translate([3, 0, 0])"));
  }

  #[test]
  fn e2e_translate_and_rotate() {
    let nodes = run_lua_scad(
      "render(cylinder { h = 3, r = 1, center = true }:rotate(90, 0, 0):translate(5, 0, 0))",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("translate([5, 0, 0])"));
    assert!(scad.contains("rotate([90, 0, 0])"));
    assert!(scad.contains("cylinder("));
  }

  #[test]
  fn e2e_default_welcome_code() {
    // Test with the default code the app starts with
    let nodes = run_lua_scad(
      "local body = cube { 4, 2, 1, center = true }\n\
       local hole = cylinder { h = 3, r = 0.5, center = true }\n\
       render(body - hole)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("difference()"));
    assert!(scad.contains("cube([4, 2, 1], center = true);"));
    assert!(scad.contains("cylinder(h = 3"));
    assert!(scad.contains("center = true"));
  }

  #[test]
  fn e2e_multiple_render_calls() {
    let nodes = run_lua_scad(
      "render(cube(1, 1, 1))\n\
       render(sphere(2))",
    );
    assert_eq!(nodes.len(), 2);
    let scad = generate_scad(&nodes);
    // Multiple objects should be wrapped in union
    assert!(scad.contains("union()"));
    assert!(scad.contains("cube([1, 1, 1]);"));
    assert!(scad.contains("sphere("));
  }

  #[test]
  fn e2e_linear_extrude() {
    let nodes = run_lua_scad(
      "local c = circle(5)\n\
       render(c:linear_extrude(10))",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("linear_extrude(height = 10)"));
    assert!(scad.contains("circle(r = 5"));
  }

  #[test]
  fn e2e_color() {
    let nodes = run_lua_scad("render(cube(5, 5, 5):setcolor(1, 0, 0))");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("color([1, 0, 0])"));
    assert!(scad.contains("cube([5, 5, 5]);"));
  }

  // =========================================================================
  // Cube tests (adapted from LuaCAD test_cube.lua)
  // =========================================================================

  #[test]
  fn cube_basic_size_table() {
    let nodes = run_lua_scad("return cube { size = { 3, 3, 3 } }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube("), "cube command not found");
    assert!(scad.contains("cube([3, 3, 3])"));
  }

  #[test]
  fn cube_custom_size() {
    let nodes = run_lua_scad("return cube { size = { 1, 2, 3 } }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([1, 2, 3])"));
  }

  #[test]
  fn cube_with_center() {
    let nodes =
      run_lua_scad("return cube { size = { 2, 2, 2 }, center = true }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube("));
    assert!(
      scad.contains("center = true"),
      "center parameter not found in SCAD output"
    );
  }

  #[test]
  fn cube_array_syntax() {
    let nodes = run_lua_scad("return cube { 3, 3, 5 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([3, 3, 5])"));
  }

  #[test]
  fn cube_array_syntax_with_center() {
    let nodes = run_lua_scad("return cube { 4, 4, 6, center = true }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([4, 4, 6]"));
    assert!(scad.contains("center = true"));
  }

  #[test]
  fn cube_single_number() {
    let nodes = run_lua_scad("return cube(2)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([2, 2, 2])"));
  }

  #[test]
  fn cube_three_number_args() {
    let nodes = run_lua_scad("return cube(1, 2, 3)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([1, 2, 3])"));
  }

  #[test]
  fn cube_nested_table() {
    let nodes = run_lua_scad("return cube { {5, 10, 15}, center = true }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cube([5, 10, 15]"));
    assert!(scad.contains("center = true"));
  }

  #[test]
  fn cube_requires_argument() {
    let result = execute_lua("return cube()");
    assert!(result.is_err(), "cube() with no args should error");
  }

  // =========================================================================
  // Sphere tests (adapted from LuaCAD test_sphere.lua)
  // =========================================================================

  #[test]
  fn sphere_with_radius() {
    let nodes = run_lua_scad("return sphere { r = 5 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("sphere("));
    assert!(scad.contains("r = 5"));
  }

  #[test]
  fn sphere_with_diameter() {
    let nodes = run_lua_scad("return sphere { d = 10 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("sphere("));
    assert!(scad.contains("r = 5"), "diameter 10 should yield radius 5");
  }

  #[test]
  fn sphere_translation() {
    let nodes = run_lua_scad("return sphere { r = 3 }:translate(10, 10, 10)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("sphere("));
    assert!(scad.contains("translate([10, 10, 10])"));
  }

  #[test]
  fn sphere_color_setting() {
    let nodes = run_lua_scad("return sphere { r = 4 }:setcolor(1, 0, 0)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("sphere("));
    assert!(scad.contains("color([1, 0, 0])"));
  }

  #[test]
  fn sphere_color_named() {
    let nodes = run_lua_scad("return sphere { r = 4 }:setcolor('red')");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("sphere("));
    assert!(scad.contains("color([1, 0, 0])"));
  }

  #[test]
  fn sphere_requires_argument() {
    let result = execute_lua("return sphere()");
    assert!(result.is_err(), "sphere() with no args should error");
  }

  #[test]
  fn sphere_positional_number() {
    let nodes = run_lua_scad("return sphere(7)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("r = 7"));
  }

  // =========================================================================
  // Cylinder tests (adapted from LuaCAD test_simple.lua)
  // =========================================================================

  #[test]
  fn cylinder_with_radius() {
    let nodes = run_lua_scad("return cylinder { h = 10, r = 5 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cylinder("));
    assert!(scad.contains("h = 10"));
    assert!(scad.contains("r1 = 5"));
    assert!(scad.contains("r2 = 5"));
  }

  #[test]
  fn cylinder_with_diameter() {
    let nodes = run_lua_scad("return cylinder { h = 10, d = 10 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cylinder("));
    assert!(scad.contains("h = 10"));
    assert!(scad.contains("r1 = 5"));
  }

  #[test]
  fn cylinder_cone_with_radii() {
    let nodes = run_lua_scad("return cylinder { h = 15, r1 = 5, r2 = 2 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cylinder("));
    assert!(scad.contains("h = 15"));
    assert!(scad.contains("r1 = 5"));
    assert!(scad.contains("r2 = 2"));
  }

  #[test]
  fn cylinder_cone_with_diameters() {
    let nodes = run_lua_scad("return cylinder { h = 15, d1 = 10, d2 = 4 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cylinder("));
    assert!(scad.contains("r1 = 5"));
    assert!(scad.contains("r2 = 2"));
  }

  #[test]
  fn cylinder_centered() {
    let nodes =
      run_lua_scad("return cylinder { h = 10, r = 5, center = true }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("cylinder("));
    assert!(scad.contains("center = true"));
  }

  #[test]
  fn cylinder_requires_argument() {
    let result = execute_lua("return cylinder()");
    assert!(result.is_err(), "cylinder() with no args should error");
  }

  // =========================================================================
  // Circle tests (adapted from LuaCAD test_circle.lua)
  // =========================================================================

  #[test]
  fn circle_with_radius() {
    let nodes = run_lua_scad("return circle { r = 10 }:linear_extrude(1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("circle("));
    assert!(scad.contains("r = 10"));
  }

  #[test]
  fn circle_with_diameter() {
    let nodes = run_lua_scad("return circle { d = 20 }:linear_extrude(1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("circle("));
    assert!(
      scad.contains("r = 10"),
      "diameter 20 should yield radius 10"
    );
  }

  #[test]
  fn circle_positional_number() {
    let nodes = run_lua_scad("return circle(5):linear_extrude(1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("circle("));
    assert!(scad.contains("r = 5"));
  }

  // =========================================================================
  // Square/rect tests (adapted from LuaCAD test_square.lua)
  // =========================================================================

  #[test]
  fn square_with_size_and_center() {
    let nodes = run_lua_scad(
      "return square { size = { 10, 20 }, center = true }:linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("square("));
    assert!(scad.contains("center = true"));
  }

  #[test]
  fn square_with_single_size() {
    let nodes = run_lua_scad(
      "return square { size = { 15 }, center = false }:linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("square("));
  }

  #[test]
  fn rect_alias() {
    let nodes = run_lua_scad(
      "return rect { size = { 30, 40 }, center = true }:linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(
      scad.contains("square("),
      "rect should generate square() in SCAD"
    );
    assert!(scad.contains("center = true"));
  }

  #[test]
  fn square_array_syntax() {
    let nodes = run_lua_scad("return square { 10, 20 }:linear_extrude(1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("square([10, 20])"));
  }

  // =========================================================================
  // Translate tests (adapted from LuaCAD test_translate.lua)
  // =========================================================================

  #[test]
  fn translate_direct_parameters() {
    let nodes =
      run_lua_scad("return cube { size = { 1, 1, 1 } }:translate(10, 20, 30)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("translate([10, 20, 30])"));
    assert!(scad.contains("cube("));
  }

  #[test]
  fn translate_preserves_values() {
    let nodes =
      run_lua_scad("return cube { size = { 1, 1, 1 } }:translate(10, 20, 30)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("10"), "x value not found");
    assert!(scad.contains("20"), "y value not found");
    assert!(scad.contains("30"), "z value not found");
  }

  // =========================================================================
  // Rotate extrude tests (adapted from LuaCAD test_rotate_extrude.lua)
  // =========================================================================

  #[test]
  fn rotate_extrude_simple() {
    let nodes = run_lua_scad(
      "local c = circle { r = 2 }:translate(5, 0)\n\
       return c:rotateextrude()",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("rotate_extrude("));
  }

  #[test]
  fn rotate_extrude_with_angle() {
    let nodes = run_lua_scad(
      "local c = circle { r = 2 }:translate(5, 0)\n\
       return c:rotateextrude(180)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("rotate_extrude(angle = 180"));
  }

  #[test]
  fn rotate_extrude_alias() {
    let nodes = run_lua_scad(
      "local c = circle { r = 2 }:translate(5, 0)\n\
       return c:rotate_extrude()",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("rotate_extrude("));
  }

  // =========================================================================
  // Minkowski tests (adapted from LuaCAD test_minkowski.lua)
  // =========================================================================

  #[test]
  fn minkowski_cube_sphere() {
    let nodes = run_lua_scad(
      "local c = cube { 10, 10, 10, center = true }\n\
       local s = sphere { r = 2 }\n\
       return c:minkowski(s)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("minkowski()"));
    assert!(scad.contains("cube("));
    assert!(scad.contains("sphere("));
  }

  #[test]
  fn minkowski_rounded_cube() {
    let nodes = run_lua_scad(
      "local c = cube { 10, 10, 10, center = true }\n\
       local s = sphere { r = 1 }\n\
       return c:minkowski(s)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("minkowski()"));
  }

  // =========================================================================
  // Projection tests (adapted from LuaCAD test_projection.lua)
  // =========================================================================

  #[test]
  fn projection_simple() {
    let nodes =
      run_lua_scad("return cube { size = { 10, 10, 10 } }:projection()");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("projection()"));
  }

  #[test]
  fn projection_cut() {
    let nodes =
      run_lua_scad("return cube { size = { 10, 10, 10 } }:projection(true)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("projection(cut = true)"));
  }

  #[test]
  fn projection_complex() {
    let nodes = run_lua_scad(
      "local c = cube { size = { 20, 20, 10 } }\n\
       local cyl = cylinder { h = 15, r = 5 }:translate(10, 10, 10)\n\
       return (c + cyl):projection()",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("projection()"));
    assert!(scad.contains("cube("));
    assert!(scad.contains("cylinder("));
  }

  // =========================================================================
  // Multmatrix tests (adapted from LuaCAD test_multmatrix.lua)
  // =========================================================================

  #[test]
  fn multmatrix_identity() {
    let nodes = run_lua_scad(
      "local c = cube { size = { 2, 2, 2 } }\n\
       local m = {1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1}\n\
       return c:multmatrix(m)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("multmatrix("));
    assert!(scad.contains("[1, 0, 0, 0]"));
  }

  #[test]
  fn multmatrix_scale() {
    let nodes = run_lua_scad(
      "local c = cube { size = { 1, 1, 1 } }\n\
       local m = {2,0,0,0, 0,2,0,0, 0,0,2,0, 0,0,0,1}\n\
       return c:multmatrix(m)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("multmatrix("));
    assert!(scad.contains("[2, 0, 0, 0]"));
  }

  #[test]
  fn multmatrix_translation() {
    let nodes = run_lua_scad(
      "local c = cube { size = { 1, 1, 1 } }\n\
       local m = {1,0,0,3, 0,1,0,2, 0,0,1,1, 0,0,0,1}\n\
       return c:multmatrix(m)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("multmatrix("));
    assert!(scad.contains("[1, 0, 0, 3]"));
  }

  #[test]
  fn multmatrix_rotation() {
    let nodes = run_lua_scad(
      "local sin45 = 0.7071067811865475\n\
       local cos45 = 0.7071067811865475\n\
       local c = cube { size = { 1, 1, 1 } }\n\
       local m = {cos45, -sin45, 0, 0, sin45, cos45, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1}\n\
       return c:multmatrix(m)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("multmatrix("));
    assert!(scad.contains("0.707107"));
  }

  #[test]
  fn multmatrix_combined() {
    let nodes = run_lua_scad(
      "local sin45 = 0.7071067811865475\n\
       local cos45 = 0.7071067811865475\n\
       local c = cube { size = { 1, 1, 1 } }\n\
       local m = {2*cos45, -2*sin45, 0, 3, 2*sin45, 2*cos45, 0, 2, 0, 0, 2, 1, 0, 0, 0, 1}\n\
       return c:multmatrix(m)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("multmatrix("));
  }

  // =========================================================================
  // Sign function tests (adapted from LuaCAD test_sign.lua)
  // =========================================================================

  #[test]
  fn sign_positive() {
    let geoms = execute_lua(
      "local r = sign(5)\n\
       assert(r == 1, 'sign(5) should be 1, got ' .. r)\n\
       assert(sign(0.1) == 1)\n\
       assert(sign(1e100) == 1)\n\
       return cube(1)",
    )
    .expect("sign positive tests failed");
    assert!(!geoms.is_empty());
  }

  #[test]
  fn sign_zero() {
    let geoms = execute_lua(
      "assert(sign(0) == 0, 'sign(0) should be 0')\n\
       return cube(1)",
    )
    .expect("sign zero test failed");
    assert!(!geoms.is_empty());
  }

  #[test]
  fn sign_negative() {
    let geoms = execute_lua(
      "assert(sign(-5) == -1)\n\
       assert(sign(-0.1) == -1)\n\
       assert(sign(-1e100) == -1)\n\
       return cube(1)",
    )
    .expect("sign negative tests failed");
    assert!(!geoms.is_empty());
  }

  #[test]
  fn sign_in_cad_model() {
    // sign function used in arithmetic for a CAD model
    let geoms = execute_lua(
      "local value = -3\n\
       local height = 2 * math.abs(value) * sign(value)\n\
       assert(height == -6, 'expected -6, got ' .. height)\n\
       return cube { size = { 10, 10, math.abs(height) } }",
    )
    .expect("sign in CAD model failed");
    assert!(!geoms.is_empty());
    let scad = generate_scad(
      &geoms
        .iter()
        .filter_map(|g| g.scad.clone())
        .collect::<Vec<_>>(),
    );
    assert!(scad.contains("cube([10, 10, 6])"));
  }

  // =========================================================================
  // Combined shapes tests (adapted from LuaCAD test_simple.lua)
  // =========================================================================

  #[test]
  fn combining_shapes_union() {
    let nodes = run_lua_scad(
      "local c = cube { size = { 1, 2, 3 } }\n\
       local s = sphere { r = 2 }\n\
       return c + s",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("union()"));
    assert!(scad.contains("cube("));
    assert!(scad.contains("sphere("));
  }

  #[test]
  fn combining_shapes_difference() {
    let nodes = run_lua_scad(
      "local c = cube { size = { 10, 10, 10 } }\n\
       local s = sphere { r = 5 }\n\
       return c - s",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("difference()"));
  }

  #[test]
  fn combining_shapes_intersection() {
    let nodes = run_lua_scad(
      "local c = cube { 10, 10, 10, center = true }\n\
       local s = sphere(7)\n\
       return c * s",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("intersection()"));
  }

  #[test]
  fn modifier_skip() {
    let nodes = run_lua_scad("return cube { size = { 10, 10, 10 } }:skip()");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("*cube("), "skip modifier (*) not found");
  }

  #[test]
  fn modifier_only() {
    let nodes = run_lua_scad("return cube { size = { 10, 10, 10 } }:only()");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("!cube("), "only modifier (!) not found");
  }

  #[test]
  fn modifier_debug() {
    let nodes = run_lua_scad("return cube { size = { 10, 10, 10 } }:debug()");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("#cube("), "debug modifier (#) not found");
  }

  #[test]
  fn modifier_transparent() {
    let nodes =
      run_lua_scad("return cube { size = { 10, 10, 10 } }:transparent()");
    let scad = generate_scad(&nodes);
    assert!(
      scad.contains("%cube("),
      "transparent modifier (%) not found"
    );
  }

  #[test]
  fn modifier_global_s_function() {
    let nodes = run_lua_scad("return s(cube { size = { 10, 10, 10 } })");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("*cube("), "s() should produce skip modifier");
  }

  #[test]
  fn modifier_global_o_function() {
    let nodes = run_lua_scad("return o(cube { size = { 10, 10, 10 } })");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("!cube("), "o() should produce only modifier");
  }

  // =========================================================================
  // Method-based boolean tests (add/sub/intersect)
  // =========================================================================

  #[test]
  fn method_add() {
    let nodes = run_lua_scad(
      "local c = cube(5, 5, 5)\n\
       local s = sphere(3)\n\
       return c:add(s)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("union()"));
  }

  #[test]
  fn method_sub() {
    let nodes = run_lua_scad(
      "local c = cube(10, 10, 10)\n\
       local s = sphere(5)\n\
       return c:sub(s)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("difference()"));
  }

  #[test]
  fn method_intersect() {
    let nodes = run_lua_scad(
      "local c = cube { 10, 10, 10, center = true }\n\
       local s = sphere(7)\n\
       return c:intersect(s)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("intersection()"));
  }

  // =========================================================================
  // Hull test
  // =========================================================================

  #[test]
  fn hull_geometry() {
    let nodes = run_lua_scad("return cube(5, 5, 5):hull()");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("hull()"));
  }

  // =========================================================================
  // Rotate test
  // =========================================================================

  #[test]
  fn rotate_basic() {
    let nodes = run_lua_scad("return cube(5, 5, 5):rotate(90, 0, 0)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("rotate([90, 0, 0])"));
  }

  #[test]
  fn rotate_around_center() {
    let nodes = run_lua_scad("return cube(5, 5, 5):rotate(5, 5, 5, 0, 0, 45)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("translate([5, 5, 5])"));
    assert!(scad.contains("rotate([0, 0, 45])"));
    assert!(scad.contains("translate([-5, -5, -5])"));
  }

  // =========================================================================
  // Scale and resize tests
  // =========================================================================

  #[test]
  fn scale_basic() {
    let nodes = run_lua_scad("return cube(5, 5, 5):scale(2, 1, 1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("scale([2, 1, 1])"));
  }

  #[test]
  fn resize_basic() {
    let nodes = run_lua_scad("return cube(5, 5, 5):resize(10, 5, 5)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("resize([10, 5, 5])"));
  }

  // =========================================================================
  // Mirror test
  // =========================================================================

  #[test]
  fn mirror_basic() {
    let nodes = run_lua_scad("return cube(5, 5, 5):mirror(1, 0, 0)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("mirror([1, 0, 0])"));
  }

  // =========================================================================
  // Color with alpha test
  // =========================================================================

  #[test]
  fn color_with_alpha() {
    let nodes = run_lua_scad("return cube(5, 5, 5):color(1, 0, 0, 0.5)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("color([1, 0, 0, 0.5])"));
  }

  // =========================================================================
  // Linear extrude with options
  // =========================================================================

  #[test]
  fn linear_extrude_centered() {
    let nodes = run_lua_scad(
      "return circle(5):linear_extrude { height = 10, center = true }",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("linear_extrude(height = 10, center = true)"));
  }

  #[test]
  fn linear_extrude_with_twist() {
    let nodes = run_lua_scad(
      "return circle(5):linear_extrude { height = 10, twist = 45 }",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("linear_extrude(height = 10, twist = 45)"));
  }

  // =========================================================================
  // Polygon test
  // =========================================================================

  #[test]
  fn polygon_basic() {
    let nodes = run_lua_scad(
      "return polygon { {0, 0}, {10, 0}, {5, 10} }:linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("polygon("));
    assert!(scad.contains("[0, 0]"));
    assert!(scad.contains("[10, 0]"));
    assert!(scad.contains("[5, 10]"));
  }

  #[test]
  fn polygon_with_points_key() {
    let nodes = run_lua_scad(
      "return polygon { points = { {0, 0}, {10, 0}, {5, 10} } }:linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("polygon("));
  }

  // =========================================================================
  // Polyhedron test
  // =========================================================================

  #[test]
  fn polyhedron_basic() {
    let nodes = run_lua_scad(
      "return polyhedron {\n\
         points = {\n\
           {0, 0, 0}, {1, 0, 0}, {0, 1, 0}, {0, 0, 1}\n\
         },\n\
         faces = {\n\
           {0, 1, 2}, {0, 3, 1}, {0, 2, 3}, {1, 3, 2}\n\
         }\n\
       }",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("polyhedron("));
    assert!(scad.contains("points = ["));
    assert!(scad.contains("faces = ["));
  }

  // =========================================================================
  // Math globals tests
  // =========================================================================

  #[test]
  fn math_globals_trig() {
    execute_lua(
      "assert(abs(-5) == 5)\n\
       assert(math.abs(sin(90) - 1) < 0.001)\n\
       assert(math.abs(cos(0) - 1) < 0.001)\n\
       assert(math.abs(cos(90)) < 0.001)\n\
       assert(floor(3.7) == 3)\n\
       assert(ceil(3.2) == 4)\n\
       assert(sqrt(9) == 3)\n\
       return cube(1)",
    )
    .expect("math globals test failed");
  }

  #[test]
  fn math_globals_pi() {
    execute_lua(
      "assert(math.abs(PI - 3.14159265358979) < 0.0001)\n\
       return cube(1)",
    )
    .expect("PI test failed");
  }

  #[test]
  fn round_function() {
    execute_lua(
      "assert(round(3.5) == 4)\n\
       assert(round(3.4) == 3)\n\
       assert(round(-1.5) == -2)\n\
       return cube(1)",
    )
    .expect("round test failed");
  }

  // =========================================================================
  // Type checking functions
  // =========================================================================

  #[test]
  fn type_checking_functions() {
    execute_lua(
      "assert(is_num(42))\n\
       assert(not is_num('hello'))\n\
       assert(is_str('hello'))\n\
       assert(not is_str(42))\n\
       assert(is_bool(true))\n\
       assert(not is_bool(42))\n\
       assert(is_table({}))\n\
       assert(not is_table(42))\n\
       assert(is_func(print))\n\
       assert(not is_func(42))\n\
       return cube(1)",
    )
    .expect("type checking functions test failed");
  }

  // =========================================================================
  // Vector tests (adapted from LuaCAD test_vector_cross.lua)
  // =========================================================================

  #[test]
  fn vector_cross_product() {
    execute_lua(
      "local a = vec(1, 0, 0)\n\
       local b = vec(0, 1, 0)\n\
       local c = a:cross(b)\n\
       assert(c:getx() == 0)\n\
       assert(c:gety() == 0)\n\
       assert(c:getz() == 1)\n\
       return cube(1)",
    )
    .expect("vector cross product test failed");
  }

  #[test]
  fn vector_operations() {
    execute_lua(
      "local a = vec(1, 2, 3)\n\
       local b = vec(4, 5, 6)\n\
       local c = a + b\n\
       assert(c:getx() == 5)\n\
       assert(c:gety() == 7)\n\
       assert(c:getz() == 9)\n\
       local d = a - b\n\
       assert(d:getx() == -3)\n\
       assert(d:gety() == -3)\n\
       assert(d:getz() == -3)\n\
       return cube(1)",
    )
    .expect("vector operations test failed");
  }

  #[test]
  fn vector_scalar_multiply() {
    execute_lua(
      "local a = vec(1, 2, 3)\n\
       local b = a * 2\n\
       assert(b:getx() == 2)\n\
       assert(b:gety() == 4)\n\
       assert(b:getz() == 6)\n\
       return cube(1)",
    )
    .expect("vector scalar multiply test failed");
  }

  #[test]
  fn vector_length() {
    execute_lua(
      "local a = vec(3, 4, 0)\n\
       assert(math.abs(a:len() - 5) < 0.001)\n\
       return cube(1)",
    )
    .expect("vector length test failed");
  }

  #[test]
  fn vector_unit() {
    execute_lua(
      "local a = vec(3, 0, 0)\n\
       local u = a:unit()\n\
       assert(math.abs(u:getx() - 1) < 0.001)\n\
       assert(math.abs(u:gety()) < 0.001)\n\
       assert(math.abs(u:getz()) < 0.001)\n\
       return cube(1)",
    )
    .expect("vector unit test failed");
  }

  #[test]
  fn vector_negate() {
    execute_lua(
      "local a = vec(1, 2, 3)\n\
       local b = -a\n\
       assert(b:getx() == -1)\n\
       assert(b:gety() == -2)\n\
       assert(b:getz() == -3)\n\
       return cube(1)",
    )
    .expect("vector negate test failed");
  }

  #[test]
  fn global_cross_function() {
    execute_lua(
      "local a = vec(1, 0, 0)\n\
       local b = vec(0, 1, 0)\n\
       local c = cross(a, b)\n\
       assert(c:getx() == 0)\n\
       assert(c:gety() == 0)\n\
       assert(c:getz() == 1)\n\
       return cube(1)",
    )
    .expect("global cross function test failed");
  }

  #[test]
  fn global_norm_function() {
    execute_lua(
      "local a = vec(3, 4, 0)\n\
       assert(math.abs(norm(a) - 5) < 0.001)\n\
       return cube(1)",
    )
    .expect("global norm function test failed");
  }

  // =========================================================================
  // Auto-render from top-level return
  // =========================================================================

  #[test]
  fn auto_render_top_level_return() {
    let geoms =
      execute_lua("return cube(5, 5, 5)").expect("auto-render failed");
    assert_eq!(geoms.len(), 1);
    assert!(geoms[0].scad.is_some());
  }

  // =========================================================================
  // Text tests
  // =========================================================================

  #[test]
  fn text_basic() {
    let nodes = run_lua_scad("return text('Hello'):linear_extrude(1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("text(\"Hello\""));
  }

  #[test]
  fn text3d_basic() {
    let nodes = run_lua_scad("return text3d('World')");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("linear_extrude("));
    assert!(scad.contains("text(\"World\""));
  }

  // =========================================================================
  // Torus test
  // =========================================================================

  #[test]
  fn torus_basic() {
    let nodes = run_lua_scad("return torus { R = 5, r = 1 }");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("rotate_extrude("));
    assert!(scad.contains("translate([5, 0, 0])"));
    assert!(scad.contains("circle(r = 1"));
  }

  // =========================================================================
  // Pyramid test
  // =========================================================================

  #[test]
  fn pyramid_basic() {
    let nodes = run_lua_scad("return pyramid(0, 0, 0, 10, 5)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("polyhedron("));
  }

  // =========================================================================
  // Concat utility
  // =========================================================================

  #[test]
  fn concat_utility() {
    execute_lua(
      "local a = {1, 2, 3}\n\
       local b = {4, 5, 6}\n\
       local c = concat(a, b)\n\
       assert(#c == 6)\n\
       assert(c[1] == 1)\n\
       assert(c[6] == 6)\n\
       return cube(1)",
    )
    .expect("concat utility test failed");
  }

  // =========================================================================
  // Lookup function
  // =========================================================================

  #[test]
  fn lookup_interpolation() {
    execute_lua(
      "local t = {{0, 0}, {10, 100}}\n\
       assert(lookup(5, t) == 50, 'expected 50 at midpoint')\n\
       assert(lookup(0, t) == 0, 'expected 0 at start')\n\
       assert(lookup(10, t) == 100, 'expected 100 at end')\n\
       return cube(1)",
    )
    .expect("lookup interpolation test failed");
  }

  // =========================================================================
  // Settings object
  // =========================================================================

  #[test]
  fn settings_object_exists() {
    execute_lua(
      "assert(settings ~= nil)\n\
       assert(settings.preview == true)\n\
       return cube(1)",
    )
    .expect("settings object test failed");
  }

  // =========================================================================
  // Import and surface (ScadNode-only)
  // =========================================================================

  #[test]
  fn import_basic() {
    let nodes = run_lua_scad("return import('model.stl')");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("import(\"model.stl\")"));
  }

  #[test]
  fn surface_basic() {
    let nodes = run_lua_scad("return surface('heightmap.dat')");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("surface(file = \"heightmap.dat\""));
  }

  // =========================================================================
  // Rands function
  // =========================================================================

  #[test]
  fn rands_function() {
    execute_lua(
      "local r = rands(0, 10, 5, 42)\n\
       assert(#r == 5, 'expected 5 random numbers')\n\
       for i = 1, 5 do\n\
         assert(r[i] >= 0 and r[i] <= 10, 'out of range')\n\
       end\n\
       return cube(1)",
    )
    .expect("rands function test failed");
  }

  // =========================================================================
  // Version function
  // =========================================================================

  #[test]
  fn version_function() {
    execute_lua(
      "local v = version()\n\
       assert(type(v) == 'table')\n\
       assert(#v == 3)\n\
       return cube(1)",
    )
    .expect("version function test failed");
  }

  // =========================================================================
  // 2D sketch operations
  // =========================================================================

  #[test]
  fn sketch_union() {
    let nodes = run_lua_scad(
      "local a = circle(5)\n\
       local b = circle(3):translate(4, 0)\n\
       return (a + b):linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("union()"));
    assert!(scad.contains("circle("));
  }

  #[test]
  fn sketch_difference() {
    let nodes = run_lua_scad(
      "local a = circle(5)\n\
       local b = circle(3)\n\
       return (a - b):linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("difference()"));
  }

  #[test]
  fn sketch_intersection() {
    let nodes = run_lua_scad(
      "local a = circle(5)\n\
       local b = circle(5):translate(3, 0)\n\
       return (a * b):linear_extrude(1)",
    );
    let scad = generate_scad(&nodes);
    assert!(scad.contains("intersection()"));
  }

  // =========================================================================
  // Offset tests
  // =========================================================================

  #[test]
  fn sketch_offset() {
    let nodes = run_lua_scad("return circle(5):offset(2):linear_extrude(1)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("offset(delta = 2)"));
  }

  // =========================================================================
  // Clone/copy
  // =========================================================================

  #[test]
  fn clone_geometry() {
    execute_lua(
      "local a = cube(5, 5, 5)\n\
       local b = a:clone()\n\
       return b",
    )
    .expect("clone test failed");
  }

  #[test]
  fn copy_geometry() {
    execute_lua(
      "local a = cube(5, 5, 5)\n\
       local b = a:copy()\n\
       return b",
    )
    .expect("copy test failed");
  }

  // =========================================================================
  // Render_node (ScadNode wrapper)
  // =========================================================================

  #[test]
  fn render_node_basic() {
    let nodes = run_lua_scad("return cube(5, 5, 5):render_node()");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("render()"));
  }

  #[test]
  fn render_node_with_convexity() {
    let nodes = run_lua_scad("return cube(5, 5, 5):render_node(10)");
    let scad = generate_scad(&nodes);
    assert!(scad.contains("render(convexity = 10)"));
  }
}
