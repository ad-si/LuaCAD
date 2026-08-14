//! BOSL2's `linalg.scad`: matrix construction, decomposition and solving.
//!
//! Matrices are lists of rows, the same as OpenSCAD's.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, matrix, num_list, register_all};

const EPS: f64 = 1e-12;

// ---------------------------------------------------------------------------
// Plain matrix arithmetic, shared by the functions below
// ---------------------------------------------------------------------------

pub fn transpose_of(m: &[Vec<f64>]) -> Vec<Vec<f64>> {
  if m.is_empty() {
    return vec![];
  }
  let cols = m.iter().map(|r| r.len()).max().unwrap_or(0);
  (0..cols)
    .map(|j| m.iter().map(|r| r.get(j).copied().unwrap_or(0.0)).collect())
    .collect()
}

pub fn mat_mul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
  if a.is_empty() || b.is_empty() || a[0].len() != b.len() {
    return None;
  }
  let cols = b[0].len();
  Some(
    a.iter()
      .map(|row| {
        (0..cols)
          .map(|j| row.iter().zip(b.iter()).map(|(x, br)| x * br[j]).sum())
          .collect()
      })
      .collect(),
  )
}

/// Reduce to row echelon form, returning the factor the determinant picks up.
///
/// Rows are swapped so the largest available pivot is used, which is what
/// keeps the elimination from amplifying rounding on nearly-singular input.
fn eliminate(m: &mut [Vec<f64>]) -> f64 {
  let rows = m.len();
  if rows == 0 {
    return 1.0;
  }
  let cols = m[0].len();
  let mut sign = 1.0;
  let mut pivot_row = 0;
  for col in 0..cols.min(rows) {
    let best = (pivot_row..rows)
      .max_by(|i, j| m[*i][col].abs().total_cmp(&m[*j][col].abs()));
    let Some(best) = best else { break };
    if m[best][col].abs() < EPS {
      continue;
    }
    if best != pivot_row {
      m.swap(best, pivot_row);
      sign = -sign;
    }
    for r in (pivot_row + 1)..rows {
      let factor = m[r][col] / m[pivot_row][col];
      if factor != 0.0 {
        let pivot: Vec<f64> = m[pivot_row][col..cols].to_vec();
        for (target, p) in m[r][col..cols].iter_mut().zip(pivot.iter()) {
          *target -= factor * p;
        }
      }
    }
    pivot_row += 1;
  }
  sign
}

pub fn determinant_of(m: &[Vec<f64>]) -> Option<f64> {
  let n = m.len();
  if n == 0 || m.iter().any(|r| r.len() != n) {
    return None;
  }
  let mut work: Vec<Vec<f64>> = m.to_vec();
  let sign = eliminate(&mut work);
  Some((0..n).map(|i| work[i][i]).product::<f64>() * sign)
}

/// Solve `Ax = b` for one or several right-hand sides.
fn solve(a: &[Vec<f64>], b: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
  let n = a.len();
  if n == 0 || a.iter().any(|r| r.len() != n) || b.len() != n {
    return None;
  }
  let width = b[0].len();
  // Solve by elimination on the matrix with the right-hand sides attached.
  let mut aug: Vec<Vec<f64>> = a
    .iter()
    .zip(b.iter())
    .map(|(ar, br)| ar.iter().chain(br.iter()).copied().collect())
    .collect();
  eliminate(&mut aug);
  if aug.iter().enumerate().any(|(i, row)| row[i].abs() < EPS) {
    return None;
  }
  let mut x = vec![vec![0.0; width]; n];
  for i in (0..n).rev() {
    for j in 0..width {
      let mut acc = aug[i][n + j];
      for k in (i + 1)..n {
        acc -= aug[i][k] * x[k][j];
      }
      x[i][j] = acc / aug[i][i];
    }
  }
  Some(x)
}

