//! Native implementations of BOSL2's 2D shapes.
//!
//! Each shape is generated as an outline and handed back as a
//! [`ScadNode::Polygon`], so it extrudes, offsets and combines like any other
//! LuaCAD sketch. The outline generators are public because the 3D shapes
//! build on them — a prismoid is two [`rect_path`]s lofted together, a
//! `regular_prism` is a [`ngon_path`] extruded.

use std::f64::consts::PI;

use mlua::Result as LuaResult;

use crate::bosl::args::Args;
use crate::bosl::attach::{Attachable, Geom, reorient};
use crate::bosl::vecmath::{Mat4, V2};
use crate::bosl::vnf::arc_pts;
use crate::geometry::CsgSketch;
use crate::scad_export::ScadNode;

/// A closed 2D outline.
pub type Path = Vec<V2>;

const EPS: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Outline generators
// ---------------------------------------------------------------------------

/// How many points BOSL2 puts on an arc: `max(3, ceil(segs(r) * angle/360))`,
/// based on the arc's own radius rather than the shape's overall size.
pub fn arc_n(full_circle: u32, sweep: f64) -> u32 {
  ((full_circle as f64 * sweep.abs() / 360.0).ceil() as u32).max(3)
}

/// An arc facetted the way BOSL2 facets one when no point count is given.
///
/// Dropping the endpoint does not stretch the remaining points over the
/// sweep: BOSL2 lays out the full arc and discards the last vertex, so the
/// spacing is the same whether or not the end is kept. Getting this wrong
/// leaves an outline whose facets are a fraction too coarse, which shows up
/// as a percent or so of volume once it is extruded.
pub fn bosl_arc(
  full_circle: u32,
  r: f64,
  cp: V2,
  start: f64,
  sweep: f64,
  endpoint: bool,
) -> Path {
  let pts = arc_pts(arc_n(full_circle, sweep), r, cp, start, sweep, true);
  if endpoint {
    pts
  } else {
    pts[..pts.len() - 1].to_vec()
  }
}

/// A rectangle, optionally rounded or chamfered per corner.
///
/// `rounding` and `chamfer` are given per corner in BOSL2's order —
/// `[+X+Y, -X+Y, -X-Y, +X-Y]` — and a negative value cuts the corner inward
/// instead of trimming it away.
pub fn rect_path(
  size: V2,
  rounding: [f64; 4],
  chamfer: [f64; 4],
  segments: u32,
) -> Result<Path, String> {
  let size = [size[0].max(0.0), size[1].max(0.0)];

  if rounding.iter().chain(chamfer.iter()).all(|v| v.abs() < EPS) {
    return Ok(vec![
      [size[0] / 2.0, -size[1] / 2.0],
      [-size[0] / 2.0, -size[1] / 2.0],
      [-size[0] / 2.0, size[1] / 2.0],
      [size[0] / 2.0, size[1] / 2.0],
    ]);
  }

  for i in 0..4 {
    if rounding[i].abs() >= EPS && chamfer[i].abs() >= EPS {
      return Err("a corner cannot be both rounded and chamfered".to_string());
    }
  }

  // Corner i of `quadpos` is the quadrant it sits in.
  let quadpos = [[1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0], [1.0, -1.0]];
  let insets: [f64; 4] = std::array::from_fn(|i| {
    if chamfer[i].abs() >= EPS {
      chamfer[i]
    } else if rounding[i].abs() >= EPS {
      rounding[i]
    } else {
      0.0
    }
  });
  let insets_x = (insets[0] + insets[1]).max(insets[2] + insets[3]);
  let insets_y = (insets[0] + insets[3]).max(insets[1] + insets[2]);
  if insets_x > size[0] + EPS {
    return Err("the roundings and chamfers exceed the rect width".into());
  }
  if insets_y > size[1] + EPS {
    return Err("the roundings and chamfers exceed the rect height".into());
  }

  // BOSL2 walks the quadrants in the order 3, 2, 1, 0, which is what makes
  // the outline start beside +X and run clockwise like the plain rectangle.
  let mut path = Path::new();
  for quad in [3usize, 2, 1, 0] {
    let inset = insets[quad];
    let qpos = quadpos[quad];
    let cp = [
      (size[0] / 2.0 - inset) * qpos[0],
      (size[1] / 2.0 - inset.abs()) * qpos[1],
    ];

    let qpts: Vec<V2> = if chamfer[quad].abs() >= EPS {
      vec![[0.0, inset.abs()], [inset, 0.0]]
    } else if rounding[quad].abs() >= EPS {
      let cverts = (segments.max(4) as f64 / 4.0).ceil().max(1.0) as u32;
      let step = 90.0 / cverts as f64;
      (0..=cverts)
        .map(|j| {
          let a = 90.0 - j as f64 * step;
          let (s, c) = a.to_radians().sin_cos();
          [inset.abs() * c * inset.signum(), inset.abs() * s]
        })
        .collect()
    } else {
      vec![[0.0, 0.0]]
    };

    // Mirror the corner into its quadrant, reversing it in the quadrants
    // where mirroring would otherwise flip the winding.
    let qfpts: Vec<V2> = qpts
      .iter()
      .map(|p| [p[0] * qpos[0], p[1] * qpos[1]])
      .collect();
    let qrpts: Vec<V2> = if qpos[0] * qpos[1] < 0.0 {
      qfpts.into_iter().rev().collect()
    } else {
      qfpts
    };
    path.extend(qrpts.into_iter().map(|p| [cp[0] + p[0], cp[1] + p[1]]));
  }

  Ok(dedup_closed(path))
}

/// An ellipse or circle, matching BOSL2's vertex placement.
///
/// `realign` turns the polygon half a facet so a flat sits where a vertex
/// otherwise would, and `circum` grows it so the polygon circumscribes the
/// true circle rather than inscribing it.
pub fn ellipse_path(r: V2, sides: u32, realign: bool, circum: bool) -> Path {
  let sides = sides.max(3);
  let offset = if realign { 180.0 / sides as f64 } else { 0.0 };
  let sc = if circum {
    1.0 / (PI / sides as f64).cos()
  } else {
    1.0
  };
  (0..sides)
    .map(|i| {
      let a = 360.0 - offset - i as f64 * 360.0 / sides as f64;
      let (s, c) = a.to_radians().sin_cos();
      [r[0] * sc * c, r[1] * sc * s]
    })
    .collect()
}

/// A regular polygon of `n` sides on a circumscribed radius `r`.
pub fn ngon_path(n: u32, r: f64, rounding: f64, segments: u32) -> Path {
  if rounding.abs() < EPS {
    return ellipse_path([r, r], n, false, false);
  }
  // The corner circle sits back from the tip by the inset its radius implies
  // at the polygon's interior angle.
  let half_interior = (180.0 - 360.0 / n as f64) / 2.0;
  let inset = rounding / half_interior.to_radians().sin();
  let steps = (segments / n).max(2);
  let mut path = Path::new();
  for i in 0..n {
    let a = 360.0 - i as f64 * 360.0 / n as f64;
    let (s, c) = a.to_radians().sin_cos();
    let cp = [(r - inset) * c, (r - inset) * s];
    path.extend(arc_pts(
      steps,
      rounding,
      cp,
      a + 180.0 / n as f64,
      -360.0 / n as f64,
      true,
    ));
  }
  // Start the outline beside +X, as the unrounded polygon does.
  rotate_to_max_x(path)
}

/// A star with `n` points, alternating between radius `r` and `ir`.
pub fn star_path(n: u32, r: f64, ir: f64) -> Path {
  (1..=2 * n)
    .rev()
    .map(|i| {
      let theta = 180.0 * i as f64 / n as f64;
      let radius = if i % 2 == 1 { ir } else { r };
      let (s, c) = theta.to_radians().sin_cos();
      [radius * c, radius * s]
    })
    .collect()
}

/// A trapezoid `h` tall, `w1` wide at the front and `w2` at the back.
pub fn trapezoid_path(h: f64, w1: f64, w2: f64, shift: f64) -> Path {
  vec![
    [w2 / 2.0 + shift, h / 2.0],
    [-w2 / 2.0 + shift, h / 2.0],
    [-w1 / 2.0, -h / 2.0],
    [w1 / 2.0, -h / 2.0],
  ]
}

