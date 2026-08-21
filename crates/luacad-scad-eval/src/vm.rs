//! A small bytecode VM for function-call-heavy expression evaluation.
//!
//! The tree-walk interpreter in `lib.rs` remains the reference semantics (and
//! the fallback + differential oracle). This VM exists purely for speed on
//! eval-bound programs, where the tree-walk's per-call allocation churn (a fresh
//! scope map, params map, and env clone on *every* call) dominates.
//!
//! Strategy: compile a user function's body to bytecode **once**, resolving
//! parameters and `let` bindings to flat local slots (a `Vec<Value>` frame)
//! instead of hashed scope maps. Self-tail-calls become a jump back to the top
//! (preserving the tree-walk's tail-call elimination). Anything the compiler
//! doesn't understand — function literals capturing locals, comprehensions,
//! named-argument calls, `echo`/`assert` prefixes — makes `compile_fn` return
//! `None`, and that function transparently uses the tree-walk instead. So the
//! VM is a pure, opt-in fast path: it never changes results, only timing.

use crate::{index_value, value, EResult, EvalError, FnClosure, Interp};
use openrscad_syntax::ast::{BinOp, Expr, ListElem, UnOp};
use std::rc::Rc;

/// Cap on self-tail-call iterations (mirrors the tree-walk's `MAX_RANGE_ITERS`).
const MAX_ITERS: usize = 10_000_000;

/// A bytecode instruction. Operands index into the chunk's `consts`/`names`
/// tables or are absolute jump targets / local slots.
#[derive(Debug, Clone)]
pub enum Op {
    /// Push `consts[idx]`.
    Const(u32),
    /// Push `locals[slot]`.
    LoadLocal(u16),
    /// Push the value of a name resolved against the (captured) scope chain —
    /// free variables, globals, and `$` specials all route through
    /// `Interp::lookup_var`.
    LoadName(u32),
    /// Pop into `locals[slot]` (a `let` binding).
    StoreLocal(u16),
    Unary(UnOp),
    /// A binary op handled by `value::binary` (arithmetic, comparison, `%`, `^`,
    /// indexing-free). `&&`/`||` are compiled to jumps instead.
    Bin(BinOp),
    /// Pop index, pop base, push `base[index]`.
    Index,
    /// Pop base, push `base.<x|y|z>` (0/1/2); any other field pushes `undef`.
    Member(u8),
    /// Pop `n` values, push them as a vector.
    MakeVector(u32),
    /// Pop end, start → 2-arg range (normalized ascending, step 1).
    MakeRange2,
    /// Pop end, step, start → 3-arg range (as written).
    MakeRange3,
    PushBool(bool),
    /// Replace top of stack with `Bool(top.truthy())`.
    ToBool,
    Jump(u32),
    /// Pop; jump if falsy.
    JumpIfFalse(u32),
    /// Pop; jump if truthy.
    JumpIfTrue(u32),
    /// Pop `argc` args, call `names[idx]` (user fn / fn-valued var / builtin).
    Call(u32, u16),
    /// Tail-position call of `names[idx]` with `argc` positional args: if it
    /// resolves to the currently-running function, rebind locals and jump to 0
    /// (TCE); otherwise it's an ordinary call whose result is left on the stack.
    TailCall(u32, u16),
    /// Return top of stack (or `undef` if empty).
    Return,
}

/// A compiled function body.
pub struct Chunk {
    code: Vec<Op>,
    consts: Vec<Value>,
    names: Vec<String>,
    n_locals: usize,
}

impl Chunk {
    /// Number of local slots (parameters + `let` bindings) the frame needs.
    pub fn n_locals(&self) -> usize {
        self.n_locals
    }
}

use value::Value;

// ===================================================================
// Compiler
// ===================================================================

