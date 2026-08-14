//! BOSL2's anchor/spin/orient placement, ported for the native shapes.
//!
//! Every BOSL2 shape is built at a canonical position and then moved so the
//! requested anchor lands on the origin, spun about Z, and tipped so its axis
//! points along `orient`. Which point an anchor names depends on the shape's
//! bounding geometry, so each builder describes itself with an [`Geom`] and
//! lets [`reorient`] do the placement.
//!
//! LuaCAD has no `attach()` parent context, so only the standalone branch of
//! BOSL2's `_attach_transform()` applies:
//!
//! ```text
//! rot_by_axis(vector_axis(UP, orient), vector_angle(UP, orient))
//!   * zrot(spin)
//!   * translate(-anchor_pos)
//! ```

use crate::bosl::args::{Anchor, Args};
use crate::bosl::vecmath::{
  EPS, Mat4, V2, V3, add, approx, mul, norm, rot_from_to_p, sub, unit_or,
  vector_angle, vector_axis, vmul,
};
use crate::scad_export::ScadNode;

pub const UP: V3 = [0.0, 0.0, 1.0];
pub const RIGHT: V3 = [1.0, 0.0, 0.0];
pub const CENTER: V3 = [0.0, 0.0, 0.0];

/// The bounding geometry an anchor is resolved against.
///
/// These mirror BOSL2's `attach_geom()` types; only the ones the native
/// shapes need are modelled.
#[derive(Clone, Debug)]
pub enum Geom {
  /// A box or truncated pyramid: `size` at the bottom, `size2` at the top,
  /// the top offset by `shift`, extruded along `axis`.
  Prismoid {
    size: V3,
    size2: V2,
    shift: V2,
    axis: V3,
  },
  /// A cylinder or cone, with elliptical ends allowed.
  Conoid {
    r1: V2,
    r2: V2,
    l: f64,
    shift: V2,
    axis: V3,
  },
  /// A sphere or ellipsoid.
  Spheroid { r: V3 },
  /// An arbitrary solid, anchored on the extent of its vertices.
  VnfExtent { points: Vec<V3> },
  /// A 2D rectangle or trapezoid: `size` at the front, `size2` wide at the
  /// back, offset by `shift`.
  Trapezoid { size: V2, size2: f64, shift: f64 },
  /// A 2D ellipse.
  Ellipse { r: V2 },
  /// An arbitrary 2D shape, anchored on the extent of its outline.
  RegionExtent { points: Vec<V2> },
}

/// A shape's bounding geometry together with its centre point and the offset
/// applied to off-centre anchors.
#[derive(Clone, Debug)]
pub struct Attachable {
  pub geom: Geom,
  pub cp: V3,
  pub offset: V3,
  /// Shape-specific named anchors, such as a bottlecap's `"tamper-ring"`.
  pub named: Vec<(String, V3)>,
}

impl Attachable {
  pub fn new(geom: Geom) -> Attachable {
    Attachable {
      geom,
      cp: CENTER,
      offset: CENTER,
      named: Vec::new(),
    }
  }

  pub fn with_cp(mut self, cp: V3) -> Attachable {
    self.cp = cp;
    self
  }

  pub fn with_offset(mut self, offset: V3) -> Attachable {
    self.offset = offset;
    self
  }

  pub fn with_named(mut self, name: &str, pos: V3) -> Attachable {
    self.named.push((name.to_string(), pos));
    self
  }

  /// Whether this geometry lives in the XY plane.
  pub fn is_2d(&self) -> bool {
    matches!(
      self.geom,
      Geom::Trapezoid { .. } | Geom::Ellipse { .. } | Geom::RegionExtent { .. }
    )
  }

