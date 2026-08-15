//! BOSL2's `math.scad`: interpolation, statistics, calculus, complex numbers
//! and polynomials.
//!
//! These compute values, not geometry, so they return numbers and lists to
//! Lua directly. Angles are in degrees throughout, matching OpenSCAD.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, num_list, register_all};

const EPS: f64 = 1e-12;

/// Collapse negative zero, which is arithmetically equal to zero but prints
/// as `-0` and reads as a mistake in a coordinate list.
fn unsign_zero(x: f64) -> f64 {
  if x == 0.0 { 0.0 } else { x }
}

// ---------------------------------------------------------------------------
// A deterministic generator, so a seeded call is reproducible
// ---------------------------------------------------------------------------

/// xoshiro-style generator. OpenSCAD's `rands()` gives no cross-implementation
/// guarantee, so the only promise kept here is BOSL2's own: the same seed
/// yields the same sequence.
pub struct Rng(u64);

impl Rng {
  pub fn new(seed: Option<f64>) -> Rng {
    let s = match seed {
      Some(v) => v.to_bits(),
      // Without a seed the sequence only has to differ between calls.
      None => {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0x9E3779B97F4A7C15);
        COUNTER.fetch_add(0x9E3779B97F4A7C15, Ordering::Relaxed)
      }
    };
    Rng(s | 1)
  }

  fn next_u64(&mut self) -> u64 {
    let mut x = self.0;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.0 = x;
    x
  }

  /// A number in `[lo, hi)`.
  pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
    let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    lo + u * (hi - lo)
  }

  /// A standard normal deviate, by the Box–Muller transform.
  pub fn normal(&mut self) -> f64 {
    let u1 = self.range(f64::MIN_POSITIVE, 1.0);
    let u2 = self.range(0.0, 1.0);
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
  }
}

// ---------------------------------------------------------------------------
// Interpolation and counting
// ---------------------------------------------------------------------------

fn count(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = match a.val("n") {
    Some(Val::Num(v)) => v as usize,
    Some(Val::List(v)) => v.len(),
    None => return a.err("n is required"),
  };
  let s = a.num_or("s", 0.0);
  let step = a.num_or("step", 1.0);
  let values: Vec<f64> = (0..n).map(|i| s + i as f64 * step).collect();
  let values = if a.bool_or("reverse", false) {
    values.into_iter().rev().collect()
  } else {
    values
  };
  num_list(lua, &values)
}

fn lerp(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let from = a.need_val("a")?;
  let to = a.need_val("b")?;
  if !from.same_shape(&to) {
    return a.err("a and b must have the same shape");
  }
  let u = a.need_val("u")?;
  match u {
    // A single factor interpolates once; a list of them interpolates at each.
    Val::Num(t) => mix(&from, &to, t, a)?.to_lua(lua),
    Val::List(ts) => {
      let mut out = Vec::with_capacity(ts.len());
      for t in ts {
        let Some(t) = t.as_num() else {
          return a.err("u must be a number or a list of numbers");
        };
        out.push(mix(&from, &to, t, a)?);
      }
      Val::List(out).to_lua(lua)
    }
  }
}

fn mix(from: &Val, to: &Val, t: f64, a: &Args) -> LuaResult<Val> {
  match from.scale(1.0 - t).add(&to.scale(t)) {
    Some(v) => Ok(v),
    None => a.err("a and b must have the same shape"),
  }
}

fn lerpn(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let from = a.need_val("a")?;
  let to = a.need_val("b")?;
  let n = a.need_num("n")? as i64;
  if n < 0 {
    return a.err("n must not be negative");
  }
  let endpoint = a.bool_or("endpoint", true);
  // Without the endpoint the last sample stops one step short, which is what
  // makes several runs join up without repeating a value.
  let d = (n - i64::from(endpoint)) as f64;
  let mut out = Vec::with_capacity(n as usize);
  for i in 0..n {
    let u = if d == 0.0 { 0.0 } else { i as f64 / d };
    out.push(mix(&from, &to, u, a)?);
  }
  Val::List(out).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Miscellaneous scalar functions
// ---------------------------------------------------------------------------

fn sqr(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  a.need_val("x")?.map_num(&|v| v * v).to_lua(lua)
}

fn log2(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(a.need_num("x")?.log2()))
}

fn hypot(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let x = a.need_num("x")?;
  let y = a.need_num("y")?;
  let z = a.num_or("z", 0.0);
  Ok(LuaValue::Number((x * x + y * y + z * z).sqrt()))
}

fn factorial(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")? as i64;
  let d = a.num_or("d", 0.0) as i64;
  if n < 0 || d < 0 {
    return a.err("factorial is defined only for non-negative integers");
  }
  if d > n {
    return a.err("d cannot be larger than n");
  }
  let mut acc = 1.0f64;
  for i in (d + 1)..=n {
    acc *= i as f64;
  }
  Ok(LuaValue::Number(acc))
}

fn binomial(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")? as i64;
  if n <= 0 {
    return a.err("n must be an integer greater than 0");
  }
  // Each coefficient follows from the last, which avoids ever forming the
  // large factorials that the direct definition would.
  let mut c = 1.0f64;
  let mut out = Vec::with_capacity(n as usize + 1);
  for i in 0..=n {
    out.push(c);
    c = c * (n - i) as f64 / (i + 1) as f64;
  }
  num_list(lua, &out)
}

fn binomial_coefficient(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")? as i64;
  let k = a.need_num("k")? as i64;
  if k < 0 || k > n {
    return Ok(LuaValue::Number(0.0));
  }
  let k = k.min(n - k);
  let mut c = 1.0f64;
  for i in 0..k {
    c = c * (n - i) as f64 / (i + 1) as f64;
  }
  Ok(LuaValue::Number(c))
}

fn gcd(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut x = a.need_num("a")?.round() as i64;
  let mut y = a.need_num("b")?.round() as i64;
  while y != 0 {
    let t = x % y;
    x = y;
    y = t;
  }
  Ok(LuaValue::Number(x.abs() as f64))
}

fn lcm_of(values: &[f64]) -> f64 {
  values.iter().fold(1.0, |acc, v| {
    let (mut x, mut y) = (acc.round() as i64, v.round() as i64);
    while y != 0 {
      let t = x % y;
      x = y;
      y = t;
    }
    if x == 0 {
      0.0
    } else {
      (acc * v / x as f64).abs()
    }
  })
}

fn lcm(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut values = Vec::new();
  for name in ["a", "b"] {
    match a.val(name) {
      Some(Val::Num(v)) => values.push(v),
      Some(Val::List(items)) => {
        for item in items {
          match item.as_num() {
            Some(v) => values.push(v),
            None => return a.err("lcm takes numbers or lists of numbers"),
          }
        }
      }
      None => {}
    }
  }
  if values.is_empty() {
    return a.err("lcm needs at least one value");
  }
  Ok(LuaValue::Number(lcm_of(&values)))
}

