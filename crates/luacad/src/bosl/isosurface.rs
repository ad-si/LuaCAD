//! BOSL2's `isosurface.scad`: metaballs and level sets.
//!
//! Every metaball is a field that is strongest at its centre and fades with
//! distance. Add several together and the surface where the total reaches
//! the isovalue bulges out around each one and merges smoothly where two are
//! close — which is what makes metaballs worth having over a union.
//!
//! The surface is found by marching over a grid of voxels. Each voxel is cut
//! into six tetrahedra and each tetrahedron contributes the flat piece of
//! surface crossing it; going by tetrahedra rather than whole cubes means no
//! lookup table and, more usefully, no ambiguous cases that could leave a
//! hole in the mesh.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, Val};
use crate::bosl::vecmath::Mat4;
use crate::bosl::vnf::Vnf;

/// What a metaball is shaped like, and how strongly it reaches out.
#[derive(Clone, Debug)]
struct Ball {
  kind: Kind,
  /// How far the field reaches before it is cut off entirely.
  cutoff: f64,
  /// The exponent the field is raised to: above 1 it falls off faster.
  exponent: f64,
  /// `-1` for a ball that hollows the surface out instead of filling it.
  sign: f64,
  /// Where the ball sits, as the inverse of its placement.
  inverse: Mat4,
}

#[derive(Clone, Debug)]
enum Kind {
  Sphere {
    r: f64,
  },
  Cuboid {
    half: [f64; 3],
    xp: f64,
  },
  Octahedron {
    r: f64,
  },
  Torus {
    r_maj: f64,
    r_min: f64,
  },
  Capsule {
    half_h: f64,
    r: f64,
  },
  Disk {
    half_h: f64,
    r: f64,
  },
  /// A revolved outline, as `[radius, z]` pairs, used for cylinders.
  RevSurf {
    profile: Vec<[f64; 2]>,
    coef: f64,
  },
}

/// How much a field is damped at `dist`, given where it is cut off.
///
/// The taper is a raised cosine in the fourth power of the distance, so the
/// field leaves its full strength gently and arrives at zero with zero
/// slope — no crease where one ball's influence ends.
fn cutoff_factor(dist: f64, cutoff: f64) -> f64 {
  if !cutoff.is_finite() {
    return 1.0;
  }
  if dist >= cutoff {
    return 0.0;
  }
  0.5 * ((std::f64::consts::PI * (dist / cutoff).powi(4)).cos() + 1.0)
}

fn norm2(x: f64, y: f64) -> f64 {
  (x * x + y * y).sqrt()
}