  /// The point an anchor direction names on this geometry.
  pub fn anchor_pos(&self, anchor: V3) -> V3 {
    // A zero component must not pick up the shape's offset, or a centred
    // anchor would drift off the centre line.
    let offset = [
      if anchor[0] == 0.0 {
        0.0
      } else {
        self.offset[0]
      },
      if anchor[1] == 0.0 {
        0.0
      } else {
        self.offset[1]
      },
      if anchor[2] == 0.0 {
        0.0
      } else {
        self.offset[2]
      },
    ];

    match &self.geom {
      Geom::Prismoid {
        size,
        size2,
        shift,
        axis,
      } => {
        let size = [size[0].max(0.0), size[1].max(0.0), size[2].max(0.0)];
        let size2 = [size2[0].max(0.0), size2[1].max(0.0)];
        let anch = rot_from_to_p(*axis, UP, anchor);
        let offset = rot_from_to_p(*axis, UP, offset);
        let h = size[2];
        let u = (anch[2] + 1.0) / 2.0;
        let axy = [anch[0], anch[1]];
        let bot = [size[0] / 2.0 * axy[0], size[1] / 2.0 * axy[1], -h / 2.0];
        let top = [
          size2[0] / 2.0 * axy[0] + shift[0],
          size2[1] / 2.0 * axy[1] + shift[1],
          h / 2.0,
        ];
        let pos = add(
          add(self.cp, crate::bosl::vecmath::lerp3(bot, top, u)),
          offset,
        );
        rot_from_to_p(UP, *axis, pos)
      }

      Geom::Conoid {
        r1,
        r2,
        l,
        shift: _,
        axis,
      } => {
        let anch = rot_from_to_p(*axis, UP, anchor);
        let offset = rot_from_to_p(*axis, UP, offset);
        let u = (anch[2] + 1.0) / 2.0;
        let dir = [anch[0], anch[1]];
        let bot2 = solve_ellipse(*r1, dir);
        let top2 = solve_ellipse(*r2, dir);
        let bot = [bot2[0], bot2[1], -l / 2.0];
        let top = [top2[0], top2[1], l / 2.0];
        let pos = add(
          add(self.cp, crate::bosl::vecmath::lerp3(bot, top, u)),
          offset,
        );
        rot_from_to_p(UP, *axis, pos)
      }

      Geom::Spheroid { r } => {
        let a = unit_or(anchor, CENTER);
        add(add(self.cp, vmul(*r, a)), offset)
      }

      Geom::VnfExtent { points } => {
        if norm(anchor) < EPS || points.is_empty() {
          return self.cp;
        }
        let m = Mat4::rot_from_to(anchor, RIGHT);
        let rpts: Vec<V3> =
          points.iter().map(|p| m.apply(sub(*p, self.cp))).collect();
        let maxx = rpts.iter().fold(f64::NEG_INFINITY, |a, p| a.max(p[0]));
        let hits: Vec<V3> = rpts
          .iter()
          .copied()
          .filter(|p| approx(p[0], maxx))
          .collect();
        // A face square to the anchor gives many hits whose average is the
        // face centre; the axis-aligned case keeps the anchor on the axis.
        let mpt = if anchor[0].abs() < EPS && anchor[1].abs() < EPS {
          [maxx, 0.0, 0.0]
        } else {
          let n = hits.len() as f64;
          hits.iter().fold(CENTER, |a, p| add(a, *p)).map(|c| c / n)
        };
        add(self.cp, rot_from_to_p(RIGHT, anchor, mpt))
      }

      Geom::Trapezoid { size, size2, shift } => {
        let u = (anchor[1] + 1.0) / 2.0;
        let frpt = [size[0] / 2.0 * anchor[0], -size[1] / 2.0];
        let bkpt = [size2 / 2.0 * anchor[0] + shift, size[1] / 2.0];
        [
          self.cp[0] + frpt[0] + (bkpt[0] - frpt[0]) * u + offset[0],
          self.cp[1] + frpt[1] + (bkpt[1] - frpt[1]) * u + offset[1],
          0.0,
        ]
      }

      Geom::Ellipse { r } => {
        let a = unit_or([anchor[0], anchor[1], 0.0], CENTER);
        let pos = if a[0].abs() < EPS {
          [0.0, a[1].signum() * r[1]]
        } else if r[0].abs() < EPS || r[1].abs() < EPS {
          [0.0, 0.0]
        } else {
          let m = a[1] / a[0];
          let px = a[0].signum()
            * (1.0 / (1.0 / (r[0] * r[0]) + m * m / (r[1] * r[1]))).sqrt();
          [px, m * px]
        };
        [
          self.cp[0] + offset[0] + pos[0],
          self.cp[1] + offset[1] + pos[1],
          0.0,
        ]
      }

      Geom::RegionExtent { points } => {
        if (anchor[0].abs() < EPS && anchor[1].abs() < EPS) || points.is_empty()
        {
          return self.cp;
        }
        let a = [anchor[0], anchor[1], 0.0];
        let m = Mat4::rot_from_to(a, RIGHT);
        let rpts: Vec<V3> =
          points.iter().map(|p| m.apply([p[0], p[1], 0.0])).collect();
        let maxx = rpts.iter().fold(f64::NEG_INFINITY, |acc, p| acc.max(p[0]));
        let ys: Vec<f64> = rpts
          .iter()
          .filter(|p| approx(p[0], maxx))
          .map(|p| p[1])
          .collect();
        let miny = ys.iter().fold(f64::INFINITY, |a, y| a.min(*y));
        let maxy = ys.iter().fold(f64::NEG_INFINITY, |a, y| a.max(*y));
        let pos = rot_from_to_p(RIGHT, a, [maxx, (miny + maxy) / 2.0, 0.0]);
        add(self.cp, [pos[0], pos[1], 0.0])
      }
    }
  }
}