/// Compile a function's body to a [`Chunk`], or return `None` to signal that
/// the tree-walk must handle it (any unsupported construct bails).
pub fn compile_fn(f: &FnClosure) -> Option<Chunk> {
    let mut c = Compiler {
        code: Vec::new(),
        consts: Vec::new(),
        names: Vec::new(),
        frames: vec![Vec::new()],
        n_locals: 0,
        n_params: f.params.len(),
        self_name: f.name.as_deref(),
        ok: true,
    };
    // Parameters occupy the first slots, in declaration order.
    for p in &f.params {
        c.declare(&p.name);
    }
    c.compile_tail(&f.body);
    c.emit(Op::Return);
    if !c.ok {
        return None;
    }
    Some(Chunk {
        code: c.code,
        consts: c.consts,
        names: c.names,
        n_locals: c.n_locals,
    })
}

struct Compiler<'a> {
    code: Vec<Op>,
    consts: Vec<Value>,
    names: Vec<String>,
    /// A stack of lexical frames; each maps a name to its local slot. `let`
    /// pushes a frame so bindings shadow correctly and are popped on exit.
    frames: Vec<Vec<(String, u16)>>,
    n_locals: usize,
    n_params: usize,
    self_name: Option<&'a str>,
    ok: bool,
}

impl Compiler<'_> {
    fn emit(&mut self, op: Op) {
        self.code.push(op);
    }

    fn bail(&mut self) {
        self.ok = false;
    }

    fn const_idx(&mut self, v: Value) -> u32 {
        let i = self.consts.len() as u32;
        self.consts.push(v);
        i
    }

    fn name_idx(&mut self, s: &str) -> u32 {
        if let Some(i) = self.names.iter().position(|n| n == s) {
            return i as u32;
        }
        let i = self.names.len() as u32;
        self.names.push(s.to_string());
        i
    }

    /// Allocate a fresh local slot for `name` in the current frame.
    fn declare(&mut self, name: &str) -> u16 {
        let slot = self.n_locals as u16;
        self.n_locals += 1;
        self.frames
            .last_mut()
            .unwrap()
            .push((name.to_string(), slot));
        slot
    }

    /// Resolve a name to a local slot, innermost frame first.
    fn resolve(&self, name: &str) -> Option<u16> {
        for frame in self.frames.iter().rev() {
            for (n, slot) in frame.iter().rev() {
                if n == name {
                    return Some(*slot);
                }
            }
        }
        None
    }

    /// Compile an expression in tail position (its value becomes the function
    /// result). Mirrors the tree-walk's `eval_tail`.
    fn compile_tail(&mut self, e: &Expr) {
        if !self.ok {
            return;
        }
        match e {
            Expr::Ternary { cond, then, els } => {
                self.compile_expr(cond);
                let jf = self.code.len();
                self.emit(Op::JumpIfFalse(0));
                self.compile_tail(then);
                let jmp = self.code.len();
                self.emit(Op::Jump(0));
                let else_target = self.code.len() as u32;
                self.compile_tail(els);
                let end = self.code.len() as u32;
                self.patch(jf, else_target);
                self.patch(jmp, end);
            }
            Expr::Let { bindings, body } => {
                self.frames.push(Vec::new());
                for (n, init) in bindings {
                    self.compile_expr(init);
                    let slot = self.declare(n);
                    self.emit(Op::StoreLocal(slot));
                }
                self.compile_tail(body);
                self.frames.pop();
            }
            // A self-call in tail position is the TCE hot path.
            Expr::Call { name, args }
                if self.self_name == Some(name.as_str())
                    && args.len() == self.n_params
                    && args.iter().all(|a| a.name.is_none()) =>
            {
                for a in args {
                    self.compile_expr(&a.value);
                }
                let ni = self.name_idx(name);
                self.emit(Op::TailCall(ni, args.len() as u16));
            }
            // A self-call that doesn't match the fast-path shape (wrong arity or
            // named args) would need the tree-walk's default-filling TCE — bail
            // the whole function so semantics stay identical.
            Expr::Call { name, args }
                if self.self_name == Some(name.as_str())
                    && (args.len() != self.n_params || args.iter().any(|a| a.name.is_some())) =>
            {
                self.bail();
            }
            // Any other tail expression: evaluate normally; the trailing
            // `Return` emitted by `compile_fn` yields it.
            _ => self.compile_expr(e),
        }
    }

    fn compile_expr(&mut self, e: &Expr) {
        if !self.ok {
            return;
        }
        match e {
            Expr::Number(n) => {
                let i = self.const_idx(Value::Number(*n));
                self.emit(Op::Const(i));
            }
            Expr::Bool(b) => self.emit(Op::PushBool(*b)),
            Expr::Str(s) => {
                let i = self.const_idx(Value::Str(s.clone()));
                self.emit(Op::Const(i));
            }
            Expr::Undef => {
                let i = self.const_idx(Value::Undef);
                self.emit(Op::Const(i));
            }
            Expr::Ident(name) => match self.resolve(name) {
                Some(slot) => self.emit(Op::LoadLocal(slot)),
                None => {
                    let ni = self.name_idx(name);
                    self.emit(Op::LoadName(ni));
                }
            },
            Expr::Vector(elems) => {
                // Only plain vector literals are supported; comprehensions bail.
                if elems.iter().any(|el| !matches!(el, ListElem::Item(_))) {
                    self.bail();
                    return;
                }
                for el in elems {
                    if let ListElem::Item(item) = el {
                        self.compile_expr(item);
                    }
                }
                self.emit(Op::MakeVector(elems.len() as u32));
            }
            Expr::Range { start, step, end } => match step {
                Some(st) => {
                    self.compile_expr(start);
                    self.compile_expr(st);
                    self.compile_expr(end);
                    self.emit(Op::MakeRange3);
                }
                None => {
                    self.compile_expr(start);
                    self.compile_expr(end);
                    self.emit(Op::MakeRange2);
                }
            },
            Expr::Unary { op, expr } => {
                self.compile_expr(expr);
                self.emit(Op::Unary(*op));
            }
            Expr::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } => {
                self.compile_expr(lhs);
                let jf = self.code.len();
                self.emit(Op::JumpIfFalse(0));
                self.compile_expr(rhs);
                self.emit(Op::ToBool);
                let jmp = self.code.len();
                self.emit(Op::Jump(0));
                let false_target = self.code.len() as u32;
                self.emit(Op::PushBool(false));
                let end = self.code.len() as u32;
                self.patch(jf, false_target);
                self.patch(jmp, end);
            }
            Expr::Binary {
                op: BinOp::Or,
                lhs,
                rhs,
            } => {
                self.compile_expr(lhs);
                let jt = self.code.len();
                self.emit(Op::JumpIfTrue(0));
                self.compile_expr(rhs);
                self.emit(Op::ToBool);
                let jmp = self.code.len();
                self.emit(Op::Jump(0));
                let true_target = self.code.len() as u32;
                self.emit(Op::PushBool(true));
                let end = self.code.len() as u32;
                self.patch(jt, true_target);
                self.patch(jmp, end);
            }
            Expr::Binary { op, lhs, rhs } => {
                self.compile_expr(lhs);
                self.compile_expr(rhs);
                self.emit(Op::Bin(*op));
            }
            Expr::Ternary { cond, then, els } => {
                self.compile_expr(cond);
                let jf = self.code.len();
                self.emit(Op::JumpIfFalse(0));
                self.compile_expr(then);
                let jmp = self.code.len();
                self.emit(Op::Jump(0));
                let else_target = self.code.len() as u32;
                self.compile_expr(els);
                let end = self.code.len() as u32;
                self.patch(jf, else_target);
                self.patch(jmp, end);
            }
            Expr::Index { base, index } => {
                self.compile_expr(base);
                self.compile_expr(index);
                self.emit(Op::Index);
            }
            Expr::Member { base, field } => {
                self.compile_expr(base);
                let k = match field.as_str() {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    _ => 255,
                };
                self.emit(Op::Member(k));
            }
            Expr::Let { bindings, body } => {
                self.frames.push(Vec::new());
                for (n, init) in bindings {
                    self.compile_expr(init);
                    let slot = self.declare(n);
                    self.emit(Op::StoreLocal(slot));
                }
                self.compile_expr(body);
                self.frames.pop();
            }
            Expr::Call { name, args } => {
                if args.iter().any(|a| a.name.is_some()) {
                    self.bail(); // named args need map-based binding
                    return;
                }
                for a in args {
                    self.compile_expr(&a.value);
                }
                let ni = self.name_idx(name);
                self.emit(Op::Call(ni, args.len() as u16));
            }
            // Unsupported: closures over locals, value-calls, echo/assert.
            Expr::FunctionLiteral { .. }
            | Expr::CallValue { .. }
            | Expr::Echo { .. }
            | Expr::Assert { .. } => {
                self.bail();
            }
        }
    }

    fn patch(&mut self, at: usize, target: u32) {
        match &mut self.code[at] {
            Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfTrue(t) => *t = target,
            _ => {}
        }
    }
}