/// A circle with a pointed or flattened cap, so it prints without support.
///
/// `ang` is the overhang angle of the cap, and `cap_h` truncates the point
/// at that height.
pub fn teardrop2d_path(
  r: f64,
  ang: f64,
  cap_h: Option<f64>,
  sides: u32,
  realign: bool,
) -> Result<Path, String> {
  let min_height = r * ang.to_radians().sin();
  let max_height = r / ang.to_radians().sin();
  if let Some(h) = cap_h
    && h < min_height - EPS
  {
    return Err(format!("cap_h cannot be less than {min_height}"));
  }
  let pointy = cap_h.is_none_or(|h| h >= max_height);
  let cap: [V2; 2] = [
    if pointy {
      [0.0, max_height]
    } else {
      let h = cap_h.expect("a blunt cap has a height");
      [(max_height - h) * ang.to_radians().tan(), h]
    },
    [r * ang.to_radians().cos(), r * ang.to_radians().sin()],
  ];

  // The circle is spun a quarter turn so the cap sits at the top.
  let circle: Path = ellipse_path([r, r], sides, realign, false)
    .into_iter()
    .map(|p| [-p[1], p[0]])
    .collect();

  let seg_len = if circle.len() >= 2 {
    let d = [circle[0][0] - circle[1][0], circle[0][1] - circle[1][1]];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
  } else {
    0.0
  };
  // A hexagonal approximation has so few points that the usual spacing test
  // would drop the whole arc, so it is allowed much shorter final segments.
  let skip = if circle.len() == 6 { 15.0 } else { 3.0 };

  let mut path = vec![cap[0], cap[1]];
  for p in &circle {
    let dx = p[0].abs() - cap[1][0];
    let dy = p[1] - cap[1][1];
    if p[1] < cap[1][1] - EPS && (dx * dx + dy * dy).sqrt() > seg_len / skip {
      path.push(*p);
    }
  }
  path.push([-cap[1][0], cap[1][1]]);
  if !pointy {
    path.push([-cap[0][0], cap[0][1]]);
  }
  Ok(path)
}

/// Two circles of radius `r` joined by a tangent waist.
pub fn glued_circles_path(
  r: f64,
  spread: f64,
  tangent: f64,
  segments: u32,
) -> Path {
  let r2 = (spread / 2.0 / tangent.to_radians().sin()) - r;
  let cp1 = [spread / 2.0, 0.0];
  let cp2 = [0.0, (r + r2) * tangent.to_radians().cos()];
  let sa1 = 90.0 - tangent;
  let ea1 = 270.0 + tangent;
  let lobe_arc = ea1 - sa1;
  let lobe_segs = (segments as f64 * lobe_arc / 360.0).ceil().max(2.0) as u32;
  let sa2 = 270.0 - tangent;
  let ea2 = 270.0 + tangent;
  let subarc = ea2 - sa2;
  let arc_segs =
    (segments as f64 * subarc.abs() / 360.0).ceil().max(2.0) as u32;

  let path: Path = if tangent.abs() < EPS {
    // Without a waist the two lobes are simply full circles meeting at a
    // point, so each arc has to carry its own end vertex.
    let mut p =
      arc_pts(lobe_segs + 1, r, [-cp1[0], -cp1[1]], sa1, ea1 - sa1, true);
    p.extend(arc_pts(lobe_segs + 1, r, cp1, sa1 + 180.0, ea1 - sa1, true));
    p
  } else {
    let mut p = arc_pts(lobe_segs, r, [-cp1[0], -cp1[1]], sa1, lobe_arc, false);
    p.extend((0..arc_segs).map(|i| {
      let theta = ea2 + 180.0 - subarc * i as f64 / arc_segs as f64;
      let (s, c) = theta.to_radians().sin_cos();
      [r2 * c - cp2[0], r2 * s - cp2[1]]
    }));
    p.extend(arc_pts(lobe_segs, r, cp1, sa1 + 180.0, lobe_arc, false));
    p.extend((0..arc_segs).map(|i| {
      let theta = ea2 - subarc * i as f64 / arc_segs as f64;
      let (s, c) = theta.to_radians().sin_cos();
      [r2 * c + cp2[0], r2 * s + cp2[1]]
    }));
    p
  };

  let rotated = rotate_to_max_x(path);
  rotated.into_iter().rev().collect()
}

/// A shape between a square and a circle, `squareness` running 0 to 1.
pub fn squircle_path(
  size: V2,
  squareness: f64,
  style: &str,
  fn_: u32,
) -> Result<Path, String> {
  let astep = if fn_ >= 12 {
    90.0 / (fn_ as f64 / 4.0).round()
  } else {
    360.0 / 48.0
  };
  let angles: Vec<f64> = {
    let mut out = Vec::new();
    let mut a = 360.0;
    while a > 0.01 {
      out.push(a);
      a -= astep;
    }
    out
  };

  match style {
    "fg" => {
      let sq = linearize_squareness(squareness);
      let aspect = size[1] / size[0];
      let r = 0.5 * size[0];
      Ok(
        angles
          .iter()
          .map(|a| {
            let theta = a + sq * (4.0 * a).to_radians().sin() * 30.0 / PI;
            let p = squircle_radius_fg(sq, r, theta);
            let (s, c) = theta.to_radians().sin_cos();
            [p * c, p * aspect * s]
          })
          .collect(),
      )
    }
    "superellipse" => {
      let n = squircle_se_exponent(squareness);
      let ra = 0.5 * size[0];
      let rb = 0.5 * size[1];
      let fgsq = linearize_squareness(squareness.min(0.998));
      Ok(
        angles
          .iter()
          .map(|a| {
            let theta = a + fgsq * (4.0 * a).to_radians().sin() * 30.0 / PI;
            let (y, x) = theta.to_radians().sin_cos();
            let r = (x.abs().powf(n) + y.abs().powf(n)).powf(1.0 / n);
            [ra * x / r, rb * y / r]
          })
          .collect(),
      )
    }
    other => Err(format!(
      "style must be \"fg\" or \"superellipse\", not \"{other}\""
    )),
  }
}

fn linearize_squareness(s: f64) -> f64 {
  // From Chamberlain Fong (2016), "Squircular Calculations".
  let c = 2.0 - 2.0 * 2f64.sqrt();
  let d = 1.0 - 0.5 * c * s;
  2.0 * ((1.0 + c) * s * s - c * s).max(0.0).sqrt() / (d * d)
}

fn squircle_radius_fg(squareness: f64, r: f64, angle: f64) -> f64 {
  let s2a = (squareness * (2.0 * angle).to_radians().sin()).abs();
  // Fong gives this as `r*sqrt(2)/s2a * sqrt(1 - sqrt(1 - s2a^2))`, but that
  // form loses all its significant digits as `s2a` approaches zero — the
  // inner difference rounds to 0 and the radius collapses to the origin,
  // right where the outline crosses an axis. Rewriting `1 - sqrt(1 - x^2)`
  // as `x^2 / (1 + sqrt(1 - x^2))` cancels the `s2a` division exactly.
  r * 2f64.sqrt() / (1.0 + (1.0 - s2a * s2a).max(0.0).sqrt()).sqrt()
}

fn squircle_se_exponent(squareness: f64) -> f64 {
  let s = squareness.min(0.998);
  let rho = 1.0 + s * (2f64.sqrt() - 1.0);
  let x = rho / 2f64.sqrt();
  0.5f64.ln() / x.ln()
}

/// A constant-width curve on `n` (odd) sides.
pub fn reuleaux_path(n: u32, r: f64, segments: u32) -> Result<Path, String> {
  if n < 3 || n.is_multiple_of(2) {
    return Err("n must be an odd number of at least 3".to_string());
  }
  let ssegs = (segments / n).max(3);
  let a = (180.0 - 180.0 / n as f64).to_radians();
  let slen = {
    let p0 = [r, 0.0];
    let p1 = [r * a.cos(), r * a.sin()];
    ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt()
  };
  let mut path = Path::new();
  for i in 0..n {
    let ca = 180.0 - (i as f64 + 0.5) * 360.0 / n as f64;
    let sa = ca + 180.0 + (90.0 / n as f64);
    let ea = ca + 180.0 - (90.0 / n as f64);
    let (s, c) = ca.to_radians().sin_cos();
    let cp = [r * c, r * s];
    path.extend(arc_pts(ssegs - 1, slen, cp, sa, ea - sa, false));
  }
  Ok(path)
}