/// Where a ray in direction `dir` leaves an ellipse with radii `r`.
fn solve_ellipse(r: V2, dir: V2) -> V2 {
  if dir[0].abs() < EPS && dir[1].abs() < EPS {
    return [0.0, 0.0];
  }
  let denom =
    (dir[0] * dir[0] * r[1] * r[1] + dir[1] * dir[1] * r[0] * r[0]).sqrt();
  if denom < EPS {
    return [0.0, 0.0];
  }
  [r[0] * dir[0] * r[1] / denom, r[0] * dir[1] * r[1] / denom]
}

/// The placement transform for a shape, or `None` when the shape is already
/// where it belongs.
pub fn placement(
  attachable: &Attachable,
  anchor: Option<&Anchor>,
  spin: f64,
  orient: V3,
) -> Option<Mat4> {
  let pos = anchor_position(attachable, anchor);

  let m = if attachable.is_2d() {
    Mat4::zrot(spin).mul(&Mat4::translate(mul(pos, -1.0)))
  } else {
    let axis = vector_axis(UP, orient);
    let ang = vector_angle(UP, orient);
    Mat4::rot_by_axis(axis, ang)
      .mul(&Mat4::zrot(spin))
      .mul(&Mat4::translate(mul(pos, -1.0)))
  };

  if m.is_identity() { None } else { Some(m) }
}

/// Resolve which anchor a call places its shape by, following BOSL2's
/// `get_anchor()`.
///
/// `center` wins over `anchor` when both are given: `true` means the middle
/// and `false` means `uncentered`, the shape's own "sitting on the plane"
/// position. With neither, the shape's `dflt` applies — and that is not
/// always the same as `uncentered`, which is why a `cyl()` with no arguments
/// straddles the XY plane but `cyl(center = false)` stands on it.
pub fn resolve_anchor(
  args: &Args,
  uncentered: V3,
  dflt: V3,
) -> mlua::Result<Anchor> {
  if let Some(center) = args.bool("center") {
    return Ok(Anchor::Vector(if center { CENTER } else { uncentered }));
  }
  Ok(args.anchor()?.unwrap_or(Anchor::Vector(dflt)))
}

/// Place a built shape, using the shape's own default anchor.
pub fn reorient_default(
  node: ScadNode,
  args: &Args,
  attachable: &Attachable,
  uncentered: V3,
  dflt: V3,
) -> mlua::Result<ScadNode> {
  let anchor = resolve_anchor(args, uncentered, dflt)?;
  reorient_at(node, args, attachable, Some(anchor))
}

/// Place a built shape according to the call's `anchor`, `spin` and `orient`.
pub fn reorient(
  node: ScadNode,
  args: &Args,
  attachable: &Attachable,
) -> mlua::Result<ScadNode> {
  let anchor = args.anchor()?;
  reorient_at(node, args, attachable, anchor)
}

fn reorient_at(
  node: ScadNode,
  args: &Args,
  attachable: &Attachable,
  anchor: Option<Anchor>,
) -> mlua::Result<ScadNode> {
  let spin = args.spin();

  // 2D placement is only a spin and a shift, and the sketch backend reads
  // rotate/translate directly — a `multmatrix` would have to be understood
  // in cross-section form as well, for no gain.
  if attachable.is_2d() {
    let pos = anchor_position(attachable, anchor.as_ref());
    let mut out = node;
    if pos[0] != 0.0 || pos[1] != 0.0 {
      out = ScadNode::Translate {
        x: -pos[0] as f32,
        y: -pos[1] as f32,
        z: 0.0,
        child: Box::new(out),
      };
    }
    if spin != 0.0 {
      out = ScadNode::Rotate {
        x: 0.0,
        y: 0.0,
        z: spin as f32,
        child: Box::new(out),
      };
    }
    return Ok(out);
  }

  Ok(
    match placement(attachable, anchor.as_ref(), spin, args.orient()) {
      None => node,
      Some(m) => ScadNode::Multmatrix {
        matrix: m.to_f32(),
        child: Box::new(node),
      },
    },
  )
}

/// The point an anchor names, resolving shape-specific names first.
fn anchor_position(attachable: &Attachable, anchor: Option<&Anchor>) -> V3 {
  match anchor {
    None => CENTER,
    Some(a) => match a {
      Anchor::Named(name) => attachable
        .named
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, p)| *p)
        .or_else(|| a.as_vector().map(|v| attachable.anchor_pos(v)))
        .unwrap_or(CENTER),
      Anchor::Vector(v) => attachable.anchor_pos(*v),
    },
  }
}