pub fn inverse_of(m: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
  let n = m.len();
  let identity: Vec<Vec<f64>> = (0..n)
    .map(|i| (0..n).map(|j| f64::from(i == j)).collect())
    .collect();
  solve(m, &identity)
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

fn is_matrix(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(m) = a.val("A").and_then(|v| v.as_matrix()) else {
    return Ok(LuaValue::Boolean(false));
  };
  if m.is_empty() {
    return Ok(LuaValue::Boolean(false));
  }
  let cols = m[0].len();
  let rectangular = m.iter().all(|r| r.len() == cols);
  let rows_ok = a.num("m").is_none_or(|n| m.len() == n as usize);
  let cols_ok = a.num("n").is_none_or(|n| cols == n as usize);
  let square_ok = !a.bool_or("square", false) || m.len() == cols;
  Ok(LuaValue::Boolean(
    rectangular && rows_ok && cols_ok && square_ok,
  ))
}

fn is_matrix_symmetric(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  let eps = a.num_or("eps", 1e-12);
  let n = m.len();
  let ok = m.iter().all(|r| r.len() == n)
    && (0..n).all(|i| (0..n).all(|j| (m[i][j] - m[j][i]).abs() <= eps));
  Ok(LuaValue::Boolean(ok))
}

fn is_rotation(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  let dim = a.num("dim").map(|d| d as usize);
  // A rotation matrix has orthonormal rows and preserves orientation, which
  // is exactly `M · Mᵀ = I` with a determinant of +1.
  let n = m.len();
  if n == 0 || m.iter().any(|r| r.len() != n) {
    return Ok(LuaValue::Boolean(false));
  }
  // An affine matrix carries a translation in its last column.
  let rot: Vec<Vec<f64>> = match dim {
    Some(d) if n == d + 1 => m[..d].iter().map(|r| r[..d].to_vec()).collect(),
    _ => m.clone(),
  };
  let k = rot.len();
  let prod = mat_mul(&rot, &transpose_of(&rot));
  let orthonormal = prod.is_some_and(|p| {
    (0..k).all(|i| (0..k).all(|j| (p[i][j] - f64::from(i == j)).abs() < 1e-9))
  });
  let det = determinant_of(&rot).unwrap_or(0.0);
  Ok(LuaValue::Boolean(orthonormal && (det - 1.0).abs() < 1e-9))
}

// ---------------------------------------------------------------------------
// Construction and reshaping
// ---------------------------------------------------------------------------

fn column(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(rows) = a.need_val("M")?.as_list().map(|s| s.to_vec()) else {
    return a.err("M must be a list");
  };
  let i = a.need_num("i")? as usize;
  let mut out = Vec::with_capacity(rows.len());
  for row in &rows {
    match row.as_list().and_then(|r| r.get(i)) {
      Some(v) => out.push(v.clone()),
      None => return a.err(format!("every row needs a column {i}")),
    }
  }
  Val::List(out).to_lua(lua)
}

fn submatrix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("M")?;
  // Each index set is a list of row or column numbers, counted from zero.
  let pick = |v: Option<Val>, len: usize| -> Vec<usize> {
    match v {
      Some(Val::Num(n)) => vec![n as usize],
      Some(Val::List(items)) => items
        .iter()
        .filter_map(|x| x.as_num())
        .map(|n| n as usize)
        .collect(),
      None => (0..len).collect(),
    }
  };
  let rows = pick(a.val("idx1"), m.len());
  let cols = pick(a.val("idx2"), m.first().map(|r| r.len()).unwrap_or(0));
  let out: Vec<Vec<f64>> = rows
    .iter()
    .filter_map(|i| m.get(*i))
    .map(|row| cols.iter().filter_map(|j| row.get(*j).copied()).collect())
    .collect();
  matrix(lua, &out)
}

fn ident(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")? as usize;
  let m: Vec<Vec<f64>> = (0..n)
    .map(|i| (0..n).map(|j| f64::from(i == j)).collect())
    .collect();
  matrix(lua, &m)
}

fn diagonal_matrix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let diag = a.need_vec("diag")?;
  let off = a.num_or("offdiag", 0.0);
  let n = diag.len();
  let m: Vec<Vec<f64>> = (0..n)
    .map(|i| (0..n).map(|j| if i == j { diag[i] } else { off }).collect())
    .collect();
  matrix(lua, &m)
}

fn transpose(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("M")?;
  matrix(lua, &transpose_of(&m))
}

fn outer_product(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let u = a.need_vec("u")?;
  let v = a.need_vec("v")?;
  let m: Vec<Vec<f64>> = u
    .iter()
    .map(|x| v.iter().map(|y| x * y).collect())
    .collect();
  matrix(lua, &m)
}

