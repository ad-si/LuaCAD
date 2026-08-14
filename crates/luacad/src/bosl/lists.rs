//! BOSL2's `lists.scad`: selecting, reshaping and combining lists.
//!
//! OpenSCAD indexes from 0 and Lua from 1. These functions keep OpenSCAD's
//! convention for the *values* they are given — `select(list, -1)` is still
//! the last entry — because scripts ported from BOSL2 pass those indices
//! straight through, and silently shifting them by one would change which
//! element every call returns.
//!
//! One name needs bracket syntax: `repeat` is a Lua keyword, so it is written
//! `bosl["repeat"](val, n)`. The same goes for the `end` parameter of
//! `select()` and `slice()`, which is why both also take it positionally.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all};

/// The entries of a list parameter.
fn items(a: &Args, name: &str) -> LuaResult<Vec<Val>> {
  match a.need_val(name)? {
    Val::List(v) => Ok(v),
    Val::Num(_) => a.err(format!("{name} must be a list")),
  }
}

/// Wrap an index into range the way OpenSCAD's `select()` does, so -1 is the
/// last entry and indices past the end come round again.
fn wrap(i: i64, len: usize) -> usize {
  if len == 0 {
    return 0;
  }
  let l = len as i64;
  (((i % l) + l) % l) as usize
}

fn is_homogeneous(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let depth = a.num_or("depth", 10.0) as usize;
  fn shape(v: &Val, depth: usize) -> Option<Vec<usize>> {
    if depth == 0 {
      return Some(vec![]);
    }
    match v {
      Val::Num(_) => Some(vec![]),
      Val::List(items) => {
        let mut s = vec![items.len()];
        if let Some(first) = items.first() {
          s.extend(shape(first, depth - 1)?);
        }
        Some(s)
      }
    }
  }
  let ok = match list.first() {
    None => true,
    Some(first) => {
      let want = shape(first, depth);
      list.iter().all(|v| shape(v, depth) == want)
    }
  };
  Ok(LuaValue::Boolean(ok))
}

/// The shortest or longest entry length.
fn extreme_length(_lua: &Lua, a: &Args, longest: bool) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let lengths = list.iter().filter_map(|v| v.len());
  let result = if longest {
    lengths.max()
  } else {
    lengths.min()
  };
  Ok(LuaValue::Number(result.unwrap_or(0) as f64))
}

fn min_length(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  extreme_length(lua, a, false)
}

fn max_length(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  extreme_length(lua, a, true)
}

fn list_shape(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  fn dims(v: &Val, out: &mut Vec<Option<usize>>) {
    if let Val::List(items) = v {
      let len = items.len();
      out.push(Some(len));
      if let Some(first) = items.first() {
        // A ragged level has no single size, so it reports as undefined.
        let consistent = items.iter().all(|x| x.len() == first.len());
        if consistent {
          dims(first, out);
        } else {
          out.push(None);
        }
      }
    }
  }
  let mut out = Vec::new();
  dims(&a.need_val("v")?, &mut out);
  if let Some(d) = a.num("depth") {
    let i = d as usize;
    return Ok(match out.get(i) {
      Some(Some(n)) => LuaValue::Number(*n as f64),
      _ => LuaValue::Nil,
    });
  }
  let t = lua.create_table()?;
  for (i, d) in out.iter().enumerate() {
    match d {
      Some(n) => t.set(i + 1, *n as f64)?,
      None => t.set(i + 1, LuaValue::Nil)?,
    }
  }
  Ok(LuaValue::Table(t))
}

fn in_list(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let needle = a.need_val("val")?;
  let list = items(a, "list")?;
  Ok(LuaValue::Boolean(list.contains(&needle)))
}

fn select(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  if list.is_empty() {
    return Val::List(vec![]).to_lua(lua);
  }
  let l = list.len();
  let start = a.need_val("start")?;

  match a.num("end") {
    // With both ends given, the range runs forward and wraps if it has to.
    Some(e) => {
      let s = wrap(a.need_num("start")? as i64, l);
      let e = wrap(e as i64, l);
      let picked: Vec<Val> = if s <= e {
        list[s..=e].to_vec()
      } else {
        list[s..].iter().chain(list[..=e].iter()).cloned().collect()
      };
      Val::List(picked).to_lua(lua)
    }
    None => match start {
      // One index picks one entry; a list of them picks several.
      Val::Num(i) => list[wrap(i as i64, l)].clone().to_lua(lua),
      Val::List(idx) => {
        let mut out = Vec::with_capacity(idx.len());
        for i in idx {
          let Some(i) = i.as_num() else {
            return a.err("the indices must be numbers");
          };
          out.push(list[wrap(i as i64, l)].clone());
        }
        Val::List(out).to_lua(lua)
      }
    },
  }
}