/// The hyperbolic functions, which OpenSCAD leaves out.
fn hyperbolic(
  f: fn(f64) -> f64,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |_lua, a| Ok(LuaValue::Number(f(a.need_num("x")?)))
}

// ---------------------------------------------------------------------------
// Quantization and ranges
// ---------------------------------------------------------------------------

fn quantize(lua: &Lua, a: &Args, round: fn(f64) -> f64) -> LuaResult<LuaValue> {
  let y = a.need_num("y")?;
  if y <= 0.0 {
    return a.err("the quantum y must be positive");
  }
  a.need_val("x")?.map_num(&|v| round(v / y) * y).to_lua(lua)
}

fn quant(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // OpenSCAD's round() breaks ties away from zero, unlike Rust's default.
  quantize(lua, a, |v| if v < 0.0 { -(-v).round() } else { v.round() })
}

fn quantdn(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  quantize(lua, a, f64::floor)
}

fn quantup(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  quantize(lua, a, f64::ceil)
}

fn constrain(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.need_num("v")?;
  let lo = a.need_num("minval")?;
  let hi = a.need_num("maxval")?;
  Ok(LuaValue::Number(hi.min(lo.max(v))))
}

fn posmod(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let x = a.need_num("x")?;
  let m = a.need_num("m")?;
  if m.abs() < EPS {
    return a.err("the divisor cannot be zero");
  }
  Ok(LuaValue::Number((x % m + m) % m))
}

fn modang(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let x = a.need_num("x")?;
  let xx = (x % 360.0 + 360.0) % 360.0;
  Ok(LuaValue::Number(if xx < 180.0 { xx } else { xx - 360.0 }))
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Sum a list whose entries may themselves be vectors or matrices.
fn sum_vals(items: &[Val]) -> Option<Val> {
  let mut iter = items.iter();
  let first = iter.next()?.clone();
  iter.try_fold(first, |acc, v| acc.add(v))
}

fn sum(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.need_val("v")?.as_list().map(|s| s.to_vec()) else {
    return a.err("v must be a list");
  };
  if items.is_empty() {
    return match a.val("dflt") {
      Some(d) => d.to_lua(lua),
      None => Ok(LuaValue::Number(0.0)),
    };
  }
  match sum_vals(&items) {
    Some(v) => v.to_lua(lua),
    None => a.err("the entries of v are not all the same shape"),
  }
}

fn mean(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.need_val("v")?.as_list().map(|s| s.to_vec()) else {
    return a.err("v must be a list");
  };
  if items.is_empty() {
    return a.err("v cannot be empty");
  }
  match sum_vals(&items) {
    Some(v) => v.scale(1.0 / items.len() as f64).to_lua(lua),
    None => a.err("the entries of v are not all the same shape"),
  }
}

fn median(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut v = a.need_vec("v")?;
  if v.is_empty() {
    return a.err("v cannot be empty");
  }
  v.sort_by(f64::total_cmp);
  let n = v.len();
  Ok(LuaValue::Number(if n % 2 == 1 {
    v[n / 2]
  } else {
    (v[n / 2 - 1] + v[n / 2]) / 2.0
  }))
}

fn deltas(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.need_val("v")?.as_list().map(|s| s.to_vec()) else {
    return a.err("v must be a list");
  };
  if items.len() < 2 {
    return a.err("v must have at least two entries");
  }
  let wrap = a.bool_or("wrap", false);
  let n = if wrap { items.len() } else { items.len() - 1 };
  let mut out = Vec::with_capacity(n);
  for i in 0..n {
    match items[(i + 1) % items.len()].sub(&items[i]) {
      Some(d) => out.push(d),
      None => return a.err("the entries of v are not all the same shape"),
    }
  }
  Val::List(out).to_lua(lua)
}

fn cumsum(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.need_val("v")?.as_list().map(|s| s.to_vec()) else {
    return a.err("v must be a list");
  };
  let mut out: Vec<Val> = Vec::with_capacity(items.len());
  for item in &items {
    let next = match out.last() {
      None => item.clone(),
      Some(prev) => match prev.add(item) {
        Some(v) => v,
        None => return a.err("the entries of v are not all the same shape"),
      },
    };
    out.push(next);
  }
  Val::List(out).to_lua(lua)
}

/// Multiply two values the way OpenSCAD would: component-wise for equal-length
/// vectors, and as a matrix product for lists of vectors.
fn mul_vals(a: &Val, b: &Val) -> Option<Val> {
  match (a, b) {
    (Val::Num(x), _) => Some(b.scale(*x)),
    (_, Val::Num(y)) => Some(a.scale(*y)),
    (Val::List(_), Val::List(_)) => match (a.as_matrix(), b.as_matrix()) {
      (Some(m), Some(n)) => {
        if m.is_empty() || n.is_empty() || m[0].len() != n.len() {
          return None;
        }
        let cols = n[0].len();
        Some(Val::list(m.iter().map(|row| {
          Val::vec(
            (0..cols)
              .map(|j| row.iter().zip(n.iter()).map(|(x, nr)| x * nr[j]).sum()),
          )
        })))
      }
      _ => {
        let (x, y) = (a.as_vec()?, b.as_vec()?);
        if x.len() != y.len() {
          return None;
        }
        Some(Val::vec(x.iter().zip(y.iter()).map(|(p, q)| p * q)))
      }
    },
  }
}

fn product(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.need_val("list")?.as_list().map(|s| s.to_vec()) else {
    return a.err("list must be a list");
  };
  if items.is_empty() {
    return Val::List(vec![]).to_lua(lua);
  }
  let mut acc = items[0].clone();
  for item in &items[1..] {
    match mul_vals(&acc, item) {
      Some(v) => acc = v,
      None => return a.err("the entries of list cannot be multiplied"),
    }
  }
  acc.to_lua(lua)
}

fn cumprod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.need_val("list")?.as_list().map(|s| s.to_vec()) else {
    return a.err("list must be a list");
  };
  let mut out: Vec<Val> = Vec::with_capacity(items.len());
  for item in &items {
    let next = match out.last() {
      None => item.clone(),
      Some(prev) => match mul_vals(prev, item) {
        Some(v) => v,
        None => return a.err("the entries of list cannot be multiplied"),
      },
    };
    out.push(next);
  }
  Val::List(out).to_lua(lua)
}

fn convolve_vecs(p: &[f64], q: &[f64]) -> Vec<f64> {
  if p.is_empty() || q.is_empty() {
    return vec![];
  }
  let (n, m) = (p.len(), q.len());
  (0..n + m - 1)
    .map(|i| {
      let k1 = i.saturating_sub(n - 1);
      let k2 = i.min(m - 1);
      (k1..=k2).map(|j| p[i - j] * q[j]).sum()
    })
    .collect()
}