/// Gielis's superformula, the family behind `supershape()`.
#[allow(clippy::too_many_arguments)]
pub fn superformula(
  theta: f64,
  m1: f64,
  m2: f64,
  n1: f64,
  n2: f64,
  n3: f64,
  a: f64,
  b: f64,
) -> f64 {
  let t1 = ((m1 * theta / 4.0).to_radians().cos() / a).abs().powf(n2);
  let t2 = ((m2 * theta / 4.0).to_radians().sin() / b).abs().powf(n3);
  (t1 + t2).powf(-1.0 / n1)
}

/// An egg outline: two end circles blended by a large side radius.
pub fn egg_path(
  length: f64,
  r1: f64,
  r2: f64,
  big_r: f64,
  segs: &dyn Fn(f64) -> u32,
) -> Result<Path, String> {
  if big_r <= length / 2.0 {
    return Err("the side radius R must be larger than length/2".into());
  }
  if length <= r1 + r2 || length <= 2.0 * r1 || length <= 2.0 * r2 {
    return Err("length must be longer than the end diameters".into());
  }
  let c1 = [-length / 2.0 + r1, 0.0];
  let c2 = [length / 2.0 - r2, 0.0];

  // The blending arc's centre sits where circles of radius R-r1 and R-r2
  // about the two ends meet. The upper flank bulges away from its centre,
  // so the centre that draws it is the one *below* the axis.
  let d = c2[0] - c1[0];
  let ra = big_r - r1;
  let rb = big_r - r2;
  let x = (d * d + ra * ra - rb * rb) / (2.0 * d);
  let h2 = ra * ra - x * x;
  if h2 <= 0.0 {
    return Err("the given radii do not meet in an egg shape".into());
  }
  let m = [c1[0] + x, -h2.sqrt()];

  let dir = |from: V2, to: V2, rad: f64| -> V2 {
    let v = [to[0] - from[0], to[1] - from[1]];
    let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
    [from[0] + rad * v[0] / n, from[1] + rad * v[1] / n]
  };
  let t1 = dir(c1, [c1[0] - (m[0] - c1[0]), -(m[1] - c1[1])], r1);
  let t2 = dir(c2, [c2[0] - (m[0] - c2[0]), -(m[1] - c2[1])], r2);
  let t1m = [t1[0], -t1[1]];
  let t2m = [t2[0], -t2[1]];
  let mm = [m[0], -m[1]];

  let ang = |cp: V2, p: V2| (p[1] - cp[1]).atan2(p[0] - cp[0]).to_degrees();
  let sweep = |from: f64, to: f64, positive: bool| {
    let mut d = to - from;
    while d <= 0.0 && positive {
      d += 360.0;
    }
    while d >= 0.0 && !positive {
      d -= 360.0;
    }
    d
  };
  let mut path = Path::new();
  let a0 = ang(c2, [length / 2.0, 0.0]);
  let a1 = ang(c2, t2);
  let sw = sweep(a0, a1, true);
  path.extend(bosl_arc(segs(r2), r2, c2, a0, sw, false));
  let b0 = ang(m, t2);
  let b1 = ang(m, t1);
  let sw = sweep(b0, b1, true);
  path.extend(bosl_arc(segs(big_r), big_r, m, b0, sw, false));
  let c0 = ang(c1, t1);
  let c1a = ang(c1, t1m);
  let sw = sweep(c0, c1a, true);
  path.extend(bosl_arc(segs(r1), r1, c1, c0, sw, false));
  let d0 = ang(mm, t1m);
  let d1 = ang(mm, t2m);
  let sw = sweep(d0, d1, true);
  path.extend(bosl_arc(segs(big_r), big_r, mm, d0, sw, false));
  let e0 = ang(c2, t2m);
  let e1 = ang(c2, [length / 2.0, 0.0]);
  let sw = sweep(e0, e1, true);
  path.extend(arc_pts(arc_n(segs(r2), sw), r2, c2, e0, sw, false));
  Ok(dedup_closed(path))
}

/// A round hole with a larger round head, joined by shoulders.
pub fn keyhole_path(
  l: f64,
  r1: f64,
  r2: f64,
  shoulder_r: f64,
  segs: &dyn Fn(f64) -> u32,
) -> Result<Path, String> {
  if l <= 0.0 || l < r1.max(r2) {
    return Err("l must be positive and at least the larger radius".into());
  }
  let cp1 = [0.0, 0.0];
  let cp2 = [0.0, -l];
  let minr = r1.min(r2) + shoulder_r;
  let maxr = r1.max(r2) + shoulder_r;
  let dy = (maxr * maxr - minr * minr).max(0.0).sqrt();
  let spt1 = if r1 > r2 {
    [cp1[0] + minr, cp1[1] - dy]
  } else {
    [cp2[0] + minr, cp2[1] + dy]
  };
  let spt2 = [-spt1[0], spt1[1]];
  let base = if r1 > r2 { cp1 } else { cp2 };
  let ds = [spt1[0] - base[0], spt1[1] - base[1]];
  let ang = ds[1].abs().atan2(ds[0].abs()).to_degrees();

  let mut path = Path::new();
  if r1 > r2 {
    if shoulder_r <= 0.0 {
      path.push(spt1);
    } else {
      path.extend(bosl_arc(
        segs(shoulder_r),
        shoulder_r,
        spt1,
        180.0 - ang,
        ang,
        false,
      ));
    }
    path.extend(bosl_arc(segs(r2), r2, cp2, 0.0, -180.0, false));
    if shoulder_r <= 0.0 {
      path.push(spt2);
    } else {
      path.extend(bosl_arc(
        segs(shoulder_r),
        shoulder_r,
        spt2,
        0.0,
        ang,
        false,
      ));
    }
    path.extend(bosl_arc(
      segs(r1),
      r1,
      cp1,
      180.0 + ang,
      -180.0 - 2.0 * ang,
      false,
    ));
  } else {
    if shoulder_r <= 0.0 {
      path.push(spt1);
    } else {
      path.extend(bosl_arc(
        segs(shoulder_r),
        shoulder_r,
        spt1,
        180.0,
        ang,
        false,
      ));
    }
    path.extend(bosl_arc(segs(r2), r2, cp2, ang, -180.0 - 2.0 * ang, false));
    if shoulder_r <= 0.0 {
      path.push(spt2);
    } else {
      path.extend(bosl_arc(
        segs(shoulder_r),
        shoulder_r,
        spt2,
        360.0 - ang,
        ang,
        false,
      ));
    }
    path.extend(bosl_arc(segs(r1), r1, cp1, 180.0, -180.0, false));
  }
  Ok(dedup_closed(path))
}

// ---------------------------------------------------------------------------
// Path utilities
// ---------------------------------------------------------------------------

/// Drop points that repeat their predecessor, treating the path as closed.
pub fn dedup_closed(path: Path) -> Path {
  let mut out: Path = Vec::with_capacity(path.len());
  for p in path {
    if out.last().is_none_or(|q: &V2| {
      (q[0] - p[0]).abs() > EPS || (q[1] - p[1]).abs() > EPS
    }) {
      out.push(p);
    }
  }
  while out.len() > 1 {
    let first = out[0];
    let last = out[out.len() - 1];
    if (first[0] - last[0]).abs() < EPS && (first[1] - last[1]).abs() < EPS {
      out.pop();
    } else {
      break;
    }
  }
  out
}

/// Rotate a closed path so it begins at its rightmost point.
fn rotate_to_max_x(path: Path) -> Path {
  if path.is_empty() {
    return path;
  }
  let idx = path
    .iter()
    .enumerate()
    .max_by(|a, b| a.1[0].total_cmp(&b.1[0]))
    .map(|(i, _)| i)
    .unwrap_or(0);
  let mut out = path[idx..].to_vec();
  out.extend_from_slice(&path[..idx]);
  out
}

/// The path with every point run through a matrix, ignoring Z.
pub fn apply_2d(m: &Mat4, path: &[V2]) -> Path {
  path
    .iter()
    .map(|p| {
      let q = m.apply([p[0], p[1], 0.0]);
      [q[0], q[1]]
    })
    .collect()
}