fn slice(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let l = list.len() as i64;
  if l == 0 {
    return Val::List(vec![]).to_lua(lua);
  }
  // Unlike `select`, a slice clips at the ends instead of wrapping.
  let mut start = a.num_or("start", 0.0) as i64;
  let mut end = a.num_or("end", -1.0) as i64;
  if start < 0 {
    start += l;
  }
  if end < 0 {
    end += l;
  }
  let start = start.max(0);
  let end = end.min(l - 1);
  if start > end {
    return Val::List(vec![]).to_lua(lua);
  }
  Val::List(list[start as usize..=end as usize].to_vec()).to_lua(lua)
}

fn last(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  match list.last() {
    Some(v) => v.clone().to_lua(lua),
    None => Ok(LuaValue::Nil),
  }
}

fn list_head(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let l = list.len() as i64;
  let to = a.num_or("to", -2.0) as i64;
  let end = if to < 0 { l + to } else { to.min(l - 1) };
  if end < 0 {
    return Val::List(vec![]).to_lua(lua);
  }
  Val::List(list[..=(end as usize)].to_vec()).to_lua(lua)
}

fn list_tail(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let l = list.len() as i64;
  let from = a.num_or("from", 1.0) as i64;
  let start = if from < 0 { (l + from).max(0) } else { from };
  if start >= l {
    return Val::List(vec![]).to_lua(lua);
  }
  Val::List(list[start as usize..].to_vec()).to_lua(lua)
}

fn bselect(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let index = items(a, "index")?;
  if index.len() != list.len() {
    return a.err("the index list must be the same length as the list");
  }
  let picked: Vec<Val> = list
    .into_iter()
    .zip(index.iter())
    .filter(|(_, keep)| keep.as_num().is_some_and(|n| n != 0.0))
    .map(|(v, _)| v)
    .collect();
  Val::List(picked).to_lua(lua)
}

fn list_bset(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let indexset = items(a, "indexset")?;
  let values = items(a, "valuelist")?;
  let dflt = a.val("dflt").unwrap_or(Val::Num(0.0));
  let wanted = indexset
    .iter()
    .filter(|v| v.as_num().is_some_and(|n| n != 0.0))
    .count();
  if wanted != values.len() {
    return a.err(format!(
      "valuelist should have {wanted} entries to match the index set"
    ));
  }
  let mut next = values.into_iter();
  let out: Vec<Val> = indexset
    .iter()
    .map(|flag| {
      if flag.as_num().is_some_and(|n| n != 0.0) {
        next.next().unwrap_or_else(|| dflt.clone())
      } else {
        dflt.clone()
      }
    })
    .collect();
  Val::List(out).to_lua(lua)
}

fn repeat_val(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let val = a.need_val("val")?;
  // A vector of counts nests the repetition one level per entry.
  fn build(val: &Val, counts: &[usize]) -> Val {
    match counts.split_first() {
      None => val.clone(),
      Some((n, rest)) => Val::List((0..*n).map(|_| build(val, rest)).collect()),
    }
  }
  let counts: Vec<usize> = match a.need_val("n")? {
    Val::Num(n) => vec![n.max(0.0) as usize],
    Val::List(v) => v
      .iter()
      .map(|x| x.as_num().unwrap_or(0.0).max(0.0) as usize)
      .collect(),
  };
  build(&val, &counts).to_lua(lua)
}

fn force_list(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.need_val("value")?;
  if matches!(v, Val::List(_)) {
    return v.to_lua(lua);
  }
  let n = a.num_or("n", 1.0).max(0.0) as usize;
  match a.val("fill") {
    // With a fill the first slot keeps the value and the rest are padding.
    Some(fill) => Val::List(
      std::iter::once(v)
        .chain(std::iter::repeat_n(fill, n.saturating_sub(1)))
        .collect(),
    )
    .to_lua(lua),
    None => Val::List(vec![v; n]).to_lua(lua),
  }
}