fn convolve(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = a.need_vec("p")?;
  let q = a.need_vec("q")?;
  num_list(lua, &convolve_vecs(&p, &q))
}

fn sum_of_sines(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let angle = a.need_num("a")?;
  let sines = a.need_matrix("sines")?;
  let mut total = 0.0;
  for s in &sines {
    if s.len() < 3 {
      return a.err("each sine must be [amplitude, frequency, phase]");
    }
    total += s[0] * (angle * s[1] + s[2]).to_radians().sin();
  }
  Ok(LuaValue::Number(total))
}

// ---------------------------------------------------------------------------
// Random values
// ---------------------------------------------------------------------------

fn rand_int(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let lo = a.need_num("minval")?;
  let hi = a.need_num("maxval")?;
  let n = a.num_or("n", 1.0) as usize;
  let mut rng = Rng::new(a.num("seed"));
  let values: Vec<f64> =
    (0..n).map(|_| rng.range(lo, hi + 1.0).floor()).collect();
  num_list(lua, &values)
}

fn random_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.num_or("n", 1.0) as usize;
  let dim = a.num_or("dim", 2.0) as usize;
  let scale = a.val("scale").unwrap_or(Val::Num(1.0));
  let mut rng = Rng::new(a.num("seed"));
  let scale_at = |i: usize| match &scale {
    Val::Num(v) => *v,
    Val::List(items) => items.get(i).and_then(|v| v.as_num()).unwrap_or(1.0),
  };
  let pts = Val::list(
    (0..n)
      .map(|_| Val::vec((0..dim).map(|d| rng.range(-1.0, 1.0) * scale_at(d)))),
  );
  pts.to_lua(lua)
}

fn gaussian_rands(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.num_or("n", 1.0) as usize;
  let mean = a.num_or("mean", 0.0);
  let cov = a.num_or("cov", 1.0);
  let mut rng = Rng::new(a.num("seed"));
  let sd = cov.abs().sqrt();
  let values: Vec<f64> = (0..n).map(|_| mean + sd * rng.normal()).collect();
  num_list(lua, &values)
}

fn exponential_rands(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.num_or("n", 1.0) as usize;
  let lambda = a.num_or("lambda", 1.0);
  if lambda <= 0.0 {
    return a.err("lambda must be positive");
  }
  let mut rng = Rng::new(a.num("seed"));
  let values: Vec<f64> = (0..n)
    .map(|_| -rng.range(f64::MIN_POSITIVE, 1.0).ln() / lambda)
    .collect();
  num_list(lua, &values)
}

fn spherical_random_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.num_or("n", 1.0) as usize;
  let radius = a.num_or("radius", 1.0);
  if radius <= 0.0 {
    return a.err("radius must be positive");
  }
  let mut rng = Rng::new(a.num("seed"));
  // Sampling the cosine of the polar angle uniformly is what spreads the
  // points evenly over the sphere rather than bunching them at the poles.
  let pts = Val::list((0..n).map(|_| {
    let theta = rng.range(0.0, 360.0);
    let cosphi = rng.range(-1.0, 1.0);
    let sinphi = (1.0 - cosphi * cosphi).max(0.0).sqrt();
    let (s, c) = theta.to_radians().sin_cos();
    Val::vec([radius * sinphi * c, radius * sinphi * s, radius * cosphi])
  }));
  pts.to_lua(lua)
}

fn random_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.num_or("n", 3.0) as usize;
  let size = a.num_or("size", 1.0);
  if n < 3 {
    return a.err("a polygon needs at least 3 vertices");
  }
  if size <= 0.0 {
    return a.err("size must be positive");
  }
  let mut rng = Rng::new(a.num("seed"));
  // Angles come from a rising sequence so the vertices stay in order and the
  // polygon cannot self-intersect.
  let mut cumm = Vec::with_capacity(n + 1);
  let mut acc = 0.0;
  for _ in 0..=n {
    acc += rng.range(0.1, 10.0);
    cumm.push(acc);
  }
  let total = cumm[n - 1];
  let pts = Val::list((0..n).map(|i| {
    let ang = 360.0 * cumm[i] / total;
    let r = rng.range(0.01, size);
    let (s, c) = ang.to_radians().sin_cos();
    Val::vec([r * c, r * s])
  }));
  pts.to_lua(lua)
}

// ---------------------------------------------------------------------------
// Calculus
// ---------------------------------------------------------------------------

/// Read the data and step for the derivative functions.
fn deriv_input(a: &Args) -> LuaResult<(Vec<Val>, f64, bool)> {
  let Some(data) = a.need_val("data")?.as_list().map(|s| s.to_vec()) else {
    return a.err("data must be a list");
  };
  Ok((data, a.num_or("h", 1.0), a.bool_or("closed", false)))
}

fn deriv(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (data, h, closed) = deriv_input(a)?;
  let l = data.len();
  if l < 2 {
    return a.err("data must have at least 2 elements");
  }
  let central = |i: usize| -> Option<Val> {
    data[(i + 1) % l]
      .sub(&data[(l + i - 1) % l])?
      .scale(1.0 / (2.0 * h))
      .into()
  };
  let mut out = Vec::with_capacity(l);
  if closed {
    for i in 0..l {
      match central(i) {
        Some(v) => out.push(v),
        None => return a.err("the entries of data are not the same shape"),
      }
    }
  } else {
    // The ends have no neighbour on one side, so they use a one-sided
    // three-point estimate of the same order as the interior.
    let ends = |i0: usize, i1: usize, i2: usize| -> Option<Val> {
      data[i0]
        .scale(-3.0)
        .add(&data[i1].scale(4.0))?
        .add(&data[i2].scale(-1.0))?
        .scale(1.0 / (2.0 * h))
        .into()
    };
    if l == 2 {
      let d = match data[1].sub(&data[0]) {
        Some(v) => v.scale(1.0 / h),
        None => return a.err("the entries of data are not the same shape"),
      };
      return Val::List(vec![d.clone(), d]).to_lua(lua);
    }
    for i in 0..l {
      let v = if i == 0 {
        ends(0, 1, 2)
      } else if i == l - 1 {
        ends(l - 1, l - 2, l - 3).map(|v| v.scale(-1.0))
      } else {
        central(i)
      };
      match v {
        Some(v) => out.push(v),
        None => return a.err("the entries of data are not the same shape"),
      }
    }
  }
  Val::List(out).to_lua(lua)
}

/// A weighted sum of the entries at the given positions.
fn weighted(data: &[Val], terms: &[(usize, f64)], k: f64) -> Option<Val> {
  let mut acc = data[terms[0].0].scale(terms[0].1);
  for (i, w) in &terms[1..] {
    acc = acc.add(&data[*i].scale(*w))?;
  }
  Some(acc.scale(k))
}

