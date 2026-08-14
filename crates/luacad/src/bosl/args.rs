//! Argument handling for the native BOSL2 shape builders.
//!
//! BOSL2 modules are called from Lua as a single table mixing positional and
//! named arguments, matching the OpenSCAD call they stand in for:
//!
//! ```lua
//! bosl.cuboid { {30, 20, 10}, rounding = 2, anchor = bosl.BOTTOM }
//! ```
//!
//! The array part supplies positional arguments in the module's declared
//! parameter order, and string keys supply named ones. Vectors are nested
//! tables, so `{10, 20}` in the array part is two positional arguments, not
//! one two-element vector — the same reading OpenSCAD gives `f(10, 20)`.

use std::collections::BTreeMap;

use mlua::{Result as LuaResult, Value as LuaValue};

/// Parameters every BOSL2 shape accepts on top of its own.
///
/// `anchor`/`spin`/`orient` come from the attachment system, and `fn`/`fa`/`fs`
/// are LuaCAD's spelling of OpenSCAD's `$fn`/`$fa`/`$fs` facet controls.
pub const COMMON_PARAMS: &[&str] =
  &["anchor", "spin", "orient", "fn", "fa", "fs"];

/// A parsed BOSL2 argument list.
#[derive(Debug)]
pub struct Args {
  func: &'static str,
  positional: Vec<LuaValue>,
  named: BTreeMap<String, LuaValue>,
  /// Positional parameter names, in the order the BOSL2 module declares them.
  params: &'static [&'static str],
}