fn reverse(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut list = items(a, "list")?;
  list.reverse();
  Val::List(list).to_lua(lua)
}

fn list_rotate(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  if list.is_empty() {
    return Val::List(list).to_lua(lua);
  }
  let n = wrap(a.num_or("n", 1.0) as i64, list.len());
  let mut out = list[n..].to_vec();
  out.extend_from_slice(&list[..n]);
  Val::List(out).to_lua(lua)
}

fn shuffle(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut list = items(a, "list")?;
  let mut rng = crate::bosl::math::Rng::new(a.num("seed"));
  // Fisher–Yates, so every ordering is equally likely.
  for i in (1..list.len()).rev() {
    let j = rng.range(0.0, (i + 1) as f64) as usize;
    list.swap(i, j.min(i));
  }
  Val::List(list).to_lua(lua)
}

fn repeat_entries(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  if list.is_empty() {
    return a.err("the list cannot be empty");
  }
  let n = a.need_val("N")?;
  let exact = a.bool_or("exact", true);
  let reps: Vec<f64> = match n {
    Val::Num(total) => {
      vec![total / list.len() as f64; list.len()]
    }
    Val::List(v) => {
      if v.len() != list.len() {
        return a.err("N must be a number or one count per entry");
      }
      v.iter().map(|x| x.as_num().unwrap_or(0.0)).collect()
    }
  };
  // Rounding each count on its own would drift off the requested total, so
  // the error is carried forward and settled on the next entry.
  let counts: Vec<usize> = if exact {
    let mut carry = 0.0;
    reps
      .iter()
      .map(|r| {
        let want = r + carry;
        let take = want.round().max(0.0);
        carry = want - take;
        take as usize
      })
      .collect()
  } else {
    reps.iter().map(|r| r.round().max(0.0) as usize).collect()
  };
  let mut out = Vec::new();
  for (v, c) in list.iter().zip(counts.iter()) {
    for _ in 0..*c {
      out.push(v.clone());
    }
  }
  Val::List(out).to_lua(lua)
}

fn list_pad(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut list = items(a, "list")?;
  let minlen = a.num_or("minlen", 0.0).max(0.0) as usize;
  let fill = a.val("fill").unwrap_or(Val::Num(0.0));
  while list.len() < minlen {
    list.push(fill.clone());
  }
  Val::List(list).to_lua(lua)
}

fn list_set(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut list = items(a, "list").unwrap_or_default();
  let dflt = a.val("dflt").unwrap_or(Val::Num(0.0));
  let (indices, values) = match (a.need_val("indices")?, a.need_val("values")?)
  {
    (Val::Num(i), v) => (vec![i], vec![v]),
    (Val::List(idx), Val::List(vals)) => {
      if idx.len() != vals.len() {
        return a.err("the index list and value list must match in length");
      }
      let mut is = Vec::with_capacity(idx.len());
      for i in idx {
        match i.as_num() {
          Some(n) => is.push(n),
          None => return a.err("the indices must be numbers"),
        }
      }
      (is, vals)
    }
    _ => return a.err("give one index and value, or a list of each"),
  };

  for (i, v) in indices.iter().zip(values) {
    let mut idx = *i as i64;
    if idx < 0 {
      idx += list.len() as i64;
    }
    if idx < 0 {
      return a.err("the index is before the start of the list");
    }
    // Setting past the end grows the list, filling the gap with `dflt`.
    while list.len() <= idx as usize {
      list.push(dflt.clone());
    }
    list[idx as usize] = v;
  }
  let minlen = a.num_or("minlen", 0.0).max(0.0) as usize;
  while list.len() < minlen {
    list.push(dflt.clone());
  }
  Val::List(list).to_lua(lua)
}