fn norm3(v: [f64; 3]) -> f64 {
  (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The shortest distance from a point to a polyline, and which side of it
/// the point falls on.
fn profile_distance(profile: &[[f64; 2]], p: [f64; 2]) -> (f64, bool) {
  let mut best = f64::INFINITY;
  let mut inside = true;
  for seg in profile.windows(2) {
    let c = [seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]];
    let s0 = [seg[0][0] - p[0], seg[0][1] - p[1]];
    let cc = c[0] * c[0] + c[1] * c[1];
    let t = if cc < 1e-18 {
      0.0
    } else {
      -(s0[0] * c[0] + s0[1] * c[1]) / cc
    };
    let d = if t < 0.0 {
      norm2(s0[0], s0[1])
    } else if t > 1.0 {
      norm2(seg[1][0] - p[0], seg[1][1] - p[1])
    } else {
      norm2(s0[0] + t * c[0], s0[1] + t * c[1])
    };
    best = best.min(d);
    // Outside if the point is to the left of any segment.
    if c[0] * (p[1] - seg[0][1]) - c[1] * (p[0] - seg[0][0]) > 1e-9 {
      inside = false;
    }
  }
  (best, inside)
}

impl Ball {
  /// This ball's contribution to the field at a world point.
  fn field(&self, world: [f64; 3]) -> f64 {
    let dv = self.inverse.apply(world);
    let (base, dist) = match &self.kind {
      Kind::Sphere { r } => {
        let d = norm3(dv);
        (r / d.max(1e-12), d)
      }
      Kind::Cuboid { half, xp } => {
        let s = [dv[0] / half[0], dv[1] / half[1], dv[2] / half[2]];
        let d = if *xp >= 1100.0 {
          s[0].abs().max(s[1].abs()).max(s[2].abs())
        } else {
          (s[0].abs().powf(*xp) + s[1].abs().powf(*xp) + s[2].abs().powf(*xp))
            .powf(1.0 / *xp)
        };
        (1.0 / d.max(1e-12), d)
      }
      Kind::Octahedron { r } => {
        let d = dv[0].abs() + dv[1].abs() + dv[2].abs();
        (r / d.max(1e-12), d)
      }
      Kind::Torus { r_maj, r_min } => {
        let d = norm2(norm2(dv[0], dv[1]) - r_maj, dv[2]);
        (r_min / d.max(1e-12), d)
      }
      Kind::Capsule { half_h, r } => {
        // Distance to the segment running along Z between the two ends.
        let d = if dv[2] < -half_h {
          norm3([dv[0], dv[1], dv[2] + half_h])
        } else if dv[2] <= *half_h {
          norm2(dv[0], dv[1])
        } else {
          norm3([dv[0], dv[1], dv[2] - half_h])
        };
        (r / d.max(1e-12), d)
      }
      Kind::Disk { half_h, r } => {
        let rd = norm2(dv[0], dv[1]);
        let d = if rd < *r {
          dv[2].abs()
        } else {
          norm2(rd - r, dv[2])
        };
        (half_h / d.max(1e-12), d)
      }
      Kind::RevSurf { profile, coef } => {
        let (d, inside) =
          profile_distance(profile, [norm2(dv[0], dv[1]), dv[2]]);
        // Inside the outline the field grows with distance from the surface
        // rather than shrinking, which is what fills the shape in.
        let base = if inside {
          coef * (1.0 + d)
        } else {
          coef / (1.0 + d)
        };
        return self.sign
          * cutoff_factor((d - coef).max(0.0), self.cutoff)
          * base.powf(self.exponent);
      }
    };
    self.sign * cutoff_factor(dist, self.cutoff) * base.powf(self.exponent)
  }
}

// ---------------------------------------------------------------------------
// Reading the ball descriptions
// ---------------------------------------------------------------------------

fn table_num(t: &mlua::Table, key: &str) -> Option<f64> {
  t.get::<LuaValue>(key)
    .ok()
    .as_ref()
    .and_then(crate::bosl::args::as_num)
}

fn table_nums(t: &mlua::Table, key: &str) -> Option<Vec<f64>> {
  t.get::<LuaValue>(key)
    .ok()
    .as_ref()
    .and_then(crate::bosl::args::as_nums)
}

/// Read one ball description, as an `mb_*` constructor left it.
fn read_ball(t: &mlua::Table, placement: Mat4) -> Result<Ball, String> {
  let kind_name = match t.get::<LuaValue>("kind") {
    Ok(LuaValue::String(s)) => {
      s.to_str().map(|s| s.to_string()).unwrap_or_default()
    }
    _ => {
      return Err(
        "not a metaball; build one with bosl.mb_sphere and friends".into(),
      );
    }
  };
  let kind = match kind_name.as_str() {
    "sphere" => Kind::Sphere {
      r: table_num(t, "r").unwrap_or(1.0),
    },
    "cuboid" => {
      let size = table_nums(t, "size").unwrap_or_else(|| vec![1.0; 3]);
      let squareness = table_num(t, "squareness").unwrap_or(0.5);
      Kind::Cuboid {
        half: [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0],
        // Squareness runs from a sphere at 0 to a hard box at 1, which is
        // the p-norm exponent running from 2 up to effectively infinite.
        xp: if squareness >= 1.0 {
          1100.0
        } else {
          2.0 / (1.0 - squareness).max(1e-6)
        },
      }
    }
    "octahedron" => Kind::Octahedron {
      r: table_num(t, "r").unwrap_or(1.0),
    },
    "torus" => Kind::Torus {
      r_maj: table_num(t, "r_maj").unwrap_or(1.0),
      r_min: table_num(t, "r_min").unwrap_or(0.25),
    },
    "capsule" => Kind::Capsule {
      half_h: table_num(t, "h").unwrap_or(1.0) / 2.0,
      r: table_num(t, "r").unwrap_or(1.0),
    },
    "disk" => Kind::Disk {
      half_h: table_num(t, "h").unwrap_or(1.0) / 2.0,
      r: table_num(t, "r").unwrap_or(1.0),
    },
    "cyl" => {
      let r1 = table_num(t, "r1").unwrap_or(1.0);
      let r2 = table_num(t, "r2").unwrap_or(r1);
      let h = table_num(t, "h").unwrap_or(1.0);
      let rounding = table_num(t, "rounding").unwrap_or(0.0);
      // The outline of the cone, pulled in by the rounding so the revolved
      // field has the rounded profile rather than a sharp corner.
      let inset = rounding.max(0.0);
      Kind::RevSurf {
        profile: vec![
          [0.0, h / 2.0 - inset],
          [(r2 - inset).max(0.0), h / 2.0 - inset],
          [(r1 - inset).max(0.0), -h / 2.0 + inset],
          [0.0, -h / 2.0 + inset],
        ],
        coef: 1.0 + rounding,
      }
    }
    other => return Err(format!("unknown metaball kind '{other}'")),
  };
  // A connector is a capsule laid between two points, so its placement is
  // folded into the transform rather than the shape.
  let extra = match t.get::<LuaValue>("between") {
    Ok(LuaValue::Table(ends)) => {
      let p1 = crate::bosl::args::as_nums(
        &ends.get::<LuaValue>(1).unwrap_or(LuaValue::Nil),
      )
      .map(|v| crate::bosl::value::v3(&v));
      let p2 = crate::bosl::args::as_nums(
        &ends.get::<LuaValue>(2).unwrap_or(LuaValue::Nil),
      )
      .map(|v| crate::bosl::value::v3(&v));
      match (p1, p2) {
        (Some(p1), Some(p2)) => {
          let mid = [
            (p1[0] + p2[0]) / 2.0,
            (p1[1] + p2[1]) / 2.0,
            (p1[2] + p2[2]) / 2.0,
          ];
          let axis = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
          Mat4::translate(mid).mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], axis))
        }
        _ => Mat4::identity(),
      }
    }
    _ => Mat4::identity(),
  };

  let placement = placement.mul(&extra);
  Ok(Ball {
    kind,
    cutoff: table_num(t, "cutoff").unwrap_or(f64::INFINITY),
    exponent: 1.0 / table_num(t, "influence").unwrap_or(1.0),
    sign: match t.get::<LuaValue>("negative") {
      Ok(LuaValue::Boolean(true)) => -1.0,
      _ => 1.0,
    },
    inverse: invert_rigid(&placement),
  })
}