impl Args {
  /// Parse a call's arguments against a module's parameter list.
  ///
  /// A single table argument is unpacked into positional and named parts.
  /// Anything else — `bosl.cuboid(30)`, `bosl.cyl(10, 5)` — is taken as a
  /// plain positional list.
  pub fn parse(
    func: &'static str,
    params: &'static [&'static str],
    args: &mlua::MultiValue,
  ) -> LuaResult<Args> {
    let mut positional = Vec::new();
    let mut named = BTreeMap::new();

    if args.len() == 1 {
      if let LuaValue::Table(t) = &args[0] {
        let len = t.raw_len();
        for i in 1..=len {
          positional.push(t.get::<LuaValue>(i)?);
        }
        for pair in t.pairs::<LuaValue, LuaValue>() {
          let (key, value) = pair?;
          if let LuaValue::String(s) = key {
            named.insert(s.to_str()?.to_string(), value);
          }
        }
      } else {
        positional.push(args[0].clone());
      }
    } else {
      positional.extend(args.iter().cloned());
    }

    let parsed = Args {
      func,
      positional,
      named,
      params,
    };
    parsed.check_named()?;
    Ok(parsed)
  }

  /// Parse the arguments of a function that computes a value.
  ///
  /// These are called the way OpenSCAD calls them — `bosl.lerp(a, b, u)`,
  /// `bosl.unit({1, 2, 3})` — so every argument is positional and a table
  /// argument is the value itself, not a wrapper around an argument list.
  /// That is the opposite of the shape modules, where the table *is* the
  /// argument list.
  ///
  /// A trailing table carrying only string keys is taken as named arguments,
  /// which is the one form that cannot be confused with a vector: every
  /// vector, path and matrix has an array part.
  pub fn parse_pure(
    func: &'static str,
    params: &'static [&'static str],
    args: &mlua::MultiValue,
  ) -> LuaResult<Args> {
    let mut positional: Vec<LuaValue> = args.iter().cloned().collect();
    let mut named = BTreeMap::new();

    if let Some(LuaValue::Table(t)) = positional.last() {
      let has_names = t
        .clone()
        .pairs::<LuaValue, LuaValue>()
        .any(|p| matches!(p, Ok((LuaValue::String(_), _))));
      if has_names && t.raw_len() == 0 {
        for pair in t.clone().pairs::<LuaValue, LuaValue>() {
          let (key, value) = pair?;
          if let LuaValue::String(s) = key {
            named.insert(s.to_str()?.to_string(), value);
          }
        }
        positional.pop();
      }
    }

    let parsed = Args {
      func,
      positional,
      named,
      params,
    };
    parsed.check_named()?;
    Ok(parsed)
  }

  /// Reject named parameters the module does not understand.
  ///
  /// Silently dropping them turns a typo — or the OpenSCAD habit of writing
  /// `$fn` — into a model that is quietly the wrong shape, which is the
  /// hardest kind of CAD bug to notice.
  fn check_named(&self) -> LuaResult<()> {
    let mut unknown: Vec<&str> = self
      .named
      .keys()
      .map(String::as_str)
      .filter(|k| !self.params.contains(k) && !COMMON_PARAMS.contains(k))
      .collect();
    if unknown.is_empty() {
      return Ok(());
    }
    unknown.sort_unstable();

    let mut valid: Vec<&str> = self
      .params
      .iter()
      .copied()
      .chain(COMMON_PARAMS.iter().copied())
      .collect();
    valid.sort_unstable();

    let mut msg = format!(
      "bosl.{}() got unknown parameter{} {}",
      self.func,
      if unknown.len() == 1 { "" } else { "s" },
      unknown
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ")
    );
    // `$fn` is the OpenSCAD spelling of `fn`, so strip the sigil before
    // looking for a near match — it makes the common port mistake obvious.
    let first = unknown[0].trim_start_matches('$');
    if first == "center" || first == "centre" {
      // Only some BOSL2 shapes take `center`; the rest are placed with
      // `anchor`, and nothing about the name hints at that.
      msg.push_str(
        "\nThis shape has no 'center' parameter. Use anchor = bosl.CENTER \
         to centre it, or anchor = bosl.BOTTOM to stand it on the XY plane.",
      );
    } else if let Some(best) = valid
      .iter()
      .map(|c| (crate::lua_engine::edit_distance(first, c), *c))
      .filter(|(dist, _)| *dist <= 2)
      .min_by_key(|(dist, _)| *dist)
      .map(|(_, name)| name)
    {
      msg.push_str(&format!("\nDid you mean '{best}'?"));
    }
    msg.push_str(&format!("\nValid parameters: {}", valid.join(", ")));
    Err(mlua::Error::RuntimeError(msg))
  }

  /// The raw value for a parameter, by name or by its positional slot.
  pub fn raw(&self, name: &str) -> Option<&LuaValue> {
    if let Some(v) = self.named.get(name) {
      return match v {
        LuaValue::Nil => None,
        v => Some(v),
      };
    }
    let idx = self.params.iter().position(|p| *p == name)?;
    match self.positional.get(idx) {
      Some(LuaValue::Nil) | None => None,
      Some(v) => Some(v),
    }
  }

  /// Whether a parameter was supplied at all.
  pub fn has(&self, name: &str) -> bool {
    self.raw(name).is_some()
  }

  pub fn func(&self) -> &'static str {
    self.func
  }

  pub fn err<T>(&self, msg: impl std::fmt::Display) -> LuaResult<T> {
    Err(mlua::Error::RuntimeError(format!(
      "bosl.{}(): {msg}",
      self.func
    )))
  }

  // -- Scalars -------------------------------------------------------------

  pub fn num(&self, name: &str) -> Option<f64> {
    self.raw(name).and_then(as_num)
  }

  pub fn num_or(&self, name: &str, default: f64) -> f64 {
    self.num(name).unwrap_or(default)
  }

  pub fn int(&self, name: &str) -> Option<i64> {
    self.num(name).map(|v| v.round() as i64)
  }

  pub fn bool(&self, name: &str) -> Option<bool> {
    match self.raw(name)? {
      LuaValue::Boolean(b) => Some(*b),
      // OpenSCAD treats any non-zero number as true.
      LuaValue::Number(n) => Some(*n != 0.0),
      LuaValue::Integer(n) => Some(*n != 0),
      _ => None,
    }
  }

  pub fn bool_or(&self, name: &str, default: bool) -> bool {
    self.bool(name).unwrap_or(default)
  }

  pub fn string(&self, name: &str) -> Option<String> {
    match self.raw(name)? {
      LuaValue::String(s) => s.to_str().ok().map(|s| s.to_string()),
      _ => None,
    }
  }

  // -- Vectors and lists ---------------------------------------------------

  /// A list of numbers, or `None` if the value is not a numeric list.
  pub fn nums(&self, name: &str) -> Option<Vec<f64>> {
    self.raw(name).and_then(as_nums)
  }

  /// A vector of exactly `n` components, broadcasting a bare scalar.
  ///
  /// BOSL2 accepts `size=10` wherever it accepts `size=[10,10,10]`, so a
  /// single number fills every component.
  pub fn sized(&self, name: &str, n: usize) -> Option<Vec<f64>> {
    let v = self.raw(name)?;
    if let Some(x) = as_num(v) {
      return Some(vec![x; n]);
    }
    let list = as_nums(v)?;
    if list.len() == n {
      Some(list)
    } else if list.len() > n {
      Some(list[..n].to_vec())
    } else {
      // A shorter list pads with its last element, so `[10,20]` used as a
      // 3-vector reads as `[10,20,20]` rather than collapsing to zero.
      let last = *list.last()?;
      let mut out = list;
      out.resize(n, last);
      Some(out)
    }
  }

  pub fn vec2(&self, name: &str) -> Option<[f64; 2]> {
    let v = self.sized(name, 2)?;
    Some([v[0], v[1]])
  }

  pub fn vec3(&self, name: &str) -> Option<[f64; 3]> {
    let v = self.sized(name, 3)?;
    Some([v[0], v[1], v[2]])
  }

  /// A list of 2D points.
  pub fn points2(&self, name: &str) -> Option<Vec<[f64; 2]>> {
    let LuaValue::Table(t) = self.raw(name)? else {
      return None;
    };
    let mut out = Vec::new();
    for i in 1..=t.raw_len() {
      let v = t.get::<LuaValue>(i).ok()?;
      let p = as_nums(&v)?;
      if p.len() < 2 {
        return None;
      }
      out.push([p[0], p[1]]);
    }
    Some(out)
  }

  /// A list of 3D points, accepting 2D points as lying in the XY plane.
  pub fn points3(&self, name: &str) -> Option<Vec<[f64; 3]>> {
    let LuaValue::Table(t) = self.raw(name)? else {
      return None;
    };
    let mut out = Vec::new();
    for i in 1..=t.raw_len() {
      let v = t.get::<LuaValue>(i).ok()?;
      let p = as_nums(&v)?;
      match p.len() {
        2 => out.push([p[0], p[1], 0.0]),
        n if n >= 3 => out.push([p[0], p[1], p[2]]),
        _ => return None,
      }
    }
    Some(out)
  }

  // -- BOSL2 conventions ---------------------------------------------------

  /// Resolve BOSL2's interchangeable radius and diameter parameters.
  ///
  /// `d` wins over `r` when both are given, matching `get_radius()`.
  pub fn radius(&self, r: &str, d: &str, default: Option<f64>) -> Option<f64> {
    if let Some(d) = self.num(d) {
      return Some(d / 2.0);
    }
    self.num(r).or(default)
  }

  /// Resolve a tapered shape's end radius, falling back to the shared
  /// `r`/`d` parameters when the per-end ones are absent.
  pub fn radius_end(
    &self,
    r1: &str,
    d1: &str,
    r: &str,
    d: &str,
    default: Option<f64>,
  ) -> Option<f64> {
    self
      .radius(r1, d1, None)
      .or_else(|| self.radius(r, d, None))
      .or(default)
  }

  /// The facet count for a circle of radius `r`, following `segs()`.
  ///
  /// `fn` fixes the count outright; otherwise the finer of the angular
  /// (`fa`) and arc-length (`fs`) limits wins, never dropping below 5.
  pub fn segments(&self, r: f64) -> u32 {
    if let Some(n) = self.int("fn") {
      return if n > 3 { n as u32 } else { 3 };
    }
    let fa = self.num_or("fa", 12.0);
    let fs = self.num_or("fs", 2.0);
    let by_angle = 360.0 / fa;
    let by_length = r.abs() * 2.0 * std::f64::consts::PI / fs;
    by_angle.min(by_length).ceil().max(5.0) as u32
  }

  /// The anchor this call places the shape by.
  pub fn anchor(&self) -> LuaResult<Option<Anchor>> {
    let Some(v) = self.raw("anchor") else {
      return Ok(None);
    };
    match v {
      LuaValue::String(s) => Ok(Some(Anchor::Named(s.to_str()?.to_string()))),
      _ => match as_nums(v) {
        Some(n) if !n.is_empty() => {
          let mut a = [0.0; 3];
          for (i, c) in n.iter().take(3).enumerate() {
            a[i] = *c;
          }
          Ok(Some(Anchor::Vector(a)))
        }
        _ => self.err("anchor must be a vector such as bosl.TOP, or a name"),
      },
    }
  }

  pub fn spin(&self) -> f64 {
    self.num_or("spin", 0.0)
  }

  pub fn orient(&self) -> [f64; 3] {
    self.vec3("orient").unwrap_or([0.0, 0.0, 1.0])
  }
}