fn deriv2(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (data, h, closed) = deriv_input(a)?;
  let l = data.len();
  if l < 3 {
    return a.err("data must have at least 3 elements");
  }
  let k = 1.0 / (h * h);
  let mut out = Vec::with_capacity(l);
  for i in 0..l {
    // The ends have no neighbour on one side. A three-point estimate there
    // is a whole order less accurate than the interior's, so BOSL2 reaches
    // further in — as far as five points once the data is long enough — to
    // keep the accuracy even along the whole list.
    let terms: Vec<(usize, f64)> = if closed {
      vec![((l + i - 1) % l, 1.0), (i, -2.0), ((i + 1) % l, 1.0)]
    } else if i == 0 {
      match l {
        3 => vec![(0, 1.0), (1, -2.0), (2, 1.0)],
        4 => vec![(0, 2.0), (1, -5.0), (2, 4.0), (3, -1.0)],
        _ => vec![
          (0, 35.0 / 12.0),
          (1, -104.0 / 12.0),
          (2, 114.0 / 12.0),
          (3, -56.0 / 12.0),
          (4, 11.0 / 12.0),
        ],
      }
    } else if i == l - 1 {
      match l {
        3 => vec![(l - 1, 1.0), (l - 2, -2.0), (l - 3, 1.0)],
        4 => vec![(l - 1, -2.0), (l - 2, 5.0), (l - 3, -4.0), (l - 4, 1.0)],
        _ => vec![
          (l - 1, 35.0 / 12.0),
          (l - 2, -104.0 / 12.0),
          (l - 3, 114.0 / 12.0),
          (l - 4, -56.0 / 12.0),
          (l - 5, 11.0 / 12.0),
        ],
      }
    } else {
      vec![(i - 1, 1.0), (i, -2.0), (i + 1, 1.0)]
    };
    match weighted(&data, &terms, k) {
      Some(v) => out.push(v),
      None => return a.err("the entries of data are not the same shape"),
    }
  }
  Val::List(out).to_lua(lua)
}

fn deriv3(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (data, h, closed) = deriv_input(a)?;
  let l = data.len();
  if l < 5 {
    return a.err("data must have at least 5 elements");
  }
  let k = 1.0 / (2.0 * h * h * h);
  let mut out = Vec::with_capacity(l);
  for i in 0..l {
    // As with the second derivative, each of the two points at either end
    // gets its own one-sided five-point estimate rather than borrowing the
    // interior formula from further along.
    let terms: Vec<(usize, f64)> = if closed {
      vec![
        ((l + i - 2) % l, -1.0),
        ((l + i - 1) % l, 2.0),
        ((i + 1) % l, -2.0),
        ((i + 2) % l, 1.0),
      ]
    } else if i == 0 {
      vec![(0, -5.0), (1, 18.0), (2, -24.0), (3, 14.0), (4, -3.0)]
    } else if i == 1 {
      vec![(0, -3.0), (1, 10.0), (2, -12.0), (3, 6.0), (4, -1.0)]
    } else if i == l - 1 {
      vec![
        (l - 1, 5.0),
        (l - 2, -18.0),
        (l - 3, 24.0),
        (l - 4, -14.0),
        (l - 5, 3.0),
      ]
    } else if i == l - 2 {
      vec![
        (l - 1, 3.0),
        (l - 2, -10.0),
        (l - 3, 12.0),
        (l - 4, -6.0),
        (l - 5, 1.0),
      ]
    } else {
      vec![(i - 2, -1.0), (i - 1, 2.0), (i + 1, -2.0), (i + 2, 1.0)]
    };
    match weighted(&data, &terms, k) {
      Some(v) => out.push(v),
      None => return a.err("the entries of data are not the same shape"),
    }
  }
  Val::List(out).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Complex numbers, held as [real, imaginary]
// ---------------------------------------------------------------------------

fn as_complex(v: &Val) -> Option<[f64; 2]> {
  match v {
    Val::Num(n) => Some([*n, 0.0]),
    Val::List(_) => {
      let v = v.as_vec()?;
      match v.len() {
        1 => Some([v[0], 0.0]),
        2 => Some([v[0], v[1]]),
        _ => None,
      }
    }
  }
}

fn cmul(z1: [f64; 2], z2: [f64; 2]) -> [f64; 2] {
  [z1[0] * z2[0] - z1[1] * z2[1], z1[0] * z2[1] + z1[1] * z2[0]]
}

fn complex(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  fn widen(v: &Val) -> Val {
    match v {
      Val::Num(n) => Val::vec([*n, 0.0]),
      Val::List(items) => Val::list(items.iter().map(widen)),
    }
  }
  widen(&a.need_val("list")?).to_lua(lua)
}

/// Apply a complex operation to two values, element-wise over lists.
fn complex_binop(
  lua: &Lua,
  a: &Args,
  op: fn([f64; 2], [f64; 2]) -> Option<[f64; 2]>,
) -> LuaResult<LuaValue> {
  let z1 = a.need_val("z1")?;
  let z2 = a.need_val("z2")?;
  let (Some(x), Some(y)) = (as_complex(&z1), as_complex(&z2)) else {
    return a.err("complex numbers are 2-vectors of [real, imaginary]");
  };
  match op(x, y) {
    Some(r) => num_list(lua, &r),
    None => a.err("the divisor cannot be zero"),
  }
}

fn c_mul(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  complex_binop(lua, a, |x, y| Some(cmul(x, y)))
}

fn c_div(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  complex_binop(lua, a, |x, y| {
    let den = y[0] * y[0] + y[1] * y[1];
    if den < EPS {
      return None;
    }
    Some([
      (x[0] * y[0] + x[1] * y[1]) / den,
      (x[1] * y[0] - x[0] * y[1]) / den,
    ])
  })
}

fn c_conj(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  fn conj(v: &Val) -> Val {
    match as_complex(v) {
      Some(z) if matches!(v, Val::List(_)) => Val::vec([z[0], -z[1]]),
      _ => match v {
        Val::List(items) => Val::list(items.iter().map(conj)),
        other => other.clone(),
      },
    }
  }
  conj(&a.need_val("z")?).to_lua(lua)
}

fn c_part(index: usize) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    fn part(v: &Val, index: usize) -> Val {
      match as_complex(v) {
        Some(z) if matches!(v, Val::List(_)) => Val::Num(z[index]),
        _ => match v {
          Val::List(items) => Val::list(items.iter().map(|x| part(x, index))),
          Val::Num(n) => Val::Num(if index == 0 { *n } else { 0.0 }),
        },
      }
    }
    part(&a.need_val("z")?, index).to_lua(lua)
  }
}

fn c_ident(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")? as usize;
  Val::list(
    (0..n)
      .map(|i| Val::list((0..n).map(|j| Val::vec([f64::from(i == j), 0.0])))),
  )
  .to_lua(lua)
}