/// The inverse of a rotation-and-translation.
fn invert_rigid(m: &Mat4) -> Mat4 {
  let mut out = Mat4::identity();
  for r in 0..3 {
    for c in 0..3 {
      out.0[r * 4 + c] = m.0[c * 4 + r];
    }
  }
  for r in 0..3 {
    out.0[r * 4 + 3] = -(0..3)
      .map(|k| out.0[r * 4 + k] * m.0[k * 4 + 3])
      .sum::<f64>();
  }
  out
}

// ---------------------------------------------------------------------------
// Marching tetrahedra
// ---------------------------------------------------------------------------

/// The six tetrahedra a unit cube splits into, by corner index.
///
/// Every one of them shares the cube's main diagonal, which is what makes
/// the pieces line up across neighbouring cubes with no cracks.
const TETS: [[usize; 4]; 6] = [
  [0, 5, 1, 6],
  [0, 1, 2, 6],
  [0, 2, 3, 6],
  [0, 3, 7, 6],
  [0, 7, 4, 6],
  [0, 4, 5, 6],
];

/// The eight corners of a voxel, in the order [`TETS`] indexes them.
const CORNERS: [[usize; 3]; 8] = [
  [0, 0, 0],
  [1, 0, 0],
  [1, 1, 0],
  [0, 1, 0],
  [0, 0, 1],
  [1, 0, 1],
  [1, 1, 1],
  [0, 1, 1],
];

