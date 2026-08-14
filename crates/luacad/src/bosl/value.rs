//! The dynamic value BOSL2's pure functions operate on.
//!
//! OpenSCAD makes no distinction between a number, a vector and a matrix —
//! they are all values, and `(1-u)*a + u*b` interpolates whichever of them it
//! is handed. The functions in `math.scad`, `vectors.scad` and friends lean on
//! that throughout, so porting them faithfully means carrying the same
//! polymorphism rather than fixing each one to a single shape.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

/// A number or an arbitrarily nested list of them.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
  Num(f64),
  List(Vec<Val>),
}

impl Val {
  pub fn vec(items: impl IntoIterator<Item = f64>) -> Val {
    Val::List(items.into_iter().map(Val::Num).collect())
  }

  pub fn list(items: impl IntoIterator<Item = Val>) -> Val {
    Val::List(items.into_iter().collect())
  }

  pub fn as_num(&self) -> Option<f64> {
    match self {
      Val::Num(n) => Some(*n),
      _ => None,
    }
  }

  pub fn as_list(&self) -> Option<&[Val]> {
    match self {
      Val::List(v) => Some(v),
      _ => None,
    }
  }

  /// The value as a flat numeric vector, or `None` if it is nested deeper.
  pub fn as_vec(&self) -> Option<Vec<f64>> {
    self.as_list()?.iter().map(|v| v.as_num()).collect()
  }

  /// The value as a list of numeric vectors — a path, or a matrix.
  pub fn as_matrix(&self) -> Option<Vec<Vec<f64>>> {
    self.as_list()?.iter().map(|v| v.as_vec()).collect()
  }

  pub fn len(&self) -> Option<usize> {
    self.as_list().map(|l| l.len())
  }

  pub fn is_empty(&self) -> bool {
    self.len() == Some(0)
  }