fn c_norm(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  fn total(v: &Val) -> f64 {
    match v {
      Val::Num(n) => n * n,
      Val::List(_) => match as_complex(v) {
        Some(z) => z[0] * z[0] + z[1] * z[1],
        None => v
          .as_list()
          .map(|l| l.iter().map(total).sum())
          .unwrap_or(0.0),
      },
    }
  }
  Ok(LuaValue::Number(total(&a.need_val("z")?).sqrt()))
}

// ---------------------------------------------------------------------------
// Polynomials, given highest power first
// ---------------------------------------------------------------------------

fn poly_trim(p: &[f64], eps: f64) -> Vec<f64> {
  let start = p.iter().position(|c| c.abs() > eps).unwrap_or(p.len());
  if start == p.len() {
    vec![0.0]
  } else {
    p[start..].to_vec()
  }
}

fn quadratic_roots(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // The three coefficients may come as one vector or as three arguments.
  let (qa, qb, qc) = match (a.val("a"), a.num("b"), a.num("c")) {
    (Some(v), None, None) => {
      let v = v.as_vec().unwrap_or_default();
      if v.len() != 3 {
        return a.err("give three coefficients, or a 3-vector of them");
      }
      (v[0], v[1], v[2])
    }
    (Some(Val::Num(x)), Some(y), Some(z)) => (x, y, z),
    _ => return a.err("give three coefficients, or a 3-vector of them"),
  };
  let real_only = a.bool_or("real", false);

  let roots: Vec<[f64; 2]> = if qa.abs() < EPS && qb.abs() < EPS {
    vec![]
  } else if qa.abs() < EPS {
    vec![[-qc / qb, 0.0]]
  } else {
    let d = qb * qb - 4.0 * qa * qc;
    if d < 0.0 {
      let s = (-d).sqrt();
      vec![
        [-qb / (2.0 * qa), s / (2.0 * qa)],
        [-qb / (2.0 * qa), -s / (2.0 * qa)],
      ]
    } else {
      // Taking the root that avoids subtracting nearly equal numbers, then
      // getting the other from the product of the roots, keeps both accurate
      // when the discriminant is close to b².
      let s = d.sqrt();
      let q = -0.5 * (qb + qb.signum() * s);
      if q.abs() < EPS {
        vec![[0.0, 0.0], [0.0, 0.0]]
      } else {
        let mut r = vec![[q / qa, 0.0], [qc / q, 0.0]];
        r.sort_by(|x, y| x[0].total_cmp(&y[0]));
        r
      }
    }
  };

  if real_only {
    let reals: Vec<f64> = roots
      .iter()
      .filter(|r| r[1] == 0.0)
      .map(|r| unsign_zero(r[0]))
      .collect();
    return num_list(lua, &reals);
  }
  Val::list(
    roots
      .iter()
      .map(|r| Val::vec([unsign_zero(r[0]), unsign_zero(r[1])])),
  )
  .to_lua(lua)
}

fn polynomial(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = poly_trim(&a.need_vec("p")?, 0.0);
  let z = a.need_val("z")?;
  match z {
    Val::Num(x) => {
      // Horner's rule: fewer multiplications and better conditioned than
      // summing the powers separately.
      let mut total = 0.0;
      for c in &p {
        total = total * x + c;
      }
      Ok(LuaValue::Number(total))
    }
    _ => {
      let Some(zc) = as_complex(&z) else {
        return a.err("z must be a real or complex number");
      };
      let mut total = [0.0, 0.0];
      for c in &p {
        total = cmul(total, zc);
        total[0] += c;
      }
      num_list(lua, &total)
    }
  }
}

fn poly_mult(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Either two polynomials, or one list of them to multiply together.
  let (p, q) = match (a.val("p"), a.val("q")) {
    (Some(p), Some(q)) => (p, q),
    (Some(p), None) => {
      let Some(items) = p.as_list() else {
        return a.err("give two polynomials, or a list of them");
      };
      let mut acc = vec![1.0];
      for item in items {
        let Some(v) = item.as_vec() else {
          return a.err("give two polynomials, or a list of them");
        };
        acc = convolve_vecs(&acc, &v);
      }
      return num_list(lua, &poly_trim(&acc, 0.0));
    }
    _ => return a.err("give two polynomials, or a list of them"),
  };
  let (Some(p), Some(q)) = (p.as_vec(), q.as_vec()) else {
    return a.err("polynomials are vectors of coefficients");
  };
  if p.iter().all(|c| *c == 0.0) || q.iter().all(|c| *c == 0.0) {
    return num_list(lua, &[0.0]);
  }
  num_list(lua, &poly_trim(&convolve_vecs(&p, &q), 0.0))
}

/// Long division of `n` by `d`, returning the quotient and the remainder.
fn poly_divmod(n: &[f64], d: &[f64]) -> (Vec<f64>, Vec<f64>) {
  let d = poly_trim(d, 0.0);
  let mut r = poly_trim(n, 0.0);
  if r.len() < d.len() {
    return (vec![0.0], r);
  }
  let mut q = vec![0.0; r.len() - d.len() + 1];
  for i in 0..q.len() {
    let factor = r[i] / d[0];
    q[i] = factor;
    for j in 0..d.len() {
      r[i + j] -= factor * d[j];
    }
  }
  let rem = poly_trim(&r[q.len()..], 0.0);
  (poly_trim(&q, 0.0), rem)
}

fn poly_div(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_vec("n")?;
  let d = a.need_vec("d")?;
  if poly_trim(&d, 0.0) == vec![0.0] {
    return a.err("the denominator cannot be the zero polynomial");
  }
  let (q, r) = poly_divmod(&n, &d);
  Val::list([Val::vec(q), Val::vec(r)]).to_lua(lua)
}

fn poly_add(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = a.need_vec("p")?;
  let q = a.need_vec("q")?;
  let n = p.len().max(q.len());
  // Coefficients are highest power first, so the shorter one is padded at
  // the front, not the back.
  let sum: Vec<f64> = (0..n)
    .map(|i| {
      let pi = (i + p.len()).checked_sub(n).and_then(|k| p.get(k));
      let qi = (i + q.len()).checked_sub(n).and_then(|k| q.get(k));
      pi.unwrap_or(&0.0) + qi.unwrap_or(&0.0)
    })
    .collect();
  num_list(lua, &poly_trim(&sum, 0.0))
}

