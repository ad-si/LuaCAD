//! BOSL2's `attachments.scad`: placing one shape against another, and
//! deciding what a group of shapes adds up to.
//!
//! BOSL2 does this with modules that wrap their children and pass hidden
//! variables down. Lua has no children — a shape is a value — so the same
//! ideas take a slightly different form here:
//!
//! * An **anchor** is resolved against a *descriptor*: a shape's size, where
//!   it sits, and what sort of body it is. Every shape can produce one from
//!   its own bounding box, and [`attachable`] builds one by hand.
//! * **Attaching** is then a function of two shapes rather than a wrapper:
//!   `attach(parent, child, "top")` gives back the child moved and turned so
//!   its anchor meets the parent's.
//! * A **tag** is carried by wrapping a shape rather than by a scoped
//!   variable, and `diff`, `intersect` and `conv_hull` read those wrappers
//!   off a list. `bosl.diff{ body, bosl.tag(hole, "remove") }` is the whole
//!   idea.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::args::Anchor;
use crate::bosl::value::{Args, PureFn, Val, register_all, register_pure};
use crate::bosl::vecmath::{self as vm, Mat4, V3};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

// ---------------------------------------------------------------------------
// Descriptors
// ---------------------------------------------------------------------------

/// What sort of body an anchor is resolved against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Body {
  /// A box, or anything anchored on its bounding box.
  Prismoid,
  /// A cylinder about Z: the sides are round, the ends flat.
  Cylinder,
  /// A ball: every anchor lands on the surface.
  Sphere,
}

impl Body {
  fn parse(name: &str) -> Option<Body> {
    match name {
      "prismoid" | "cuboid" | "box" | "hull" => Some(Body::Prismoid),
      "cyl" | "cylinder" | "conoid" => Some(Body::Cylinder),
      "sphere" | "spheroid" => Some(Body::Sphere),
      _ => None,
    }
  }

  fn name(self) -> &'static str {
    match self {
      Body::Prismoid => "prismoid",
      Body::Cylinder => "cyl",
      Body::Sphere => "sphere",
    }
  }
}

/// A shape's size and where it sits, enough to resolve an anchor against.
#[derive(Clone, Debug)]
struct Desc {
  body: Body,
  /// The full extent of the shape, corner to corner.
  size: V3,
  /// Where the middle of it sits.
  centre: V3,
  /// Anchors given a name of their own rather than a direction.
  named: Vec<(String, V3, V3)>,
}

impl Desc {
  fn to_lua(&self, lua: &Lua) -> LuaResult<LuaValue> {
    let t = lua.create_table()?;
    t.set("type", self.body.name())?;
    t.set("size", Val::vec(self.size).to_lua(lua)?)?;
    t.set("centre", Val::vec(self.centre).to_lua(lua)?)?;
    let named = lua.create_table()?;
    for (i, (name, pos, dir)) in self.named.iter().enumerate() {
      let entry = lua.create_table()?;
      entry.set(1, name.clone())?;
      entry.set(2, Val::vec(*pos).to_lua(lua)?)?;
      entry.set(3, Val::vec(*dir).to_lua(lua)?)?;
      named.set(i + 1, entry)?;
    }
    t.set("anchors", named)?;
    Ok(LuaValue::Table(t))
  }

  fn from_table(t: &mlua::Table) -> Option<Desc> {
    let body = match t.get::<LuaValue>("type") {
      Ok(LuaValue::String(s)) => Body::parse(s.to_str().ok()?.as_ref())?,
      _ => Body::Prismoid,
    };
    let read = |key: &str| -> V3 {
      t.get::<LuaValue>(key)
        .ok()
        .as_ref()
        .and_then(crate::bosl::args::as_nums)
        .map(|v| crate::bosl::value::v3(&v))
        .unwrap_or([0.0; 3])
    };
    let mut named = Vec::new();
    if let Ok(LuaValue::Table(list)) = t.get::<LuaValue>("anchors") {
      for i in 1..=list.raw_len() {
        if let Ok(LuaValue::Table(e)) = list.get::<LuaValue>(i) {
          let name = e.get::<String>(1).unwrap_or_default();
          let pos = e
            .get::<LuaValue>(2)
            .ok()
            .as_ref()
            .and_then(crate::bosl::args::as_nums)
            .map(|v| crate::bosl::value::v3(&v))
            .unwrap_or([0.0; 3]);
          let dir = e
            .get::<LuaValue>(3)
            .ok()
            .as_ref()
            .and_then(crate::bosl::args::as_nums)
            .map(|v| crate::bosl::value::v3(&v))
            .unwrap_or([0.0, 0.0, 1.0]);
          named.push((name, pos, dir));
        }
      }
    }
    Some(Desc {
      body,
      size: read("size"),
      centre: read("centre"),
      named,
    })
  }

