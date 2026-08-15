//! BOSL2's texture catalogue from `skin.scad`.
//!
//! A texture is a tile that covers the unit square and repeats over a swept
//! surface. It comes in two forms. A *heightfield* is a grid of heights
//! between 0 and 1, sampled evenly across the tile — cheap to build and easy
//! to blend. A *VNF tile* is an explicit little mesh, which is what a texture
//! with overhangs or vertical walls needs, since a heightfield can only hold
//! one height per point.
//!
//! Every tile meets its neighbours exactly at the edges of the unit square,
//! so a surface tiled with copies of one has no seams.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, Val};

/// A texture is either a grid of heights or an explicit mesh tile.
#[derive(Debug)]
enum Texture {
  Heights(Vec<Vec<f64>>),
  Tile {
    points: Vec<[f64; 3]>,
    faces: Vec<Vec<usize>>,
  },
}

impl Texture {
  fn to_lua(&self, lua: &Lua) -> LuaResult<LuaValue> {
    match self {
      Texture::Heights(rows) => {
        Val::list(rows.iter().map(|r| Val::vec(r.iter().copied()))).to_lua(lua)
      }
      Texture::Tile { points, faces } => Val::list([
        Val::list(points.iter().map(|p| Val::vec(*p))),
        Val::list(faces.iter().map(|f| Val::vec(f.iter().map(|i| *i as f64)))),
      ])
      .to_lua(lua),
    }
  }
}

// ---------------------------------------------------------------------------
// Small helpers, matching the BOSL2 functions of the same name
// ---------------------------------------------------------------------------

/// `n` values running from `a` to `b`; `endpoint` says whether `b` is one.
fn lerpn(a: f64, b: f64, n: usize, endpoint: bool) -> Vec<f64> {
  let d = (n - usize::from(endpoint)) as f64;
  (0..n)
    .map(|i| {
      let u = if d == 0.0 { 0.0 } else { i as f64 / d };
      a * (1.0 - u) + b * u
    })
    .collect()
}

/// Round up to the next multiple of `q`.
fn quantup(x: f64, q: f64) -> f64 {
  (x / q).ceil() * q
}

fn deg_cos(a: f64) -> f64 {
  a.to_radians().cos()
}

fn deg_sin(a: f64) -> f64 {
  a.to_radians().sin()
}

fn adj_ang_to_hyp(adj: f64, ang: f64) -> f64 {
  adj / deg_cos(ang)
}

fn adj_ang_to_opp(adj: f64, ang: f64) -> f64 {
  adj * deg_sin(ang) / deg_cos(ang)
}

fn opp_ang_to_adj(opp: f64, ang: f64) -> f64 {
  opp * deg_cos(ang) / deg_sin(ang)
}

fn opp_ang_to_hyp(opp: f64, ang: f64) -> f64 {
  opp / deg_sin(ang)
}

fn spherical_to_xyz(r: f64, theta: f64, phi: f64) -> [f64; 3] {
  [
    r * deg_cos(theta) * deg_sin(phi),
    r * deg_sin(theta) * deg_sin(phi),
    r * deg_cos(phi),
  ]
}

fn cylindrical_to_xyz(r: f64, theta: f64, z: f64) -> [f64; 3] {
  [r * deg_cos(theta), r * deg_sin(theta), z]
}

/// The unit square, counter-clockwise from the origin.
fn square(w: f64, h: f64) -> Vec<[f64; 3]> {
  vec![[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, h, 0.0], [0.0, h, 0.0]]
}

/// A rectangle centred on the origin.
fn rect(w: f64, h: f64) -> Vec<[f64; 3]> {
  vec![
    [-w / 2.0, -h / 2.0, 0.0],
    [w / 2.0, -h / 2.0, 0.0],
    [w / 2.0, h / 2.0, 0.0],
    [-w / 2.0, h / 2.0, 0.0],
  ]
}

fn circle(d: f64, n: usize, spin: f64) -> Vec<[f64; 3]> {
  (0..n)
    .map(|i| {
      let a = 360.0 * i as f64 / n as f64 + spin;
      [d / 2.0 * deg_cos(a), d / 2.0 * deg_sin(a), 0.0]
    })
    .collect()
}

fn at_z(pts: &[[f64; 3]], z: f64) -> Vec<[f64; 3]> {
  pts.iter().map(|p| [p[0], p[1], z]).collect()
}