fn submatrix_set(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut m = a.need_matrix("M")?;
  let sub = a.need_matrix("A")?;
  let r0 = a.num_or("m", 0.0) as usize;
  let c0 = a.num_or("n", 0.0) as usize;
  for (i, row) in sub.iter().enumerate() {
    for (j, v) in row.iter().enumerate() {
      if let Some(target) = m.get_mut(r0 + i).and_then(|r| r.get_mut(c0 + j)) {
        *target = *v;
      }
    }
  }
  matrix(lua, &m)
}

fn hstack(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Either two or three matrices, or one list of them.
  let parts: Vec<Vec<Vec<f64>>> = match (a.val("M1"), a.val("M2")) {
    (Some(first), None) => {
      let Some(items) = first.as_list() else {
        return a.err("give the matrices to stack");
      };
      // A list of matrices, unless it is itself just one matrix.
      match items
        .iter()
        .map(|v| v.as_matrix())
        .collect::<Option<Vec<_>>>()
      {
        Some(ms) => ms,
        None => return a.err("give the matrices to stack"),
      }
    }
    _ => ["M1", "M2", "M3"]
      .iter()
      .filter_map(|n| a.val(n))
      .filter_map(|v| v.as_matrix())
      .collect(),
  };
  if parts.is_empty() {
    return a.err("give the matrices to stack");
  }
  let rows = parts[0].len();
  if parts.iter().any(|p| p.len() != rows) {
    return a.err("every matrix must have the same number of rows");
  }
  let out: Vec<Vec<f64>> = (0..rows)
    .map(|i| parts.iter().flat_map(|p| p[i].iter().copied()).collect())
    .collect();
  matrix(lua, &out)
}

fn block_matrix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(grid) = a.need_val("M")?.as_list().map(|s| s.to_vec()) else {
    return a.err("M must be a list of rows of blocks");
  };
  let mut out: Vec<Vec<f64>> = Vec::new();
  for band in &grid {
    let Some(blocks) = band.as_list() else {
      return a.err("each row of M must be a list of blocks");
    };
    let mats: Vec<Vec<Vec<f64>>> =
      blocks.iter().filter_map(|b| b.as_matrix()).collect();
    if mats.len() != blocks.len() {
      return a.err("every block must be a matrix");
    }
    let height = mats.first().map(|m| m.len()).unwrap_or(0);
    if mats.iter().any(|m| m.len() != height) {
      return a.err("the blocks in a row must have the same height");
    }
    for i in 0..height {
      out.push(mats.iter().flat_map(|m| m[i].iter().copied()).collect());
    }
  }
  matrix(lua, &out)
}

// ---------------------------------------------------------------------------
// Solving and decomposition
// ---------------------------------------------------------------------------

fn linear_solve(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  let b = a.need_val("b")?;
  // The right-hand side may be one vector or several columns of them.
  let (rhs, vector_input) = match b.as_vec() {
    Some(v) => (v.iter().map(|x| vec![*x]).collect::<Vec<_>>(), true),
    None => match b.as_matrix() {
      Some(m) => (m, false),
      None => return a.err("b must be a vector or a matrix"),
    },
  };

  // An over- or under-determined system is solved in the least-squares
  // sense, via the normal equations, which is what BOSL2 does too.
  let solution = if m.len() == m.first().map(|r| r.len()).unwrap_or(0) {
    solve(&m, &rhs)
  } else {
    let mt = transpose_of(&m);
    match (mat_mul(&mt, &m), mat_mul(&mt, &rhs)) {
      (Some(ata), Some(atb)) => solve(&ata, &atb),
      _ => None,
    }
  };
  let Some(x) = solution else {
    return Ok(LuaValue::Nil);
  };
  if vector_input {
    num_list(lua, &x.iter().map(|r| r[0]).collect::<Vec<_>>())
  } else {
    matrix(lua, &x)
  }
}

fn linear_solve3(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  linear_solve(lua, a)
}

fn matrix_inverse(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  match inverse_of(&m) {
    Some(inv) => matrix(lua, &inv),
    None => a.err("the matrix is singular and cannot be inverted"),
  }
}