  /// Where an anchor lands, and which way it faces.
  ///
  /// On a box the anchor runs out to the face, edge or corner it names. On a
  /// cylinder the sides are round, so a sideways anchor lands on the curved
  /// surface rather than on a bounding face; on a sphere every anchor does.
  fn anchor(&self, v: V3) -> (V3, V3) {
    let half = vm::mul(self.size, 0.5);
    let dir = vm::unit_or(v, [0.0, 0.0, 1.0]);
    let local = match self.body {
      Body::Prismoid => [v[0] * half[0], v[1] * half[1], v[2] * half[2]],
      Body::Cylinder => {
        // The radius is taken across the flats, so an oval cylinder anchors
        // on its own ellipse rather than on the box round it.
        let flat = (v[0] * v[0] + v[1] * v[1]).sqrt();
        if flat < 1e-12 {
          [0.0, 0.0, v[2] * half[2]]
        } else {
          [v[0] / flat * half[0], v[1] / flat * half[1], v[2] * half[2]]
        }
      }
      Body::Sphere => {
        let u = vm::unit_or(v, [0.0, 0.0, 1.0]);
        [u[0] * half[0], u[1] * half[1], u[2] * half[2]]
      }
    };
    (vm::add(self.centre, local), dir)
  }

  /// The anchor a name or a vector asks for.
  fn resolve(&self, anchor: &Anchor) -> Option<(V3, V3)> {
    if let Anchor::Named(name) = anchor
      && let Some((_, pos, dir)) = self.named.iter().find(|(n, _, _)| n == name)
    {
      return Some((*pos, *dir));
    }
    anchor.as_vector().map(|v| self.anchor(v))
  }
}

// ---------------------------------------------------------------------------
// Reading shapes and descriptors out of Lua
// ---------------------------------------------------------------------------

/// A shape, and the descriptor it anchors by.
struct Shape {
  node: ScadNode,
  desc: Desc,
  sketch: bool,
}

/// Pull the node out of a geometry or sketch userdata.
fn node_of(ud: &mlua::AnyUserData) -> Option<(ScadNode, bool)> {
  if let Ok(g) = ud.borrow::<CsgGeometry>() {
    return g.scad.clone().map(|n| (n, false));
  }
  ud.borrow::<CsgSketch>()
    .ok()
    .and_then(|s| s.scad.clone())
    .map(|n| (n, true))
}

/// Measure a shape, so its anchors have something to resolve against.
///
/// A shape carries no descriptor of its own, so one is taken from its
/// bounding box. That is exactly BOSL2's `atype="hull"`, which is what a
/// box, a prismoid and most other shapes anchor by anyway.
fn measure(node: &ScadNode) -> Desc {
  let m = crate::export::materialize_scad_manifold(node);
  let (lo, hi) = m.bounding_box();
  let size = [
    (hi[0] - lo[0]) as f64,
    (hi[1] - lo[1]) as f64,
    (hi[2] - lo[2]) as f64,
  ];
  let centre = [
    ((hi[0] + lo[0]) / 2.0) as f64,
    ((hi[1] + lo[1]) / 2.0) as f64,
    ((hi[2] + lo[2]) / 2.0) as f64,
  ];
  Desc {
    body: Body::Prismoid,
    size,
    centre,
    named: Vec::new(),
  }
}

fn read_shape(a: &Args, name: &str) -> LuaResult<Shape> {
  let Some(LuaValue::UserData(ud)) = a.raw(name) else {
    return a.err(format!("{name} must be a shape"));
  };
  let Some((node, sketch)) = node_of(ud) else {
    return a.err(format!("{name} must be a shape"));
  };
  let desc = measure(&node);
  Ok(Shape { node, desc, sketch })
}

/// Read either a shape or a descriptor as something to anchor against.
fn read_target(a: &Args, name: &str) -> LuaResult<Desc> {
  match a.raw(name) {
    Some(LuaValue::UserData(ud)) => match node_of(ud) {
      Some((node, _)) => Ok(measure(&node)),
      None => a.err(format!("{name} must be a shape or a descriptor")),
    },
    Some(LuaValue::Table(t)) => match Desc::from_table(t) {
      Some(d) => Ok(d),
      None => a.err(format!("{name} is not a descriptor")),
    },
    _ => a.err(format!("{name} must be a shape or a descriptor")),
  }
}

/// The anchor a parameter names, defaulting to the centre.
fn read_anchor(a: &Args, name: &str) -> LuaResult<Anchor> {
  match a.raw(name) {
    None => Ok(Anchor::Vector([0.0; 3])),
    Some(LuaValue::String(s)) => Ok(Anchor::Named(s.to_str()?.to_string())),
    Some(v) => match crate::bosl::args::as_nums(v) {
      Some(n) if !n.is_empty() => {
        Ok(Anchor::Vector(crate::bosl::value::v3(&n)))
      }
      _ => a.err(format!("{name} must be an anchor such as bosl.TOP")),
    },
  }
}

fn wrap(lua: &Lua, shape: &Shape, node: ScadNode) -> LuaResult<LuaValue> {
  if shape.sketch {
    return Ok(LuaValue::UserData(lua.create_userdata(CsgSketch {
      #[cfg(feature = "csgrs")]
      sketch: crate::geometry::empty_sketch(),
      #[cfg(not(feature = "csgrs"))]
      sketch: (),
      color: None,
      scad: Some(node),
    })?));
  }
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(node),
  })?))
}

fn transformed(node: ScadNode, m: &Mat4) -> ScadNode {
  ScadNode::Multmatrix {
    matrix: m.to_f32(),
    child: Box::new(node),
  }
}