fn moved(pts: &[[f64; 3]], d: [f64; 3]) -> Vec<[f64; 3]> {
  pts
    .iter()
    .map(|p| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
    .collect()
}

/// Put a point on each edge midpoint as well as each corner.
fn subdivide_closed(path: &[[f64; 3]]) -> Vec<[f64; 3]> {
  let n = path.len();
  (0..n)
    .flat_map(|i| {
      let a = path[i];
      let b = path[(i + 1) % n];
      [
        a,
        [
          (a[0] + b[0]) / 2.0,
          (a[1] + b[1]) / 2.0,
          (a[2] + b[2]) / 2.0,
        ],
      ]
    })
    .collect()
}

/// A repeatable pseudo-random sequence.
///
/// The rough textures only need noise that is the same every run, not the
/// same noise OpenSCAD's `rands()` would produce, so this is a plain
/// xorshift rather than an attempt to mirror another generator.
fn rands(lo: f64, hi: f64, n: usize, seed: u64) -> Vec<f64> {
  let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
  (0..n)
    .map(|_| {
      state ^= state << 13;
      state ^= state >> 7;
      state ^= state << 17;
      let u = (state >> 11) as f64 / (1u64 << 53) as f64;
      lo + (hi - lo) * u
    })
    .collect()
}

// ---------------------------------------------------------------------------
// The catalogue
// ---------------------------------------------------------------------------

/// Every texture name, so an unrecognised one can suggest the near misses.
pub const TEXTURE_NAMES: &[&str] = &[
  "bricks",
  "bricks_vnf",
  "checkers",
  "cones",
  "cubes",
  "diamonds",
  "diamonds_vnf",
  "dimples",
  "dots",
  "hex_grid",
  "hills",
  "pyramids",
  "pyramids_vnf",
  "ribs",
  "rough",
  "tri_grid",
  "trunc_diamonds",
  "trunc_pyramids",
  "trunc_pyramids_vnf",
  "trunc_ribs",
  "trunc_ribs_vnf",
  "wave_ribs",
];

struct TexArgs {
  n: Option<f64>,
  border: Option<f64>,
  gap: Option<f64>,
  roughness: Option<f64>,
  fn_: Option<usize>,
}

fn build(name: &str, t: &TexArgs) -> Result<Texture, String> {
  match name {
    "ribs" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = quantup(t.n.unwrap_or(2.0), 2.0) as usize;
      let mut row = lerpn(1.0, 0.0, n / 2, false);
      row.extend(lerpn(0.0, 1.0, n / 2, false));
      Ok(Texture::Heights(vec![row]))
    }
    "trunc_ribs" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = quantup(t.n.unwrap_or(4.0), 4.0) as usize;
      let mut row = vec![0.0; n / 4];
      row.extend(lerpn(0.0, 1.0, n / 4, false));
      row.extend(vec![1.0; n / 4]);
      row.extend(lerpn(1.0, 0.0, n / 4, false));
      Ok(Texture::Heights(vec![row]))
    }
    "wave_ribs" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = t.n.unwrap_or(8.0).max(6.0) as usize;
      let row = (0..n)
        .map(|i| (deg_cos(360.0 * i as f64 / n as f64) + 1.0) / 2.0)
        .collect();
      Ok(Texture::Heights(vec![row]))
    }
    "diamonds" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = quantup(t.n.unwrap_or(2.0), 2.0) as usize;
      let mut path = lerpn(0.0, 1.0, n / 2, false);
      path.extend(lerpn(1.0, 0.0, n / 2, false));
      let at = |k: i64| path[(k.rem_euclid(n as i64)) as usize];
      Ok(Texture::Heights(
        (0..n as i64)
          .map(|i| (0..n as i64).map(|j| at(i + j).min(at(i - j))).collect())
          .collect(),
      ))
    }
    "pyramids" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = quantup(t.n.unwrap_or(2.0), 2.0);
      let ni = n as usize;
      Ok(Texture::Heights(
        (0..ni)
          .map(|i| {
            (0..ni)
              .map(|j| {
                let d =
                  (i as f64 - n / 2.0).abs().max((j as f64 - n / 2.0).abs());
                1.0 - d / (n / 2.0)
              })
              .collect()
          })
          .collect(),
      ))
    }
    "trunc_pyramids" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = quantup(t.n.unwrap_or(6.0), 3.0);
      let ni = n as usize;
      Ok(Texture::Heights(
        (0..ni)
          .map(|i| {
            (0..ni)
              .map(|j| {
                let d = (n / 6.0)
                  .max((i as f64 - n / 2.0).abs())
                  .max((j as f64 - n / 2.0).abs());
                (1.0 - d / (n / 2.0)) * 1.5
              })
              .collect()
          })
          .collect(),
      ))
    }
    "hills" => {
      reject(t, name, &["gap", "border", "roughness"])?;
      let n = t.n.unwrap_or(12.0) as usize;
      Ok(Texture::Heights(
        (0..n)
          .map(|i| {
            let a = 360.0 * i as f64 / n as f64;
            (0..n)
              .map(|j| {
                let b = 360.0 * j as f64 / n as f64;
                (deg_cos(a) * deg_cos(b) + 1.0) / 2.0
              })
              .collect()
          })
          .collect(),
      ))
    }
    "bricks" => {
      reject(t, name, &["gap", "border"])?;
      let n = quantup(t.n.unwrap_or(24.0), 2.0) as usize;
      let rough = t.roughness.unwrap_or(0.05);
      Ok(Texture::Heights(
        (0..n)
          .map(|y| {
            let noise =
              rands(-rough / 2.0, rough / 2.0, n, 12345 + y as u64 * 678);
            (0..n)
              .map(|x| {
                let step = (n / 16).max(1);
                let base = if y % (n / 2) <= step {
                  0.0
                } else {
                  let even = if (y / (n / 2)) % 2 == 1 { n / 2 } else { 0 };
                  if (x + even) % n <= step { 0.0 } else { 0.5 }
                };
                base + noise[x]
              })
              .collect()
          })
          .collect(),
      ))
    }
    "rough" => {
      reject(t, name, &["gap", "border"])?;
      let n = t.n.unwrap_or(32.0) as usize;
      let rough = t.roughness.unwrap_or(0.2);
      Ok(Texture::Heights(
        (0..n)
          .map(|y| rands(0.0, rough, n, 123456 + 29 * y as u64))
          .collect(),
      ))
    }

    // --- VNF tiles -------------------------------------------------------
    "trunc_ribs_vnf" => {
      reject_n(t, name)?;
      let border = t.border.unwrap_or(0.25) * 2.0;
      let gap = t.gap.unwrap_or(0.25);
      if border < 0.0 || gap < 0.0 {
        return Err("trunc_ribs_vnf needs gap >= 0 and border >= 0".into());
      }
      if gap + border > 1.0 {
        return Err("trunc_ribs_vnf needs gap + 2*border <= 1".into());
      }
      let mut points =
        moved(&at_z(&rect(1.0 - gap, 1.0), 0.0), [0.5, 0.5, 0.0]);
      points.extend(moved(
        &at_z(&rect(1.0 - gap - border, 1.0), 1.0),
        [0.5, 0.5, 0.0],
      ));
      points.extend(at_z(&square(1.0, 1.0), 0.0));
      let mut faces = vec![vec![4, 7, 3, 0], vec![1, 2, 6, 5]];
      if gap + border < 1.0 - 1e-9 {
        faces.push(vec![4, 5, 6, 7]);
      }
      if gap > 1e-9 {
        faces.push(vec![1, 9, 10, 2]);
        faces.push(vec![0, 3, 11, 8]);
      }
      Ok(Texture::Tile { points, faces })
    }
    "diamonds_vnf" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "border", "roughness"])?;
      Ok(Texture::Tile {
        points: vec![
          [0.0, 1.0, 1.0],
          [0.5, 1.0, 0.0],
          [1.0, 1.0, 1.0],
          [0.0, 0.5, 0.0],
          [0.5, 0.5, 1.0],
          [1.0, 0.5, 0.0],
          [0.0, 0.0, 1.0],
          [0.5, 0.0, 0.0],
          [1.0, 0.0, 1.0],
        ],
        faces: vec![
          vec![0, 1, 3],
          vec![2, 5, 1],
          vec![8, 7, 5],
          vec![6, 3, 7],
          vec![1, 5, 4],
          vec![5, 7, 4],
          vec![7, 3, 4],
          vec![4, 3, 1],
        ],
      })
    }
    "pyramids_vnf" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "border", "roughness"])?;
      Ok(Texture::Tile {
        points: vec![
          [0.0, 1.0, 0.0],
          [1.0, 1.0, 0.0],
          [0.5, 0.5, 1.0],
          [0.0, 0.0, 0.0],
          [1.0, 0.0, 0.0],
        ],
        faces: vec![vec![2, 0, 1], vec![2, 1, 4], vec![2, 4, 3], vec![2, 3, 0]],
      })
    }
    "trunc_pyramids_vnf" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.1);
      if border <= 0.0 || border >= 0.5 {
        return Err("trunc_pyramids_vnf needs border in (0, 0.5)".into());
      }
      let mut points = at_z(&square(1.0, 1.0), 0.0);
      points.extend(moved(
        &at_z(&rect(1.0 - 2.0 * border, 1.0 - 2.0 * border), 1.0),
        [0.5, 0.5, 0.0],
      ));
      let mut faces: Vec<Vec<usize>> = (0..4)
        .map(|i| vec![i, (i + 1) % 4, (i + 1) % 4 + 4, i + 4])
        .collect();
      faces.push(vec![4, 5, 6, 7]);
      Ok(Texture::Tile { points, faces })
    }
    "cubes" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "border", "roughness"])?;
      Ok(Texture::Tile {
        points: vec![
          [0.0, 1.0, 0.5],
          [1.0, 1.0, 0.5],
          [0.5, 5.0 / 6.0, 1.0],
          [0.0, 4.0 / 6.0, 0.0],
          [1.0, 4.0 / 6.0, 0.0],
          [0.5, 3.0 / 6.0, 0.5],
          [0.0, 2.0 / 6.0, 1.0],
          [1.0, 2.0 / 6.0, 1.0],
          [0.5, 1.0 / 6.0, 0.0],
          [0.0, 0.0, 0.5],
          [1.0, 0.0, 0.5],
        ],
        faces: vec![
          vec![0, 1, 2],
          vec![0, 2, 3],
          vec![1, 4, 2],
          vec![2, 5, 3],
          vec![2, 4, 5],
          vec![6, 3, 5],
          vec![4, 7, 5],
          vec![7, 8, 5],
          vec![6, 5, 8],
          vec![10, 8, 7],
          vec![9, 6, 8],
          vec![10, 9, 8],
        ],
      })
    }
    "cones" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.0);
      if !(0.0..0.5).contains(&border) {
        return Err("cones needs border in [0, 0.5)".into());
      }
      let n = match t.fn_ {
        Some(f) if f > 0 => quantup(f as f64, 4.0) as usize,
        _ => 16,
      };
      let mut points =
        moved(&circle(1.0 - 2.0 * border, n, 0.0), [0.5, 0.5, 0.0]);
      points.push([0.5, 0.5, 1.0]);
      let base = if border > 0.0 {
        at_z(&subdivide_closed(&square(1.0, 1.0)), 0.0)
      } else {
        at_z(&square(1.0, 1.0), 0.0)
      };
      points.extend(base);

      let mut faces: Vec<Vec<usize>> =
        (0..n).map(|i| vec![i, (i + 1) % n, n]).collect();
      faces.extend(skirt(n, border > 0.0));
      Ok(Texture::Tile { points, faces })
    }
    "dimples" | "dots" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.05);
      if !(0.0..0.5).contains(&border) {
        return Err(format!("{name} needs border in [0, 0.5)"));
      }
      let n = match t.fn_ {
        Some(f) if f > 0 => quantup(f as f64, 4.0) as usize,
        _ => 16,
      };
      let rows = n.div_ceil(4);
      let r = adj_ang_to_hyp(0.5 - border, 45.0);
      let dots = name == "dots";
      let cp = [0.5, 0.5, r * deg_sin(45.0) * if dots { -1.0 } else { 1.0 }];
      let sc = 1.0 / (r - cp[2].abs());

      let mut points: Vec<[f64; 3]> = Vec::new();
      for p in 0..rows {
        for k in 0..n {
          let theta = -(360.0 * k as f64 / n as f64);
          let phi = if dots {
            45.0 - 45.0 * p as f64 / rows as f64
          } else {
            135.0 + 45.0 * p as f64 / rows as f64
          };
          let s = spherical_to_xyz(r, theta, phi);
          points.push([cp[0] + s[0], cp[1] + s[1], cp[2] + s[2]]);
        }
      }
      let pole = if dots { 1.0 } else { -1.0 };
      points.push([cp[0], cp[1], cp[2] + r * pole]);
      let base = if border > 0.0 {
        at_z(&subdivide_closed(&square(1.0, 1.0)), 0.0)
      } else {
        at_z(&square(1.0, 1.0), 0.0)
      };
      points.extend(base);
      // The whole dome is squashed so it spans exactly one unit of height.
      for p in points.iter_mut().take(rows * n + 1) {
        p[2] *= sc;
      }

      let mut faces: Vec<Vec<usize>> = Vec::new();
      for i in 0..rows.saturating_sub(1) {
        for j in 0..n {
          faces.push(vec![
            i * n + j,
            i * n + (j + 1) % n,
            (i + 1) * n + (j + 1) % n,
            (i + 1) * n + j,
          ]);
        }
      }
      for i in 0..n {
        faces.push(vec![
          (rows - 1) * n + i,
          (rows - 1) * n + (i + 1) % n,
          rows * n,
        ]);
      }
      faces.extend(skirt_at(rows * n, n, border > 0.0));
      Ok(Texture::Tile { points, faces })
    }
    "checkers" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.05);
      if !(0.0 < border && border < 0.5) {
        return Err("checkers needs border in (0, 0.5)".into());
      }
      let s = 0.5 - border;
      let mut points: Vec<[f64; 3]> = Vec::new();
      points.extend(at_z(&square(s, s), 1.0));
      points.extend(moved(&at_z(&square(s, s), 0.0), [0.0, 0.5, 0.0]));
      points.extend(moved(&at_z(&square(s, s), 0.0), [0.5, 0.0, 0.0]));
      points.extend(moved(&at_z(&square(s, s), 1.0), [0.5, 0.5, 0.0]));
      points.extend([
        [0.5 - border / 2.0, 0.5 - border / 2.0, 0.5],
        [0.0, 1.0, 1.0],
        [0.5 - border, 1.0, 1.0],
        [0.5, 1.0, 0.0],
        [1.0 - border, 1.0, 0.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.5 - border, 1.0],
        [1.0, 0.5, 0.0],
        [1.0, 1.0 - border, 0.0],
        [1.0, 1.0, 1.0],
        [0.5 - border / 2.0, 1.0 - border / 2.0, 0.5],
        [1.0 - border / 2.0, 1.0 - border / 2.0, 0.5],
        [1.0 - border / 2.0, 0.5 - border / 2.0, 0.5],
      ]);
      let mut faces: Vec<Vec<usize>> = (0..4)
        .map(|k| {
          let i = k * 4;
          vec![i, i + 1, i + 2, i + 3]
        })
        .collect();
      faces.extend([
        vec![10, 16, 13, 12, 28, 11],
        vec![9, 0, 3, 16, 10],
        vec![11, 28, 22, 21, 8],
        vec![4, 7, 26, 14, 13, 16],
        vec![7, 6, 17, 18, 26],
        vec![5, 4, 16, 3, 2],
        vec![19, 20, 27, 15, 14, 26],
        vec![20, 25, 27],
        vec![19, 26, 18],
        vec![23, 28, 12, 15, 27, 24],
        vec![23, 22, 28],
        vec![24, 27, 25],
      ]);
      Ok(Texture::Tile { points, faces })
    }
    "trunc_diamonds" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.1) / 2f64.sqrt() * 2.0;
      if !(0.0 < border && border < 0.5) {
        return Err("trunc_diamonds needs border in (0, 0.5)".into());
      }
      let mut points = moved(&circle(1.0, 4, 0.0), [0.5, 0.5, 0.0]);
      points.extend(moved(
        &at_z(&circle(1.0 - border * 2.0, 4, 0.0), 1.0),
        [0.5, 0.5, 0.0],
      ));
      for k in 0..4 {
        let a = -(90.0 * k as f64);
        for p in [[0.5, border, 1.0], [border, 0.5, 1.0], [0.5, 0.5, 1.0]] {
          let (c, s) = (deg_cos(a), deg_sin(a));
          points.push([
            0.5 + p[0] * c - p[1] * s,
            0.5 + p[0] * s + p[1] * c,
            p[2],
          ]);
        }
      }
      let mut faces: Vec<Vec<usize>> = Vec::new();
      for i in 0..4 {
        let j = i * 3 + 8;
        faces.push(vec![i, (i + 1) % 4, (i + 1) % 4 + 4, i + 4]);
        faces.push(vec![j, j + 1, j + 2]);
        faces.push(vec![i, (i + 3) % 4, j + 1, j]);
      }
      faces.push(vec![4, 5, 6, 7]);
      Ok(Texture::Tile { points, faces })
    }
    "tri_grid" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.05) * 3f64.sqrt();
      if !(0.0 < border && border < 3f64.sqrt() / 6.0) {
        return Err("tri_grid needs border in (0, 1/6)".into());
      }
      let adj = opp_ang_to_adj(border, 30.0);
      let y1 = border / adj_ang_to_opp(1.0, 60.0);
      let y2 = 2.0 * y1;
      let y3 = 0.5 - y1;
      let y4 = 0.5 + y1;
      let y5 = 1.0 - y2;
      let y6 = 1.0 - y1;
      Ok(Texture::Tile {
        points: vec![
          [0.0, 0.0, 0.0],
          [1.0, 0.0, 0.0],
          [adj, y1, 1.0],
          [1.0 - adj, y1, 1.0],
          [0.0, y2, 1.0],
          [1.0, y2, 1.0],
          [0.5, 0.5 - y2, 1.0],
          [0.0, y3, 1.0],
          [0.5 - adj, y3, 1.0],
          [0.5 + adj, y3, 1.0],
          [1.0, y3, 1.0],
          [0.0, 0.5, 0.0],
          [0.5, 0.5, 0.0],
          [1.0, 0.5, 0.0],
          [0.0, y4, 1.0],
          [0.5 - adj, y4, 1.0],
          [0.5 + adj, y4, 1.0],
          [1.0, y4, 1.0],
          [0.5, 0.5 + y2, 1.0],
          [0.0, y5, 1.0],
          [1.0, y5, 1.0],
          [adj, y6, 1.0],
          [1.0 - adj, y6, 1.0],
          [0.0, 1.0, 0.0],
          [1.0, 1.0, 0.0],
        ],
        faces: vec![
          vec![0, 2, 3, 1],
          vec![21, 23, 24, 22],
          vec![2, 6, 3],
          vec![0, 12, 6, 2],
          vec![1, 3, 6, 12],
          vec![0, 4, 8, 12],
          vec![4, 7, 8],
          vec![8, 7, 11, 12],
          vec![1, 12, 9, 5],
          vec![5, 9, 10],
          vec![10, 9, 12, 13],
          vec![11, 14, 15, 12],
          vec![19, 15, 14],
          vec![19, 23, 12, 15],
          vec![16, 17, 13, 12],
          vec![16, 20, 17],
          vec![12, 24, 20, 16],
          vec![21, 22, 18],
          vec![12, 23, 21, 18],
          vec![12, 18, 22, 24],
        ],
      })
    }
    "hex_grid" => {
      reject_n(t, name)?;
      reject(t, name, &["gap", "roughness"])?;
      let border = t.border.unwrap_or(0.1);
      if !(0.0 < border && border < 0.5) {
        return Err("hex_grid needs border in (0, 0.5)".into());
      }
      let diag = opp_ang_to_hyp(border, 60.0);
      let hyp = adj_ang_to_hyp(0.5, 30.0);
      let sc = 1.0 / 3.0 / hyp;
      let hex: Vec<[f64; 3]> = vec![
        [1.0, 2.0 / 6.0, 0.0],
        [0.5, 1.0 / 6.0, 0.0],
        [0.0, 2.0 / 6.0, 0.0],
        [0.0, 4.0 / 6.0, 0.0],
        [0.5, 5.0 / 6.0, 0.0],
        [1.0, 4.0 / 6.0, 0.0],
      ];
      let mut points = hex.clone();
      // The inner hexagon is squashed in y so it sits flat against the tile.
      let inner: Vec<[f64; 3]> = circle(1.0 - 2.0 * border, 6, -30.0)
        .iter()
        .map(|p| [0.5 + p[0], 0.5 + p[1] * sc, 1.0])
        .collect();
      points.extend(inner);
      points.push([hex[0][0], hex[0][1] - diag * sc, 1.0]);
      for ang in [270.0 + 60.0, 270.0 - 60.0] {
        let c = cylindrical_to_xyz(diag, ang, 1.0);
        points.push([hex[1][0] + c[0], hex[1][1] + c[1] * sc, c[2]]);
      }
      points.push([hex[2][0], hex[2][1] - diag * sc, 1.0]);
      points.extend([
        [0.0, 0.0, 1.0],
        [0.5 - border, 0.0, 1.0],
        [0.5, 0.0, 0.0],
        [0.5 + border, 0.0, 1.0],
        [1.0, 0.0, 1.0],
      ]);
      points.push([hex[3][0], hex[3][1] + diag * sc, 1.0]);
      for ang in [90.0 + 60.0, 90.0 - 60.0] {
        let c = cylindrical_to_xyz(diag, ang, 1.0);
        points.push([hex[4][0] + c[0], hex[4][1] + c[1] * sc, c[2]]);
      }
      points.push([hex[5][0], hex[5][1] + diag * sc, 1.0]);
      points.extend([
        [0.0, 1.0, 1.0],
        [0.5 - border, 1.0, 1.0],
        [0.5, 1.0, 0.0],
        [0.5 + border, 1.0, 1.0],
        [1.0, 1.0, 1.0],
      ]);
      let mut faces: Vec<Vec<usize>> = vec![(6..12).collect()];
      for i in 0..6 {
        faces.push(vec![i, (i + 1) % 6, (i + 1) % 6 + 6, i + 6]);
      }
      faces.extend([
        vec![20, 19, 13, 12],
        vec![17, 16, 15, 14],
        vec![21, 25, 26, 22],
        vec![23, 28, 29, 24],
        vec![0, 12, 13, 1],
        vec![1, 14, 15, 2],
        vec![3, 21, 22, 4],
        vec![4, 23, 24, 5],
        vec![1, 13, 19, 18],
        vec![1, 18, 17, 14],
        vec![4, 22, 26, 27],
        vec![4, 27, 28, 23],
      ]);
      Ok(Texture::Tile { points, faces })
    }
    "bricks_vnf" => {
      reject_n(t, name)?;
      reject(t, name, &["roughness"])?;
      let border = t.border.unwrap_or(0.05);
      let gap = t.gap.unwrap_or(0.05);
      if border < 0.0 {
        return Err("bricks_vnf needs a non-negative border".into());
      }
      if gap <= 0.0 {
        return Err("bricks_vnf needs a gap greater than 0".into());
      }
      if gap + border >= 0.5 {
        return Err("bricks_vnf needs gap + border < 0.5".into());
      }
      let mut points = at_z(&square(1.0, 1.0), 0.0);
      points.extend(moved(
        &at_z(&square(1.0 - gap, 0.5 - gap), 0.0),
        [gap / 2.0, gap / 2.0, 0.0],
      ));
      points.extend(moved(
        &at_z(&square(1.0 - gap - border, 0.5 - gap - border), 1.0),
        [gap / 2.0 + border / 2.0, gap / 2.0 + border / 2.0, 0.0],
      ));
      points.extend(moved(
        &at_z(&square(0.5 - gap / 2.0, 0.5 - gap), 0.0),
        [0.0, 0.5 + gap / 2.0, 0.0],
      ));
      points.extend(moved(
        &at_z(
          &square(0.5 - gap / 2.0 - border / 2.0, 0.5 - gap - border),
          1.0,
        ),
        [0.0, 0.5 + gap / 2.0 + border / 2.0, 0.0],
      ));
      points.extend(moved(
        &at_z(&square(0.5 - gap / 2.0, 0.5 - gap), 0.0),
        [0.5 + gap / 2.0, 0.5 + gap / 2.0, 0.0],
      ));
      points.extend(moved(
        &at_z(
          &square(0.5 - gap / 2.0 - border / 2.0, 0.5 - gap - border),
          1.0,
        ),
        [
          0.5 + gap / 2.0 + border / 2.0,
          0.5 + gap / 2.0 + border / 2.0,
          0.0,
        ],
      ));
      Ok(Texture::Tile {
        points,
        faces: vec![
          vec![0, 4, 7, 20],
          vec![4, 8, 11, 7],
          vec![9, 8, 4, 5],
          vec![4, 0, 1, 5],
          vec![10, 9, 5, 6],
          vec![20, 7, 6, 13, 12, 21],
          vec![2, 3, 23, 22, 15, 14],
          vec![15, 19, 18, 14],
          vec![22, 23, 27, 26],
          vec![16, 19, 15, 12],
          vec![13, 6, 5, 1],
          vec![26, 25, 21, 22],
          vec![8, 9, 10, 11],
          vec![7, 11, 10, 6],
          vec![17, 16, 12, 13],
          vec![22, 21, 12, 15],
          vec![16, 17, 18, 19],
          vec![24, 25, 26, 27],
          vec![25, 24, 20, 21],
        ],
      })
    }
    other => Err(format!(
      "unrecognised texture name '{other}'. The textures are: {}",
      TEXTURE_NAMES.join(", ")
    )),
  }
}