/// Where a shape is anchored, either a direction vector or a shape-specific
/// name such as `"origin"`.
#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
  Vector([f64; 3]),
  Named(String),
}

impl Anchor {
  /// The anchor as a direction vector, resolving the names shared by all
  /// shapes. Shape-specific names resolve to `None`.
  pub fn as_vector(&self) -> Option<[f64; 3]> {
    match self {
      Anchor::Vector(v) => Some(*v),
      Anchor::Named(name) => match name.as_str() {
        "origin" | "center" | "centre" => Some([0.0, 0.0, 0.0]),
        "top" => Some([0.0, 0.0, 1.0]),
        "bottom" | "bot" => Some([0.0, 0.0, -1.0]),
        "left" => Some([-1.0, 0.0, 0.0]),
        "right" => Some([1.0, 0.0, 0.0]),
        "front" | "fwd" | "forward" => Some([0.0, -1.0, 0.0]),
        "back" => Some([0.0, 1.0, 0.0]),
        _ => None,
      },
    }
  }
}

// ---------------------------------------------------------------------------
// Lua value coercion
// ---------------------------------------------------------------------------

pub fn as_num(v: &LuaValue) -> Option<f64> {
  match v {
    LuaValue::Number(n) => Some(*n),
    LuaValue::Integer(n) => Some(*n as f64),
    _ => None,
  }
}