// ---------------------------------------------------------------------------
// Placing one shape against another
// ---------------------------------------------------------------------------

/// Move a shape so its own origin lands on the parent's anchor point.
fn position(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let parent = read_target(a, "parent")?;
  let child = read_shape(a, "child")?;
  let at = read_anchor(a, "at")?;
  let Some((point, _)) = parent.resolve(&at) else {
    return a.err("that anchor is not one this shape has");
  };
  let node = transformed(child.node.clone(), &Mat4::translate(point));
  wrap(lua, &child, node)
}

/// Turn a shape to face the way an anchor points, without moving it.
fn orient(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let child = read_shape(a, "p")?;
  let anchor = read_anchor(a, "anchor")?;
  let Some(dir) = anchor.as_vector() else {
    return a.err("orient takes a direction, not a named anchor");
  };
  let m = Mat4::rot_from_to([0.0, 0.0, 1.0], dir)
    .mul(&Mat4::zrot(a.num_or("spin", 0.0)));
  let node = transformed(child.node.clone(), &m);
  wrap(lua, &child, node)
}

/// Stand a shape on a parent's anchor, facing outward.
///
/// The child's own anchor — `to`, or the face opposite `from` by default —
/// is brought to the parent's anchor point and turned to face back along it,
/// so the two sit flush. `overlap` sinks the child in by that much, which is
/// what stops a union leaving a zero-thickness skin between them.
fn attach(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let parent_desc = read_target(a, "parent")?;
  let child = read_shape(a, "child")?;
  let from = read_anchor(a, "from")?;
  let Some((point, dir)) = parent_desc.resolve(&from) else {
    return a.err("that anchor is not one the parent has");
  };

  // The child meets the parent on the face pointing back at it.
  let to = match a.raw("to") {
    Some(_) => read_anchor(a, "to")?,
    None => Anchor::Vector([-dir[0], -dir[1], -dir[2]]),
  };
  let inside = a.bool_or("inside", false);
  let facing = if inside {
    dir
  } else {
    [-dir[0], -dir[1], -dir[2]]
  };
  let Some((child_point, _)) = child.desc.resolve(&to) else {
    return a.err("that anchor is not one the child has");
  };

  // Turn the child so its own anchor direction lands on the parent's, then
  // slide it so the two anchor points coincide.
  let turn = Mat4::rot_from_to([0.0, 0.0, 1.0], facing)
    .mul(&Mat4::zrot(a.num_or("spin", 0.0)))
    .mul(&Mat4::rot_from_to(
      to.as_vector().unwrap_or([0.0, 0.0, -1.0]),
      [0.0, 0.0, 1.0],
    ));
  let turned = turn.apply(child_point);
  let overlap = a.num_or("overlap", 0.0);
  let target = vm::sub(point, vm::mul(dir, overlap));
  let m = Mat4::translate(vm::sub(target, turned)).mul(&turn);
  let node = transformed(child.node.clone(), &m);
  wrap(lua, &child, node)
}

/// Line a shape up against a parent's face without overlapping it.
///
/// `anchor` picks the face and `align` slides the child along it, so
/// `align(box, lid, TOP, RIGHT)` sets the lid on the top, flush right.
fn align(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let parent = read_target(a, "parent")?;
  let child = read_shape(a, "child")?;
  let anchor = read_anchor(a, "anchor")?;
  let Some((point, dir)) = parent.resolve(&anchor) else {
    return a.err("that anchor is not one the parent has");
  };
  let align_v = match a.raw("align") {
    Some(_) => read_anchor(a, "align")?.as_vector().unwrap_or([0.0; 3]),
    None => [0.0; 3],
  };
  let inside = a.bool_or("inside", false);
  let inset = a.num_or("inset", 0.0);
  let overlap = a.num_or("overlap", 0.0);

  // Sitting on the face means the child's own opposite side touches it,
  // which is half its size back along the anchor direction.
  let stand = if inside {
    child.desc.anchor(dir).0
  } else {
    child.desc.anchor([-dir[0], -dir[1], -dir[2]]).0
  };
  // The slide along the face is the child's own extent in that direction.
  let slide = child.desc.anchor(align_v).0;
  let inset_shift = vm::mul(align_v, -inset);
  let target = vm::add(vm::sub(point, vm::mul(dir, overlap)), inset_shift);
  let m = Mat4::translate(vm::sub(target, vm::add(stand, slide)));
  let node = transformed(child.node.clone(), &m);
  wrap(lua, &child, node)
}

/// Put a shape back into the frame a descriptor was taken in.
fn restore(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let desc = read_target(a, "desc")?;
  let child = read_shape(a, "p")?;
  let node = transformed(child.node.clone(), &Mat4::translate(desc.centre));
  wrap(lua, &child, node)
}

/// The descriptor a shape anchors by.
fn parent(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // BOSL2 reads this off the enclosing `attachable()` scope, which Lua has
  // no equivalent of, so the shape is named outright.
  let desc = read_target(a, "p")?;
  desc.to_lua(lua)
}

fn desc_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let desc = read_target(a, "desc")?;
  let anchor = read_anchor(a, "anchor")?;
  let Some((point, _)) = desc.resolve(&anchor) else {
    return a.err("that anchor is not one this shape has");
  };
  Val::vec(point).to_lua(lua)
}