fn list_insert(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let (indices, values) = match (a.need_val("indices")?, a.need_val("values")?)
  {
    (Val::Num(i), v) => (vec![i as i64], vec![v]),
    (Val::List(idx), Val::List(vals)) => {
      if idx.len() != vals.len() {
        return a.err("the index list and value list must match in length");
      }
      (
        idx
          .iter()
          .map(|i| i.as_num().unwrap_or(0.0) as i64)
          .collect(),
        vals,
      )
    }
    _ => return a.err("give one index and value, or a list of each"),
  };

  // Inserting back to front keeps the earlier indices meaning what they did
  // before anything moved.
  let mut pairs: Vec<(i64, Val)> = indices.into_iter().zip(values).collect();
  pairs.sort_by_key(|(i, _)| std::cmp::Reverse(*i));
  let mut out = list;
  for (i, v) in pairs {
    let idx = if i < 0 { i + out.len() as i64 } else { i };
    if idx < 0 || idx > out.len() as i64 {
      return a.err("the insertion index is outside the list");
    }
    out.insert(idx as usize, v);
  }
  Val::List(out).to_lua(lua)
}

fn list_remove(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let drop: Vec<usize> = match a.need_val("ind")? {
    Val::Num(i) => {
      let i = i as i64;
      if i < 0 || i >= list.len() as i64 {
        return Val::List(list).to_lua(lua);
      }
      vec![i as usize]
    }
    Val::List(v) => v
      .iter()
      .filter_map(|x| x.as_num())
      .map(|n| n as usize)
      .collect(),
  };
  let out: Vec<Val> = list
    .into_iter()
    .enumerate()
    .filter(|(i, _)| !drop.contains(i))
    .map(|(_, v)| v)
    .collect();
  Val::List(out).to_lua(lua)
}

fn list_remove_values(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let all = a.bool_or("all", false);
  let values = match a.val("values") {
    Some(Val::List(v)) => v,
    Some(other) => vec![other],
    None => vec![],
  };
  let mut out = list;
  for v in values {
    if all {
      out.retain(|x| *x != v);
    } else if let Some(pos) = out.iter().position(|x| *x == v) {
      out.remove(pos);
    }
  }
  Val::List(out).to_lua(lua)
}

fn idx(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  // The indices are OpenSCAD's, so they start at zero.
  let step = a.num_or("step", 1.0);
  let l = list.len() as i64;
  if l == 0 {
    return Val::List(vec![]).to_lua(lua);
  }
  let s = wrap(a.num_or("s", 0.0) as i64, list.len()) as i64;
  let e = wrap(a.num_or("e", -1.0) as i64, list.len()) as i64;
  let mut out = Vec::new();
  let mut i = s as f64;
  while (step > 0.0 && i <= e as f64) || (step < 0.0 && i >= e as f64) {
    out.push(Val::Num(i));
    i += step;
  }
  Val::List(out).to_lua(lua)
}

/// Consecutive runs of `n` entries, optionally wrapping past the end.
fn windows(lua: &Lua, a: &Args, n: usize) -> LuaResult<LuaValue> {
  let list = items(a, "list")?;
  let wrap_around = a.bool_or("wrap", false);
  let l = list.len();
  if l < n {
    return Val::List(vec![]).to_lua(lua);
  }
  let count = if wrap_around { l } else { l - n + 1 };
  let out: Vec<Val> = (0..count)
    .map(|i| Val::List((0..n).map(|k| list[(i + k) % l].clone()).collect()))
    .collect();
  Val::List(out).to_lua(lua)
}

fn pair(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  windows(lua, a, 2)
}

fn triplet(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  windows(lua, a, 3)
}

fn combinations(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "l")?;
  let n = a.num_or("n", 2.0) as usize;
  if n < 1 || n > list.len() {
    return a.err("n must be between 1 and the length of the list");
  }
  fn build(list: &[Val], n: usize, start: usize, out: &mut Vec<Val>) {
    if n == 1 {
      for item in list.iter().skip(start) {
        out.push(Val::List(vec![item.clone()]));
      }
      return;
    }
    for i in start..=list.len() - n {
      let mut rest = Vec::new();
      build(list, n - 1, i + 1, &mut rest);
      for r in rest {
        let mut combo = vec![list[i].clone()];
        if let Val::List(tail) = r {
          combo.extend(tail);
        }
        out.push(Val::List(combo));
      }
    }
  }
  let mut out = Vec::new();
  build(&list, n, 0, &mut out);
  Val::List(out).to_lua(lua)
}