/// A closed outline as a polygon node.
pub fn path_node(path: &[V2]) -> ScadNode {
  ScadNode::Polygon {
    points: path.iter().map(|p| [p[0] as f32, p[1] as f32]).collect(),
    paths: None,
  }
}

/// The outline's bounding size, used to anchor rectangle-like shapes.
fn extent_geom(path: &[V2]) -> Attachable {
  Attachable::new(Geom::RegionExtent {
    points: path.to_vec(),
  })
}

// ---------------------------------------------------------------------------
// Shared argument handling
// ---------------------------------------------------------------------------

/// Read a parameter that is either one number or one per corner.
fn per_corner(args: &Args, name: &str) -> [f64; 4] {
  match args.raw(name) {
    None => [0.0; 4],
    Some(_) => match args.nums(name) {
      Some(v) if v.len() == 4 => [v[0], v[1], v[2], v[3]],
      _ => [args.num_or(name, 0.0); 4],
    },
  }
}

fn build_err(args: &Args, e: String) -> mlua::Error {
  mlua::Error::RuntimeError(format!("bosl.{}(): {e}", args.func()))
}

// ---------------------------------------------------------------------------
// Shape builders
// ---------------------------------------------------------------------------

type Build = fn(&Args) -> LuaResult<Option<ScadNode>>;

/// Place a finished outline, in the form the builders return.
fn placed(
  node: ScadNode,
  args: &Args,
  attachable: &Attachable,
) -> LuaResult<Option<ScadNode>> {
  Ok(Some(reorient(node, args, attachable)?))
}

/// The parameter list and builder for a 2D shape, if it has a native one.
pub fn builder(name: &str) -> Option<(&'static [&'static str], Build)> {
  Some(match name {
    "rect" => (
      &["size", "rounding", "chamfer", "atype", "corner_flip"],
      build_rect as Build,
    ),
    "ellipse" => (
      &["r", "d", "realign", "circum", "uniform"],
      build_ellipse as Build,
    ),
    "regular_ngon" => (NGON_PARAMS, build_ngon as Build),
    "pentagon" => (NGON_PARAMS, build_pentagon as Build),
    "hexagon" => (NGON_PARAMS, build_hexagon as Build),
    "octagon" => (NGON_PARAMS, build_octagon as Build),
    "right_triangle" => (&["size", "center"], build_right_triangle as Build),
    "trapezoid" => (
      &[
        "h", "w1", "w2", "ang", "shift", "chamfer", "rounding", "flip", "atype",
      ],
      build_trapezoid as Build,
    ),
    "star" => (
      &[
        "n",
        "r",
        "ir",
        "d",
        "or",
        "od",
        "id",
        "step",
        "realign",
        "align_tip",
        "align_pit",
        "atype",
      ],
      build_star as Build,
    ),
    "teardrop2d" => (
      &["r", "ang", "cap_h", "d", "circum", "realign"],
      build_teardrop2d as Build,
    ),
    "egg" => (
      &["length", "r1", "r2", "R", "d1", "d2", "D"],
      build_egg as Build,
    ),
    "glued_circles" => (
      &["r", "spread", "tangent", "d"],
      build_glued_circles as Build,
    ),
    "squircle" => (
      &["size", "squareness", "style", "atype"],
      build_squircle as Build,
    ),
    "keyhole" => (
      &["l", "r1", "r2", "shoulder_r", "d1", "d2", "length"],
      build_keyhole as Build,
    ),
    "reuleaux_polygon" => (&["n", "r", "d"], build_reuleaux as Build),
    "supershape" => (
      &[
        "step", "n", "m1", "m2", "n1", "n2", "n3", "a", "b", "r", "d", "atype",
      ],
      build_supershape as Build,
    ),
    "jittered_poly" => (&["path", "dist"], build_jittered_poly as Build),
    "ring" => (
      &[
        "n",
        "ring_width",
        "r",
        "r1",
        "r2",
        "angle",
        "d",
        "d1",
        "d2",
        "cp",
        "points",
        "corner",
        "width",
        "thickness",
        "start",
        "long",
        "full",
        "cw",
        "ccw",
      ],
      build_ring as Build,
    ),
    _ => return None,
  })
}

const NGON_PARAMS: &[&str] = &[
  "n",
  "r",
  "d",
  "or",
  "od",
  "ir",
  "id",
  "side",
  "rounding",
  "realign",
  "align_tip",
  "align_side",
];

fn build_rect(args: &Args) -> LuaResult<Option<ScadNode>> {
  let size = args.vec2("size").unwrap_or([1.0, 1.0]);
  let rounding = per_corner(args, "rounding");
  let chamfer = per_corner(args, "chamfer");
  let segments = args.segments(
    rounding
      .iter()
      .chain(chamfer.iter())
      .fold(0.0f64, |a, v| a.max(v.abs())),
  );
  let path = rect_path(size, rounding, chamfer, segments)
    .map_err(|e| build_err(args, e))?;

  // A plain rectangle anchors on its own box, so a corner anchor lands on
  // the corner even when rounding has cut it away.
  let attachable = if args.string("atype").as_deref() == Some("perim") {
    extent_geom(&path)
  } else {
    Attachable::new(Geom::Trapezoid {
      size,
      size2: size[0],
      shift: 0.0,
    })
  };
  placed(path_node(&path), args, &attachable)
}

fn build_ellipse(args: &Args) -> LuaResult<Option<ScadNode>> {
  let r = match args.raw("d") {
    Some(_) => args.vec2("d").map(|d| [d[0] / 2.0, d[1] / 2.0]),
    None => args.vec2("r"),
  }
  .unwrap_or([1.0, 1.0]);
  if r[0] <= 0.0 || r[1] <= 0.0 {
    return args.err("all components of the radius must be positive");
  }
  let sides = args.segments(r[0].max(r[1]));
  let circum = args.bool_or("circum", false);
  let path = ellipse_path(r, sides, args.bool_or("realign", false), circum);
  let sc = if circum {
    1.0 / (PI / sides as f64).cos()
  } else {
    1.0
  };
  placed(
    path_node(&path),
    args,
    &Attachable::new(Geom::Ellipse {
      r: [r[0] * sc, r[1] * sc],
    }),
  )
}

/// Resolve the many ways a regular polygon's size can be given.
fn ngon_radius(args: &Args, n: u32) -> LuaResult<f64> {
  let sc = 1.0 / (PI / n as f64).cos();
  // An inscribed radius measures to the flat, so it grows to the tip.
  if let Some(ir) = args.num("ir") {
    return Ok(ir * sc);
  }
  if let Some(id) = args.num("id") {
    return Ok(id / 2.0 * sc);
  }
  if let Some(or) = args.num("or") {
    return Ok(or);
  }
  if let Some(od) = args.num("od") {
    return Ok(od / 2.0);
  }
  if let Some(r) = args.radius("r", "d", None) {
    return Ok(r);
  }
  if let Some(side) = args.num("side") {
    return Ok(side / 2.0 / (PI / n as f64).sin());
  }
  args.err("need one of r, d, or, od, ir, id or side")
}

fn ngon_with_sides(args: &Args, n: u32) -> LuaResult<Option<ScadNode>> {
  if n < 3 {
    return args.err("n must be at least 3");
  }
  let r = ngon_radius(args, n)?;
  if r <= 0.0 {
    return args.err("the polygon size must be positive");
  }
  let rounding = args.num_or("rounding", 0.0);
  let path = ngon_path(n, r, rounding, args.segments(r));

  let mut m = Mat4::identity();
  if args.bool_or("realign", false) {
    m = Mat4::zrot(-180.0 / n as f64);
  }
  if let Some(tip) = args.vec2("align_tip") {
    m = m.mul(&Mat4::rot_from_to([1.0, 0.0, 0.0], [tip[0], tip[1], 0.0]));
  } else if let Some(side) = args.vec2("align_side") {
    m = m
      .mul(&Mat4::rot_from_to([1.0, 0.0, 0.0], [side[0], side[1], 0.0]))
      .mul(&Mat4::zrot(180.0 / n as f64));
  }
  let path = apply_2d(&m, &path);
  placed(path_node(&path), args, &extent_geom(&path))
}

fn build_ngon(args: &Args) -> LuaResult<Option<ScadNode>> {
  ngon_with_sides(args, args.int("n").unwrap_or(6) as u32)
}