fn desc_dist(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let d1 = read_target(a, "desc1")?;
  let d2 = read_target(a, "desc2")?;
  let a1 = read_anchor(a, "anchor1")?;
  let a2 = read_anchor(a, "anchor2")?;
  let (Some((p1, _)), Some((p2, _))) = (d1.resolve(&a1), d2.resolve(&a2))
  else {
    return a.err("one of those anchors is not one its shape has");
  };
  Ok(LuaValue::Number(vm::norm(vm::sub(p2, p1))))
}

// ---------------------------------------------------------------------------
// Building descriptors by hand
// ---------------------------------------------------------------------------

fn build_desc(a: &Args) -> LuaResult<Desc> {
  let body = match a.string("atype").or_else(|| a.string("type")) {
    Some(name) => match Body::parse(&name) {
      Some(b) => b,
      None => return a.err("type must be \"prismoid\", \"cyl\" or \"sphere\""),
    },
    None => {
      if a.has("r") || a.has("d") || a.has("r1") || a.has("r2") {
        if a.has("l") || a.has("h") {
          Body::Cylinder
        } else {
          Body::Sphere
        }
      } else {
        Body::Prismoid
      }
    }
  };
  let size = match a.sized("size", 3) {
    Some(s) => crate::bosl::value::v3(&s),
    None => {
      let r = a
        .radius("r", "d", None)
        .or_else(|| a.radius("r1", "d1", None))
        .unwrap_or(0.5);
      let h = a
        .num("l")
        .or_else(|| a.num("h"))
        .or_else(|| a.num("height"))
        .unwrap_or(2.0 * r);
      [2.0 * r, 2.0 * r, h]
    }
  };
  let centre = a.vec3("cp").unwrap_or([0.0; 3]);
  let mut named = Vec::new();
  if let Some(LuaValue::Table(list)) = a.raw("anchors") {
    for i in 1..=list.raw_len() {
      if let Ok(LuaValue::Table(e)) = list.get::<LuaValue>(i) {
        let name = e.get::<String>(1).unwrap_or_default();
        let read = |k: i64, dflt: V3| {
          e.get::<LuaValue>(k)
            .ok()
            .as_ref()
            .and_then(crate::bosl::args::as_nums)
            .map(|v| crate::bosl::value::v3(&v))
            .unwrap_or(dflt)
        };
        named.push((name, read(2, [0.0; 3]), read(3, [0.0, 0.0, 1.0])));
      }
    }
  }
  Ok(Desc {
    body,
    size,
    centre,
    named,
  })
}

fn attachable(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  build_desc(a)?.to_lua(lua)
}

fn attach_geom(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  build_desc(a)?.to_lua(lua)
}

/// An anchor with a name rather than a direction.
fn named_anchor(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(name) = a.string("name") else {
    return a.err("name is required");
  };
  let t = lua.create_table()?;
  t.set(1, name)?;
  t.set(2, Val::vec(a.vec3("pos").unwrap_or([0.0; 3])).to_lua(lua)?)?;
  t.set(
    3,
    Val::vec(a.vec3("orient").unwrap_or([0.0, 0.0, 1.0])).to_lua(lua)?,
  )?;
  t.set(4, a.num_or("spin", 0.0))?;
  Ok(LuaValue::Table(t))
}

/// Place a shape by anchor, spin and orientation.
fn reorient(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let child = read_shape(a, "p")?;
  let desc = match a.raw("geom") {
    Some(_) => read_target(a, "geom")?,
    None => child.desc.clone(),
  };
  let anchor = read_anchor(a, "anchor")?;
  let Some((point, _)) = desc.resolve(&anchor) else {
    return a.err("that anchor is not one this shape has");
  };
  // Anchoring moves the named point to the origin, then spin and orient
  // turn what is left.
  let m = Mat4::rot_from_to(
    [0.0, 0.0, 1.0],
    a.vec3("orient").unwrap_or([0.0, 0.0, 1.0]),
  )
  .mul(&Mat4::zrot(a.num_or("spin", 0.0)))
  .mul(&Mat4::translate([-point[0], -point[1], -point[2]]));
  let node = transformed(child.node.clone(), &m);
  wrap(lua, &child, node)
}

// ---------------------------------------------------------------------------
// Tags, and what a group of shapes adds up to
// ---------------------------------------------------------------------------

/// A shape with a tag on it, as `tag()` hands one back.
struct Tagged {
  node: ScadNode,
  tag: String,
}