fn permutations(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "l")?;
  let n = a.num_or("n", 2.0) as usize;
  if n < 1 || n > list.len() {
    return a.err("n must be between 1 and the length of the list");
  }
  fn build(list: &[Val], n: usize, out: &mut Vec<Val>) {
    if n == 1 {
      for item in list {
        out.push(Val::List(vec![item.clone()]));
      }
      return;
    }
    for i in 0..list.len() {
      let rest: Vec<Val> = list
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, v)| v.clone())
        .collect();
      let mut tails = Vec::new();
      build(&rest, n - 1, &mut tails);
      for t in tails {
        let mut perm = vec![list[i].clone()];
        if let Val::List(tail) = t {
          perm.extend(tail);
        }
        out.push(Val::List(perm));
      }
    }
  }
  let mut out = Vec::new();
  build(&list, n, &mut out);
  Val::List(out).to_lua(lua)
}

fn list_to_matrix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "v")?;
  let cnt = a.need_num("cnt")? as usize;
  if cnt == 0 {
    return a.err("cnt must be at least 1");
  }
  let dflt = a.val("dflt");
  let rows: Vec<Val> = list
    .chunks(cnt)
    .map(|chunk| {
      let mut row = chunk.to_vec();
      if let Some(fill) = &dflt {
        while row.len() < cnt {
          row.push(fill.clone());
        }
      }
      Val::List(row)
    })
    .collect();
  Val::List(rows).to_lua(lua)
}

fn flatten(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let list = items(a, "l")?;
  // One level only: a list of lists becomes a list of their contents.
  let mut out = Vec::new();
  for item in list {
    match item {
      Val::List(inner) => out.extend(inner),
      other => out.push(other),
    }
  }
  Val::List(out).to_lua(lua)
}

fn full_flatten(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  fn walk(v: &Val, out: &mut Vec<Val>) {
    match v {
      Val::List(items) => {
        for item in items {
          walk(item, out);
        }
      }
      other => out.push(other.clone()),
    }
  }
  let mut out = Vec::new();
  walk(&a.need_val("l")?, &mut out);
  Val::List(out).to_lua(lua)
}

fn set_union(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut out = items(a, "a")?;
  for v in items(a, "b")? {
    if !out.contains(&v) {
      out.push(v);
    }
  }
  Val::List(out).to_lua(lua)
}

fn set_difference(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let b = items(a, "b")?;
  let out: Vec<Val> = items(a, "a")?
    .into_iter()
    .filter(|v| !b.contains(v))
    .collect();
  Val::List(out).to_lua(lua)
}

fn set_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let b = items(a, "b")?;
  let out: Vec<Val> = items(a, "a")?
    .into_iter()
    .filter(|v| b.contains(v))
    .collect();
  Val::List(out).to_lua(lua)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      (
        "is_homogeneous",
        &["list", "depth"],
        is_homogeneous as PureFn,
      ),
      ("min_length", &["list"], min_length),
      ("max_length", &["list"], max_length),
      ("list_shape", &["v", "depth"], list_shape),
      ("in_list", &["val", "list"], in_list),
      ("select", &["list", "start", "end"], select),
      ("slice", &["list", "start", "end"], slice),
      ("last", &["list"], last),
      ("list_head", &["list", "to"], list_head),
      ("list_tail", &["list", "from"], list_tail),
      ("bselect", &["list", "index"], bselect),
      ("list_bset", &["indexset", "valuelist", "dflt"], list_bset),
      ("repeat", &["val", "n"], repeat_val),
      ("force_list", &["value", "n", "fill"], force_list),
      ("reverse", &["list"], reverse),
      ("list_rotate", &["list", "n"], list_rotate),
      ("shuffle", &["list", "seed"], shuffle),
      ("repeat_entries", &["list", "N", "exact"], repeat_entries),
      ("list_pad", &["list", "minlen", "fill"], list_pad),
      (
        "list_set",
        &["list", "indices", "values", "dflt", "minlen"],
        list_set,
      ),
      ("list_insert", &["list", "indices", "values"], list_insert),
      ("list_remove", &["list", "ind"], list_remove),
      (
        "list_remove_values",
        &["list", "values", "all"],
        list_remove_values,
      ),
      ("idx", &["list", "s", "e", "step"], idx),
      ("pair", &["list", "wrap"], pair),
      ("triplet", &["list", "wrap"], triplet),
      ("combinations", &["l", "n"], combinations),
      ("permutations", &["l", "n"], permutations),
      ("list_to_matrix", &["v", "cnt", "dflt"], list_to_matrix),
      ("flatten", &["l"], flatten),
      ("full_flatten", &["l"], full_flatten),
      ("set_union", &["a", "b"], set_union),
      ("set_difference", &["a", "b"], set_difference),
      ("set_intersection", &["a", "b"], set_intersection),
      // `list()` turns any value into a list, which is what `force_list`
      // already does with its defaults.
      ("list", &["value", "n", "fill"], force_list),
    ],
  )
}