fn rot_inverse(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("T")?;
  let n = m.len();
  if n == 0 || m.iter().any(|r| r.len() != n) {
    return a.err("T must be a square matrix");
  }
  // A rigid transform inverts without any elimination: the rotation part is
  // orthogonal, so transposing it undoes it, and the translation follows.
  let d = n - 1;
  let rot: Vec<Vec<f64>> = m[..d].iter().map(|r| r[..d].to_vec()).collect();
  let rt = transpose_of(&rot);
  let translate: Vec<f64> = (0..d).map(|i| m[i][d]).collect();
  let mut out: Vec<Vec<f64>> = (0..n)
    .map(|i| {
      if i < d {
        let mut row = rt[i].clone();
        row.push(
          -rt[i]
            .iter()
            .zip(translate.iter())
            .map(|(x, y)| x * y)
            .sum::<f64>(),
        );
        row
      } else {
        (0..n).map(|j| f64::from(j == d)).collect()
      }
    })
    .collect();
  out.truncate(n);
  matrix(lua, &out)
}

/// A basis for the null space of `A`, as a list of vectors.
fn null_space(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  let eps = a.num_or("eps", 1e-12);
  if m.is_empty() {
    return Val::List(vec![]).to_lua(lua);
  }
  let cols = m[0].len();
  let mut work = m.clone();
  eliminate(&mut work);

  // Reduce each pivot row so the pivot is 1 and stands alone in its column;
  // the free columns then read straight off as basis vectors.
  let mut pivot_of_row: Vec<Option<usize>> = Vec::with_capacity(work.len());
  for row in &work {
    pivot_of_row.push(row.iter().position(|v| v.abs() > eps));
  }
  for r in 0..work.len() {
    if let Some(p) = pivot_of_row[r] {
      let d = work[r][p];
      for cell in work[r].iter_mut().take(cols) {
        *cell /= d;
      }
      // Clear the pivot's column in every other row, so each pivot stands
      // alone and the free columns can be read straight off.
      let pivot_row: Vec<f64> = work[r][..cols].to_vec();
      for (other, row) in work.iter_mut().enumerate() {
        if other == r || row[p].abs() <= eps {
          continue;
        }
        let factor = row[p];
        for (cell, pv) in row[..cols].iter_mut().zip(pivot_row.iter()) {
          *cell -= factor * pv;
        }
      }
    }
  }

  let pivots: Vec<usize> = pivot_of_row.iter().flatten().copied().collect();
  let free: Vec<usize> = (0..cols).filter(|c| !pivots.contains(c)).collect();
  let basis: Vec<Vec<f64>> = free
    .iter()
    .map(|f| {
      let mut v = vec![0.0; cols];
      v[*f] = 1.0;
      for (r, p) in pivot_of_row.iter().enumerate() {
        if let Some(p) = p {
          v[*p] = -work[r][*f];
        }
      }
      v
    })
    .collect();
  matrix(lua, &basis)
}

/// The QR decomposition, by Gram–Schmidt with reorthogonalisation.
fn qr_factor(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  if m.is_empty() {
    return a.err("A cannot be empty");
  }
  let rows = m.len();
  let cols = m[0].len();
  let mut q: Vec<Vec<f64>> = vec![vec![0.0; cols.min(rows)]; rows];
  let mut r = vec![vec![0.0; cols]; cols.min(rows)];

  for j in 0..cols.min(rows) {
    let mut v: Vec<f64> = (0..rows).map(|i| m[i][j]).collect();
    // Subtracting the projections twice removes the loss of orthogonality
    // that a single pass leaves on ill-conditioned columns.
    for _ in 0..2 {
      for k in 0..j {
        let dot: f64 = (0..rows).map(|i| q[i][k] * v[i]).sum();
        r[k][j] += dot;
        for (i, vi) in v.iter_mut().enumerate() {
          *vi -= dot * q[i][k];
        }
      }
    }
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    r[j][j] = norm;
    if norm > EPS {
      for (i, vi) in v.iter().enumerate() {
        q[i][j] = vi / norm;
      }
    }
  }
  // The remaining entries of R come from projecting the later columns.
  for j in cols.min(rows)..cols {
    for k in 0..cols.min(rows) {
      r[k][j] = (0..rows).map(|i| q[i][k] * m[i][j]).sum();
    }
  }

  Val::list([
    Val::list(q.iter().map(|row| Val::vec(row.iter().copied()))),
    Val::list(r.iter().map(|row| Val::vec(row.iter().copied()))),
  ])
  .to_lua(lua)
}