/// Read a list of shapes, some of which may be tagged.
fn read_tagged(a: &Args, name: &str) -> LuaResult<Vec<Tagged>> {
  let Some(raw) = a.raw(name) else {
    return a.err(format!("{name} is required"));
  };
  let mut out = Vec::new();
  let take = |v: &LuaValue| -> Option<Tagged> {
    match v {
      LuaValue::UserData(ud) => node_of(ud).map(|(node, _)| Tagged {
        node,
        tag: String::new(),
      }),
      // A tagged shape is a table carrying the shape and its tag.
      LuaValue::Table(t) => {
        let tag = t.get::<String>("tag").ok()?;
        let LuaValue::UserData(ud) = t.get::<LuaValue>("shape").ok()? else {
          return None;
        };
        node_of(&ud).map(|(node, _)| Tagged { node, tag })
      }
      _ => None,
    }
  };
  match raw {
    LuaValue::Table(t) if t.raw_len() > 0 => {
      for i in 1..=t.raw_len() {
        let v = t.get::<LuaValue>(i)?;
        match take(&v) {
          Some(s) => out.push(s),
          None => {
            return a.err(format!("{name} entry {i} is not a shape"));
          }
        }
      }
    }
    other => match take(other) {
      Some(s) => out.push(s),
      None => {
        return a.err(format!("{name} must be a shape or a list of them"));
      }
    },
  }
  if out.is_empty() {
    return a.err(format!("{name} must have at least one shape in it"));
  }
  Ok(out)
}

fn solid(lua: &Lua, node: ScadNode) -> LuaResult<LuaValue> {
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(node),
  })?))
}

/// Put a tag on a shape, so a later `diff` or `intersect` can pick it out.
fn tag(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(LuaValue::UserData(ud)) = a.raw("p") else {
    return a.err("p must be a shape");
  };
  if node_of(ud).is_none() {
    return a.err("p must be a shape");
  }
  let Some(name) = a.string("tag") else {
    return a.err("tag must be a name");
  };
  let t = lua.create_table()?;
  t.set("shape", LuaValue::UserData(ud.clone()))?;
  t.set("tag", name)?;
  Ok(LuaValue::Table(t))
}

/// Combine a list, taking out everything tagged for removal.
fn diff(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shapes = read_tagged(a, "shapes")?;
  let remove = a.string("remove").unwrap_or_else(|| "remove".to_string());
  let keep = a.string("keep").unwrap_or_else(|| "keep".to_string());
  let body: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag != remove && s.tag != keep)
    .map(|s| s.node.clone())
    .collect();
  let holes: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag == remove)
    .map(|s| s.node.clone())
    .collect();
  let kept: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag == keep)
    .map(|s| s.node.clone())
    .collect();
  if body.is_empty() {
    return a.err("nothing to cut into: every shape was tagged");
  }
  let mut node = ScadNode::Union(body);
  if !holes.is_empty() {
    node = ScadNode::Difference(vec![node, ScadNode::Union(holes)]);
  }
  // Anything tagged to keep goes back on afterwards, so a hole cut through
  // one part does not cut through it.
  if !kept.is_empty() {
    node = ScadNode::Union(vec![node, ScadNode::Union(kept)]);
  }
  solid(lua, node)
}

/// Keep only what the tagged shapes have in common with the rest.
fn intersect(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shapes = read_tagged(a, "shapes")?;
  let mask = a
    .string("intersect")
    .unwrap_or_else(|| "intersect".to_string());
  let keep = a.string("keep").unwrap_or_else(|| "keep".to_string());
  let body: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag != mask && s.tag != keep)
    .map(|s| s.node.clone())
    .collect();
  let masks: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag == mask)
    .map(|s| s.node.clone())
    .collect();
  let kept: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag == keep)
    .map(|s| s.node.clone())
    .collect();
  if body.is_empty() || masks.is_empty() {
    return a.err(
      "intersect needs both untagged shapes and at least one tagged with the \
       intersect tag",
    );
  }
  let mut node =
    ScadNode::Intersection(vec![ScadNode::Union(body), ScadNode::Union(masks)]);
  if !kept.is_empty() {
    node = ScadNode::Union(vec![node, ScadNode::Union(kept)]);
  }
  solid(lua, node)
}

/// Wrap a list in a convex hull, leaving anything tagged to keep alone.
fn conv_hull(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shapes = read_tagged(a, "shapes")?;
  let keep = a.string("keep").unwrap_or_else(|| "keep".to_string());
  let body: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag != keep)
    .map(|s| s.node.clone())
    .collect();
  let kept: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| s.tag == keep)
    .map(|s| s.node.clone())
    .collect();
  if body.is_empty() {
    return a.err("nothing to hull: every shape was tagged to keep");
  }
  let mut node = ScadNode::Hull(Box::new(ScadNode::Union(body)));
  if !kept.is_empty() {
    node = ScadNode::Union(vec![node, ScadNode::Union(kept)]);
  }
  solid(lua, node)
}

/// Keep or drop shapes by tag.
fn filter_by_tag(
  lua: &Lua,
  a: &Args,
  keep_if: impl Fn(&str, &[String]) -> bool,
) -> LuaResult<LuaValue> {
  let shapes = read_tagged(a, "shapes")?;
  let wanted: Vec<String> = match a.raw("tags") {
    Some(LuaValue::String(s)) => s
      .to_str()?
      .split_whitespace()
      .map(|t| t.to_string())
      .collect(),
    Some(LuaValue::Table(t)) => {
      let mut v = Vec::new();
      for i in 1..=t.raw_len() {
        if let Ok(s) = t.get::<String>(i) {
          v.push(s);
        }
      }
      v
    }
    _ => Vec::new(),
  };
  let left: Vec<ScadNode> = shapes
    .iter()
    .filter(|s| keep_if(&s.tag, &wanted))
    .map(|s| s.node.clone())
    .collect();
  solid(lua, ScadNode::Union(left))
}