fn build_pentagon(args: &Args) -> LuaResult<Option<ScadNode>> {
  ngon_with_sides(args, 5)
}

fn build_hexagon(args: &Args) -> LuaResult<Option<ScadNode>> {
  ngon_with_sides(args, 6)
}

fn build_octagon(args: &Args) -> LuaResult<Option<ScadNode>> {
  ngon_with_sides(args, 8)
}

fn build_right_triangle(args: &Args) -> LuaResult<Option<ScadNode>> {
  let size = args.vec2("size").unwrap_or([1.0, 1.0]);
  if size[0] <= 0.0 || size[1] <= 0.0 {
    return args.err("size must be positive");
  }
  let path = vec![
    [size[0] / 2.0, -size[1] / 2.0],
    [-size[0] / 2.0, -size[1] / 2.0],
    [-size[0] / 2.0, size[1] / 2.0],
  ];
  // A right triangle's default anchor is its square corner, not the middle.
  let attachable = Attachable::new(Geom::Trapezoid {
    size,
    size2: size[0],
    shift: 0.0,
  })
  .with_named("hypot", [0.0, 0.0, 0.0]);
  let corner = [-1.0, -1.0, 0.0];
  Ok(Some(crate::bosl::attach::reorient_default(
    path_node(&path),
    args,
    &attachable,
    corner,
    corner,
  )?))
}

/// Fill in whichever of height, widths and angles the caller left out.
fn trapezoid_dims(args: &Args) -> LuaResult<(f64, f64, f64, f64)> {
  let h = args.num("h").or_else(|| args.num("height"));
  let w1 = args.num("w1");
  let w2 = args.num("w2");
  let ang = args.nums("ang").map(|v| {
    if v.len() >= 2 {
      [v[0], v[1]]
    } else {
      [v[0], v[0]]
    }
  });
  let shift = args.num_or("shift", 0.0);

  match (h, w1, w2, ang) {
    (Some(h), Some(w1), Some(w2), None) => Ok((h, w1, w2, shift)),
    // With one width and the wall angles, the other width follows from how
    // far the walls lean over the height.
    (Some(h), Some(w1), None, Some(a)) => {
      let dx = h / a[0].to_radians().tan() + h / a[1].to_radians().tan();
      Ok((h, w1, w1 - dx, 0.0))
    }
    (Some(h), None, Some(w2), Some(a)) => {
      let dx = h / a[0].to_radians().tan() + h / a[1].to_radians().tan();
      Ok((h, w2 + dx, w2, 0.0))
    }
    (None, Some(w1), Some(w2), Some(a)) => {
      let t = 1.0 / a[0].to_radians().tan() + 1.0 / a[1].to_radians().tan();
      if t.abs() < EPS {
        return args.err("the given angles never close the trapezoid");
      }
      Ok(((w1 - w2) / t, w1, w2, 0.0))
    }
    _ => args.err("give exactly three of h, w1, w2 and ang"),
  }
}

fn build_trapezoid(args: &Args) -> LuaResult<Option<ScadNode>> {
  let (h, w1, w2, shift) = trapezoid_dims(args)?;
  if h <= 0.0 || w1 < 0.0 || w2 < 0.0 || w1 + w2 <= 0.0 {
    return args.err("degenerate trapezoid geometry");
  }
  let path = trapezoid_path(h, w1, w2, shift);
  let attachable = Attachable::new(Geom::Trapezoid {
    size: [w1, h],
    size2: w2,
    shift,
  });
  placed(path_node(&path), args, &attachable)
}

fn build_star(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(n) = args.int("n").map(|v| v as u32) else {
    return args.err("must specify the number of points, n");
  };
  let r = args
    .num("or")
    .or_else(|| args.num("od").map(|d| d / 2.0))
    .or_else(|| args.radius("r", "d", None));
  let Some(r) = r else {
    return args.err("must specify the outer radius r");
  };
  let ir = match args.int("step") {
    Some(step) => {
      if step <= 1 || (step as f64) >= n as f64 / 2.0 {
        return args.err("step must be between 2 and n/2");
      }
      let a = (180.0 * step as f64 / n as f64).to_radians().cos();
      let b = (180.0 * (step - 1) as f64 / n as f64).to_radians().cos();
      r * a / b
    }
    None => match args.num("ir").or_else(|| args.num("id").map(|d| d / 2.0)) {
      Some(ir) => ir,
      None => return args.err("must specify exactly one of ir, id or step"),
    },
  };

  let mut m = Mat4::identity();
  if args.bool_or("realign", false) {
    m = Mat4::zrot(-180.0 / n as f64);
  }
  if let Some(tip) = args.vec2("align_tip") {
    m = m.mul(&Mat4::rot_from_to([1.0, 0.0, 0.0], [tip[0], tip[1], 0.0]));
  } else if let Some(pit) = args.vec2("align_pit") {
    m = m
      .mul(&Mat4::rot_from_to([1.0, 0.0, 0.0], [pit[0], pit[1], 0.0]))
      .mul(&Mat4::zrot(180.0 / n as f64));
  }
  let path = apply_2d(&m, &star_path(n, r, ir));
  placed(path_node(&path), args, &extent_geom(&path))
}

fn build_teardrop2d(args: &Args) -> LuaResult<Option<ScadNode>> {
  let r = args.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let ang = args.num_or("ang", 45.0);
  let path = teardrop2d_path(
    r,
    ang,
    args.num("cap_h"),
    args.segments(r),
    args.bool_or("realign", false),
  )
  .map_err(|e| build_err(args, e))?;
  placed(path_node(&path), args, &extent_geom(&path))
}

fn build_egg(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(length) = args.num("length") else {
    return args.err("length is required");
  };
  let r1 = args.radius_end("r1", "d1", "r1", "d1", None);
  let r2 = args.radius_end("r2", "d2", "r2", "d2", None);
  let big = args.num("R").or_else(|| args.num("D").map(|d| d / 2.0));
  let (Some(r1), Some(r2), Some(big)) = (r1, r2, big) else {
    return args.err("r1, r2 and R are all required");
  };
  let path = egg_path(length, r1, r2, big, &|r| args.segments(r))
    .map_err(|e| build_err(args, e))?;
  let attachable = extent_geom(&path)
    .with_named("left", [-length / 2.0 + r1, 0.0, 0.0])
    .with_named("right", [length / 2.0 - r2, 0.0, 0.0]);
  placed(path_node(&path), args, &attachable)
}

fn build_glued_circles(args: &Args) -> LuaResult<Option<ScadNode>> {
  let r = args.radius("r", "d", Some(10.0)).unwrap_or(10.0);
  let path = glued_circles_path(
    r,
    args.num_or("spread", 10.0),
    args.num_or("tangent", 30.0),
    args.segments(r),
  );
  placed(path_node(&path), args, &extent_geom(&path))
}

fn build_squircle(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(size) = args.vec2("size") else {
    return args.err("size is required");
  };
  let squareness = args.num_or("squareness", 0.5);
  if !(0.0..=1.0).contains(&squareness) {
    return args.err("squareness must be between 0 and 1");
  }
  let style = args.string("style").unwrap_or_else(|| "fg".to_string());
  let path =
    squircle_path(size, squareness, &style, args.int("fn").unwrap_or(0) as u32)
      .map_err(|e| build_err(args, e))?;
  let attachable = if args.string("atype").as_deref() == Some("perim") {
    extent_geom(&path)
  } else {
    Attachable::new(Geom::Trapezoid {
      size,
      size2: size[0],
      shift: 0.0,
    })
  };
  placed(path_node(&path), args, &attachable)
}

fn build_keyhole(args: &Args) -> LuaResult<Option<ScadNode>> {
  let l = args.num("l").or_else(|| args.num("length")).unwrap_or(15.0);
  let r1 = args.radius("r1", "d1", Some(5.0)).unwrap_or(5.0);
  let r2 = args.radius("r2", "d2", Some(10.0)).unwrap_or(10.0);
  let shoulder_r = args.num_or("shoulder_r", 0.0);
  let path = keyhole_path(l, r1, r2, shoulder_r, &|r| args.segments(r))
    .map_err(|e| build_err(args, e))?;
  placed(path_node(&path), args, &extent_geom(&path))
}