/// The gradient of the linear field over one tetrahedron.
///
/// Three edges from the first corner give three directional derivatives, and
/// solving that 3×3 system recovers the gradient. `None` when the corners are
/// flat enough that the system has no answer.
fn tet_gradient(
  tet: &[usize; 4],
  grid: &[[usize; 3]],
  vals: &[f64],
  at: &impl Fn(usize, usize, usize) -> [f64; 3],
) -> Option<[f64; 3]> {
  let p0 = at(grid[tet[0]][0], grid[tet[0]][1], grid[tet[0]][2]);
  let v0 = vals[tet[0]];
  let mut m = [[0.0f64; 3]; 3];
  let mut rhs = [0.0f64; 3];
  for (row, c) in tet[1..].iter().enumerate() {
    let p = at(grid[*c][0], grid[*c][1], grid[*c][2]);
    m[row] = [p[0] - p0[0], p[1] - p0[1], p[2] - p0[2]];
    rhs[row] = vals[*c] - v0;
  }
  let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
    - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
    + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
  if det.abs() < 1e-18 {
    return None;
  }
  // Cramer's rule: cheap and stable enough at three unknowns.
  let solve = |col: usize| {
    let mut a = m;
    for (row, r) in rhs.iter().enumerate() {
      a[row][col] = *r;
    }
    (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
      - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
      + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
      / det
  };
  let g = [solve(0), solve(1), solve(2)];
  if g.iter().all(|c| c.abs() < 1e-18) {
    return None;
  }
  Some(g)
}

/// Build the surface where `field` crosses `isovalue`.
fn march(
  field: &dyn Fn([f64; 3]) -> f64,
  lo: [f64; 3],
  hi: [f64; 3],
  step: [f64; 3],
  isovalue: f64,
  reverse: bool,
) -> Vnf {
  let counts: Vec<usize> = (0..3)
    .map(|k| (((hi[k] - lo[k]) / step[k]).ceil() as usize).max(1))
    .collect();
  let at = |i: usize, j: usize, k: usize| {
    [
      lo[0] + i as f64 * step[0],
      lo[1] + j as f64 * step[1],
      lo[2] + k as f64 * step[2],
    ]
  };

  // Sample once per grid point rather than once per tetrahedron corner.
  let (nx, ny, nz) = (counts[0] + 1, counts[1] + 1, counts[2] + 1);
  let mut values = vec![0.0f64; nx * ny * nz];
  for k in 0..nz {
    for j in 0..ny {
      for i in 0..nx {
        values[(k * ny + j) * nx + i] = field(at(i, j, k)) - isovalue;
      }
    }
  }
  let sample = |i: usize, j: usize, k: usize| values[(k * ny + j) * nx + i];

  let mut points: Vec<[f64; 3]> = Vec::new();
  let mut faces: Vec<Vec<usize>> = Vec::new();
  // Where an edge has already been cut, so the two tetrahedra sharing it
  // use the same vertex and the surface comes out watertight.
  let mut cut: std::collections::HashMap<(u64, u64), usize> =
    std::collections::HashMap::new();

  let mut vertex_on = |points: &mut Vec<[f64; 3]>,
                       a: ([usize; 3], f64),
                       b: ([usize; 3], f64)|
   -> usize {
    let key_of =
      |g: [usize; 3]| (g[0] as u64) << 42 | (g[1] as u64) << 21 | g[2] as u64;
    let (ka, kb) = (key_of(a.0), key_of(b.0));
    let key = if ka < kb { (ka, kb) } else { (kb, ka) };
    if let Some(i) = cut.get(&key) {
      return *i;
    }
    let (first, second) = if ka < kb { (a, b) } else { (b, a) };
    let (pa, va) = (at(first.0[0], first.0[1], first.0[2]), first.1);
    let (pb, vb) = (at(second.0[0], second.0[1], second.0[2]), second.1);
    let t = if (vb - va).abs() < 1e-18 {
      0.5
    } else {
      (-va / (vb - va)).clamp(0.0, 1.0)
    };
    points.push([
      pa[0] + (pb[0] - pa[0]) * t,
      pa[1] + (pb[1] - pa[1]) * t,
      pa[2] + (pb[2] - pa[2]) * t,
    ]);
    cut.insert(key, points.len() - 1);
    points.len() - 1
  };

  for k in 0..counts[2] {
    for j in 0..counts[1] {
      for i in 0..counts[0] {
        let grid: Vec<[usize; 3]> = CORNERS
          .iter()
          .map(|c| [i + c[0], j + c[1], k + c[2]])
          .collect();
        let vals: Vec<f64> =
          grid.iter().map(|g| sample(g[0], g[1], g[2])).collect();

        for tet in &TETS {
          // Split the tetrahedron's corners by which side they are on.
          let inside: Vec<usize> =
            tet.iter().copied().filter(|c| vals[*c] >= 0.0).collect();
          let outside: Vec<usize> =
            tet.iter().copied().filter(|c| vals[*c] < 0.0).collect();
          if inside.is_empty() || outside.is_empty() {
            continue;
          }
          // The six tetrahedra a cube splits into do not all wind the same
          // way, so rather than trust the corner order, each triangle is
          // turned to face down the field — from the solid side towards the
          // empty one. Over one tetrahedron the field is exactly the linear
          // interpolation the cut points came from, so its gradient is the
          // exact surface normal there and every triangle agrees with its
          // neighbours.
          let outward = match tet_gradient(tet, &grid, &vals, &at) {
            Some(g) => [-g[0], -g[1], -g[2]],
            // A tetrahedron whose corners are degenerate has no gradient to
            // read; nothing crosses it, so nothing is emitted.
            None => continue,
          };
          let mut tri = |points: &mut Vec<[f64; 3]>,
                         faces: &mut Vec<Vec<usize>>,
                         edges: [(usize, usize); 3]| {
            let v: Vec<usize> = edges
              .iter()
              .map(|(a, b)| {
                vertex_on(points, (grid[*a], vals[*a]), (grid[*b], vals[*b]))
              })
              .collect();
            if v[0] == v[1] || v[1] == v[2] || v[2] == v[0] {
              return;
            }
            let (p, q, r) = (points[v[0]], points[v[1]], points[v[2]]);
            let e1 = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
            let e2 = [r[0] - p[0], r[1] - p[1], r[2] - p[2]];
            let n = [
              e1[1] * e2[2] - e1[2] * e2[1],
              e1[2] * e2[0] - e1[0] * e2[2],
              e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let facing =
              n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2];
            faces.push(if facing >= 0.0 {
              vec![v[0], v[1], v[2]]
            } else {
              vec![v[2], v[1], v[0]]
            });
          };

          match (inside.len(), outside.len()) {
            // One corner on its own: a single triangle cutting it off.
            (1, 3) => {
              let a = inside[0];
              tri(
                &mut points,
                &mut faces,
                [(a, outside[0]), (a, outside[1]), (a, outside[2])],
              );
            }
            (3, 1) => {
              let a = outside[0];
              tri(
                &mut points,
                &mut faces,
                [(a, inside[0]), (a, inside[1]), (a, inside[2])],
              );
            }
            // Two and two: a quad across the middle, as two triangles.
            (2, 2) => {
              let (a, b) = (inside[0], inside[1]);
              let (c, d) = (outside[0], outside[1]);
              tri(&mut points, &mut faces, [(a, c), (b, c), (a, d)]);
              tri(&mut points, &mut faces, [(b, c), (b, d), (a, d)]);
            }
            _ => {}
          }
        }
      }
    }
  }

  let vnf = Vnf { points, faces };
  if reverse { vnf.reversed() } else { vnf }
}

// ---------------------------------------------------------------------------
// The Lua surface
// ---------------------------------------------------------------------------

/// Build the descriptor table an `mb_*` constructor hands back.
fn ball_table(
  lua: &Lua,
  kind: &str,
  fields: &[(&str, f64)],
  a: &Args,
) -> LuaResult<LuaValue> {
  let t = lua.create_table()?;
  t.set("kind", kind)?;
  for (k, v) in fields {
    t.set(*k, *v)?;
  }
  if let Some(c) = a.num("cutoff") {
    if c <= 0.0 {
      return a.err("cutoff must be positive");
    }
    t.set("cutoff", c)?;
  }
  if let Some(i) = a.num("influence") {
    if i <= 0.0 {
      return a.err("influence must be positive");
    }
    t.set("influence", i)?;
  }
  if a.bool_or("negative", false) {
    t.set("negative", true)?;
  }
  Ok(LuaValue::Table(t))
}

fn mb_sphere(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(r) = a.radius("r", "d", None) else {
    return a.err("a radius or diameter is required");
  };
  if r <= 0.0 {
    return a.err("the radius must be positive");
  }
  ball_table(lua, "sphere", &[("r", r)], a)
}

fn mb_cuboid(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.sized("size", 3).unwrap_or_else(|| vec![1.0; 3]);
  if size.iter().any(|s| *s <= 0.0) {
    return a.err("size must be positive");
  }
  let squareness = a.num_or("squareness", 0.5).clamp(0.0, 1.0);
  let t = lua.create_table()?;
  t.set("kind", "cuboid")?;
  t.set("size", Val::vec(size).to_lua(lua)?)?;
  t.set("squareness", squareness)?;
  if let Some(c) = a.num("cutoff") {
    t.set("cutoff", c)?;
  }
  if let Some(i) = a.num("influence") {
    t.set("influence", i)?;
  }
  if a.bool_or("negative", false) {
    t.set("negative", true)?;
  }
  Ok(LuaValue::Table(t))
}

fn mb_octahedron(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(r) = a.radius("r", "d", None) else {
    return a.err("a radius or diameter is required");
  };
  ball_table(lua, "octahedron", &[("r", r)], a)
}

fn mb_torus(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r_maj = a.radius("r_maj", "d_maj", None).or_else(|| {
    match (a.num("or"), a.num("ir")) {
      (Some(o), Some(i)) => Some((o + i) / 2.0),
      _ => None,
    }
  });
  let r_min = a.radius("r_min", "d_min", None).or_else(|| {
    match (a.num("or"), a.num("ir")) {
      (Some(o), Some(i)) => Some((o - i) / 2.0),
      _ => None,
    }
  });
  let (Some(r_maj), Some(r_min)) = (r_maj, r_min) else {
    return a.err("a major and minor radius are required");
  };
  ball_table(lua, "torus", &[("r_maj", r_maj), ("r_min", r_min)], a)
}

fn length_of(a: &Args) -> Option<f64> {
  a.num("h")
    .or_else(|| a.num("l"))
    .or_else(|| a.num("height"))
    .or_else(|| a.num("length"))
}

fn mb_capsule(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (Some(h), Some(r)) = (length_of(a), a.radius("r", "d", None)) else {
    return a.err("a length and a radius are required");
  };
  ball_table(lua, "capsule", &[("h", h), ("r", r)], a)
}

fn mb_disk(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (Some(h), Some(r)) = (length_of(a), a.radius("r", "d", None)) else {
    return a.err("a thickness and a radius are required");
  };
  ball_table(lua, "disk", &[("h", h), ("r", r)], a)
}

fn mb_cyl(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(h) = length_of(a) else {
    return a.err("a length is required");
  };
  let r = a.radius("r", "d", None);
  let r1 = a.radius("r1", "d1", None).or(r);
  let r2 = a.radius("r2", "d2", None).or(r);
  let (Some(r1), Some(r2)) = (r1, r2) else {
    return a.err("a radius is required");
  };
  let rounding = a.num_or("rounding", 0.0);
  if rounding < 0.0 {
    return a.err("rounding must not be negative");
  }
  if rounding > r1.min(r2) {
    return a.err("the rounding is larger than the radius it has to fit in");
  }
  ball_table(
    lua,
    "cyl",
    &[("h", h), ("r1", r1), ("r2", r2), ("rounding", rounding)],
    a,
  )
}

fn mb_connector(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (Some(p1), Some(p2)) = (a.vec3("p1"), a.vec3("p2")) else {
    return a.err("p1 and p2 are required");
  };
  let Some(r) = a.radius("r", "d", None) else {
    return a.err("a radius or diameter is required");
  };
  let span = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
  let h = norm3(span);
  if h < 1e-12 {
    return a.err("p1 and p2 must be different points");
  }
  let v = ball_table(lua, "capsule", &[("h", h), ("r", r)], a)?;
  if let LuaValue::Table(t) = &v {
    // The two ends are kept so the ball can be laid along them.
    let ends = lua.create_table()?;
    ends.set(1, Val::vec(p1).to_lua(lua)?)?;
    ends.set(2, Val::vec(p2).to_lua(lua)?)?;
    t.set("between", ends)?;
  }
  Ok(v)
}

/// How much a field is damped at a distance — the taper metaballs fade with.
fn mb_cutoff_fn(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let dist = a.need_num("dist")?;
  let cutoff = a.need_num("cutoff")?;
  Ok(LuaValue::Number(cutoff_factor(dist, cutoff)))
}

/// Read the bounding box, which may be given as a pair of corners or as one
/// size centred on the origin.
fn read_box(a: &Args) -> LuaResult<([f64; 3], [f64; 3])> {
  let Some(v) = a.val("bounding_box") else {
    return a.err("bounding_box is required");
  };
  if let Val::Num(s) = v {
    let h = s.abs() / 2.0;
    return Ok(([-h, -h, -h], [h, h, h]));
  }
  match v.as_matrix() {
    Some(rows) if rows.len() == 2 && rows.iter().all(|r| r.len() >= 3) => Ok((
      crate::bosl::value::v3(&rows[0]),
      crate::bosl::value::v3(&rows[1]),
    )),
    _ => match v.as_vec() {
      Some(s) if s.len() >= 3 => Ok((
        [-s[0] / 2.0, -s[1] / 2.0, -s[2] / 2.0],
        [s[0] / 2.0, s[1] / 2.0, s[2] / 2.0],
      )),
      _ => a.err("bounding_box must be a size or a pair of opposite corners"),
    },
  }
}

fn read_step(a: &Args) -> LuaResult<[f64; 3]> {
  let Some(v) = a.val("voxel_size") else {
    return a.err("voxel_size is required");
  };
  let step = match v {
    Val::Num(s) => [s, s, s],
    other => match other.as_vec() {
      Some(s) if s.len() >= 3 => [s[0], s[1], s[2]],
      Some(s) if !s.is_empty() => [s[0], s[0], s[0]],
      _ => return a.err("voxel_size must be a number or a 3-vector"),
    },
  };
  if step.iter().any(|s| *s <= 0.0) {
    return a.err("voxel_size must be positive");
  }
  Ok(step)
}

/// Guard against a request that would take effectively forever.
fn check_grid(
  a: &Args,
  lo: [f64; 3],
  hi: [f64; 3],
  step: [f64; 3],
) -> LuaResult<()> {
  let cells: f64 = (0..3)
    .map(|k| ((hi[k] - lo[k]) / step[k]).ceil().max(1.0))
    .product();
  if cells > 40_000_000.0 {
    return a.err(format!(
      "that bounding box and voxel size come to {cells:.0} voxels; use a \
       larger voxel_size or a smaller box"
    ));
  }
  Ok(())
}

/// The surface where a sum of metaball fields reaches the isovalue.
fn metaballs(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.val("spec").and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("spec must be a list of transforms and metaballs");
  };
  let Some(LuaValue::Table(raw)) = a.raw("spec").cloned() else {
    return a.err("spec must be a list of transforms and metaballs");
  };
  if items.len() % 2 != 0 {
    return a.err("spec must alternate a transform and a metaball");
  }

  let mut balls: Vec<Ball> = Vec::new();
  for i in (0..items.len()).step_by(2) {
    let Some(rows) = items[i].as_matrix() else {
      return a.err(format!("spec entry {i} must be a 4x4 matrix"));
    };
    if rows.len() != 4 || rows.iter().any(|r| r.len() != 4) {
      return a.err(format!("spec entry {i} must be a 4x4 matrix"));
    }
    let mut m = [0.0; 16];
    for (r, row) in rows.iter().enumerate() {
      m[r * 4..r * 4 + 4].copy_from_slice(row);
    }
    let Ok(LuaValue::Table(desc)) = raw.get::<LuaValue>(i + 2) else {
      return a.err(format!("spec entry {} must be a metaball", i + 1));
    };
    match read_ball(&desc, Mat4(m)) {
      Ok(b) => balls.push(b),
      Err(e) => return a.err(format!("spec entry {}: {e}", i + 1)),
    }
  }
  if balls.is_empty() {
    return a.err("spec must contain at least one metaball");
  }

  let (lo, hi) = read_box(a)?;
  let step = read_step(a)?;
  check_grid(a, lo, hi, step)?;
  let isovalue = a.num_or("isovalue", 1.0);
  let field = move |p: [f64; 3]| balls.iter().map(|b| b.field(p)).sum::<f64>();
  let vnf = march(&field, lo, hi, step, isovalue, false);
  crate::bosl::vnf_lua::write_vnf(lua, &vnf)
}