// ---------------------------------------------------------------------------
// Things to look at while debugging
// ---------------------------------------------------------------------------

fn arrow(size: f64) -> ScadNode {
  // A stalk with a cone on the end, pointing up Z.
  ScadNode::Union(vec![
    ScadNode::Cylinder {
      r1: (size / 12.0) as f32,
      r2: (size / 12.0) as f32,
      h: (size * 0.7) as f32,
      center: false,
      segments: 12,
    },
    ScadNode::Translate {
      x: 0.0,
      y: 0.0,
      z: (size * 0.7) as f32,
      child: Box::new(ScadNode::Cylinder {
        r1: (size / 5.0) as f32,
        r2: 0.0,
        h: (size * 0.3) as f32,
        center: false,
        segments: 12,
      }),
    },
  ])
}

fn as_geometry(
  lua: &Lua,
  name: &'static str,
  a: &Args,
  node: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    name,
    a.scad_args().to_string(),
    vec![],
    Some(node),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(scad),
  })?))
}

fn anchor_arrow(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  as_geometry(lua, "anchor_arrow", a, arrow(a.num_or("s", 10.0)))
}

fn anchor_arrow2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = a.num_or("s", 15.0);
  // Flattened, so it reads against a 2D drawing.
  let node = ScadNode::Projection {
    cut: false,
    child: Box::new(ScadNode::Rotate {
      x: -90.0,
      y: 0.0,
      z: 0.0,
      child: Box::new(arrow(s)),
    }),
  };
  as_geometry(lua, "anchor_arrow2d", a, node)
}

/// The three axes, so which way is which can be seen at a glance.
fn frame_ref(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = a.num_or("s", 15.0);
  let node = ScadNode::Union(vec![
    ScadNode::Color {
      r: 1.0,
      g: 0.0,
      b: 0.0,
      a: 1.0,
      child: Box::new(ScadNode::Rotate {
        x: 0.0,
        y: 90.0,
        z: 0.0,
        child: Box::new(arrow(s)),
      }),
    },
    ScadNode::Color {
      r: 0.0,
      g: 1.0,
      b: 0.0,
      a: 1.0,
      child: Box::new(ScadNode::Rotate {
        x: -90.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(arrow(s)),
      }),
    },
    ScadNode::Color {
      r: 0.0,
      g: 0.0,
      b: 1.0,
      a: 1.0,
      child: Box::new(arrow(s)),
    },
  ]);
  as_geometry(lua, "frame_ref", a, node)
}

/// An arrow at every standard anchor of a shape.
fn show_anchors(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let desc = read_target(a, "desc")?;
  let s = a.num_or("s", 10.0);
  let mut parts = Vec::new();
  for x in [-1.0f64, 0.0, 1.0] {
    for y in [-1.0f64, 0.0, 1.0] {
      for z in [-1.0f64, 0.0, 1.0] {
        if x == 0.0 && y == 0.0 && z == 0.0 {
          continue;
        }
        let (point, dir) = desc.anchor([x, y, z]);
        let m =
          Mat4::translate(point).mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], dir));
        parts.push(transformed(arrow(s), &m));
      }
    }
  }
  for (_, pos, dir) in &desc.named {
    let m =
      Mat4::translate(*pos).mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], *dir));
    parts.push(transformed(arrow(s), &m));
  }
  as_geometry(lua, "show_anchors", a, ScadNode::Union(parts))
}

/// A shape whose orientation is obvious from any angle.
///
/// Nose forward, one wing longer than the other, fin up: whichever way it
/// ends up, it is clear what happened to it.
fn generic_airplane(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = a.num_or("s", 10.0);
  let body = ScadNode::Cylinder {
    r1: (s / 6.0) as f32,
    r2: (s / 6.0) as f32,
    h: s as f32,
    center: true,
    segments: 16,
  };
  let node = ScadNode::Union(vec![
    // The fuselage lies along X, nose at +X.
    ScadNode::Rotate {
      x: 0.0,
      y: 90.0,
      z: 0.0,
      child: Box::new(body),
    },
    ScadNode::Translate {
      x: (s * 0.55) as f32,
      y: 0.0,
      z: 0.0,
      child: Box::new(ScadNode::Rotate {
        x: 0.0,
        y: 90.0,
        z: 0.0,
        child: Box::new(ScadNode::Cylinder {
          r1: (s / 6.0) as f32,
          r2: 0.0,
          h: (s / 3.0) as f32,
          center: false,
          segments: 16,
        }),
      }),
    },
    // Wings, the port one longer so left and right cannot be confused.
    ScadNode::Cube {
      w: (s / 4.0) as f32,
      d: (s * 1.4) as f32,
      h: (s / 12.0) as f32,
      center: true,
    },
    ScadNode::Translate {
      x: 0.0,
      y: (s * 0.85) as f32,
      z: 0.0,
      child: Box::new(ScadNode::Cube {
        w: (s / 4.0) as f32,
        d: (s * 0.3) as f32,
        h: (s / 12.0) as f32,
        center: true,
      }),
    },
    // The fin, which fixes up from down.
    ScadNode::Translate {
      x: (-s * 0.4) as f32,
      y: 0.0,
      z: (s * 0.25) as f32,
      child: Box::new(ScadNode::Cube {
        w: (s / 4.0) as f32,
        d: (s / 12.0) as f32,
        h: (s / 2.0) as f32,
        center: true,
      }),
    },
  ]);
  as_geometry(lua, "generic_airplane", a, node)
}