/// Every root of a polynomial, by the Aberth–Ehrlich method.
///
/// All the roots are refined at once, each one repelled by the others, which
/// converges on multiple and clustered roots where deflation drifts.
fn poly_roots_of(p: &[f64]) -> Option<Vec<[f64; 2]>> {
  let p = poly_trim(p, 0.0);
  let n = p.len().checked_sub(1)?;
  if n == 0 {
    return Some(vec![]);
  }
  let eval = |z: [f64; 2]| {
    let mut acc = [0.0, 0.0];
    for c in &p {
      acc = cmul(acc, z);
      acc[0] += c;
    }
    acc
  };
  let deriv: Vec<f64> = (0..n).map(|i| p[i] * (n - i) as f64).collect();
  let eval_d = |z: [f64; 2]| {
    let mut acc = [0.0, 0.0];
    for c in &deriv {
      acc = cmul(acc, z);
      acc[0] += c;
    }
    acc
  };

  // Start on a circle wide enough to enclose every root (Cauchy's bound),
  // offset so no two guesses coincide.
  let bound = 1.0
    + p[1..]
      .iter()
      .map(|c| (c / p[0]).abs())
      .fold(0.0f64, f64::max);
  let mut zs: Vec<[f64; 2]> = (0..n)
    .map(|k| {
      let ang = std::f64::consts::TAU * k as f64 / n as f64 + 0.4;
      [bound * 0.5 * ang.cos(), bound * 0.5 * ang.sin()]
    })
    .collect();

  let cdiv = |x: [f64; 2], y: [f64; 2]| -> [f64; 2] {
    let den = y[0] * y[0] + y[1] * y[1];
    if den == 0.0 {
      return [0.0, 0.0];
    }
    [
      (x[0] * y[0] + x[1] * y[1]) / den,
      (x[1] * y[0] - x[0] * y[1]) / den,
    ]
  };

  for _ in 0..500 {
    let mut moved = 0.0f64;
    for i in 0..n {
      let pz = eval(zs[i]);
      let dz = eval_d(zs[i]);
      let newton = cdiv(pz, dz);
      // Sum of 1/(z_i - z_j): the term that pushes the roots apart.
      let mut repel = [0.0, 0.0];
      for j in 0..n {
        if i != j {
          let diff = [zs[i][0] - zs[j][0], zs[i][1] - zs[j][1]];
          let d = diff[0] * diff[0] + diff[1] * diff[1];
          if d > 0.0 {
            repel[0] += diff[0] / d;
            repel[1] += -diff[1] / d;
          }
        }
      }
      let denom = [
        1.0 - (newton[0] * repel[0] - newton[1] * repel[1]),
        -(newton[0] * repel[1] + newton[1] * repel[0]),
      ];
      let step = cdiv(newton, denom);
      zs[i][0] -= step[0];
      zs[i][1] -= step[1];
      moved = moved.max(step[0].abs().max(step[1].abs()));
    }
    if moved < 1e-14 {
      break;
    }
  }
  // A root that is real to within rounding should read as exactly real.
  for z in &mut zs {
    if z[1].abs() < 1e-9 * (1.0 + z[0].abs()) {
      z[1] = 0.0;
    }
  }
  zs.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
  Some(zs)
}

/// How far each root could be from its true value.
///
/// A repeated root is inherently ill-conditioned: a double root can only be
/// located to about the square root of machine epsilon, a triple root to the
/// cube root, and the iteration leaves the cluster spread over a small circle
/// rather than landing on one point. Nearby roots are therefore only
/// distinguishable down to the spread of their own cluster, and that spread
/// is the honest error bar.
fn root_error_bounds(zs: &[[f64; 2]]) -> Vec<f64> {
  zs.iter()
    .enumerate()
    .map(|(i, z)| {
      let nearest = zs
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, w)| ((z[0] - w[0]).powi(2) + (z[1] - w[1]).powi(2)).sqrt())
        .fold(f64::INFINITY, f64::min);
      if nearest.is_finite() { nearest } else { 0.0 }
    })
    .collect()
}

fn poly_roots(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = a.need_vec("p")?;
  match poly_roots_of(&p) {
    Some(roots) => Val::list(roots.iter().map(|r| Val::vec(*r))).to_lua(lua),
    None => a.err("the input polynomial cannot be zero"),
  }
}

/// The real roots of a polynomial, highest power first.
pub fn real_roots_of(p: &[f64]) -> Vec<f64> {
  let Some(roots) = poly_roots_of(p) else {
    return vec![];
  };
  let bounds = root_error_bounds(&roots);
  let mut out: Vec<f64> = roots
    .iter()
    .zip(bounds.iter())
    .filter(|(z, bound)| {
      let norm = (z[0] * z[0] + z[1] * z[1]).sqrt();
      z[1].abs() <= 1e-9 * (1.0 + norm) + **bound
    })
    .map(|(z, _)| unsign_zero(z[0]))
    .collect();
  out.sort_by(f64::total_cmp);
  out
}

fn real_roots(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = a.need_vec("p")?;
  let Some(roots) = poly_roots_of(&p) else {
    return a.err("the input polynomial cannot be zero");
  };
  let bounds = root_error_bounds(&roots);
  // An explicit `eps` sets the threshold outright; otherwise each root is
  // judged against how well it can be located at all, so the parts of a
  // repeated real root are not mistaken for a complex pair.
  let explicit = a.num("eps");
  let mut reals: Vec<f64> = roots
    .iter()
    .zip(bounds.iter())
    .filter(|(z, bound)| {
      let norm = (z[0] * z[0] + z[1] * z[1]).sqrt();
      match explicit {
        Some(eps) => z[1].abs() / (1.0 + norm) < eps,
        None => z[1].abs() <= 1e-9 * (1.0 + norm) + **bound,
      }
    })
    .map(|(z, _)| unsign_zero(z[0]))
    .collect();
  reals.sort_by(f64::total_cmp);
  num_list(lua, &reals)
}

