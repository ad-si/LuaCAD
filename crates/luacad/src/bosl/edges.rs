//! BOSL2's way of naming the edges of a box.
//!
//! `cuboid()` and the edge masks let you round or chamfer any subset of a
//! box's twelve edges, named by the axis they run along, by a face they
//! border, by a corner they touch, or by a direction vector:
//!
//! ```lua
//! bosl.cuboid { {30, 20, 10}, rounding = 3, edges = "Z" }
//! bosl.cuboid { {30, 20, 10}, rounding = 3, edges = bosl.TOP }
//! bosl.cuboid { {30, 20, 10}, rounding = 3, except = bosl.BOTTOM }
//! ```

use mlua::Value as LuaValue;

use crate::bosl::args::{Args, as_nums};
use crate::bosl::vecmath::V3;

/// Which of a box's twelve edges are selected.
///
/// Indexed as `set[axis][i]`, where `axis` is the one the edge runs along and
/// `i` counts the four edges of that axis in the order BOSL2 uses: the two
/// remaining coordinates take `(-1,-1)`, `(1,-1)`, `(-1,1)`, `(1,1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeSet(pub [[bool; 4]; 3]);

pub const EDGES_ALL: EdgeSet = EdgeSet([[true; 4]; 3]);
pub const EDGES_NONE: EdgeSet = EdgeSet([[false; 4]; 3]);

/// The direction of edge `i` of `axis`, as the sign of the two coordinates
/// the edge does not run along.
///
/// For an X edge that is `[0, a, b]`, for a Y edge `[a, 0, b]`, and for a Z
/// edge `[a, b, 0]`, with `a` cycling fastest.
pub fn edge_vector(axis: usize, i: usize) -> V3 {
  let a = if i.is_multiple_of(2) { -1.0 } else { 1.0 };
  let b = if i < 2 { -1.0 } else { 1.0 };
  match axis {
    0 => [0.0, a, b],
    1 => [a, 0.0, b],
    _ => [a, b, 0.0],
  }
}

impl EdgeSet {
  pub fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
    (0..3).flat_map(move |ax| {
      (0..4).filter_map(move |i| self.0[ax][i].then_some((ax, i)))
    })
  }

  pub fn is_empty(&self) -> bool {
    self.0.iter().all(|ax| ax.iter().all(|e| !e))
  }

  pub fn count(&self) -> usize {
    self.iter().count()
  }

  /// Remove every edge in `other`.
  pub fn minus(&self, other: &EdgeSet) -> EdgeSet {
    let mut out = *self;
    for ax in 0..3 {
      for i in 0..4 {
        out.0[ax][i] &= !other.0[ax][i];
      }
    }
    out
  }

  pub fn union(&self, other: &EdgeSet) -> EdgeSet {
    let mut out = *self;
    for ax in 0..3 {
      for i in 0..4 {
        out.0[ax][i] |= other.0[ax][i];
      }
    }
    out
  }

  /// Whether all three edges meeting at `corner` are selected.
  pub fn corner_is_full(&self, corner: V3) -> bool {
    (0..3).all(|axis| {
      let (u, v) = other_axes(axis);
      let i = edge_index(corner[u], corner[v]);
      self.0[axis][i]
    })
  }
}

/// The two axes an edge along `axis` spans, in BOSL2's order.
pub fn other_axes(axis: usize) -> (usize, usize) {
  match axis {
    0 => (1, 2),
    1 => (0, 2),
    _ => (0, 1),
  }
}

/// The slot an edge occupies given the signs of its two spanning coordinates.
pub fn edge_index(a: f64, b: f64) -> usize {
  usize::from(a > 0.0) + if b > 0.0 { 2 } else { 0 }
}