/// Make a shape see-through, so what is inside it can be seen.
fn expose_anchors(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shape = read_shape(a, "p")?;
  let opacity = a.num_or("opacity", 0.2);
  let node = ScadNode::Color {
    r: 0.8,
    g: 0.8,
    b: 0.8,
    a: opacity as f32,
    child: Box::new(shape.node.clone()),
  };
  wrap(lua, &shape, node)
}

/// Draw a chain of transforms as a trail of frames.
fn show_transform_list(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) =
    a.val("tlist").and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("tlist must be a list of 4x4 matrices");
  };
  let s = a.num_or("s", 10.0);
  let mut parts = Vec::new();
  for item in &items {
    let Some(rows) = item.as_matrix() else {
      continue;
    };
    if rows.len() != 4 || rows.iter().any(|r| r.len() != 4) {
      continue;
    }
    let mut m = [0.0; 16];
    for (i, row) in rows.iter().enumerate() {
      m[i * 4..i * 4 + 4].copy_from_slice(row);
    }
    parts.push(transformed(arrow(s), &Mat4(m)));
  }
  as_geometry(lua, "show_transform_list", a, ScadNode::Union(parts))
}

/// Whether two shapes actually meet, and what the shared part looks like.
fn show_int(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shapes = read_tagged(a, "shapes")?;
  if shapes.len() < 2 {
    return a.err("show_int needs at least two shapes");
  }
  let node =
    ScadNode::Intersection(shapes.iter().map(|s| s.node.clone()).collect());
  as_geometry(lua, "show_int", a, node)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_pure(
    lua,
    bosl,
    "position",
    &["parent", "child", "at", "from"],
    position,
  )?;
  register_pure(lua, bosl, "orient", &["p", "anchor", "spin"], orient)?;
  register_pure(
    lua,
    bosl,
    "attach",
    &[
      "parent", "child", "from", "to", "overlap", "align", "spin", "norot",
      "inset", "shiftout", "inside",
    ],
    attach,
  )?;
  register_pure(
    lua,
    bosl,
    "align",
    &[
      "parent", "child", "anchor", "align", "inside", "inset", "shiftout",
      "overlap",
    ],
    align,
  )?;
  register_pure(lua, bosl, "restore", &["desc", "p"], restore)?;
  register_pure(lua, bosl, "parent", &["p"], parent)?;
  register_pure(lua, bosl, "desc_point", &["desc", "anchor"], desc_point)?;
  register_pure(
    lua,
    bosl,
    "desc_dist",
    &["desc1", "anchor1", "desc2", "anchor2"],
    desc_dist,
  )?;

  const DESC_PARAMS: &[&str] = &[
    "size", "size2", "shift", "r", "r1", "r2", "d", "d1", "d2", "l", "h",
    "height", "vnf", "path", "region", "extent", "cp", "offset", "anchors",
    "two_d", "axis", "override", "atype", "type", "geom", "anchor", "spin",
    "orient", "p",
  ];
  register_pure(lua, bosl, "attachable", DESC_PARAMS, attachable)?;
  register_pure(lua, bosl, "attach_geom", DESC_PARAMS, attach_geom)?;
  register_pure(
    lua,
    bosl,
    "named_anchor",
    &["name", "pos", "orient", "spin", "rot", "flip", "info"],
    named_anchor,
  )?;
  register_pure(lua, bosl, "reorient", DESC_PARAMS, reorient)?;

  // Tagging. `tag_this`, `force_tag` and `default_tag` differ in BOSL2 only
  // by how far down the tree they reach, which a value-based model settles
  // at the point the tag is put on.
  for name in ["tag", "tag_this", "force_tag", "default_tag"] {
    register_pure(lua, bosl, name, &["p", "tag", "do_tag"], tag)?;
  }
  register_pure(lua, bosl, "diff", &["shapes", "remove", "keep"], diff)?;
  register_pure(
    lua,
    bosl,
    "intersect",
    &["shapes", "intersect", "keep"],
    intersect,
  )?;
  register_pure(lua, bosl, "conv_hull", &["shapes", "keep"], conv_hull)?;
  // `tag_diff` and friends are the same operations that also put a tag on
  // what they produce; the tag is applied by the caller here.
  register_pure(
    lua,
    bosl,
    "tag_diff",
    &["shapes", "tag", "remove", "keep"],
    diff,
  )?;
  register_pure(
    lua,
    bosl,
    "tag_intersect",
    &["shapes", "tag", "intersect", "keep"],
    intersect,
  )?;
  register_pure(
    lua,
    bosl,
    "tag_conv_hull",
    &["shapes", "tag", "keep"],
    conv_hull,
  )?;
  register_pure(lua, bosl, "tag_scope", &["shapes", "scope"], |lua, a| {
    // A scope keeps a group's tags from leaking out of it, which for a list
    // of values means combining them and handing back one untagged shape.
    let shapes = read_tagged(a, "shapes")?;
    solid(
      lua,
      ScadNode::Union(shapes.iter().map(|s| s.node.clone()).collect()),
    )
  })?;

  register_pure(lua, bosl, "hide", &["shapes", "tags"], |lua, a| {
    filter_by_tag(lua, a, |tag, wanted| !wanted.iter().any(|w| w == tag))
  })?;
  register_pure(lua, bosl, "hide_this", &["shapes"], |lua, a| {
    // Hiding everything leaves nothing, which is the point: it is how a
    // shape is kept for its anchors alone.
    let _ = read_tagged(a, "shapes")?;
    solid(lua, ScadNode::Union(vec![]))
  })?;
  register_pure(lua, bosl, "show_only", &["shapes", "tags"], |lua, a| {
    filter_by_tag(lua, a, |tag, wanted| wanted.iter().any(|w| w == tag))
  })?;
  register_pure(lua, bosl, "show_all", &["shapes"], |lua, a| {
    let shapes = read_tagged(a, "shapes")?;
    solid(
      lua,
      ScadNode::Union(shapes.iter().map(|s| s.node.clone()).collect()),
    )
  })?;

  register_all(
    lua,
    bosl,
    &[
      (
        "anchor_arrow",
        &["s", "color", "flag", "anchor", "spin", "orient"],
        anchor_arrow as PureFn,
      ),
      (
        "anchor_arrow2d",
        &["s", "color", "anchor", "spin"],
        anchor_arrow2d,
      ),
      ("frame_ref", &["s", "opacity"], frame_ref),
      (
        "show_anchors",
        &["desc", "s", "std", "custom"],
        show_anchors,
      ),
      (
        "generic_airplane",
        &["s", "anchor", "spin", "orient"],
        generic_airplane,
      ),
      ("expose_anchors", &["p", "opacity"], expose_anchors),
      (
        "show_transform_list",
        &["tlist", "s", "color"],
        show_transform_list,
      ),
      ("show_int", &["shapes"], show_int),
    ],
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn box_desc(size: V3, centre: V3) -> Desc {
    Desc {
      body: Body::Prismoid,
      size,
      centre,
      named: Vec::new(),
    }
  }

  #[test]
  fn a_box_anchors_on_its_own_faces() {
    let d = box_desc([20.0, 10.0, 6.0], [0.0; 3]);
    assert_eq!(d.anchor([0.0, 0.0, 1.0]).0, [0.0, 0.0, 3.0]);
    assert_eq!(d.anchor([1.0, 0.0, 0.0]).0, [10.0, 0.0, 0.0]);
    // A corner is all three at once.
    assert_eq!(d.anchor([1.0, -1.0, 1.0]).0, [10.0, -5.0, 3.0]);
  }

  #[test]
  fn an_offset_box_anchors_where_it_actually_is() {
    let d = box_desc([20.0, 20.0, 20.0], [5.0, 0.0, 0.0]);
    assert_eq!(d.anchor([0.0, 0.0, 1.0]).0, [5.0, 0.0, 10.0]);
  }

  #[test]
  fn a_cylinder_anchors_on_its_curved_side_not_its_bounding_box() {
    let d = Desc {
      body: Body::Cylinder,
      size: [20.0, 20.0, 10.0],
      centre: [0.0; 3],
      named: Vec::new(),
    };
    // Diagonally sideways still lands on the circle, at radius 10.
    let (p, _) = d.anchor([1.0, 1.0, 0.0]);
    assert!((p[0].hypot(p[1]) - 10.0).abs() < 1e-9, "{p:?}");
    // A box would have put it at the corner, further out than that.
    assert!(p[0] < 10.0, "{p:?}");
  }

  #[test]
  fn a_sphere_anchors_on_its_surface_in_every_direction() {
    let d = Desc {
      body: Body::Sphere,
      size: [20.0; 3],
      centre: [0.0; 3],
      named: Vec::new(),
    };
    let (p, _) = d.anchor([1.0, 1.0, 1.0]);
    assert!((vm::norm(p) - 10.0).abs() < 1e-9, "{p:?}");
  }

  #[test]
  fn an_anchor_faces_the_way_it_points() {
    let d = box_desc([10.0; 3], [0.0; 3]);
    let (_, dir) = d.anchor([0.0, 0.0, -1.0]);
    assert_eq!(dir, [0.0, 0.0, -1.0]);
  }

  #[test]
  fn a_named_anchor_wins_over_the_direction_it_would_have_had() {
    let d = Desc {
      body: Body::Prismoid,
      size: [10.0; 3],
      centre: [0.0; 3],
      named: vec![("spout".to_string(), [1.0, 2.0, 3.0], [0.0, 1.0, 0.0])],
    };
    let (p, dir) = d.resolve(&Anchor::Named("spout".into())).unwrap();
    assert_eq!(p, [1.0, 2.0, 3.0]);
    assert_eq!(dir, [0.0, 1.0, 0.0]);
    // A name nothing answers to still resolves if it is one of the standard
    // ones, and otherwise does not.
    assert!(d.resolve(&Anchor::Named("top".into())).is_some());
    assert!(d.resolve(&Anchor::Named("nozzle".into())).is_none());
  }
}