fn back_substitute(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.need_matrix("R")?;
  let b = a.need_vec("b")?;
  let transposed = a.bool_or("transpose", false);
  let n = r.len();
  if n == 0 || b.len() != n {
    return a.err("R and b must agree in size");
  }
  let mut x = vec![0.0; n];
  if transposed {
    // A transposed upper triangle is lower triangular, so it solves forwards.
    for i in 0..n {
      let mut acc = b[i];
      for k in 0..i {
        acc -= r[k][i] * x[k];
      }
      if r[i][i].abs() < EPS {
        return a.err("R is singular");
      }
      x[i] = acc / r[i][i];
    }
  } else {
    for i in (0..n).rev() {
      let mut acc = b[i];
      for k in (i + 1)..n {
        acc -= r[i][k] * x[k];
      }
      if r[i][i].abs() < EPS {
        return a.err("R is singular");
      }
      x[i] = acc / r[i][i];
    }
  }
  num_list(lua, &x)
}

/// The Cholesky factor `L` with `L·Lᵀ = A`, or nothing if `A` is not
/// positive definite.
fn cholesky(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  let n = m.len();
  if n == 0 || m.iter().any(|r| r.len() != n) {
    return a.err("A must be square");
  }
  let mut l = vec![vec![0.0; n]; n];
  for i in 0..n {
    for j in 0..=i {
      let mut acc = m[i][j];
      for (li, lj) in l[i][..j].iter().zip(l[j][..j].iter()) {
        acc -= li * lj;
      }
      if i == j {
        if acc <= 0.0 {
          return Ok(LuaValue::Nil);
        }
        l[i][j] = acc.sqrt();
      } else {
        l[i][j] = acc / l[j][j];
      }
    }
  }
  matrix(lua, &l)
}

// ---------------------------------------------------------------------------
// Determinants and norms
// ---------------------------------------------------------------------------

fn det_n(n: usize) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |_lua, a| {
    let m = a.need_matrix("M")?;
    if m.len() != n || m.iter().any(|r| r.len() != n) {
      return a.err(format!("M must be a {n}×{n} matrix"));
    }
    match determinant_of(&m) {
      Some(d) => Ok(LuaValue::Number(d)),
      None => a.err("M must be square"),
    }
  }
}

fn determinant(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("M")?;
  match determinant_of(&m) {
    Some(d) => Ok(LuaValue::Number(d)),
    None => a.err("M must be square"),
  }
}

fn norm_fro(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  fn total(v: &Val) -> f64 {
    match v {
      Val::Num(n) => n * n,
      Val::List(items) => items.iter().map(total).sum(),
    }
  }
  Ok(LuaValue::Number(total(&a.need_val("A")?).sqrt()))
}

fn matrix_trace(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("M")?;
  let n = m.len().min(m.first().map(|r| r.len()).unwrap_or(0));
  Ok(LuaValue::Number((0..n).map(|i| m[i][i]).sum()))
}

