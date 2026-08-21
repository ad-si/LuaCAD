//! Runtime values and their operational semantics.

use openrscad_syntax::ast::{Arg, BinOp, Expr, ListElem, Param, UnOp};
use std::rc::Rc;

/// A runtime value.
#[derive(Clone)]
pub enum Value {
    Undef,
    Bool(bool),
    Number(f64),
    Str(String),
    /// A list. `Rc`-backed so cloning a value (e.g. a variable lookup of a
    /// large array) is O(1) — critical for mesh-generating scripts.
    Vector(Rc<Vec<Value>>),
    /// A numeric range `[start : step : end]`.
    Range {
        start: f64,
        step: f64,
        end: f64,
    },
    /// A function value: parameters, body, and the lexical environment it
    /// closed over at definition time.
    Function(Rc<crate::FnClosure>),
}

// Manual impls: `Function` captures a scope chain (with reference cycles), so
// deriving Debug/PartialEq would recurse. Compare functions by identity.
impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Undef => write!(f, "undef"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Number(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Vector(v) => f.debug_list().entries(v.iter()).finish(),
            Value::Range { start, step, end } => write!(f, "[{start}:{step}:{end}]"),
            Value::Function(_) => write!(f, "<function>"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        value_eq(self, other)
    }
}

/// Construct a list value from an owned `Vec`.
pub fn vector(v: Vec<Value>) -> Value {
    Value::Vector(Rc::new(v))
}

impl Value {
    /// OpenSCAD truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undef => false,
            Value::Bool(b) => *b,
            // OpenSCAD treats every number except zero as true, including NaN.
            Value::Number(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Vector(v) => !v.is_empty(),
            Value::Range { .. } => true,
            Value::Function(_) => true,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Interpret this value as a 3-component vector, broadcasting a scalar to
    /// all three components and zero-filling short vectors.
    pub fn as_vec3(&self) -> Option<[f64; 3]> {
        match self {
            Value::Number(n) => Some([*n, *n, *n]),
            Value::Vector(v) => {
                let get = |i: usize| v.get(i).and_then(Value::as_number).unwrap_or(0.0);
                if v.iter().all(|e| matches!(e, Value::Number(_))) || !v.is_empty() {
                    Some([get(0), get(1), get(2)])
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The display representation used by `echo` and inside vectors: strings
    /// are quoted, matching OpenSCAD.
    pub fn repr(&self) -> String {
        match self {
            Value::Undef => "undef".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => format_number(*n),
            Value::Str(s) => format!("\"{}\"", escape_string(s)),
            Value::Vector(v) => {
                let parts: Vec<String> = v.iter().map(|e| e.repr()).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Range { start, step, end } => format!(
                "[{} : {} : {}]",
                format_number(*start),
                format_number(*step),
                format_number(*end)
            ),
            Value::Function(f) => format_function(&f.params, &f.body),
        }
    }

    /// The `str()` representation: a top-level string is emitted raw (no
    /// quotes); everything else uses [`Value::repr`] (so vector elements are
    /// still quoted).
    pub fn to_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            other => other.repr(),
        }
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// Serialize a function value to OpenSCAD's textual form — `function(params)
/// body`, with the body expression pretty-printed. Binary and ternary
/// sub-expressions are parenthesized; unary ops, calls, indexing, member access,
/// vectors, ranges, `let`, and literals are not. Matches OpenSCAD 2024.12, which
/// BOSL2's `test_fnliterals` suite asserts on via `str(<function literal>)`.
pub(crate) fn format_function(params: &[Param], body: &Expr) -> String {
    format!("function({}) {}", fmt_params(params), fmt_expr(body))
}

fn fmt_params(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| match &p.default {
            Some(d) => format!("{} = {}", p.name, fmt_expr(d)),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_args(args: &[Arg]) -> String {
    args.iter()
        .map(|a| match &a.name {
            Some(n) => format!("{} = {}", n, fmt_expr(&a.value)),
            None => fmt_expr(&a.value),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_bindings(bindings: &[(String, Expr)]) -> String {
    bindings
        .iter()
        .map(|(n, e)| format!("{} = {}", n, fmt_expr(e)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_listelem(el: &ListElem) -> String {
    match el {
        ListElem::Item(e) => fmt_expr(e),
        ListElem::Each(inner) => format!("each {}", fmt_listelem(inner)),
        // A comprehension body is itself wrapped in parens by OpenSCAD, so a
        // binary body ends up double-parenthesized (e.g. `for(i=r) ((i * 2))`).
        ListElem::For { bindings, body } => {
            format!("for({}) ({})", fmt_bindings(bindings), fmt_listelem(body))
        }
        ListElem::CFor {
            init,
            cond,
            update,
            body,
        } => format!(
            "for({}; {}; {}) ({})",
            fmt_bindings(init),
            fmt_expr(cond),
            fmt_bindings(update),
            fmt_listelem(body)
        ),
        ListElem::If { cond, then, els } => match els {
            Some(e) => format!(
                "if({}) ({}) else ({})",
                fmt_expr(cond),
                fmt_listelem(then),
                fmt_listelem(e)
            ),
            None => format!("if({}) ({})", fmt_expr(cond), fmt_listelem(then)),
        },
        ListElem::Let { bindings, body } => {
            format!("let({}) {}", fmt_bindings(bindings), fmt_listelem(body))
        }
    }
}

fn fmt_expr(e: &Expr) -> String {
    match e {
        Expr::Number(n) => format_number(*n),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => format!("\"{}\"", escape_string(s)),
        Expr::Undef => "undef".to_string(),
        Expr::Ident(n) => n.clone(),
        Expr::Vector(elems) => format!(
            "[{}]",
            elems
                .iter()
                .map(fmt_listelem)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Range { start, step, end } => match step {
            Some(s) => format!(
                "[{} : {} : {}]",
                fmt_expr(start),
                fmt_expr(s),
                fmt_expr(end)
            ),
            None => format!("[{} : {}]", fmt_expr(start), fmt_expr(end)),
        },
        // OpenSCAD drops a unary plus and never parenthesizes a unary operand.
        Expr::Unary { op, expr } => match op {
            UnOp::Neg => format!("-{}", fmt_expr(expr)),
            UnOp::Not => format!("!{}", fmt_expr(expr)),
            UnOp::Pos => fmt_expr(expr),
        },
        Expr::Binary { op, lhs, rhs } => {
            format!("({} {} {})", fmt_expr(lhs), binop_str(*op), fmt_expr(rhs))
        }
        Expr::Ternary { cond, then, els } => {
            format!(
                "({} ? {} : {})",
                fmt_expr(cond),
                fmt_expr(then),
                fmt_expr(els)
            )
        }
        Expr::Index { base, index } => format!("{}[{}]", fmt_expr(base), fmt_expr(index)),
        Expr::Member { base, field } => format!("{}.{}", fmt_expr(base), field),
        Expr::Call { name, args } => format!("{}({})", name, fmt_args(args)),
        Expr::CallValue { callee, args } => format!("{}({})", fmt_expr(callee), fmt_args(args)),
        Expr::Let { bindings, body } => {
            format!("let({}) {}", fmt_bindings(bindings), fmt_expr(body))
        }
        Expr::FunctionLiteral { params, body } => format_function(params, body),
        Expr::Echo { args, body } => format!("echo({}) {}", fmt_args(args), fmt_expr(body)),
        Expr::Assert { args, body } => format!("assert({}) {}", fmt_args(args), fmt_expr(body)),
    }
}

fn binop_str(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        Mod => "%",
        Pow => "^",
        Eq => "==",
        Ne => "!=",
        Lt => "<",
        Le => "<=",
        Gt => ">",
        Ge => ">=",
        And => "&&",
        Or => "||",
    }
}

/// Format a number like OpenSCAD: 6 significant digits (`%g`-style), with the
/// exponent written without leading zeros (`1e+6`, `1.234e-6`) and exact ties
/// rounded half-away-from-zero (matching OpenSCAD, not Rust's half-to-even).
pub fn format_number(n: f64) -> String {
    if n.is_nan() {
        return "nan".to_string();
    }
    if n.is_infinite() {
        return if n < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if n == 0.0 {
        return "0".to_string();
    }

    const P: usize = 6;
    let neg = n < 0.0;

    // Extract the exact significant digits of |n| and the exponent of the first.
    let sci = format!("{:e}", n.abs()); // e.g. "1.250025e9"
    let (mant, exp_s) = sci.split_once('e').unwrap();
    let mut exp: i32 = exp_s.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mant
        .bytes()
        .filter(|b| *b != b'.')
        .map(|b| b - b'0')
        .collect();

    // Round to P significant digits, half-away-from-zero.
    if digits.len() > P {
        let round_up = digits[P] >= 5;
        digits.truncate(P);
        if round_up {
            let mut i = P;
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    exp += 1;
                    digits.truncate(P);
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    } else {
        digits.resize(P, 0);
    }

    let sign = if neg { "-" } else { "" };
    let digit_ch = |d: &u8| (b'0' + d) as char;

    if exp < -4 || exp >= P as i32 {
        // scientific
        let mut frac: String = digits[1..].iter().map(digit_ch).collect();
        while frac.ends_with('0') {
            frac.pop();
        }
        let mant_str = if frac.is_empty() {
            digits[0].to_string()
        } else {
            format!("{}.{}", digits[0], frac)
        };
        let esign = if exp < 0 { "-" } else { "+" };
        format!("{sign}{mant_str}e{esign}{}", exp.abs())
    } else if exp >= 0 {
        // fixed with an integer part of exp+1 digits
        let ip = exp as usize + 1;
        let mut int_part: String = digits.iter().take(ip.min(P)).map(digit_ch).collect();
        for _ in P..ip {
            int_part.push('0');
        }
        let mut frac: String = digits.iter().skip(ip).map(digit_ch).collect();
        while frac.ends_with('0') {
            frac.pop();
        }
        if frac.is_empty() {
            format!("{sign}{int_part}")
        } else {
            format!("{sign}{int_part}.{frac}")
        }
    } else {
        // 0.00…digits  (exp in [-4, -1])
        let mut frac = "0".repeat((-exp - 1) as usize);
        frac.extend(digits.iter().map(digit_ch));
        while frac.ends_with('0') {
            frac.pop();
        }
        format!("{sign}0.{frac}")
    }
}

/// Apply a unary operator.
pub fn unary(op: UnOp, v: Value) -> Value {
    match op {
        UnOp::Pos => v,
        UnOp::Neg => match v {
            Value::Number(n) => Value::Number(-n),
            Value::Vector(xs) => vector(xs.iter().map(|e| unary(UnOp::Neg, e.clone())).collect()),
            _ => Value::Undef,
        },
        UnOp::Not => Value::Bool(!v.truthy()),
    }
}

/// Apply a binary operator with OpenSCAD semantics (undef propagation on type
/// mismatch, elementwise vector arithmetic, vector dot product).
pub fn binary(op: BinOp, l: Value, r: Value) -> Value {
    use Value::*;
    match op {
        BinOp::And => return Bool(l.truthy() && r.truthy()),
        BinOp::Or => return Bool(l.truthy() || r.truthy()),
        BinOp::Eq => return Bool(value_eq(&l, &r)),
        BinOp::Ne => return Bool(!value_eq(&l, &r)),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => return compare(op, &l, &r),
        _ => {}
    }

    match (op, l, r) {
        // numeric
        (BinOp::Add, Number(a), Number(b)) => Number(a + b),
        (BinOp::Sub, Number(a), Number(b)) => Number(a - b),
        (BinOp::Mul, Number(a), Number(b)) => Number(a * b),
        (BinOp::Div, Number(a), Number(b)) => Number(a / b),
        (BinOp::Mod, Number(a), Number(b)) => Number(a % b),
        (BinOp::Pow, Number(a), Number(b)) => Number(a.powf(b)),

        // vector +/- vector (elementwise, equal length)
        (BinOp::Add, Vector(a), Vector(b)) if a.len() == b.len() => {
            vector(zip_map(&a, &b, BinOp::Add))
        }
        (BinOp::Sub, Vector(a), Vector(b)) if a.len() == b.len() => {
            vector(zip_map(&a, &b, BinOp::Sub))
        }

        // scalar * vector, vector * scalar
        (BinOp::Mul, Number(s), Vector(v)) | (BinOp::Mul, Vector(v), Number(s)) => vector(
            v.iter()
                .map(|e| binary(BinOp::Mul, Number(s), e.clone()))
                .collect(),
        ),
        // list * list — dot product / matrix·vector / vector·matrix / matrix·matrix,
        // matching OpenSCAD's linear-algebra `*` (see `mul_lists`).
        (BinOp::Mul, Vector(a), Vector(b)) => mul_lists(&a, &b),
        // vector / scalar
        (BinOp::Div, Vector(v), Number(s)) => vector(
            v.iter()
                .map(|e| binary(BinOp::Div, e.clone(), Number(s)))
                .collect(),
        ),

        _ => Undef,
    }
}

/// OpenSCAD's `*` between two lists. A list whose first element is itself a list
/// is treated as a matrix (rows); otherwise as a vector of numbers:
///
/// * vector · vector      → scalar dot product (equal length),
/// * matrix (m×n) · vector (n) → vector (m),
/// * vector (n) · matrix (n×r) → vector (r)  (row-vector times matrix),
/// * matrix (m×n) · matrix (n×r) → matrix (m×r).
///
/// Any dimension mismatch or non-numeric entry yields `undef`, as in OpenSCAD.
fn mul_lists(a: &[Value], b: &[Value]) -> Value {
    let a_mat = matches!(a.first(), Some(Value::Vector(_)));
    let b_mat = matches!(b.first(), Some(Value::Vector(_)));
    match (a_mat, b_mat) {
        (false, false) => dot(a, b),
        (true, false) => {
            // matrix · vector: each row dotted with b.
            let rows: Option<Vec<Value>> = a
                .iter()
                .map(|row| match row {
                    Value::Vector(r) => match dot(r, b) {
                        Value::Undef => None,
                        v => Some(v),
                    },
                    _ => None,
                })
                .collect();
            rows.map(vector).unwrap_or(Value::Undef)
        }
        (false, true) => {
            // vector · matrix: a[i] weights row b[i]; requires a.len()==b.len().
            let bm = as_matrix(b);
            let (Some(bm), true) = (bm, a.len() == b.len()) else {
                return Value::Undef;
            };
            let cols = bm[0].len();
            let mut out = vec![0.0; cols];
            for (i, bi) in bm.iter().enumerate() {
                let Some(ai) = a[i].as_number() else {
                    return Value::Undef;
                };
                if bi.len() != cols {
                    return Value::Undef;
                }
                for j in 0..cols {
                    out[j] += ai * bi[j];
                }
            }
            vector(out.into_iter().map(Value::Number).collect())
        }
        (true, true) => {
            // matrix · matrix.
            let (Some(am), Some(bm)) = (as_matrix(a), as_matrix(b)) else {
                return Value::Undef;
            };
            let inner = bm.len();
            let cols = bm[0].len();
            // Each row of `a` must have length == number of rows of `b`.
            if am.iter().any(|r| r.len() != inner) || bm.iter().any(|r| r.len() != cols) {
                return Value::Undef;
            }
            let mut out = Vec::with_capacity(am.len());
            for ai in &am {
                let mut row = vec![0.0; cols];
                for k in 0..inner {
                    for j in 0..cols {
                        row[j] += ai[k] * bm[k][j];
                    }
                }
                out.push(vector(row.into_iter().map(Value::Number).collect()));
            }
            vector(out)
        }
    }
}

/// Dot product of two numeric vectors; `undef` on length mismatch or non-numbers.
fn dot(a: &[Value], b: &[Value]) -> Value {
    if a.is_empty() || a.len() != b.len() {
        return Value::Undef;
    }
    let mut sum = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        match (x.as_number(), y.as_number()) {
            (Some(x), Some(y)) => sum += x * y,
            _ => return Value::Undef,
        }
    }
    Value::Number(sum)
}

/// Interpret a list of numeric rows as a matrix of `f64`s (None if ragged/
/// non-numeric/empty).
fn as_matrix(rows: &[Value]) -> Option<Vec<Vec<f64>>> {
    if rows.is_empty() {
        return None;
    }
    rows.iter()
        .map(|row| match row {
            Value::Vector(r) if !r.is_empty() => r.iter().map(|e| e.as_number()).collect(),
            _ => None,
        })
        .collect()
}

fn zip_map(a: &[Value], b: &[Value], op: BinOp) -> Vec<Value> {
    a.iter()
        .zip(b)
        .map(|(x, y)| binary(op, x.clone(), y.clone()))
        .collect()
}

pub fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Undef, Value::Undef) => true,
        (Value::Vector(x), Value::Vector(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| value_eq(p, q))
        }
        (
            Value::Range {
                start: a,
                step: b,
                end: c,
            },
            Value::Range {
                start: d,
                step: e,
                end: f,
            },
        ) => a == d && b == e && c == f,
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn compare(op: BinOp, a: &Value, b: &Value) -> Value {
    match value_cmp(a, b) {
        None => Value::Undef,
        Some(o) => {
            let res = match op {
                BinOp::Lt => o.is_lt(),
                BinOp::Le => o.is_le(),
                BinOp::Gt => o.is_gt(),
                BinOp::Ge => o.is_ge(),
                _ => unreachable!(),
            };
            Value::Bool(res)
        }
    }
}

/// Ordering comparison with OpenSCAD semantics: numbers numerically, strings
/// and vectors lexicographically (vectors recursively). Mixed/other types are
/// incomparable (`None` -> `undef`).
fn value_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        (Value::Vector(x), Value::Vector(y)) => {
            for (p, q) in x.iter().zip(y.iter()) {
                match value_cmp(p, q) {
                    Some(Ordering::Equal) => continue,
                    other => return other,
                }
            }
            Some(x.len().cmp(&y.len()))
        }
        _ => None,
    }
}