/// Read one edge descriptor: a name, a direction vector, or a nested list.
fn edge_set_from_value(v: &LuaValue) -> Result<EdgeSet, String> {
  if let LuaValue::String(s) = v {
    let name = s.to_str().map_err(|e| e.to_string())?.to_string();
    return match name.as_str() {
      "X" => Ok(EdgeSet([[true; 4], [false; 4], [false; 4]])),
      "Y" => Ok(EdgeSet([[false; 4], [true; 4], [false; 4]])),
      "Z" => Ok(EdgeSet([[false; 4], [false; 4], [true; 4]])),
      "ALL" => Ok(EDGES_ALL),
      "NONE" => Ok(EDGES_NONE),
      other => Err(format!(
        "'{other}' does not name any edges; use \"X\", \"Y\", \"Z\", \"ALL\" or \"NONE\""
      )),
    };
  }

  // A direction vector is three numbers, each -1, 0 or 1. Anything else that
  // is a list — including one mixing names and vectors — is a set of
  // descriptors whose union is taken.
  let as_direction = as_nums(v).filter(|nums| {
    nums.len() == 3 && nums.iter().all(|c| [-1.0, 0.0, 1.0].contains(c))
  });

  let Some(nums) = as_direction else {
    let LuaValue::Table(t) = v else {
      return Err(
        "an edge must be a name, a direction vector, or a list of them"
          .to_string(),
      );
    };
    let len = t.raw_len();
    if len == 0 {
      return Err(
        "an edge vector must have 3 components, each -1, 0 or 1".to_string(),
      );
    }
    let mut out = EDGES_NONE;
    for i in 1..=len {
      let item: LuaValue = t.get(i).map_err(|e| e.to_string())?;
      out = out.union(&edge_set_from_value(&item)?);
    }
    return Ok(out);
  };

  let v3: V3 = [nums[0], nums[1], nums[2]];
  let nonzero = v3.iter().filter(|c| **c != 0.0).count();
  let mut out = EDGES_NONE;
  for axis in 0..3 {
    for i in 0..4 {
      let ev = edge_vector(axis, i);
      let matches =
        (0..3).filter(|k| v3[*k] != 0.0 && v3[*k] == ev[*k]).count();
      out.0[axis][i] = match nonzero {
        // An edge direction names that one edge.
        2 => ev == v3,
        // A face direction names the four edges around it.
        1 => matches == 1,
        // A corner names the three edges that meet there.
        _ => matches == 2,
      };
    }
  }
  Ok(out)
}

/// Resolve a call's `edges` and `except` parameters.
pub fn from_args(args: &Args) -> mlua::Result<EdgeSet> {
  let selected = match args.raw("edges") {
    None => EDGES_ALL,
    Some(v) => edge_set_from_value(v).map_err(|e| {
      mlua::Error::RuntimeError(format!("bosl.{}(): edges: {e}", args.func()))
    })?,
  };
  let excluded = match args.raw("except").or_else(|| args.raw("except_edges")) {
    None => EDGES_NONE,
    Some(v) => edge_set_from_value(v).map_err(|e| {
      mlua::Error::RuntimeError(format!("bosl.{}(): except: {e}", args.func()))
    })?,
  };
  Ok(selected.minus(&excluded))
}

#[cfg(test)]
mod tests {
  use super::*;
  use mlua::Lua;

  fn parse(code: &str) -> EdgeSet {
    let lua = Lua::new();
    let v: LuaValue = lua.load(code).eval().unwrap();
    edge_set_from_value(&v).unwrap()
  }

  #[test]
  fn an_axis_name_selects_the_four_edges_along_it() {
    let z = parse("return 'Z'");
    assert_eq!(z.count(), 4);
    assert!(z.iter().all(|(ax, _)| ax == 2));
  }

  #[test]
  fn a_face_direction_selects_the_four_edges_around_it() {
    let top = parse("return {0, 0, 1}");
    assert_eq!(top.count(), 4);
    // The top face is bordered by X and Y edges, never by a vertical one.
    assert!(top.iter().all(|(ax, _)| ax != 2));
  }

  #[test]
  fn an_edge_direction_selects_exactly_that_edge() {
    let e = parse("return {0, -1, 1}");
    assert_eq!(e.count(), 1);
    let (ax, i) = e.iter().next().unwrap();
    assert_eq!(ax, 0);
    assert_eq!(edge_vector(ax, i), [0.0, -1.0, 1.0]);
  }

  #[test]
  fn a_corner_selects_the_three_edges_meeting_there() {
    let c = parse("return {1, 1, 1}");
    assert_eq!(c.count(), 3);
    assert!(c.corner_is_full([1.0, 1.0, 1.0]));
    assert!(!c.corner_is_full([-1.0, 1.0, 1.0]));
  }

  #[test]
  fn a_list_of_descriptors_is_their_union() {
    let both = parse("return {'X', {0, 0, 1}}");
    assert_eq!(both.union(&parse("return 'X'")), both);
    assert!(both.count() > 4);
  }

  #[test]
  fn except_removes_edges_from_the_selection() {
    let all = EDGES_ALL;
    let top = parse("return {0, 0, 1}");
    assert_eq!(all.minus(&top).count(), 12 - 4);
  }

  #[test]
  fn edge_indices_and_vectors_agree() {
    for axis in 0..3 {
      for i in 0..4 {
        let v = edge_vector(axis, i);
        let (u, w) = other_axes(axis);
        assert_eq!(edge_index(v[u], v[w]), i);
      }
    }
  }
}