/// Apply a matrix to a node, skipping the wrapper when it would be a no-op.
pub fn transform(node: ScadNode, m: Mat4) -> ScadNode {
  if m.is_identity() {
    node
  } else {
    ScadNode::Multmatrix {
      matrix: m.to_f32(),
      child: Box::new(node),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::bosl::vecmath::approx3;

  #[test]
  fn a_cuboid_anchors_on_its_faces_and_corners() {
    let a = Attachable::new(Geom::Prismoid {
      size: [10.0, 20.0, 30.0],
      size2: [10.0, 20.0],
      shift: [0.0, 0.0],
      axis: UP,
    });
    assert!(approx3(a.anchor_pos(CENTER), [0.0, 0.0, 0.0]));
    assert!(approx3(a.anchor_pos([0.0, 0.0, -1.0]), [0.0, 0.0, -15.0]));
    assert!(approx3(a.anchor_pos([1.0, 0.0, 0.0]), [5.0, 0.0, 0.0]));
    assert!(approx3(a.anchor_pos([1.0, 1.0, 1.0]), [5.0, 10.0, 15.0]));
  }

  #[test]
  fn a_tapered_prismoid_anchors_on_the_narrower_top() {
    let a = Attachable::new(Geom::Prismoid {
      size: [40.0, 40.0, 30.0],
      size2: [20.0, 20.0],
      shift: [0.0, 0.0],
      axis: UP,
    });
    assert!(approx3(a.anchor_pos([1.0, 0.0, 1.0]), [10.0, 0.0, 15.0]));
    assert!(approx3(a.anchor_pos([1.0, 0.0, -1.0]), [20.0, 0.0, -15.0]));
  }

  #[test]
  fn a_cone_anchors_on_its_slanted_side() {
    let a = Attachable::new(Geom::Conoid {
      r1: [10.0, 10.0],
      r2: [5.0, 5.0],
      l: 20.0,
      shift: [0.0, 0.0],
      axis: UP,
    });
    assert!(approx3(a.anchor_pos([0.0, 0.0, 1.0]), [0.0, 0.0, 10.0]));
    assert!(approx3(a.anchor_pos([1.0, 0.0, -1.0]), [10.0, 0.0, -10.0]));
    assert!(approx3(a.anchor_pos([1.0, 0.0, 1.0]), [5.0, 0.0, 10.0]));
  }

  #[test]
  fn a_spheroid_anchors_on_its_surface() {
    let a = Attachable::new(Geom::Spheroid { r: [10.0; 3] });
    assert!(approx3(a.anchor_pos([0.0, 0.0, 1.0]), [0.0, 0.0, 10.0]));
    let d = a.anchor_pos([1.0, 1.0, 0.0]);
    let s = 10.0 / 2f64.sqrt();
    assert!(approx3(d, [s, s, 0.0]), "{d:?}");
  }

  #[test]
  fn an_ellipse_anchors_on_its_perimeter() {
    let a = Attachable::new(Geom::Ellipse { r: [10.0, 5.0] });
    assert!(approx3(a.anchor_pos([1.0, 0.0, 0.0]), [10.0, 0.0, 0.0]));
    assert!(approx3(a.anchor_pos([0.0, 1.0, 0.0]), [0.0, 5.0, 0.0]));
  }

  #[test]
  fn anchoring_bottom_lifts_the_shape_onto_the_xy_plane() {
    let a = Attachable::new(Geom::Prismoid {
      size: [10.0, 10.0, 10.0],
      size2: [10.0, 10.0],
      shift: [0.0, 0.0],
      axis: UP,
    });
    let m = placement(&a, Some(&Anchor::Vector([0.0, 0.0, -1.0])), 0.0, UP)
      .expect("bottom anchor moves the shape");
    assert!(approx3(m.apply([0.0, 0.0, -5.0]), [0.0, 0.0, 0.0]));
    assert!(approx3(m.apply([0.0, 0.0, 5.0]), [0.0, 0.0, 10.0]));
  }

  #[test]
  fn orient_tips_the_shape_axis_onto_the_given_direction() {
    let a = Attachable::new(Geom::Conoid {
      r1: [1.0, 1.0],
      r2: [1.0, 1.0],
      l: 10.0,
      shift: [0.0, 0.0],
      axis: UP,
    });
    let m = placement(&a, None, 0.0, RIGHT).expect("orient rotates the shape");
    let tip = m.apply([0.0, 0.0, 5.0]);
    assert!(approx3(tip, [5.0, 0.0, 0.0]), "{tip:?}");
  }
}