fn build_reuleaux(args: &Args) -> LuaResult<Option<ScadNode>> {
  let n = args.int("n").unwrap_or(3) as u32;
  let r = args.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let path =
    reuleaux_path(n, r, args.segments(r)).map_err(|e| build_err(args, e))?;
  placed(path_node(&path), args, &extent_geom(&path))
}

fn build_supershape(args: &Args) -> LuaResult<Option<ScadNode>> {
  let step = args.num_or("step", 0.5);
  let n = args
    .int("n")
    .map(|v| v as u32)
    .unwrap_or_else(|| (360.0 / step).ceil() as u32)
    .max(3);
  let m1 = args.num_or("m1", 4.0);
  let m2 = args.num_or("m2", m1);
  let n1 = args.num_or("n1", 1.0);
  let n2 = args.num_or("n2", n1);
  let n3 = args.num_or("n3", n2);
  let a = args.num_or("a", 1.0);
  let b = args.num_or("b", a);

  let angles: Vec<f64> = (0..n)
    .map(|i| 360.0 - 360.0 * i as f64 / n as f64)
    .collect();
  let rvals: Vec<f64> = angles
    .iter()
    .map(|t| superformula(*t, m1, m2, n1, n2, n3, a, b))
    .collect();
  let scale = match args.radius("r", "d", None) {
    Some(r) => {
      let max = rvals.iter().fold(0.0f64, |acc, v| acc.max(*v));
      if max <= 0.0 { 1.0 } else { r / max }
    }
    None => 1.0,
  };
  let path: Path = angles
    .iter()
    .zip(rvals.iter())
    .map(|(t, rv)| {
      let (s, c) = t.to_radians().sin_cos();
      [scale * rv * c, scale * rv * s]
    })
    .collect();
  placed(path_node(&path), args, &extent_geom(&path))
}