/// The ring of faces joining a round texture to the edge of its tile.
fn skirt(n: usize, subdivided: bool) -> Vec<Vec<usize>> {
  skirt_at(n, n, subdivided)
}

fn skirt_at(apex: usize, n: usize, subdivided: bool) -> Vec<Vec<usize>> {
  let base = apex + 1;
  (0..4)
    .map(|i| {
      let mut face: Vec<usize> = ((i * n / 4)..=((i + 1) * n / 4))
        .rev()
        .map(|j| j % n)
        .collect();
      if subdivided {
        face.push((2 * i + 7) % 8 + base);
        face.push((2 * i) % 8 + base);
        face.push((2 * i + 1) % 8 + base);
      } else {
        face.push(i + base);
      }
      face
    })
    .collect()
}

/// Reject the settings a texture has no use for, so a typo is not silently
/// ignored.
fn reject(t: &TexArgs, name: &str, unwanted: &[&str]) -> Result<(), String> {
  for which in unwanted {
    let given = match *which {
      "gap" => t.gap.is_some(),
      "border" => t.border.is_some(),
      "roughness" => t.roughness.is_some(),
      _ => false,
    };
    if given {
      return Err(format!("the {name} texture does not accept {which}"));
    }
  }
  Ok(())
}

fn reject_n(t: &TexArgs, name: &str) -> Result<(), String> {
  if t.n.is_some() {
    return Err(format!(
      "the {name} texture is a mesh tile, so it does not accept n. Set its \
       sample rate with tex_samples instead"
    ));
  }
  Ok(())
}