fn echo_matrix(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("A")?;
  for row in &m {
    let cells: Vec<String> = row.iter().map(|v| format!("{v:>10.4}")).collect();
    println!("[{}]", cells.join(" "));
  }
  Ok(LuaValue::Nil)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("is_matrix", &["A", "m", "n", "square"], is_matrix as PureFn),
      ("is_matrix_symmetric", &["A", "eps"], is_matrix_symmetric),
      ("is_rotation", &["A", "dim", "centered"], is_rotation),
      ("echo_matrix", &["A"], echo_matrix),
      ("column", &["M", "i"], column),
      ("submatrix", &["M", "idx1", "idx2"], submatrix),
      ("ident", &["n"], ident),
      ("diagonal_matrix", &["diag", "offdiag"], diagonal_matrix),
      ("transpose", &["M", "reverse"], transpose),
      ("outer_product", &["u", "v"], outer_product),
      ("submatrix_set", &["M", "A", "m", "n"], submatrix_set),
      ("hstack", &["M1", "M2", "M3"], hstack),
      ("block_matrix", &["M"], block_matrix),
      ("linear_solve", &["A", "b", "pivot"], linear_solve),
      ("linear_solve3", &["A", "b"], linear_solve3),
      ("matrix_inverse", &["A"], matrix_inverse),
      ("rot_inverse", &["T"], rot_inverse),
      ("null_space", &["A", "eps"], null_space),
      ("qr_factor", &["A", "pivot"], qr_factor),
      ("back_substitute", &["R", "b", "transpose"], back_substitute),
      ("cholesky", &["A"], cholesky),
      ("determinant", &["M"], determinant),
      ("norm_fro", &["A"], norm_fro),
      ("matrix_trace", &["M"], matrix_trace),
    ],
  )?;

  for (name, n) in [("det2", 2usize), ("det3", 3), ("det4", 4)] {
    let f = det_n(n);
    let func = lua.create_function(move |lua, args: mlua::MultiValue| {
      let parsed = Args::parse_pure(name, &["M"], &args)?;
      f(lua, &parsed)
    })?;
    bosl.set(name, func)?;
  }
  Ok(())
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

  fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
  }

  #[test]
  fn identity_and_diagonal_matrices_are_built_as_asked() {
    let m: Vec<Vec<f64>> = eval("return bosl.ident(3)");
    assert_eq!(
      m,
      vec![
        vec![1.0, 0.0, 0.0],
        vec![0.0, 1.0, 0.0],
        vec![0.0, 0.0, 1.0]
      ]
    );
    let m: Vec<Vec<f64>> = eval("return bosl.diagonal_matrix({2,3})");
    assert_eq!(m, vec![vec![2.0, 0.0], vec![0.0, 3.0]]);
  }

  #[test]
  fn transpose_swaps_rows_and_columns() {
    let m: Vec<Vec<f64>> = eval("return bosl.transpose({{1,2,3},{4,5,6}})");
    assert_eq!(m, vec![vec![1.0, 4.0], vec![2.0, 5.0], vec![3.0, 6.0]]);
  }

  #[test]
  fn determinants_match_the_hand_computed_values() {
    assert_eq!(eval::<f64>("return bosl.det2({{1,2},{3,4}})"), -2.0);
    let d: f64 = eval("return bosl.det3({{2,0,0},{0,3,0},{0,0,4}})");
    assert!((d - 24.0).abs() < 1e-9);
    let d: f64 = eval("return bosl.determinant({{1,2},{2,4}})");
    assert!(d.abs() < 1e-9, "a singular matrix has determinant zero");
  }

  #[test]
  fn inverting_a_matrix_undoes_it() {
    let p: Vec<Vec<f64>> = eval(
      "local A = {{4,7},{2,6}}
       local Ai = bosl.matrix_inverse(A)
       local out = {}
       for i = 1, 2 do
         out[i] = {}
         for j = 1, 2 do
           local s = 0
           for k = 1, 2 do s = s + A[i][k] * Ai[k][j] end
           out[i][j] = s
         end
       end
       return out",
    );
    assert!(
      close(&p[0], &[1.0, 0.0]) && close(&p[1], &[0.0, 1.0]),
      "{p:?}"
    );
  }

  #[test]
  fn a_singular_matrix_cannot_be_inverted() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.matrix_inverse({{1,2},{2,4}})")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("singular"), "{err}");
  }

  #[test]
  fn linear_solve_finds_the_solution_of_a_square_system() {
    let x: Vec<f64> = eval("return bosl.linear_solve({{2,1},{1,3}}, {5,10})");
    assert!(close(&x, &[1.0, 3.0]), "{x:?}");
  }

  #[test]
  fn an_overdetermined_system_is_solved_in_the_least_squares_sense() {
    // Three points on the line y = 2x, fitted through the origin.
    let x: Vec<f64> = eval("return bosl.linear_solve({{1},{2},{3}}, {2,4,6})");
    assert!((x[0] - 2.0).abs() < 1e-9, "{x:?}");
  }

  #[test]
  fn qr_factorisation_rebuilds_the_original_matrix() {
    let p: Vec<Vec<f64>> = eval(
      "local A = {{12,-51},{6,167},{-4,24}}
       local qr = bosl.qr_factor(A)
       local Q, R = qr[1], qr[2]
       local out = {}
       for i = 1, 3 do
         out[i] = {}
         for j = 1, 2 do
           local s = 0
           for k = 1, 2 do s = s + Q[i][k] * R[k][j] end
           out[i][j] = s
         end
       end
       return out",
    );
    assert!(close(&p[0], &[12.0, -51.0]), "{p:?}");
    assert!(close(&p[1], &[6.0, 167.0]), "{p:?}");
    assert!(close(&p[2], &[-4.0, 24.0]), "{p:?}");
  }

  #[test]
  fn cholesky_factors_a_positive_definite_matrix() {
    let p: Vec<Vec<f64>> = eval(
      "local A = {{4,2},{2,3}}
       local L = bosl.cholesky(A)
       local out = {}
       for i = 1, 2 do
         out[i] = {}
         for j = 1, 2 do
           local s = 0
           for k = 1, 2 do s = s + L[i][k] * L[j][k] end
           out[i][j] = s
         end
       end
       return out",
    );
    assert!(
      close(&p[0], &[4.0, 2.0]) && close(&p[1], &[2.0, 3.0]),
      "{p:?}"
    );
  }

  #[test]
  fn cholesky_declines_a_matrix_that_is_not_positive_definite() {
    let nil: Option<Vec<Vec<f64>>> =
      eval("return bosl.cholesky({{1,2},{2,1}})");
    assert!(nil.is_none());
  }

  #[test]
  fn back_substitution_solves_a_triangular_system() {
    let x: Vec<f64> = eval("return bosl.back_substitute({{2,1},{0,3}}, {5,9})");
    assert!(close(&x, &[1.0, 3.0]), "{x:?}");
  }

  #[test]
  fn the_null_space_spans_the_directions_the_matrix_flattens() {
    let basis: Vec<Vec<f64>> = eval("return bosl.null_space({{1,1},{2,2}})");
    assert_eq!(basis.len(), 1);
    // Whatever scaling comes out, the vector must satisfy x + y = 0.
    assert!((basis[0][0] + basis[0][1]).abs() < 1e-9, "{basis:?}");
  }

  #[test]
  fn rot_inverse_undoes_a_rigid_transform() {
    let p: Vec<Vec<f64>> = eval(
      "local T = {{0,-1,0,5},{1,0,0,7},{0,0,1,0},{0,0,0,1}}
       local Ti = bosl.rot_inverse(T)
       local out = {}
       for i = 1, 4 do
         out[i] = {}
         for j = 1, 4 do
           local s = 0
           for k = 1, 4 do s = s + T[i][k] * Ti[k][j] end
           out[i][j] = s
         end
       end
       return out",
    );
    for (i, row) in p.iter().enumerate() {
      for (j, v) in row.iter().enumerate() {
        assert!((v - f64::from(i == j)).abs() < 1e-9, "{p:?}");
      }
    }
  }

  #[test]
  fn matrix_predicates_recognise_their_shapes() {
    assert!(eval::<bool>("return bosl.is_matrix({{1,2},{3,4}})"));
    assert!(!eval::<bool>("return bosl.is_matrix({{1,2},{3}})"));
    assert!(eval::<bool>("return bosl.is_matrix({{1,2},{3,4}}, 2, 2)"));
    assert!(eval::<bool>(
      "return bosl.is_matrix_symmetric({{1,2},{2,1}})"
    ));
    assert!(!eval::<bool>(
      "return bosl.is_matrix_symmetric({{1,2},{3,1}})"
    ));
    assert!(eval::<bool>("return bosl.is_rotation({{0,-1},{1,0}})"));
    assert!(!eval::<bool>("return bosl.is_rotation({{2,0},{0,2}})"));
  }

  #[test]
  fn norms_traces_and_columns_read_off_a_matrix() {
    assert_eq!(eval::<f64>("return bosl.norm_fro({{3,4}})"), 5.0);
    assert_eq!(eval::<f64>("return bosl.matrix_trace({{1,9},{9,4}})"), 5.0);
    assert_eq!(
      eval::<Vec<f64>>("return bosl.column({{1,2},{3,4}}, 1)"),
      vec![2.0, 4.0]
    );
  }

  #[test]
  fn matrices_stack_side_by_side_and_in_blocks() {
    let m: Vec<Vec<f64>> = eval("return bosl.hstack({{1},{2}}, {{3},{4}})");
    assert_eq!(m, vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
    let m: Vec<Vec<f64>> =
      eval("return bosl.block_matrix({{ {{1}}, {{2}} }, { {{3}}, {{4}} }})");
    assert_eq!(m, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
  }

  #[test]
  fn submatrix_picks_out_the_rows_and_columns_asked_for() {
    let m: Vec<Vec<f64>> =
      eval("return bosl.submatrix({{1,2,3},{4,5,6},{7,8,9}}, {0,1}, {1,2})");
    assert_eq!(m, vec![vec![2.0, 3.0], vec![5.0, 6.0]]);
  }
}