/// Nudge collinear points off the line they sit on.
///
/// A twisted `linear_extrude()` only bends a polygon at its vertices, so a
/// long straight run comes out as one flat facet. Moving the intermediate
/// points a fraction to alternate sides keeps them as real corners, and the
/// twist follows the subdivision.
pub fn jitter_path(path: &[V2], dist: f64) -> Path {
  if path.len() < 3 {
    return path.to_vec();
  }
  let n = path.len();
  let mut out = Vec::with_capacity(n);
  out.push(path[0]);
  for i in 1..n {
    let prev = path[i - 1];
    let here = path[i];
    let next = path[(i + 1) % n];
    // Only a point its neighbours already line up with needs moving.
    let cross = (here[0] - prev[0]) * (next[1] - prev[1])
      - (here[1] - prev[1]) * (next[0] - prev[0]);
    if cross.abs() > EPS {
      out.push(here);
      continue;
    }
    // The segment's left normal, which is the direction to move along.
    let d = [here[0] - prev[0], here[1] - prev[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len <= EPS {
      out.push(here);
      continue;
    }
    let normal = [-d[1] / len, d[0] / len];
    // Alternating sides keeps the outline's area unchanged to first order.
    let amount = dist * ((i % 2) as f64 * 2.0 - 1.0);
    out.push([here[0] + normal[0] * amount, here[1] + normal[1] * amount]);
  }
  out
}

fn build_jittered_poly(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(path) = args.points2("path") else {
    return args.err("path is required");
  };
  let jittered = jitter_path(&path, args.num_or("dist", 1.0 / 512.0));
  placed(path_node(&jittered), args, &extent_geom(&jittered))
}

/// A ring: the area between two concentric circles, whole or as an arc.
fn build_ring(args: &Args) -> LuaResult<Option<ScadNode>> {
  // The point, corner and width/thickness forms fit the circle to given
  // points rather than taking a radius; those still go through OpenSCAD.
  if [
    "points",
    "corner",
    "width",
    "thickness",
    "start",
    "long",
    "cw",
    "ccw",
  ]
  .iter()
  .any(|p| args.has(p))
  {
    return Ok(None);
  }
  let r = args.radius("r", "d", None);
  let r1 = args.radius("r1", "d1", None);
  let r2 = args.radius("r2", "d2", None);
  let ring_width = args.num("ring_width");
  // Either both edges are named, or one edge and the width across it. A
  // negative width puts the second edge inside the first.
  let (inner, outer) = match (r1, r2, r, ring_width) {
    (Some(a), Some(b), None, None) => (a.min(b), a.max(b)),
    (None, None, Some(r), Some(w)) => (r.min(r + w), r.max(r + w)),
    _ => return args.err("give r1 and r2, or r and ring_width"),
  };
  if inner <= 0.0 || outer <= inner {
    return args.err("the ring's radii must be positive and different");
  }
  let cp = args.vec2("cp").unwrap_or([0.0, 0.0]);
  let facets = args
    .int("n")
    .map(|v| (v as u32).max(3))
    .unwrap_or_else(|| args.segments(outer));

  // An angle always means a partial ring, whatever `full` says.
  let angle = args.nums("angle");
  let (start, sweep) = match angle.as_deref() {
    Some([a, b]) => (*a, b - a),
    Some([a]) => (0.0, *a),
    _ => (0.0, 360.0),
  };
  let whole = angle.is_none() || sweep.abs() >= 360.0;

  let node = if whole {
    // Two full circles, so the hole is a hole rather than a slit.
    let circle = |r: f64| ScadNode::Translate {
      x: cp[0] as f32,
      y: cp[1] as f32,
      z: 0.0,
      child: Box::new(ScadNode::Circle {
        r: r as f32,
        segments: facets,
      }),
    };
    ScadNode::Difference(vec![circle(outer), circle(inner)])
  } else {
    let mut path = bosl_arc(facets, outer, cp, start, sweep, true);
    path.extend(bosl_arc(facets, inner, cp, start + sweep, -sweep, true));
    path_node(&path)
  };

  let attachable = if whole {
    Attachable::new(Geom::Ellipse { r: [outer, outer] })
  } else {
    let mut path = bosl_arc(facets, outer, cp, start, sweep, true);
    path.extend(bosl_arc(facets, inner, cp, start + sweep, -sweep, true));
    extent_geom(&path)
  };
  placed(node, args, &attachable)
}

// ---------------------------------------------------------------------------
// The 2D operators, which reshape a sketch rather than make one
// ---------------------------------------------------------------------------

/// Read the sketch a 2D operator works on.
///
/// BOSL2 takes it as a child; LuaCAD has no child syntax, so it comes in as
/// the `p` argument the way the transforms take theirs.
fn read_child(args: &Args) -> LuaResult<ScadNode> {
  match args.raw("p") {
    Some(mlua::Value::UserData(ud)) => match ud.borrow::<CsgSketch>() {
      Ok(s) => Ok(s.scad.clone().unwrap_or_else(|| ScadNode::Union(vec![]))),
      Err(_) => args.err("p must be a 2D shape"),
    },
    _ => args.err("p is required, and must be a 2D shape"),
  }
}

fn offset_r(child: ScadNode, r: f64) -> ScadNode {
  if r == 0.0 {
    return child;
  }
  ScadNode::Offset {
    delta: None,
    r: Some(r as f32),
    chamfer: false,
    child: Box::new(child),
  }
}

fn offset_delta(child: ScadNode, delta: f64, chamfer: bool) -> ScadNode {
  if delta == 0.0 {
    return child;
  }
  ScadNode::Offset {
    delta: Some(delta as f32),
    r: None,
    chamfer,
    child: Box::new(child),
  }
}

/// Round a shape's corners by offsetting out and back in again.
///
/// Growing then shrinking rounds the convex corners and leaves the concave
/// ones; the chamfered inward step first keeps the concave corners from
/// being rounded when only `or` was asked for.
fn round2d_node(child: ScadNode, or: f64, ir: f64) -> ScadNode {
  offset_r(offset_r(offset_delta(child, ir, true), -ir - or), or)
}

/// One radius given as either a single number or `[convex, concave]`.
fn radius_pair(args: &Args, name: &str) -> [f64; 2] {
  match args.nums(name) {
    Some(v) if v.len() >= 2 => [v[0], v[1]],
    _ => [args.num_or(name, 0.0); 2],
  }
}

fn sketch_value(
  lua: &mlua::Lua,
  function: &'static str,
  args: String,
  child: ScadNode,
  native: ScadNode,
) -> LuaResult<mlua::Value> {
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    function,
    args,
    vec![child],
    Some(native),
  );
  Ok(mlua::Value::UserData(lua.create_userdata(CsgSketch {
    #[cfg(feature = "csgrs")]
    sketch: crate::geometry::empty_sketch(),
    #[cfg(not(feature = "csgrs"))]
    sketch: (),
    color: None,
    material: None,
    scad: Some(scad),
  })?))
}

fn round2d(lua: &mlua::Lua, args: &Args) -> LuaResult<mlua::Value> {
  let r = args.num("r");
  let or = args.num("or").or(r).unwrap_or(0.0);
  let ir = args.num("ir").or(r).unwrap_or(0.0);
  let child = read_child(args)?;
  let native = round2d_node(child.clone(), or, ir);
  sketch_value(
    lua,
    "round2d",
    format!("or = {or}, ir = {ir}"),
    child,
    native,
  )
}

fn shell2d(lua: &mlua::Lua, args: &Args) -> LuaResult<mlua::Value> {
  // A positive thickness grows the shell outward, a negative one eats it
  // inward, and a pair does both at once.
  let thickness = match args.nums("thickness") {
    Some(v) if v.len() >= 2 => [v[0].min(v[1]), v[0].max(v[1])],
    _ => {
      let t = args.num_or("thickness", 1.0);
      if t < 0.0 { [t, 0.0] } else { [0.0, t] }
    }
  };
  let orad = radius_pair(args, "or");
  let irad = radius_pair(args, "ir");
  let child = read_child(args)?;

  let native = ScadNode::Difference(vec![
    round2d_node(
      offset_delta(child.clone(), thickness[1], false),
      orad[0],
      orad[1],
    ),
    round2d_node(
      offset_delta(child.clone(), thickness[0], false),
      irad[1],
      irad[0],
    ),
  ]);
  sketch_value(
    lua,
    "shell2d",
    format!("thickness = [{}, {}]", thickness[0], thickness[1]),
    child,
    native,
  )
}

/// Register the 2D operators, which the generic shim cannot express because
/// they take a shape rather than only numbers.
pub fn register(lua: &mlua::Lua, bosl: &mlua::Table) -> LuaResult<()> {
  crate::bosl::threading::register_shape(
    lua,
    bosl,
    "round2d",
    &["r", "or", "ir", "p"],
    round2d,
  )?;
  crate::bosl::threading::register_shape(
    lua,
    bosl,
    "shell2d",
    &["thickness", "or", "ir", "p"],
    shell2d,
  )?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn area(path: &[V2]) -> f64 {
    crate::bosl::vnf::signed_area(path).abs()
  }

  #[test]
  fn a_plain_rect_is_its_nominal_size() {
    let p = rect_path([10.0, 20.0], [0.0; 4], [0.0; 4], 32).unwrap();
    assert_eq!(p.len(), 4);
    assert!((area(&p) - 200.0).abs() < 1e-9);
  }

  #[test]
  fn rounding_a_rect_removes_the_corners_it_cuts() {
    let r = 3.0;
    let p = rect_path([20.0, 20.0], [r; 4], [0.0; 4], 128).unwrap();
    // Four quarter-circles replace four squares of side r.
    let expected = 400.0 - 4.0 * r * r + PI * r * r;
    assert!((area(&p) - expected).abs() < 0.05, "{}", area(&p));
  }

  #[test]
  fn chamfering_a_rect_cuts_a_triangle_off_each_corner() {
    let c = 4.0;
    let p = rect_path([20.0, 20.0], [0.0; 4], [c; 4], 32).unwrap();
    assert!((area(&p) - (400.0 - 4.0 * c * c / 2.0)).abs() < 1e-9);
  }

  #[test]
  fn a_rect_corner_cannot_be_both_rounded_and_chamfered() {
    assert!(rect_path([10.0, 10.0], [2.0; 4], [2.0; 4], 32).is_err());
  }

  #[test]
  fn rounding_larger_than_the_rect_is_rejected() {
    assert!(rect_path([10.0, 10.0], [6.0; 4], [0.0; 4], 32).is_err());
  }

  #[test]
  fn an_ellipse_approaches_its_true_area_as_facets_are_added() {
    let p = ellipse_path([10.0, 5.0], 512, false, false);
    assert!((area(&p) - PI * 50.0).abs() < 0.01);
  }

  #[test]
  fn circumscribing_makes_the_polygon_larger_than_the_circle() {
    let inscribed = ellipse_path([10.0, 10.0], 8, false, false);
    let circumscribed = ellipse_path([10.0, 10.0], 8, false, true);
    assert!(area(&inscribed) < PI * 100.0);
    assert!(area(&circumscribed) > PI * 100.0);
  }

  #[test]
  fn a_hexagon_has_six_corners_on_the_given_radius() {
    let p = ngon_path(6, 10.0, 0.0, 32);
    assert_eq!(p.len(), 6);
    for q in &p {
      assert!(((q[0] * q[0] + q[1] * q[1]).sqrt() - 10.0).abs() < 1e-9);
    }
  }

  #[test]
  fn a_star_alternates_between_its_two_radii() {
    let p = star_path(5, 10.0, 4.0);
    assert_eq!(p.len(), 10);
    let radii: Vec<f64> = p
      .iter()
      .map(|q| (q[0] * q[0] + q[1] * q[1]).sqrt())
      .collect();
    for pair in radii.chunks(2) {
      assert!((pair[0] - pair[1]).abs() > 1.0);
    }
  }

  #[test]
  fn a_trapezoid_has_the_area_of_its_two_widths() {
    let p = trapezoid_path(10.0, 20.0, 10.0, 0.0);
    assert!((area(&p) - (20.0 + 10.0) / 2.0 * 10.0).abs() < 1e-9);
  }

  #[test]
  fn a_teardrop_is_taller_than_the_circle_it_caps() {
    let p = teardrop2d_path(10.0, 45.0, None, 64, false).unwrap();
    let top = p.iter().fold(f64::NEG_INFINITY, |a, q| a.max(q[1]));
    assert!(top > 10.0, "{top}");
    assert!((top - 10.0 / 45f64.to_radians().sin()).abs() < 1e-9);
  }

  #[test]
  fn a_blunt_teardrop_stops_at_its_cap_height() {
    let p = teardrop2d_path(10.0, 45.0, Some(12.0), 64, false).unwrap();
    let top = p.iter().fold(f64::NEG_INFINITY, |a, q| a.max(q[1]));
    assert!((top - 12.0).abs() < 1e-9, "{top}");
  }

  #[test]
  fn a_cap_below_the_circle_is_rejected() {
    assert!(teardrop2d_path(10.0, 45.0, Some(1.0), 64, false).is_err());
  }

  #[test]
  fn a_squircle_lies_between_its_circle_and_its_square() {
    let p = squircle_path([20.0, 20.0], 0.5, "fg", 64).unwrap();
    let a = area(&p);
    assert!(a > PI * 100.0 && a < 400.0, "{a}");
  }

  #[test]
  fn squareness_zero_is_a_circle_and_one_is_a_square() {
    let round = area(&squircle_path([20.0, 20.0], 0.0, "fg", 128).unwrap());
    let square = area(&squircle_path([20.0, 20.0], 1.0, "fg", 128).unwrap());
    assert!((round - PI * 100.0).abs() < 1.0, "{round}");
    assert!((square - 400.0).abs() < 5.0, "{square}");
  }

  #[test]
  fn a_reuleaux_triangle_has_constant_width() {
    let p = reuleaux_path(3, 10.0, 128).unwrap();
    // Width is the same measured along any pair of opposite directions.
    let width_at = |deg: f64| {
      let (s, c) = deg.to_radians().sin_cos();
      let proj: Vec<f64> = p.iter().map(|q| q[0] * c + q[1] * s).collect();
      proj.iter().fold(f64::NEG_INFINITY, |a, v| a.max(*v))
        - proj.iter().fold(f64::INFINITY, |a, v| a.min(*v))
    };
    let w0 = width_at(0.0);
    for deg in [17.0, 45.0, 90.0, 123.0] {
      assert!(
        (width_at(deg) - w0).abs() < 0.05,
        "{deg}: {}",
        width_at(deg)
      );
    }
  }

  #[test]
  fn an_even_sided_reuleaux_polygon_is_rejected() {
    assert!(reuleaux_path(4, 10.0, 64).is_err());
  }

  #[test]
  fn the_superformula_with_flat_exponents_is_a_circle() {
    for theta in [0.0, 30.0, 75.0, 200.0] {
      let r = superformula(theta, 4.0, 4.0, 2.0, 2.0, 2.0, 1.0, 1.0);
      assert!((r - 1.0).abs() < 1e-9, "{theta}: {r}");
    }
  }

  #[test]
  fn an_egg_is_longer_than_it_is_wide() {
    let p = egg_path(50.0, 8.0, 12.0, 40.0, &|_| 64).unwrap();
    let w = |i: usize| {
      p.iter().fold(f64::NEG_INFINITY, |a, q| a.max(q[i]))
        - p.iter().fold(f64::INFINITY, |a, q| a.min(q[i]))
    };
    assert!((w(0) - 50.0).abs() < 0.5, "{}", w(0));
    assert!(w(1) < w(0));
  }

  #[test]
  fn a_keyhole_spans_its_length_plus_both_radii() {
    let p = keyhole_path(15.0, 5.0, 10.0, 0.0, &|_| 64).unwrap();
    let top = p.iter().fold(f64::NEG_INFINITY, |a, q| a.max(q[1]));
    let bot = p.iter().fold(f64::INFINITY, |a, q| a.min(q[1]));
    assert!((top - 5.0).abs() < 0.2, "{top}");
    assert!((bot + 25.0).abs() < 0.2, "{bot}");
  }

  #[test]
  fn glued_circles_are_wider_than_one_circle() {
    let p = glued_circles_path(10.0, 30.0, 30.0, 64);
    let w = p.iter().fold(f64::NEG_INFINITY, |a, q| a.max(q[0]))
      - p.iter().fold(f64::INFINITY, |a, q| a.min(q[0]));
    assert!(w > 30.0, "{w}");
  }

  /// The area of a 2D shape, measured by extruding it one unit.
  fn area_of(code: &str) -> (f64, ([f32; 3], [f32; 3])) {
    let geoms = crate::lua_engine::execute_lua(&format!(
      "render(({code}):linear_extrude(1))"
    ))
    .unwrap();
    let m =
      crate::export::materialize_scad_manifold(&geoms[0].scad.clone().unwrap());
    (m.volume() as f64, m.bounding_box())
  }

  #[test]
  fn jitter_only_moves_the_points_between_two_collinear_neighbours() {
    // The corners of the square stay put; the midpoints of its sides move.
    let path = vec![
      [0.0, 0.0],
      [5.0, 0.0],
      [10.0, 0.0],
      [10.0, 10.0],
      [0.0, 10.0],
    ];
    let out = jitter_path(&path, 0.5);
    assert_eq!(out.len(), path.len());
    assert_eq!(out[0], path[0]);
    assert_eq!(out[3], path[3]);
    assert!((out[1][1] - path[1][1]).abs() > 0.4, "{:?}", out[1]);
    assert!((out[1][0] - path[1][0]).abs() < 1e-9, "{:?}", out[1]);
  }

  #[test]
  fn a_jittered_polygon_is_still_the_shape_it_started_as() {
    let (v, _) = area_of(
      "bosl.jittered_poly { path = { {0,0}, {10,0}, {20,0}, {20,20}, {0,20} },
                            dist = 0.01 }",
    );
    assert!((v - 400.0).abs() < 1.0, "{v}");
  }

  #[test]
  fn a_ring_is_the_area_between_its_two_circles() {
    let (v, (_, hi)) = area_of("bosl.ring { r1 = 10, r2 = 6, n = 256 }");
    let ideal = PI * (100.0 - 36.0);
    assert!((v - ideal).abs() / ideal < 0.001, "{v} against {ideal}");
    assert!((hi[0] - 10.0).abs() < 0.01, "{hi:?}");
  }

  #[test]
  fn a_ring_can_be_given_as_one_radius_and_a_width() {
    let both = area_of("bosl.ring { r1 = 6, r2 = 10, n = 256 }").0;
    let width = area_of("bosl.ring { r = 6, ring_width = 4, n = 256 }").0;
    assert!((both - width).abs() < 1e-6, "{both} against {width}");
    // A negative width puts the second edge inside the first.
    let inward = area_of("bosl.ring { r = 10, ring_width = -4, n = 256 }").0;
    assert!((both - inward).abs() < 1e-6, "{both} against {inward}");
  }

  #[test]
  fn an_arc_of_a_ring_covers_only_its_angle() {
    let full = area_of("bosl.ring { r1 = 10, r2 = 6, n = 256 }").0;
    let (half, (lo, _)) =
      area_of("bosl.ring { r1 = 10, r2 = 6, n = 256, angle = {0, 180} }");
    assert!((half / full - 0.5).abs() < 0.01, "{half} against {full}");
    // The lower half is gone, so nothing reaches below the axis.
    assert!(lo[1] > -0.01, "{lo:?}");
  }

  #[test]
  fn a_ring_needs_a_pair_of_radii_that_make_sense() {
    let err = crate::lua_engine::execute_lua("bosl.ring { r = 10 }")
      .unwrap_err()
      .to_string();
    assert!(err.contains("r1 and r2"), "{err}");
  }

  #[test]
  fn rounding_a_square_takes_its_corners_off() {
    let square = area_of("bosl.rect { {20, 20} }").0;
    let rounded =
      area_of("bosl.round2d { r = 4, p = bosl.rect { {20, 20} } }").0;
    // Four corners each lose a square less its inscribed quarter circle.
    // The arcs are facetted at OpenSCAD's default, which at this radius is
    // only about three segments a corner, so they cut a little deeper than
    // the true circle would.
    let lost = 4.0 * (16.0 - PI * 16.0 / 4.0);
    let actual = square - rounded;
    assert!(actual > lost, "{actual} against {lost}");
    assert!((actual - lost) / lost < 0.2, "{actual} against {lost}");
  }

  #[test]
  fn rounding_only_the_inside_corners_leaves_the_outside_ones_sharp() {
    // A star has both: sharp points outside and sharp valleys inside.
    let star = "bosl.star { n = 5, r = 50, ir = 25 }";
    let sharp = area_of(star).0;
    let outside =
      area_of(&format!("bosl.round2d {{ ['or'] = 5, p = {star} }}")).0;
    let inside = area_of(&format!("bosl.round2d {{ ir = 5, p = {star} }}")).0;
    // Rounding the points takes material off and rounding the valleys puts
    // it on, so the two land on opposite sides of the sharp shape.
    assert!(outside < sharp, "{outside} against {sharp}");
    assert!(inside > sharp, "{inside} against {sharp}");
  }

  #[test]
  fn a_shell_is_the_wall_left_between_two_offsets() {
    // A 2 mm wall taken inward from a 20 mm square.
    let (v, (_, hi)) =
      area_of("bosl.shell2d { thickness = -2, p = bosl.rect { {20, 20} } }");
    let ideal = 400.0 - 16.0f64.powi(2);
    assert!((v - ideal).abs() / ideal < 0.01, "{v} against {ideal}");
    assert!((hi[0] - 10.0).abs() < 0.01, "{hi:?}");
  }

  #[test]
  fn a_positive_shell_grows_outward_instead() {
    let (v, (_, hi)) =
      area_of("bosl.shell2d { thickness = 2, p = bosl.rect { {20, 20} } }");
    // The wall is outside the square, so the shape gets bigger.
    assert!((hi[0] - 12.0).abs() < 0.01, "{hi:?}");
    assert!(v > 400.0 - 16.0f64.powi(2), "{v}");
  }

  #[test]
  fn a_two_sided_shell_straddles_the_outline() {
    let (v, (_, hi)) = area_of(
      "bosl.shell2d { thickness = {-1, 1}, p = bosl.rect { {20, 20} } }",
    );
    assert!((hi[0] - 11.0).abs() < 0.01, "{hi:?}");
    let ideal = 22.0f64.powi(2) - 18.0f64.powi(2);
    assert!((v - ideal).abs() / ideal < 0.01, "{v} against {ideal}");
  }

  #[test]
  fn a_2d_operator_needs_a_2d_shape_to_work_on() {
    let err = crate::lua_engine::execute_lua("bosl.round2d { r = 2 }")
      .unwrap_err()
      .to_string();
    assert!(err.contains("2D shape"), "{err}");
  }
}