/// Build one of the named textures.
fn texture(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(name) = a.string("tex") else {
    return a.err("tex must be the name of a texture");
  };
  if a.has("border") && a.has("inset") {
    return a.err("inset has been replaced by border; give only one");
  }
  let t = TexArgs {
    n: a.num("n"),
    border: a.num("border").or_else(|| a.num("inset")),
    gap: a.num("gap"),
    roughness: a.num("roughness"),
    fn_: a.int("fn").map(|v| v.max(0) as usize),
  };
  if let Some(n) = t.n
    && n <= 0.0
  {
    return a.err("n must be positive");
  }
  match build(&name, &t) {
    Ok(tex) => tex.to_lua(lua),
    Err(e) => a.err(e),
  }
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  crate::bosl::value::register_pure(
    lua,
    bosl,
    "texture",
    &["tex", "n", "border", "gap", "roughness", "inset"],
    texture,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn heights(name: &str, t: TexArgs) -> Vec<Vec<f64>> {
    match build(name, &t).unwrap() {
      Texture::Heights(h) => h,
      Texture::Tile { .. } => panic!("{name} is a mesh tile"),
    }
  }

  fn blank() -> TexArgs {
    TexArgs {
      n: None,
      border: None,
      gap: None,
      roughness: None,
      fn_: None,
    }
  }

  #[test]
  fn ribs_run_down_and_back_up_across_the_tile() {
    let h = heights(
      "ribs",
      TexArgs {
        n: Some(4.0),
        ..blank()
      },
    );
    assert_eq!(h.len(), 1);
    assert_eq!(h[0], vec![1.0, 0.5, 0.0, 0.5]);
  }

  #[test]
  fn a_pyramid_peaks_in_the_middle_of_its_tile() {
    let h = heights(
      "pyramids",
      TexArgs {
        n: Some(4.0),
        ..blank()
      },
    );
    assert_eq!(h.len(), 4);
    assert!((h[2][2] - 1.0).abs() < 1e-12, "{h:?}");
    assert!((h[0][0] - 0.0).abs() < 1e-12, "{h:?}");
  }

  #[test]
  fn every_texture_in_the_catalogue_builds() {
    for name in TEXTURE_NAMES {
      let t = TexArgs {
        // The VNF tiles refuse `n`, and the heightfields do not need it.
        n: None,
        ..blank()
      };
      assert!(
        build(name, &t).is_ok(),
        "{name} failed: {:?}",
        build(name, &t).err()
      );
    }
  }

  #[test]
  fn a_mesh_tile_refuses_a_sample_count() {
    let t = TexArgs {
      n: Some(8.0),
      ..blank()
    };
    assert!(build("cubes", &t).is_err());
  }

  #[test]
  fn a_texture_refuses_settings_it_has_no_use_for() {
    let t = TexArgs {
      gap: Some(0.1),
      ..blank()
    };
    assert!(build("ribs", &t).is_err());
  }

  #[test]
  fn an_unknown_name_lists_the_ones_that_exist() {
    let e = build("knurled", &blank()).unwrap_err();
    assert!(e.contains("pyramids"), "{e}");
  }

  #[test]
  fn every_mesh_tile_indexes_points_that_exist() {
    for name in TEXTURE_NAMES {
      if let Ok(Texture::Tile { points, faces }) = build(name, &blank()) {
        for face in &faces {
          assert!(face.len() >= 3, "{name} has a face with too few corners");
          for i in face {
            assert!(
              *i < points.len(),
              "{name} indexes point {i} of {}",
              points.len()
            );
          }
        }
      }
    }
  }

  #[test]
  fn a_heightfield_stays_between_zero_and_one() {
    for name in [
      "ribs",
      "trunc_ribs",
      "wave_ribs",
      "diamonds",
      "pyramids",
      "hills",
    ] {
      for row in heights(name, blank()) {
        for v in row {
          assert!((0.0..=1.0).contains(&v), "{name} produced {v}");
        }
      }
    }
  }
}