#[cfg(test)]
mod tests {
  use crate::bosl::register_bosl;
  use mlua::Lua;

  fn eval<T: mlua::FromLua>(code: &str) -> T {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    lua
      .load(code)
      .eval()
      .unwrap_or_else(|e| panic!("evaluating {code}: {e}"))
  }

  #[test]
  fn select_indexes_from_zero_and_wraps_round_the_ends() {
    assert_eq!(eval::<f64>("return bosl.select({10,20,30}, 0)"), 10.0);
    assert_eq!(eval::<f64>("return bosl.select({10,20,30}, -1)"), 30.0);
    assert_eq!(eval::<f64>("return bosl.select({10,20,30}, 3)"), 10.0);
  }

  #[test]
  fn select_with_two_ends_takes_a_run_that_may_wrap() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.select({10,20,30,40}, 1, 2)"),
      vec![20.0, 30.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.select({10,20,30,40}, 3, 0)"),
      vec![40.0, 10.0]
    );
  }

  #[test]
  fn slice_clips_at_the_ends_rather_than_wrapping() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.slice({10,20,30,40}, 1, 2)"),
      vec![20.0, 30.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.slice({10,20,30,40}, 2)"),
      vec![30.0, 40.0]
    );
    assert!(eval::<Vec<f64>>("return bosl.slice({10,20}, 5, 9)").is_empty());
  }

  #[test]
  fn head_and_tail_split_a_list() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_head({1,2,3,4})"),
      vec![1.0, 2.0, 3.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_tail({1,2,3,4})"),
      vec![2.0, 3.0, 4.0]
    );
    assert_eq!(eval::<f64>("return bosl.last({1,2,3})"), 3.0);
  }

  /// `repeat` is a Lua keyword, so the call has to go through the bracket
  /// form. Everything else about it matches BOSL2.
  #[test]
  fn repeat_builds_flat_and_nested_lists() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl['repeat'](7, 3)"),
      vec![7.0, 7.0, 7.0]
    );
    let nested: Vec<Vec<f64>> = eval("return bosl['repeat'](1, {2, 3})");
    assert_eq!(nested, vec![vec![1.0; 3]; 2]);
  }

  #[test]
  fn force_list_wraps_a_bare_value_but_leaves_a_list_alone() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.force_list(5, 3)"),
      vec![5.0, 5.0, 5.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.force_list(5, 3, 0)"),
      vec![5.0, 0.0, 0.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.force_list({1,2})"),
      vec![1.0, 2.0]
    );
  }

  #[test]
  fn rotation_moves_entries_round_the_list() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_rotate({1,2,3,4}, 1)"),
      vec![2.0, 3.0, 4.0, 1.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_rotate({1,2,3,4}, -1)"),
      vec![4.0, 1.0, 2.0, 3.0]
    );
  }

  #[test]
  fn pairs_and_triplets_slide_along_the_list() {
    let p: Vec<Vec<f64>> = eval("return bosl.pair({1,2,3})");
    assert_eq!(p, vec![vec![1.0, 2.0], vec![2.0, 3.0]]);
    let p: Vec<Vec<f64>> = eval("return bosl.pair({1,2,3}, true)");
    assert_eq!(p, vec![vec![1.0, 2.0], vec![2.0, 3.0], vec![3.0, 1.0]]);
    let t: Vec<Vec<f64>> = eval("return bosl.triplet({1,2,3,4})");
    assert_eq!(t, vec![vec![1.0, 2.0, 3.0], vec![2.0, 3.0, 4.0]]);
  }

  #[test]
  fn insert_remove_and_set_edit_a_list() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_insert({1,2,4}, 2, 3)"),
      vec![1.0, 2.0, 3.0, 4.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_remove({1,2,3,4}, 1)"),
      vec![1.0, 3.0, 4.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_set({1,2,3}, 1, 9)"),
      vec![1.0, 9.0, 3.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_remove_values({1,2,2,3}, 2)"),
      vec![1.0, 2.0, 3.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_remove_values({1,2,2,3}, 2, true)"),
      vec![1.0, 3.0]
    );
  }

  #[test]
  fn boolean_selection_picks_the_flagged_entries() {
    assert_eq!(
      eval::<Vec<f64>>(
        "return bosl.bselect({1,2,3,4}, {true,false,true,false})"
      ),
      vec![1.0, 3.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.list_bset({true,false,true}, {5,6})"),
      vec![5.0, 0.0, 6.0]
    );
  }

  #[test]
  fn flatten_removes_one_level_and_full_flatten_removes_them_all() {
    let f: Vec<Vec<f64>> = eval("return bosl.flatten({{{1,2}},{{3,4}}})");
    assert_eq!(f, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    let f: Vec<f64> = eval("return bosl.full_flatten({{{1,2}},{{3,4}}})");
    assert_eq!(f, vec![1.0, 2.0, 3.0, 4.0]);
  }

  #[test]
  fn set_operations_treat_lists_as_sets() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.set_union({1,2}, {2,3})"),
      vec![1.0, 2.0, 3.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.set_difference({1,2,3}, {2})"),
      vec![1.0, 3.0]
    );
    assert_eq!(
      eval::<Vec<f64>>("return bosl.set_intersection({1,2,3}, {2,3,4})"),
      vec![2.0, 3.0]
    );
  }

  #[test]
  fn combinations_and_permutations_enumerate_the_choices() {
    let c: Vec<Vec<f64>> = eval("return bosl.combinations({1,2,3}, 2)");
    assert_eq!(c, vec![vec![1.0, 2.0], vec![1.0, 3.0], vec![2.0, 3.0]]);
    let p: Vec<Vec<f64>> = eval("return bosl.permutations({1,2,3}, 2)");
    assert_eq!(p.len(), 6);
  }

  #[test]
  fn list_to_matrix_groups_entries_into_rows() {
    let m: Vec<Vec<f64>> = eval("return bosl.list_to_matrix({1,2,3,4,5,6}, 3)");
    assert_eq!(m, vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]);
  }

  #[test]
  fn membership_and_shape_report_on_a_list() {
    assert!(eval::<bool>("return bosl.in_list(2, {1,2,3})"));
    assert!(!eval::<bool>("return bosl.in_list(9, {1,2,3})"));
    assert!(eval::<bool>("return bosl.is_homogeneous({{1,2},{3,4}})"));
    assert!(!eval::<bool>("return bosl.is_homogeneous({{1,2},{3}})"));
    assert_eq!(eval::<f64>("return bosl.min_length({{1},{2,3}})"), 1.0);
    assert_eq!(eval::<f64>("return bosl.max_length({{1},{2,3}})"), 2.0);
    let s: Vec<f64> = eval("return bosl.list_shape({{1,2,3},{4,5,6}})");
    assert_eq!(s, vec![2.0, 3.0]);
  }

  #[test]
  fn idx_lists_the_openscad_style_indices() {
    assert_eq!(
      eval::<Vec<f64>>("return bosl.idx({10,20,30})"),
      vec![0.0, 1.0, 2.0]
    );
  }

  #[test]
  fn repeat_entries_hits_the_requested_total() {
    let r: Vec<f64> = eval("return bosl.repeat_entries({1,2,3}, 6)");
    assert_eq!(r.len(), 6);
    let r: Vec<f64> = eval("return bosl.repeat_entries({1,2,3}, {1,2,3})");
    assert_eq!(r, vec![1.0, 2.0, 2.0, 3.0, 3.0, 3.0]);
  }

  #[test]
  fn a_seeded_shuffle_repeats_and_keeps_every_entry() {
    let same: bool = eval(
      "local a = bosl.shuffle({1,2,3,4,5}, 9)
       local b = bosl.shuffle({1,2,3,4,5}, 9)
       for i = 1, 5 do if a[i] ~= b[i] then return false end end
       return true",
    );
    assert!(same);
    let total: f64 = eval(
      "local s = 0
       for _, v in ipairs(bosl.shuffle({1,2,3,4,5}, 3)) do s = s + v end
       return s",
    );
    assert_eq!(total, 15.0);
  }
}