/// Find a zero of a Lua function between two points, by bisection refined
/// with the secant rule.
fn root_find(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(LuaValue::Function(f)) = a.raw("f").cloned() else {
    return a.err("f must be a function");
  };
  let mut lo = a.need_num("x0")?;
  let mut hi = a.need_num("x1")?;
  let tol = a.num_or("tol", 1e-15);
  let call = |x: f64| -> LuaResult<f64> { f.call::<f64>(x) };

  let mut flo = call(lo)?;
  let mut fhi = call(hi)?;
  if flo == 0.0 {
    return Ok(LuaValue::Number(lo));
  }
  if fhi == 0.0 {
    return Ok(LuaValue::Number(hi));
  }
  if flo * fhi > 0.0 {
    return a.err("f must have opposite signs at x0 and x1");
  }

  for _ in 0..200 {
    // The secant estimate, kept inside the bracket so it cannot run away.
    let mid = 0.5 * (lo + hi);
    let secant = hi - fhi * (hi - lo) / (fhi - flo);
    let x = if secant.is_finite() && (secant - lo) * (secant - hi) < 0.0 {
      secant
    } else {
      mid
    };
    let fx = call(x)?;
    if fx == 0.0 || (hi - lo).abs() < tol * (1.0 + x.abs()) {
      return Ok(LuaValue::Number(x));
    }
    if flo * fx < 0.0 {
      hi = x;
      fhi = fx;
    } else {
      lo = x;
      flo = fx;
    }
  }
  Ok(LuaValue::Number(0.5 * (lo + hi)))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  bosl.set("PHI", (1.0_f64 + 5.0_f64.sqrt()) / 2.0)?;
  bosl.set("EPSILON", 1e-9_f64)?;

  register_all(
    lua,
    bosl,
    &[
      ("count", &["n", "s", "step", "reverse"], count as PureFn),
      ("lerp", &["a", "b", "u"], lerp),
      ("lerpn", &["a", "b", "n", "endpoint"], lerpn),
      ("sqr", &["x"], sqr),
      ("log2", &["x"], log2),
      ("hypot", &["x", "y", "z"], hypot),
      ("factorial", &["n", "d"], factorial),
      ("binomial", &["n"], binomial),
      ("binomial_coefficient", &["n", "k"], binomial_coefficient),
      ("gcd", &["a", "b"], gcd),
      ("lcm", &["a", "b"], lcm),
      ("quant", &["x", "y"], quant),
      ("quantdn", &["x", "y"], quantdn),
      ("quantup", &["x", "y"], quantup),
      ("constrain", &["v", "minval", "maxval"], constrain),
      ("posmod", &["x", "m"], posmod),
      ("modang", &["x"], modang),
      ("sum", &["v", "dflt"], sum),
      ("mean", &["v"], mean),
      ("median", &["v"], median),
      ("deltas", &["v", "wrap"], deltas),
      ("cumsum", &["v"], cumsum),
      ("product", &["list"], product),
      ("cumprod", &["list"], cumprod),
      ("convolve", &["p", "q"], convolve),
      ("sum_of_sines", &["a", "sines"], sum_of_sines),
      ("rand_int", &["minval", "maxval", "n", "seed"], rand_int),
      (
        "random_points",
        &["n", "dim", "scale", "seed"],
        random_points,
      ),
      (
        "gaussian_rands",
        &["n", "mean", "cov", "seed"],
        gaussian_rands,
      ),
      (
        "exponential_rands",
        &["n", "lambda", "seed"],
        exponential_rands,
      ),
      (
        "spherical_random_points",
        &["n", "radius", "seed"],
        spherical_random_points,
      ),
      ("random_polygon", &["n", "size", "seed"], random_polygon),
      ("deriv", &["data", "h", "closed"], deriv),
      ("deriv2", &["data", "h", "closed"], deriv2),
      ("deriv3", &["data", "h", "closed"], deriv3),
      ("complex", &["list"], complex),
      ("c_mul", &["z1", "z2"], c_mul),
      ("c_div", &["z1", "z2"], c_div),
      ("c_conj", &["z"], c_conj),
      ("c_ident", &["n"], c_ident),
      ("c_norm", &["z"], c_norm),
      ("quadratic_roots", &["a", "b", "c", "real"], quadratic_roots),
      ("polynomial", &["p", "z"], polynomial),
      ("poly_mult", &["p", "q"], poly_mult),
      ("poly_div", &["n", "d"], poly_div),
      ("poly_add", &["p", "q"], poly_add),
      ("poly_roots", &["p", "tol"], poly_roots),
      ("real_roots", &["p", "eps", "tol"], real_roots),
      ("root_find", &["f", "x0", "x1", "tol"], root_find),
    ],
  )?;

  // The closures need their own registration, since each captures a function.
  for (name, f) in [
    ("sinh", f64::sinh as fn(f64) -> f64),
    ("cosh", f64::cosh),
    ("tanh", f64::tanh),
    ("asinh", f64::asinh),
    ("acosh", f64::acosh),
    ("atanh", f64::atanh),
  ] {
    let g = hyperbolic(f);
    let func = lua.create_function(move |lua, args: mlua::MultiValue| {
      let parsed = Args::parse_pure(name, &["x"], &args)?;
      g(lua, &parsed)
    })?;
    bosl.set(name, func)?;
  }
  for (name, index) in [("c_real", 0usize), ("c_imag", 1)] {
    let g = c_part(index);
    let func = lua.create_function(move |lua, args: mlua::MultiValue| {
      let parsed = Args::parse_pure(name, &["z"], &args)?;
      g(lua, &parsed)
    })?;
    bosl.set(name, func)?;
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::bosl::register_bosl;

  fn eval(code: &str) -> String {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    lua
      .load(format!(
        "local v = {code}
         local function fmt(x)
           if type(x) == 'table' then
             local parts = {{}}
             for i, e in ipairs(x) do parts[i] = fmt(e) end
             return '[' .. table.concat(parts, ',') .. ']'
           end
           if type(x) == 'number' then
             return string.format('%.6g', x)
           end
           return tostring(x)
         end
         return fmt(v)"
      ))
      .eval::<String>()
      .unwrap_or_else(|e| panic!("evaluating {code}: {e}"))
  }

  fn err(code: &str) -> String {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    lua.load(code).eval::<LuaValue>().unwrap_err().to_string()
  }

  #[test]
  fn interpolation_works_on_numbers_and_on_points_alike() {
    assert_eq!(eval("bosl.lerp(0, 10, 0.25)"), "2.5");
    assert_eq!(eval("bosl.lerp({0,0}, {10,20}, 0.5)"), "[5,10]");
    assert_eq!(eval("bosl.lerp(0, 10, {0, 0.5, 1})"), "[0,5,10]");
  }

  #[test]
  fn lerpn_spaces_samples_with_and_without_the_endpoint() {
    assert_eq!(eval("bosl.lerpn(0, 10, 5)"), "[0,2.5,5,7.5,10]");
    assert_eq!(eval("bosl.lerpn(0, 10, 5, false)"), "[0,2,4,6,8]");
  }

  #[test]
  fn count_produces_an_index_run() {
    assert_eq!(eval("bosl.count(4)"), "[0,1,2,3]");
    assert_eq!(eval("bosl.count(4, 1, 2)"), "[1,3,5,7]");
    assert_eq!(eval("bosl.count(3, 0, 1, true)"), "[2,1,0]");
  }

  #[test]
  fn quantization_rounds_to_the_nearest_multiple_each_way() {
    assert_eq!(eval("bosl.quant(12, 5)"), "10");
    assert_eq!(eval("bosl.quantup(12, 5)"), "15");
    assert_eq!(eval("bosl.quantdn(12, 5)"), "10");
    // It applies through a list as readily as to a number.
    assert_eq!(eval("bosl.quant({1, 6, 12}, 5)"), "[0,5,10]");
  }

  #[test]
  fn posmod_and_modang_normalise_angles() {
    assert_eq!(eval("bosl.posmod(-5, 360)"), "355");
    assert_eq!(eval("bosl.modang(270)"), "-90");
    assert_eq!(eval("bosl.modang(-270)"), "90");
  }

  #[test]
  fn statistics_reduce_lists_of_numbers_and_of_points() {
    assert_eq!(eval("bosl.sum({1,2,3,4})"), "10");
    assert_eq!(eval("bosl.sum({{1,2},{3,4}})"), "[4,6]");
    assert_eq!(eval("bosl.mean({2,4,6})"), "4");
    assert_eq!(eval("bosl.median({5,1,3})"), "3");
    assert_eq!(eval("bosl.median({4,1,3,2})"), "2.5");
    assert_eq!(eval("bosl.sum({})"), "0");
  }

  #[test]
  fn cumulative_functions_keep_the_running_total() {
    assert_eq!(eval("bosl.cumsum({1,2,3})"), "[1,3,6]");
    assert_eq!(eval("bosl.deltas({1,4,9})"), "[3,5]");
    assert_eq!(eval("bosl.deltas({1,4,9}, true)"), "[3,5,-8]");
    assert_eq!(eval("bosl.product({2,3,4})"), "24");
    assert_eq!(eval("bosl.cumprod({2,3,4})"), "[2,6,24]");
  }

  #[test]
  fn number_theory_helpers_agree_with_the_definitions() {
    assert_eq!(eval("bosl.gcd(54, 24)"), "6");
    assert_eq!(eval("bosl.lcm(4, 6)"), "12");
    assert_eq!(eval("bosl.lcm({4, 6, 10})"), "60");
    assert_eq!(eval("bosl.factorial(5)"), "120");
    assert_eq!(eval("bosl.factorial(6, 3)"), "120");
    assert_eq!(eval("bosl.binomial_coefficient(6, 2)"), "15");
    assert_eq!(eval("bosl.binomial(4)"), "[1,4,6,4,1]");
  }

  #[test]
  fn convolve_multiplies_coefficient_sequences() {
    assert_eq!(eval("bosl.convolve({1,1}, {1,1})"), "[1,2,1]");
  }

  #[test]
  fn derivatives_of_a_straight_line_are_constant() {
    assert_eq!(eval("bosl.deriv({0,1,2,3,4})"), "[1,1,1,1,1]");
    assert_eq!(eval("bosl.deriv2({0,1,4,9,16})"), "[2,2,2,2,2]");
  }

  #[test]
  fn complex_arithmetic_follows_the_usual_rules() {
    assert_eq!(eval("bosl.c_mul({0,1}, {0,1})"), "[-1,0]");
    assert_eq!(eval("bosl.c_div({1,0}, {0,1})"), "[0,-1]");
    assert_eq!(eval("bosl.c_conj({2,3})"), "[2,-3]");
    assert_eq!(eval("bosl.c_real({2,3})"), "2");
    assert_eq!(eval("bosl.c_imag({2,3})"), "3");
    assert_eq!(eval("bosl.c_norm({3,4})"), "5");
  }

  #[test]
  fn polynomials_evaluate_multiply_and_divide() {
    // x^2 + 2x + 1 at x = 3
    assert_eq!(eval("bosl.polynomial({1,2,1}, 3)"), "16");
    assert_eq!(eval("bosl.poly_mult({1,1}, {1,-1})"), "[1,0,-1]");
    assert_eq!(eval("bosl.poly_add({1,1}, {1,-1})"), "[2,0]");
    // (x^2 - 1) / (x - 1) = x + 1, remainder 0
    assert_eq!(eval("bosl.poly_div({1,0,-1}, {1,-1})"), "[[1,1],[0]]");
  }

  #[test]
  fn quadratic_roots_cover_the_real_and_complex_cases() {
    assert_eq!(eval("bosl.quadratic_roots(1, -3, 2, true)"), "[1,2]");
    assert_eq!(eval("bosl.quadratic_roots(1, 0, 1, true)"), "[]");
    assert_eq!(eval("bosl.quadratic_roots(1, 0, 1)"), "[[0,1],[0,-1]]");
  }

  #[test]
  fn poly_roots_finds_every_distinct_root() {
    // (x-1)(x-2)(x-3) = x^3 - 6x^2 + 11x - 6
    assert_eq!(eval("bosl.real_roots({1,-6,11,-6})"), "[1,2,3]");
  }

  /// A repeated root cannot be located to full precision — a triple root is
  /// only good to about the cube root of machine epsilon — so all three come
  /// back near 2 rather than exactly on it.
  #[test]
  fn a_repeated_root_is_reported_once_per_multiplicity() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let roots: Vec<f64> = lua
      .load("return bosl.real_roots({1,-6,12,-8})")
      .eval()
      .unwrap();
    assert_eq!(roots.len(), 3, "{roots:?}");
    for r in roots {
      assert!((r - 2.0).abs() < 1e-4, "{r}");
    }
  }

  #[test]
  fn root_find_locates_a_zero_of_a_lua_function() {
    assert_eq!(
      eval("bosl.root_find(function(x) return x*x - 4 end, 0, 10)"),
      "2"
    );
  }

  #[test]
  fn seeded_random_values_repeat_and_unseeded_ones_do_not() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let same: bool = lua
      .load(
        "local a = bosl.gaussian_rands(5, 0, 1, 42)
         local b = bosl.gaussian_rands(5, 0, 1, 42)
         for i = 1, 5 do if a[i] ~= b[i] then return false end end
         return true",
      )
      .eval()
      .unwrap();
    assert!(same, "the same seed should give the same values");

    let differ: bool = lua
      .load(
        "local a = bosl.gaussian_rands(5)
         local b = bosl.gaussian_rands(5)
         for i = 1, 5 do if a[i] ~= b[i] then return true end end
         return false",
      )
      .eval()
      .unwrap();
    assert!(differ, "unseeded values should not repeat");
  }

  #[test]
  fn random_points_stay_inside_the_scale_they_are_given() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let ok: bool = lua
      .load(
        "local pts = bosl.random_points(50, 2, {10, 4}, 7)
         for _, p in ipairs(pts) do
           if math.abs(p[1]) > 10 or math.abs(p[2]) > 4 then return false end
         end
         return #pts == 50",
      )
      .eval()
      .unwrap();
    assert!(ok);
  }

  #[test]
  fn a_pure_function_returns_a_value_not_a_shape() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let kind: String = lua
      .load("return type(bosl.lerp(0, 10, 0.5))")
      .eval()
      .unwrap();
    assert_eq!(kind, "number");
  }

  #[test]
  fn bad_input_is_reported_rather_than_guessed_at() {
    assert!(
      err("return bosl.lerp({1,2}, {1,2,3}, 0.5)").contains("same shape")
    );
    assert!(err("return bosl.quant(10, 0)").contains("positive"));
    assert!(err("return bosl.posmod(5, 0)").contains("zero"));
    assert!(err("return bosl.median({})").contains("empty"));
  }
}