/// Read a value as a list of numbers, accepting both Lua tables and the
/// `vector()` userdata LuaCAD hands out.
pub fn as_nums(v: &LuaValue) -> Option<Vec<f64>> {
  match v {
    LuaValue::Table(t) => {
      let len = t.raw_len();
      let mut out = Vec::with_capacity(len);
      for i in 1..=len {
        out.push(as_num(&t.get::<LuaValue>(i).ok()?)?);
      }
      Some(out)
    }
    LuaValue::UserData(ud) => {
      let v = ud.borrow::<crate::lua_engine::LuaVector>().ok()?;
      Some(vec![v.x, v.y, v.z])
    }
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use mlua::Lua;

  const CUBOID: &[&str] = &["size", "chamfer", "rounding"];

  fn args_from(lua: &Lua, code: &str) -> Args {
    let v: LuaValue = lua.load(code).eval().unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    Args::parse("cuboid", CUBOID, &mv).unwrap()
  }

  #[test]
  fn array_part_supplies_positional_parameters() {
    let lua = Lua::new();
    let a = args_from(&lua, "return { {30, 20, 10}, 2 }");
    assert_eq!(a.vec3("size"), Some([30.0, 20.0, 10.0]));
    assert_eq!(a.num("chamfer"), Some(2.0));
  }

  #[test]
  fn named_parameters_override_position() {
    let lua = Lua::new();
    let a = args_from(&lua, "return { {1, 2, 3}, rounding = 4 }");
    assert_eq!(a.num("rounding"), Some(4.0));
    assert_eq!(a.num("chamfer"), None);
  }

  #[test]
  fn a_bare_scalar_broadcasts_to_every_component() {
    let lua = Lua::new();
    let a = args_from(&lua, "return { size = 12 }");
    assert_eq!(a.vec3("size"), Some([12.0, 12.0, 12.0]));
  }

  #[test]
  fn a_short_vector_pads_with_its_last_component() {
    let lua = Lua::new();
    let a = args_from(&lua, "return { size = {5, 6} }");
    assert_eq!(a.vec3("size"), Some([5.0, 6.0, 6.0]));
  }

  #[test]
  fn diameter_wins_over_radius() {
    let lua = Lua::new();
    let v: LuaValue = lua.load("return { r = 3, d = 10 }").eval().unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let a = Args::parse("cyl", &["r", "d"], &mv).unwrap();
    assert_eq!(a.radius("r", "d", None), Some(5.0));
  }

  #[test]
  fn unknown_named_parameters_are_rejected_with_a_suggestion() {
    let lua = Lua::new();
    let v: LuaValue = lua.load("return { roundng = 2 }").eval().unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let err = Args::parse("cuboid", CUBOID, &mv).unwrap_err().to_string();
    assert!(err.contains("'roundng'"), "{err}");
    assert!(err.contains("Did you mean 'rounding'"), "{err}");
  }

  #[test]
  fn the_openscad_spelling_of_fn_is_recognised_as_a_typo() {
    let lua = Lua::new();
    let v: LuaValue = lua.load("return { ['$fn'] = 64 }").eval().unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let err = Args::parse("cuboid", CUBOID, &mv).unwrap_err().to_string();
    assert!(err.contains("Did you mean 'fn'"), "{err}");
  }

  #[test]
  fn fn_fixes_the_facet_count_and_fs_refines_it() {
    let lua = Lua::new();
    let a = args_from(&lua, "return { ['fn'] = 64 }");
    assert_eq!(a.segments(10.0), 64);

    // With the defaults ($fa=12, $fs=2) a small circle is limited by arc
    // length and a large one by angle.
    let b = args_from(&lua, "return { }");
    assert_eq!(b.segments(1.0), 5);
    assert_eq!(b.segments(100.0), 30);
  }
}