/// The surface where an arbitrary function reaches the isovalue.
fn isosurface(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(LuaValue::Function(f)) = a.raw("f").cloned() else {
    return a.err("f must be a function of x, y and z");
  };
  let (lo, hi) = read_box(a)?;
  let step = read_step(a)?;
  check_grid(a, lo, hi, step)?;
  let isovalue = a.num_or("isovalue", 0.0);
  let reverse = a.bool_or("reverse", false);

  // Any error the Lua function raises is kept and reported once, rather
  // than swallowed at every one of the thousands of sample points.
  let failure = std::cell::RefCell::new(None::<mlua::Error>);
  let field = |p: [f64; 3]| -> f64 {
    if failure.borrow().is_some() {
      return 0.0;
    }
    match f.call::<f64>((p[0], p[1], p[2])) {
      Ok(v) => v,
      Err(e) => {
        *failure.borrow_mut() = Some(e);
        0.0
      }
    }
  };
  let vnf = march(&field, lo, hi, step, isovalue, reverse);
  if let Some(e) = failure.into_inner() {
    return Err(e);
  }
  crate::bosl::vnf_lua::write_vnf(lua, &vnf)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  use crate::bosl::value::register_pure;
  register_pure(
    lua,
    bosl,
    "mb_sphere",
    &["r", "cutoff", "influence", "negative", "d"],
    mb_sphere,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_cuboid",
    &["size", "squareness", "cutoff", "influence", "negative"],
    mb_cuboid,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_octahedron",
    &["r", "cutoff", "influence", "negative", "d"],
    mb_octahedron,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_torus",
    &[
      "r_maj",
      "r_min",
      "cutoff",
      "influence",
      "negative",
      "d_maj",
      "d_min",
      "or",
      "od",
      "ir",
      "id",
    ],
    mb_torus,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_capsule",
    &[
      "h",
      "r",
      "cutoff",
      "influence",
      "negative",
      "d",
      "l",
      "height",
      "length",
    ],
    mb_capsule,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_disk",
    &[
      "h",
      "r",
      "cutoff",
      "influence",
      "negative",
      "d",
      "l",
      "height",
      "length",
    ],
    mb_disk,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_cyl",
    &[
      "h",
      "r",
      "rounding",
      "r1",
      "r2",
      "l",
      "height",
      "length",
      "d1",
      "d2",
      "d",
      "cutoff",
      "influence",
      "negative",
    ],
    mb_cyl,
  )?;
  register_pure(
    lua,
    bosl,
    "mb_connector",
    &["p1", "p2", "r", "cutoff", "influence", "negative", "d"],
    mb_connector,
  )?;
  register_pure(lua, bosl, "mb_cutoff", &["dist", "cutoff"], mb_cutoff_fn)?;
  register_pure(
    lua,
    bosl,
    "metaballs",
    &[
      "spec",
      "voxel_size",
      "bounding_box",
      "isovalue",
      "closed",
      "show_stats",
    ],
    metaballs,
  )?;
  register_pure(
    lua,
    bosl,
    "isosurface",
    &[
      "f",
      "isovalue",
      "voxel_size",
      "bounding_box",
      "reverse",
      "closed",
      "show_stats",
    ],
    isosurface,
  )?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The volume a closed mesh encloses.
  fn volume(v: &Vnf) -> f64 {
    let mut total = 0.0;
    for f in &v.faces {
      for j in 1..f.len() - 1 {
        let (a, b, c) = (v.points[f[0]], v.points[f[j]], v.points[f[j + 1]]);
        total += a[0] * (b[1] * c[2] - b[2] * c[1])
          - a[1] * (b[0] * c[2] - b[2] * c[0])
          + a[2] * (b[0] * c[1] - b[1] * c[0]);
      }
    }
    total / 6.0
  }

  #[test]
  fn a_sphere_field_meshes_to_a_sphere_of_the_right_size() {
    // The field of a unit sphere ball is r/|p|, so it reaches 1 at |p| = r.
    let ball = Ball {
      kind: Kind::Sphere { r: 10.0 },
      cutoff: f64::INFINITY,
      exponent: 1.0,
      sign: 1.0,
      inverse: Mat4::identity(),
    };
    let f = |p: [f64; 3]| ball.field(p);
    let v = march(&f, [-15.0; 3], [15.0; 3], [0.75; 3], 1.0, false);
    let expected = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
    let got = volume(&v).abs();
    assert!(
      (got - expected).abs() / expected < 0.02,
      "got {got}, expected about {expected}"
    );
  }

  #[test]
  fn the_surface_comes_out_closed() {
    let ball = Ball {
      kind: Kind::Sphere { r: 8.0 },
      cutoff: f64::INFINITY,
      exponent: 1.0,
      sign: 1.0,
      inverse: Mat4::identity(),
    };
    let f = |p: [f64; 3]| ball.field(p);
    let v = march(&f, [-12.0; 3], [12.0; 3], [1.0; 3], 1.0, false);
    // Every edge of a closed surface is shared by exactly two triangles.
    let mut counts: std::collections::HashMap<(usize, usize), usize> =
      std::collections::HashMap::new();
    for face in &v.faces {
      for i in 0..face.len() {
        let (a, b) = (face[i], face[(i + 1) % face.len()]);
        *counts.entry((a.min(b), a.max(b))).or_insert(0) += 1;
      }
    }
    let open = counts.values().filter(|c| **c != 2).count();
    assert_eq!(open, 0, "{open} edges are not shared by two faces");
  }

  #[test]
  fn two_balls_close_together_merge_into_one_surface() {
    let make = |x: f64| Ball {
      kind: Kind::Sphere { r: 6.0 },
      cutoff: f64::INFINITY,
      exponent: 1.0,
      sign: 1.0,
      inverse: invert_rigid(&Mat4::translate([x, 0.0, 0.0])),
    };
    let (a, b) = (make(-4.0), make(4.0));
    let f = |p: [f64; 3]| a.field(p) + b.field(p);
    let v = march(
      &f,
      [-16.0, -12.0, -12.0],
      [16.0, 12.0, 12.0],
      [1.0; 3],
      1.0,
      false,
    );
    // Merged, the pair encloses more than two separate balls would.
    let one = 4.0 / 3.0 * std::f64::consts::PI * 216.0;
    assert!(volume(&v).abs() > 2.0 * one, "{}", volume(&v));
  }

  #[test]
  fn the_cutoff_taper_leaves_and_arrives_flat() {
    assert!((cutoff_factor(0.0, 10.0) - 1.0).abs() < 1e-12);
    assert_eq!(cutoff_factor(10.0, 10.0), 0.0);
    assert_eq!(cutoff_factor(11.0, 10.0), 0.0);
    // Still nearly full strength close in, which is the point of the
    // fourth power.
    assert!(cutoff_factor(3.0, 10.0) > 0.99);
  }
}