  /// Whether two values nest the same way, which is what the arithmetic
  /// below needs to line up.
  pub fn same_shape(&self, other: &Val) -> bool {
    match (self, other) {
      (Val::Num(_), Val::Num(_)) => true,
      (Val::List(a), Val::List(b)) => {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.same_shape(y))
      }
      _ => false,
    }
  }

  /// Apply a function to every number, keeping the structure.
  pub fn map_num(&self, f: &impl Fn(f64) -> f64) -> Val {
    match self {
      Val::Num(n) => Val::Num(f(*n)),
      Val::List(v) => Val::List(v.iter().map(|x| x.map_num(f)).collect()),
    }
  }

  /// Component-wise sum. `None` when the shapes do not line up.
  pub fn add(&self, other: &Val) -> Option<Val> {
    match (self, other) {
      (Val::Num(a), Val::Num(b)) => Some(Val::Num(a + b)),
      (Val::List(a), Val::List(b)) if a.len() == b.len() => Some(Val::List(
        a.iter()
          .zip(b)
          .map(|(x, y)| x.add(y))
          .collect::<Option<Vec<_>>>()?,
      )),
      _ => None,
    }
  }

  pub fn sub(&self, other: &Val) -> Option<Val> {
    self.add(&other.scale(-1.0))
  }

  pub fn scale(&self, k: f64) -> Val {
    self.map_num(&|n| n * k)
  }

  /// The sum of the component-wise products, OpenSCAD's `a * b` for two
  /// equal-length vectors.
  pub fn dot(&self, other: &Val) -> Option<f64> {
    let a = self.as_vec()?;
    let b = other.as_vec()?;
    if a.len() != b.len() {
      return None;
    }
    Some(a.iter().zip(b.iter()).map(|(x, y)| x * y).sum())
  }

  /// Read a Lua value, accepting both tables and LuaCAD's `vector()`.
  pub fn from_lua(v: &LuaValue) -> Option<Val> {
    match v {
      LuaValue::Number(n) => Some(Val::Num(*n)),
      LuaValue::Integer(n) => Some(Val::Num(*n as f64)),
      LuaValue::Boolean(b) => Some(Val::Num(f64::from(*b))),
      LuaValue::Table(t) => {
        let len = t.raw_len();
        let mut out = Vec::with_capacity(len);
        for i in 1..=len {
          out.push(Val::from_lua(&t.get::<LuaValue>(i).ok()?)?);
        }
        Some(Val::List(out))
      }
      LuaValue::UserData(ud) => {
        let v = ud.borrow::<crate::lua_engine::LuaVector>().ok()?;
        Some(Val::vec([v.x, v.y, v.z]))
      }
      _ => None,
    }
  }

  pub fn to_lua(&self, lua: &Lua) -> LuaResult<LuaValue> {
    match self {
      Val::Num(n) => Ok(LuaValue::Number(*n)),
      Val::List(items) => {
        let t = lua.create_table()?;
        for (i, item) in items.iter().enumerate() {
          t.set(i + 1, item.to_lua(lua)?)?;
        }
        Ok(LuaValue::Table(t))
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Conversions used by the function bodies
// ---------------------------------------------------------------------------

pub fn v2(v: &[f64]) -> [f64; 2] {
  [
    v.first().copied().unwrap_or(0.0),
    v.get(1).copied().unwrap_or(0.0),
  ]
}

pub fn v3(v: &[f64]) -> [f64; 3] {
  [
    v.first().copied().unwrap_or(0.0),
    v.get(1).copied().unwrap_or(0.0),
    v.get(2).copied().unwrap_or(0.0),
  ]
}

pub fn num_list(lua: &Lua, v: &[f64]) -> LuaResult<LuaValue> {
  Val::vec(v.iter().copied()).to_lua(lua)
}

pub fn matrix(lua: &Lua, rows: &[Vec<f64>]) -> LuaResult<LuaValue> {
  Val::list(rows.iter().map(|r| Val::vec(r.iter().copied()))).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// A BOSL2 function that computes a value rather than geometry.
pub type PureFn = fn(&Lua, &Args) -> LuaResult<LuaValue>;

pub use crate::bosl::args::Args;

impl Args {
  /// A parameter as a dynamic value.
  pub fn val(&self, name: &str) -> Option<Val> {
    Val::from_lua(self.raw(name)?)
  }

  /// A required parameter, reported by name when it is missing.
  pub fn need_val(&self, name: &str) -> LuaResult<Val> {
    match self.val(name) {
      Some(v) => Ok(v),
      None => self.err(format!("{name} is required")),
    }
  }

  /// A required numeric parameter.
  pub fn need_num(&self, name: &str) -> LuaResult<f64> {
    match self.num(name) {
      Some(v) => Ok(v),
      None => self.err(format!("{name} must be a number")),
    }
  }

  /// A required numeric vector.
  pub fn need_vec(&self, name: &str) -> LuaResult<Vec<f64>> {
    match self.val(name).and_then(|v| v.as_vec()) {
      Some(v) => Ok(v),
      None => self.err(format!("{name} must be a vector of numbers")),
    }
  }

  /// A required list of numeric vectors — a path, or a matrix.
  pub fn need_matrix(&self, name: &str) -> LuaResult<Vec<Vec<f64>>> {
    match self.val(name).and_then(|v| v.as_matrix()) {
      Some(v) => Ok(v),
      None => self.err(format!("{name} must be a list of vectors")),
    }
  }

  /// A required list of points, widened to 3D.
  pub fn need_points3(&self, name: &str) -> LuaResult<Vec<[f64; 3]>> {
    Ok(self.need_matrix(name)?.iter().map(|p| v3(p)).collect())
  }

  /// A required list of points, narrowed to 2D.
  pub fn need_points2(&self, name: &str) -> LuaResult<Vec<[f64; 2]>> {
    Ok(self.need_matrix(name)?.iter().map(|p| v2(p)).collect())
  }
}

/// Register a pure function under `bosl.<name>`.
pub fn register_pure(
  lua: &Lua,
  table: &mlua::Table,
  name: &'static str,
  params: &'static [&'static str],
  f: PureFn,
) -> LuaResult<()> {
  let func = lua.create_function(move |lua, args: mlua::MultiValue| {
    let parsed = Args::parse_pure(name, params, &args)?;
    f(lua, &parsed)
  })?;
  table.set(name, func)?;
  Ok(())
}

/// Register a batch of pure functions that share nothing but their shape.
pub fn register_all(
  lua: &Lua,
  table: &mlua::Table,
  entries: &[(&'static str, &'static [&'static str], PureFn)],
) -> LuaResult<()> {
  for (name, params, f) in entries {
    register_pure(lua, table, name, params, *f)?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_number_and_a_vector_add_the_same_way() {
    let a = Val::Num(2.0);
    let b = Val::Num(3.0);
    assert_eq!(a.add(&b), Some(Val::Num(5.0)));

    let u = Val::vec([1.0, 2.0]);
    let v = Val::vec([10.0, 20.0]);
    assert_eq!(u.add(&v), Some(Val::vec([11.0, 22.0])));
  }

  #[test]
  fn adding_mismatched_shapes_fails_rather_than_guessing() {
    assert_eq!(Val::Num(1.0).add(&Val::vec([1.0])), None);
    assert_eq!(Val::vec([1.0]).add(&Val::vec([1.0, 2.0])), None);
  }

  #[test]
  fn nested_lists_keep_their_structure_through_arithmetic() {
    let m = Val::list([Val::vec([1.0, 2.0]), Val::vec([3.0, 4.0])]);
    assert_eq!(
      m.scale(2.0),
      Val::list([Val::vec([2.0, 4.0]), Val::vec([6.0, 8.0])])
    );
    assert_eq!(m.as_matrix(), Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]]));
  }

  #[test]
  fn a_vector_of_vectors_is_not_a_flat_vector() {
    let m = Val::list([Val::vec([1.0, 2.0])]);
    assert_eq!(m.as_vec(), None);
  }

  #[test]
  fn values_round_trip_through_lua() {
    let lua = Lua::new();
    let original =
      Val::list([Val::Num(1.5), Val::vec([2.0, 3.0]), Val::List(vec![])]);
    let lua_value = original.to_lua(&lua).unwrap();
    assert_eq!(Val::from_lua(&lua_value), Some(original));
  }
}