// ===================================================================
// VM
// ===================================================================

/// Execute a compiled chunk with the given local frame (parameters already in
/// their slots). Free variables resolve against `interp`'s current scope chain,
/// which the caller has set to the closure's captured environment.
pub fn run(
    interp: &mut Interp,
    chunk: &Rc<Chunk>,
    mut locals: Vec<Value>,
    f: &Rc<FnClosure>,
) -> EResult<Value> {
    let mut stack: Vec<Value> = Vec::with_capacity(16);
    let mut ip = 0usize;
    let mut iters = 0usize;
    let code = &chunk.code;
    // Whether this chunk's `TailCall`s (which the compiler only emits for the
    // function's own name) actually resolve to this same closure. It's
    // loop-invariant for the whole run — the scope chain is fixed to `f.env` —
    // so resolve it once instead of on every tail iteration.
    let tail_is_self = f
        .name
        .as_deref()
        .and_then(|n| interp.lookup_func(n))
        .is_some_and(|g| Rc::ptr_eq(&g, f));
    loop {
        // One fuel unit per opcode — bounds compiled function bodies, recursion,
        // and in-function comprehensions/tail loops under a budget.
        interp.burn()?;
        match &code[ip] {
            Op::Const(i) => stack.push(chunk.consts[*i as usize].clone()),
            Op::LoadLocal(s) => stack.push(locals[*s as usize].clone()),
            Op::LoadName(i) => {
                let v = interp.lookup_var(&chunk.names[*i as usize]);
                stack.push(v);
            }
            Op::StoreLocal(s) => {
                let v = stack.pop().unwrap_or(Value::Undef);
                locals[*s as usize] = v;
            }
            Op::Unary(op) => {
                let v = stack.pop().unwrap_or(Value::Undef);
                stack.push(value::unary(*op, v));
            }
            Op::Bin(op) => {
                let r = stack.pop().unwrap_or(Value::Undef);
                let l = stack.pop().unwrap_or(Value::Undef);
                // Fast path for the overwhelmingly common number-op-number case,
                // bypassing `value::binary`'s layered type dispatch. Must match
                // its semantics exactly — in particular NaN comparisons yield
                // `undef` (via `partial_cmp`), not `false`.
                if let (Value::Number(a), Value::Number(b)) = (&l, &r) {
                    let (a, b) = (*a, *b);
                    let v = match op {
                        BinOp::Add => Value::Number(a + b),
                        BinOp::Sub => Value::Number(a - b),
                        BinOp::Mul => Value::Number(a * b),
                        BinOp::Div => Value::Number(a / b),
                        BinOp::Mod => Value::Number(a % b),
                        BinOp::Pow => Value::Number(a.powf(b)),
                        BinOp::Eq => Value::Bool(a == b),
                        BinOp::Ne => Value::Bool(a != b),
                        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match a.partial_cmp(&b) {
                            Some(o) => Value::Bool(match op {
                                BinOp::Lt => o.is_lt(),
                                BinOp::Le => o.is_le(),
                                BinOp::Gt => o.is_gt(),
                                _ => o.is_ge(),
                            }),
                            None => Value::Undef,
                        },
                        // And/Or are compiled to jumps and never reach here.
                        BinOp::And | BinOp::Or => value::binary(*op, l, r),
                    };
                    stack.push(v);
                } else {
                    stack.push(value::binary(*op, l, r));
                }
            }
            Op::Index => {
                let i = stack.pop().unwrap_or(Value::Undef);
                let b = stack.pop().unwrap_or(Value::Undef);
                stack.push(index_value(&b, &i));
            }
            Op::Member(k) => {
                let b = stack.pop().unwrap_or(Value::Undef);
                let v = if *k < 3 {
                    index_value(&b, &Value::Number(*k as f64))
                } else {
                    Value::Undef
                };
                stack.push(v);
            }
            Op::MakeVector(n) => {
                let start = stack.len() - *n as usize;
                let items = stack.split_off(start);
                stack.push(value::vector(items));
            }
            Op::MakeRange2 => {
                let e = stack
                    .pop()
                    .unwrap_or(Value::Undef)
                    .as_number()
                    .unwrap_or(f64::NAN);
                let s = stack
                    .pop()
                    .unwrap_or(Value::Undef)
                    .as_number()
                    .unwrap_or(f64::NAN);
                let (lo, hi) = if s <= e { (s, e) } else { (e, s) };
                stack.push(Value::Range {
                    start: lo,
                    step: 1.0,
                    end: hi,
                });
            }
            Op::MakeRange3 => {
                let e = stack
                    .pop()
                    .unwrap_or(Value::Undef)
                    .as_number()
                    .unwrap_or(f64::NAN);
                let st = stack
                    .pop()
                    .unwrap_or(Value::Undef)
                    .as_number()
                    .unwrap_or(1.0);
                let s = stack
                    .pop()
                    .unwrap_or(Value::Undef)
                    .as_number()
                    .unwrap_or(f64::NAN);
                stack.push(Value::Range {
                    start: s,
                    step: st,
                    end: e,
                });
            }
            Op::PushBool(b) => stack.push(Value::Bool(*b)),
            Op::ToBool => {
                let v = stack.pop().unwrap_or(Value::Undef);
                stack.push(Value::Bool(v.truthy()));
            }
            Op::Jump(t) => {
                ip = *t as usize;
                continue;
            }
            Op::JumpIfFalse(t) => {
                let v = stack.pop().unwrap_or(Value::Undef);
                if !v.truthy() {
                    ip = *t as usize;
                    continue;
                }
            }
            Op::JumpIfTrue(t) => {
                let v = stack.pop().unwrap_or(Value::Undef);
                if v.truthy() {
                    ip = *t as usize;
                    continue;
                }
            }
            Op::Call(ni, argc) => {
                let start = stack.len() - *argc as usize;
                let argv = stack.split_off(start);
                let name = chunk.names[*ni as usize].clone();
                let r = interp.call_named_values(&name, argv)?;
                stack.push(r);
            }
            Op::TailCall(ni, argc) => {
                let start = stack.len() - *argc as usize;
                if tail_is_self {
                    // TCE: rebind param slots and loop.
                    let newvals = stack.split_off(start);
                    for (k, v) in newvals.into_iter().enumerate() {
                        locals[k] = v;
                    }
                    iters += 1;
                    if iters > MAX_ITERS {
                        return Err(EvalError::new("tail recursion exceeded iteration limit"));
                    }
                    ip = 0;
                    continue;
                } else {
                    // Name resolved to something other than this closure — an
                    // ordinary call whose result is left on the stack.
                    let argv = stack.split_off(start);
                    let name = chunk.names[*ni as usize].clone();
                    let r = interp.call_named_values(&name, argv)?;
                    stack.push(r);
                }
            }
            Op::Return => return Ok(stack.pop().unwrap_or(Value::Undef)),
        }
        ip += 1;
    }
}
