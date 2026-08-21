//! Tree-walk evaluator: AST -> CSG tree ([`openrscad_ir::Node`]).
//!
//! Scoping matches OpenSCAD: ordinary variables, functions, and modules are
//! lexically scoped (closures capture the scope chain at definition time),
//! while `$` special variables are dynamically scoped (a separate frame stack
//! that mirrors execution nesting). Function values and module `children()`
//! both close over their definition / call-site environments.

mod color;
mod text;
mod value;
mod vm;

pub use text::{font_completions, register_font_data, register_system_fonts, FontCompletion};
pub use value::{format_number, Value};

use openrscad_ir::{FragmentSpec, Node, Vec3};
use openrscad_syntax::ast::*;
use std::collections::HashSet;

/// Fast hash map for the interpreter's hot maps (scopes, params, specials).
/// Variable names are short strings hashed on every lookup/insert; FxHash is
/// far faster than the default SipHash and deterministic (no per-run seed).
type FastMap<K, V> = rustc_hash::FxHashMap<K, V>;

/// A file loaded by an `include`/`use` resolver.
pub struct LoadedFile {
    /// A canonical key identifying the file (for cycle detection).
    pub key: String,
    pub source: String,
    /// Directory of the loaded file (base for its own relative includes).
    pub dir: String,
}

/// Resolves `include`/`use` paths to source. Native builds read from disk;
/// the browser can supply an in-memory map.
pub trait FileResolver {
    fn load(&self, path: &str, from_dir: &str) -> Option<LoadedFile>;
    /// Load raw bytes (for `import()` of binary meshes). Defaults to none.
    fn load_bytes(&self, path: &str, from_dir: &str) -> Option<Vec<u8>> {
        let _ = (path, from_dir);
        None
    }
}

/// A resolver that never finds anything (include/use become warnings).
pub struct NullResolver;
impl FileResolver for NullResolver {
    fn load(&self, _path: &str, _from_dir: &str) -> Option<LoadedFile> {
        None
    }
}

const MAX_RANGE_ITERS: usize = 10_000_000;
/// Max function/module call nesting before erroring — a graceful error instead
/// of a native stack overflow. Each release frame is ~1.6 KiB, so 6000 fits the
/// CLI's 256 MiB worker thread with wide margin; callers must run eval on an
/// ample stack (the CLI does). In the browser, V8's own wasm call-frame limit
/// trips first and is caught by the engine wrapper (the tab never crashes);
/// deep *tail* recursion is turned into a loop by TCE and is unbounded. Sits
/// just above OpenSCAD's own limit (it accepts ~5000-deep recursion).
const MAX_CALL_DEPTH: usize = 6_000;

/// An evaluation error, optionally carrying the byte span (into the *main*
/// source) of the statement that produced it, for inline editor diagnostics.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct EvalError {
    pub message: String,
    pub span: Option<std::ops::Range<usize>>,
}

impl EvalError {
    pub fn new(message: impl Into<String>) -> Self {
        EvalError {
            message: message.into(),
            span: None,
        }
    }

    /// Fill in the span if not already set (the innermost statement wins).
    fn or_span(mut self, span: Option<std::ops::Range<usize>>) -> Self {
        if self.span.is_none() {
            self.span = span;
        }
        self
    }
}

type EResult<T> = Result<T, EvalError>;

fn err<T>(msg: impl Into<String>) -> EResult<T> {
    Err(EvalError::new(msg))
}

/// A warning, optionally carrying the byte span (into the main source) of the
/// statement that produced it.
#[derive(Debug, Clone)]
pub struct Warning {
    pub message: String,
    pub span: Option<std::ops::Range<usize>>,
}

/// A structured diagnostic for the frontend (inline squiggles). `start`/`end`
/// are byte offsets into the main source, or `-1` when no span is available.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    pub severity: &'static str, // "error" | "warning"
    pub message: String,
    pub start: i64,
    pub end: i64,
}

impl Diagnostic {
    fn from_span(
        severity: &'static str,
        message: String,
        span: &Option<std::ops::Range<usize>>,
    ) -> Self {
        let (start, end) = match span {
            Some(r) => (r.start as i64, r.end as i64),
            None => (-1, -1),
        };
        Diagnostic {
            severity,
            message,
            start,
            end,
        }
    }
}

/// Serialize an optional error plus warnings into the diagnostics JSON the
/// frontend consumes. Shared by the wasm and desktop boundaries.
pub fn diagnostics_json(error: Option<&Diagnostic>, warnings: &[Warning]) -> String {
    let mut all: Vec<Diagnostic> = Vec::new();
    if let Some(e) = error {
        all.push(e.clone());
    }
    for w in warnings {
        all.push(Diagnostic::from_span("warning", w.message.clone(), &w.span));
    }
    serde_json::to_string(&all).unwrap_or_else(|_| "[]".to_string())
}

/// Build the error `Diagnostic` for a parse error (span into the main source).
pub fn parse_error_diagnostic(message: String, span: std::ops::Range<usize>) -> Diagnostic {
    Diagnostic::from_span("error", message, &Some(span))
}

/// Build the error `Diagnostic` for an evaluation error (span if known).
pub fn eval_error_diagnostic(err: &EvalError) -> Diagnostic {
    Diagnostic::from_span("error", err.message.clone(), &err.span)
}

/// Resolve `path` against directory `dir`, normalizing `.`/`..` segments. Used
/// to make a `use` path inside an included file resolvable later (when the
/// evaluator's current directory is no longer that file's). Absolute paths and
/// an empty `dir` are returned unchanged.
fn join_dir(dir: &str, path: &str) -> String {
    if dir.is_empty() || path.starts_with('/') {
        return path.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in dir.split('/').chain(path.split('/')) {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    // Preserve a leading '/' from an absolute `dir`.
    let joined = parts.join("/");
    if dir.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    }
}

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

type ScopeRef = Rc<RefCell<Scope>>;

/// A function value: parameters, body, and the lexical environment (scope
/// chain) captured at definition time.
pub struct FnClosure {
    params: Vec<Param>,
    body: Expr,
    env: Vec<ScopeRef>,
    /// The defining name (for named `function` defs), used by the VM to detect
    /// self-tail-calls. `None` for anonymous function literals.
    name: Option<String>,
    /// Lazily-compiled bytecode for the fast path; `Some(None)` means the body
    /// isn't VM-compilable and the tree-walk is used. Computed on first call.
    chunk: OnceCell<Option<Rc<vm::Chunk>>>,
}

/// Outcome of evaluating a function body in tail position.
enum TailResult {
    Value(Value),
    /// Re-invoke the same function with these freshly-bound arguments.
    TailCall(FastMap<String, Value>),
}

/// A module definition with its captured lexical environment.
struct ModClosure {
    params: Vec<Param>,
    body: Vec<Spanned<Stmt>>,
    env: Vec<ScopeRef>,
    /// Whether the module was defined in the main file — only then do its body's
    /// statement spans index into the main source (used for diagnostics).
    is_main: bool,
}

#[derive(Default)]
struct Scope {
    vars: FastMap<String, Value>,
    funcs: FastMap<String, Rc<FnClosure>>,
    modules: FastMap<String, Rc<ModClosure>>,
}

/// The output of evaluating a program.
#[derive(Debug)]
pub struct EvalOutput {
    pub node: Node,
    pub echoes: Vec<String>,
    pub warnings: Vec<Warning>,
    /// Number of `assert()`s that actually executed during evaluation. Used by
    /// the BOSL2 oracle to reject vacuous passes (eval succeeds but ran zero
    /// assertions, e.g. because a test module was never invoked).
    pub asserts_run: usize,
    /// Final top-level values of the `$vp*` viewport variables (a script may
    /// assign them to drive the camera). `None` when the value isn't a usable
    /// number/vector.
    pub viewport: Viewport,
}

/// The `$vpr`/`$vpt`/`$vpd`/`$vpf` viewport variables after evaluation.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Viewport {
    pub vpr: Option<[f64; 3]>,
    pub vpt: Option<[f64; 3]>,
    pub vpd: Option<f64>,
    pub vpf: Option<f64>,
}

/// Serialize the viewport variables to JSON (`{"vpr":[…],"vpt":[…],"vpd":…,
/// "vpf":…}`, `null` for unset) for the frontend camera channel.
pub fn viewport_json(v: &Viewport) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string())
}

struct Interp<'a> {
    /// Lexical scope chain (swapped to a closure's captured env during calls).
    scopes: Vec<ScopeRef>,
    /// Dynamic frames for `$` variables (mirrors execution nesting; NOT swapped
    /// on calls, giving `$vars` dynamic scoping).
    specials: Vec<FastMap<String, Value>>,
    echoes: Vec<String>,
    warnings: Vec<Warning>,
    /// Count of `assert()`s that executed (see `EvalOutput::asserts_run`).
    asserts_run: usize,
    root: Option<Node>,
    depth: usize,
    /// Remaining evaluation steps; `u64::MAX` means unlimited (the default).
    /// Charged per expression, per VM opcode, and per `for`-loop iteration so a
    /// budget bounds *total* work — the per-construct limits (`MAX_CALL_DEPTH`,
    /// `MAX_RANGE_ITERS`) don't, since nested loops multiply into an effectively
    /// unbounded runtime. Opt-in via [`eval_program_with_budget`]; also the hook
    /// for the playground's render cancellation.
    fuel: u64,
    /// True while executing statements whose spans index into the *main* source
    /// (the main program + modules defined in it); false inside `use`d/`include`d
    /// files. Gates diagnostic span attribution.
    in_main: bool,
    /// Span of the main-source statement currently executing (for warnings/errors).
    cur_span: Option<std::ops::Range<usize>>,
    /// For each active module call: the child statements, the caller's lexical
    /// scope chain (so `children()` evaluates them at the call site), and the
    /// caller's `in_main` flag (so their spans attribute correctly).
    children_stack: Vec<(Vec<Spanned<Stmt>>, Vec<ScopeRef>, bool)>,
    /// Active user-module instantiations, outermost to innermost. Built-in
    /// modules do not participate in OpenSCAD's `$parent_modules` stack.
    module_stack: Vec<String>,
    /// `include`/`use` file resolver.
    resolver: &'a dyn FileResolver,
    /// Directory of the file currently being evaluated (for relative includes).
    cur_dir: String,
    /// Files currently being loaded, for include/use cycle detection.
    loading: HashSet<String>,
    /// Customizer / `-D` overrides: these replace matching *top-level*
    /// assignments in the main file (they win, like OpenSCAD's `-D`).
    overrides: FastMap<String, Value>,
    /// De-dup key set for the dead-assignment lint. A re-entered module body
    /// re-runs its assignment phase, and OpenSCAD would warn each time; OpenRSCAD's
    /// warnings back editor squiggles, so we emit once per (name + source span).
    warned: HashSet<(String, usize)>,
}

/// Evaluate a parsed program into a CSG tree plus console output (no file
/// access; `include`/`use` become warnings).
pub fn eval_program(prog: &Program) -> EResult<EvalOutput> {
    eval_program_with(prog, &NullResolver, ".")
}

/// Convert a customizer parameter value into a runtime [`Value`], for use as an
/// override in [`eval_program_with_params`].
pub fn value_from_param(p: &openrscad_syntax::customizer::ParamValue) -> Value {
    use openrscad_syntax::customizer::ParamValue as P;
    match p {
        P::Number(n) => Value::Number(*n),
        P::Bool(b) => Value::Bool(*b),
        P::Text(s) => Value::Str(s.clone()),
        P::Vector(xs) => value::vector(xs.iter().map(|n| Value::Number(*n)).collect()),
    }
}

/// Evaluate a program with `include`/`use` support via `resolver`, resolving
/// relative paths against `base_dir`.
pub fn eval_program_with(
    prog: &Program,
    resolver: &dyn FileResolver,
    base_dir: &str,
) -> EResult<EvalOutput> {
    eval_program_with_params(prog, resolver, base_dir, &[])
}

/// Like [`eval_program_with`], but bounded by a step *budget*: evaluation is
/// charged one unit of fuel per expression, per VM opcode, and per `for`-loop
/// iteration, and fails with an "evaluation budget exhausted" error once the
/// budget runs out. This is the only way to guarantee termination on adversarial
/// input (nested loops otherwise multiply the per-construct limits into an
/// effectively unbounded runtime), and is the hook the playground uses to make a
/// runaway render cancellable. `budget == u64::MAX` is equivalent to the
/// unbudgeted entry points.
pub fn eval_program_with_budget(
    prog: &Program,
    resolver: &dyn FileResolver,
    base_dir: &str,
    budget: u64,
) -> EResult<EvalOutput> {
    eval_program_impl(prog, resolver, base_dir, &[], budget)
}

/// Like [`eval_program_with`], but with customizer / `-D`-style parameter
/// overrides: each `(name, value)` replaces the main file's top-level
/// assignment of `name` (the override wins, matching OpenSCAD's `-D`).
pub fn eval_program_with_params(
    prog: &Program,
    resolver: &dyn FileResolver,
    base_dir: &str,
    overrides: &[(String, Value)],
) -> EResult<EvalOutput> {
    eval_program_impl(prog, resolver, base_dir, overrides, u64::MAX)
}

fn eval_program_impl(
    prog: &Program,
    resolver: &dyn FileResolver,
    base_dir: &str,
    overrides: &[(String, Value)],
    fuel: u64,
) -> EResult<EvalOutput> {
    let mut base = Scope::default();
    base.vars
        .insert("PI".into(), Value::Number(std::f64::consts::PI));

    // `$` special variables live in the dynamic frame stack.
    let mut globals = FastMap::default();
    globals.insert("$fn".to_string(), Value::Number(0.0));
    globals.insert("$fa".to_string(), Value::Number(12.0));
    globals.insert("$fs".to_string(), Value::Number(2.0));
    globals.insert("$t".to_string(), Value::Number(0.0));
    globals.insert("$preview".to_string(), Value::Bool(true));
    // Viewport variables (the frontend overrides these with the live camera;
    // these defaults let scripts read them off-viewport, e.g. in the CLI).
    globals.insert(
        "$vpr".to_string(),
        value::vector(vec![
            Value::Number(55.0),
            Value::Number(0.0),
            Value::Number(25.0),
        ]),
    );
    globals.insert(
        "$vpt".to_string(),
        value::vector(vec![
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
        ]),
    );
    globals.insert("$vpd".to_string(), Value::Number(140.0));
    globals.insert("$vpf".to_string(), Value::Number(45.0));
    // A `$`-named override (e.g. `$t` for animation, `$fn`) seeds the global
    // special-variable frame rather than a top-level assignment.
    for (name, value) in overrides {
        if name.starts_with('$') {
            globals.insert(name.clone(), value.clone());
        }
    }

    let mut interp = Interp {
        scopes: vec![Rc::new(RefCell::new(base))],
        specials: vec![globals],
        echoes: Vec::new(),
        warnings: Vec::new(),
        asserts_run: 0,
        root: None,
        depth: 0,
        fuel,
        in_main: true,
        cur_span: None,
        children_stack: Vec::new(),
        module_stack: Vec::new(),
        resolver,
        cur_dir: base_dir.to_string(),
        loading: HashSet::new(),
        overrides: overrides.iter().cloned().collect(),
        warned: HashSet::new(),
    };

    let nodes = interp.eval_stmts(prog)?;
    let node = interp.root.take().unwrap_or_else(|| Node::group(nodes));
    // The final top-level `$vp*` values (a script may have assigned them).
    let g = &interp.specials[0];
    let viewport = Viewport {
        vpr: g.get("$vpr").and_then(Value::as_vec3),
        vpt: g.get("$vpt").and_then(Value::as_vec3),
        vpd: g.get("$vpd").and_then(Value::as_number),
        vpf: g.get("$vpf").and_then(Value::as_number),
    };
    Ok(EvalOutput {
        node,
        echoes: interp.echoes,
        warnings: interp.warnings,
        asserts_run: interp.asserts_run,
        viewport,
    })
}

impl Interp<'_> {
    /// Charge one unit of evaluation fuel; error once the budget is exhausted.
    /// A no-op when running unbudgeted (`fuel == u64::MAX`, the default), so the
    /// normal path pays only a single predictable branch.
    #[inline]
    fn burn(&mut self) -> EResult<()> {
        if self.fuel != u64::MAX {
            match self.fuel.checked_sub(1) {
                Some(f) => self.fuel = f,
                None => return err("evaluation budget exhausted"),
            }
        }
        Ok(())
    }

    // ---- scope helpers -------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(Rc::new(RefCell::new(Scope::default())));
        self.specials.push(FastMap::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.specials.pop();
    }

    fn set_var(&mut self, name: &str, val: Value) {
        if name.starts_with('$') {
            self.specials
                .last_mut()
                .unwrap()
                .insert(name.to_string(), val);
        } else {
            self.scopes
                .last()
                .unwrap()
                .borrow_mut()
                .vars
                .insert(name.to_string(), val);
        }
    }

    fn lookup_var(&self, name: &str) -> Value {
        if name.starts_with('$') {
            for frame in self.specials.iter().rev() {
                if let Some(v) = frame.get(name) {
                    return v.clone();
                }
            }
        } else {
            for scope in self.scopes.iter().rev() {
                if let Some(v) = scope.borrow().vars.get(name) {
                    return v.clone();
                }
            }
        }
        Value::Undef
    }

    fn lookup_func(&self, name: &str) -> Option<Rc<FnClosure>> {
        for scope in self.scopes.iter().rev() {
            if let Some(f) = scope.borrow().funcs.get(name) {
                return Some(f.clone());
            }
        }
        None
    }

    fn lookup_module(&self, name: &str) -> Option<Rc<ModClosure>> {
        for scope in self.scopes.iter().rev() {
            if let Some(m) = scope.borrow().modules.get(name) {
                return Some(m.clone());
            }
        }
        None
    }

    // ---- statements ----------------------------------------------------

    /// The diagnostic span for a statement: its byte range when we're executing
    /// main-source statements and the span isn't a splice sentinel; else `None`.
    fn stmt_span(&self, s: &Spanned<Stmt>) -> Option<std::ops::Range<usize>> {
        if self.in_main && s.span.start != usize::MAX {
            Some(s.span.clone())
        } else {
            None
        }
    }

    /// Record a warning, tagged with the statement currently executing.
    fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(Warning {
            message: message.into(),
            span: self.cur_span.clone(),
        });
    }

    /// Record a dead-assignment lint at most once per (name, span). A `None`
    /// span means the assignment lives in an `include`d/`use`d library (sentinel
    /// span) — not the user's editable source — so it is skipped. See
    /// [`Interp::warned`] for why this de-dups where `warn` does not.
    fn warn_overwritten(&mut self, name: &str, span: Option<std::ops::Range<usize>>) {
        let Some(range) = span else { return };
        if !self.warned.insert((name.to_string(), range.start)) {
            return;
        }
        self.warnings.push(Warning {
            message: format!(
                "variable '{name}' is assigned again later; this assignment is overwritten"
            ),
            span: Some(range),
        });
    }

    fn eval_stmts(&mut self, stmts: &[Spanned<Stmt>]) -> EResult<Vec<Node>> {
        // Splice `include`d files in first (only when present, to avoid cloning).
        let expanded;
        let effective: &[Spanned<Stmt>] =
            if stmts.iter().any(|s| matches!(s.node, Stmt::Include { .. })) {
                expanded = self.expand_includes(stmts, false)?;
                &expanded
            } else {
                stmts
            };

        // Phases 1 (definitions + `use` imports) and 2 (assignments).
        self.eval_defs_and_assigns(effective)?;

        // Phase 3: geometry.
        let mut out = Vec::new();
        for s in effective {
            match s.node {
                Stmt::Assign { .. }
                | Stmt::FunctionDef { .. }
                | Stmt::ModuleDef { .. }
                | Stmt::Use { .. }
                | Stmt::Include { .. } => {}
                _ => {
                    let sp = self.stmt_span(s);
                    let saved = self.cur_span.take();
                    self.cur_span = sp.clone();
                    let r = self.eval_geom(&s.node);
                    self.cur_span = saved;
                    out.extend(r.map_err(|e| e.or_span(sp))?);
                }
            }
        }
        Ok(out)
    }

    /// Phase 1 (hoist definitions + process `use` imports) and phase 2 (hoist
    /// assignments, last write wins) into the current scope.
    fn eval_defs_and_assigns(&mut self, stmts: &[Spanned<Stmt>]) -> EResult<()> {
        for s in stmts {
            match &s.node {
                Stmt::FunctionDef { name, params, body } => {
                    let env = self.scopes.clone();
                    self.scopes.last().unwrap().borrow_mut().funcs.insert(
                        name.clone(),
                        Rc::new(FnClosure {
                            params: params.clone(),
                            body: body.clone(),
                            env,
                            name: Some(name.clone()),
                            chunk: OnceCell::new(),
                        }),
                    );
                }
                Stmt::ModuleDef { name, params, body } => {
                    let env = self.scopes.clone();
                    self.scopes.last().unwrap().borrow_mut().modules.insert(
                        name.clone(),
                        Rc::new(ModClosure {
                            params: params.clone(),
                            body: body.clone(),
                            env,
                            is_main: self.in_main,
                        }),
                    );
                }
                Stmt::Use { path } => self.import_use(path)?,
                _ => {}
            }
        }
        // Phase 2: hoist assignments with OpenSCAD's last-write-wins semantics.
        // Within a scope only a variable's *final* assignment is evaluated, and
        // it is evaluated at the position the variable was *first introduced*.
        // Concretely: `p = 1; q = p; p = 5;` yields `q == 5` (the read of `p`
        // sees its final value, not the intermediate `1`), and the overwritten
        // `p = 1` is discarded entirely — including any side effects in its RHS.
        // There are *no* forward references: a read of a variable introduced
        // later in the scope does not see it and falls through to an outer
        // binding or `undef` (`y = x; x = 5;` yields `y == undef` at top level),
        // matching the OpenSCAD oracle.
        //
        // Only the *main file's* top-level assignments (scope depth 1) are
        // customizer parameters; `use`d files run deeper and modules deeper
        // still, so their internal variables are never overridden.
        let top_level = self.scopes.len() == 1;

        // Keep the last assignment per name, remembering first-introduction order.
        // A name assigned more than once in the scope means the earlier write is
        // dead — warn on it (OpenSCAD: "X was assigned … but was overwritten").
        let mut order: Vec<&str> = Vec::new();
        let mut last: FastMap<&str, (&Expr, Option<std::ops::Range<usize>>)> = FastMap::default();
        for s in stmts {
            if let Stmt::Assign { name, value } = &s.node {
                let span = self.stmt_span(s);
                match last.insert(name.as_str(), (value, span)) {
                    Some((_, dead_span)) => self.warn_overwritten(name, dead_span),
                    None => order.push(name.as_str()),
                }
            }
        }

        // Evaluate in first-introduction order, binding each before the next so a
        // reference to an earlier-introduced name sees its (final) value.
        for name in order {
            let (value, sp) = &last[name];
            let saved = self.cur_span.take();
            self.cur_span = sp.clone();
            let v = match self.overrides.get(name) {
                Some(ov) if top_level => Ok(ov.clone()),
                _ => self.eval_expr(value),
            };
            self.cur_span = saved;
            let v = v.map_err(|e| e.or_span(sp.clone()))?;
            self.set_var(name, v);
        }
        Ok(())
    }

    /// Recursively splice `include`d files' top-level statements in place.
    ///
    /// `in_include` is true while expanding a file reached via `include`/`use`
    /// (i.e. not the main program). In that case a deferred `use` must remember
    /// the file it came from — the flattened statements are later resolved
    /// against the *main* directory, so we rewrite the `use` path to an absolute
    /// one relative to the including file's directory. (Top-level `use`s are
    /// left relative so they still search library paths / the CDN registry.)
    fn expand_includes(
        &mut self,
        stmts: &[Spanned<Stmt>],
        in_include: bool,
    ) -> EResult<Vec<Spanned<Stmt>>> {
        // Statements spliced in from an included file get a sentinel span so the
        // evaluator never attributes a diagnostic to a library byte offset in the
        // main editor.
        const SENTINEL: std::ops::Range<usize> = usize::MAX..usize::MAX;
        let mut out = Vec::new();
        for s in stmts {
            match &s.node {
                Stmt::Include { path } => {
                    let Some(lf) = self.resolver.load(path, &self.cur_dir) else {
                        self.warn(format!("Can't open include file '{path}'"));
                        continue;
                    };
                    if !self.loading.insert(lf.key.clone()) {
                        continue; // cycle: already loading this file
                    }
                    let prog = openrscad_syntax::parse(&lf.source).map_err(|e| {
                        EvalError::new(format!("in include '{path}': {}", e.message))
                    })?;
                    let prev = std::mem::replace(&mut self.cur_dir, lf.dir.clone());
                    let expanded = self.expand_includes(&prog, true);
                    self.cur_dir = prev;
                    self.loading.remove(&lf.key);
                    out.extend(expanded?);
                }
                Stmt::Use { path } if in_include => {
                    out.push(Spanned::new(
                        Stmt::Use {
                            path: join_dir(&self.cur_dir, path),
                        },
                        SENTINEL,
                    ));
                }
                node => {
                    let span = if in_include { SENTINEL } else { s.span.clone() };
                    out.push(Spanned::new(node.clone(), span));
                }
            }
        }
        Ok(out)
    }

    /// Import a `use`d file's module/function definitions (only) into the
    /// current scope. The file is evaluated in isolation; its definitions close
    /// over its own top-level scope so they can use its helpers/constants.
    fn import_use(&mut self, path: &str) -> EResult<()> {
        let Some(lf) = self.resolver.load(path, &self.cur_dir) else {
            self.warn(format!("Can't open 'use' file '{path}'"));
            return Ok(());
        };
        if !self.loading.insert(lf.key.clone()) {
            return Ok(()); // cycle
        }
        let prog = openrscad_syntax::parse(&lf.source)
            .map_err(|e| EvalError::new(format!("in use '{path}': {}", e.message)))?;

        let file_scope: ScopeRef = Rc::new(RefCell::new(Scope::default()));
        let base = self.scopes[0].clone();
        let saved = std::mem::replace(&mut self.scopes, vec![base, file_scope.clone()]);
        let prev_dir = std::mem::replace(&mut self.cur_dir, lf.dir.clone());
        self.specials.push(FastMap::default());
        // A `use`d file's statement spans index into that file, not the main
        // source — don't attribute diagnostics to them.
        let prev_main = std::mem::replace(&mut self.in_main, false);

        let expanded = self.expand_includes(&prog, true);
        let result = expanded.and_then(|eff| self.eval_defs_and_assigns(&eff));

        self.in_main = prev_main;
        self.specials.pop();
        self.cur_dir = prev_dir;
        self.scopes = saved;
        self.loading.remove(&lf.key);
        result?;

        // Import only the definitions.
        let fs = file_scope.borrow();
        let target = self.scopes.last().unwrap();
        let mut t = target.borrow_mut();
        for (k, v) in fs.funcs.iter() {
            t.funcs.insert(k.clone(), v.clone());
        }
        for (k, v) in fs.modules.iter() {
            t.modules.insert(k.clone(), v.clone());
        }
        Ok(())
    }

    fn eval_geom(&mut self, stmt: &Stmt) -> EResult<Vec<Node>> {
        match stmt {
            Stmt::Block(stmts) => {
                self.push_scope();
                let r = self.eval_stmts(stmts);
                self.pop_scope();
                r
            }
            Stmt::If { cond, then, els } => {
                let c = self.eval_expr(cond)?;
                let branch = if c.truthy() { then } else { els };
                self.push_scope();
                let r = self.eval_stmts(branch);
                self.pop_scope();
                r
            }
            Stmt::For { bindings, body } => self.eval_for(bindings, body),
            Stmt::Let { bindings, body } => {
                self.push_scope();
                for (n, e) in bindings {
                    let v = self.eval_expr(e)?;
                    self.set_var(n, v);
                }
                let r = self.eval_stmts(body);
                self.pop_scope();
                r
            }
            Stmt::ModuleCall {
                modifier,
                name,
                args,
                children,
            } => self.eval_module_call(*modifier, name, args, children),
            _ => Ok(Vec::new()),
        }
    }

    fn eval_for(
        &mut self,
        bindings: &[(String, Expr)],
        body: &[Spanned<Stmt>],
    ) -> EResult<Vec<Node>> {
        let mut out = Vec::new();
        self.eval_for_rec(bindings, body, &mut out)?;
        Ok(out)
    }

    fn eval_for_rec(
        &mut self,
        bindings: &[(String, Expr)],
        body: &[Spanned<Stmt>],
        out: &mut Vec<Node>,
    ) -> EResult<()> {
        if bindings.is_empty() {
            self.push_scope();
            let r = self.eval_stmts(body);
            self.pop_scope();
            out.extend(r?);
            return Ok(());
        }
        let (name, expr) = &bindings[0];
        let iter = self.eval_expr(expr)?;
        let values = iter_values(&iter)?;
        for v in values {
            // Charge per iteration: an empty loop body evaluates no expressions,
            // so without this a nested `for` (product of two large ranges) would
            // spin unbounded even under a fuel budget.
            self.burn()?;
            self.push_scope();
            self.set_var(name, v);
            let r = self.eval_for_rec(&bindings[1..], body, out);
            self.pop_scope();
            r?;
        }
        Ok(())
    }

    /// Legacy `intersection_for(...)`: evaluate the child once per Cartesian
    /// binding combination, then intersect those per-iteration child groups.
    /// Later binding expressions see earlier named bindings.
    fn b_intersection_for(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let mut operands = Vec::new();
        if !args.is_empty() {
            self.intersection_for_rec(args, children, &mut operands)?;
        }
        Ok(Node::Intersection(operands))
    }

    fn intersection_for_rec(
        &mut self,
        args: &[Arg],
        children: &[Spanned<Stmt>],
        operands: &mut Vec<Node>,
    ) -> EResult<()> {
        let Some((arg, rest)) = args.split_first() else {
            operands.push(Node::group(self.eval_children(children)?));
            return Ok(());
        };

        let values = iter_values(&self.eval_expr(&arg.value)?)?;
        for value in values {
            self.burn()?;
            self.push_scope();
            if let Some(name) = &arg.name {
                self.set_var(name, value);
            }
            let result = self.intersection_for_rec(rest, children, operands);
            self.pop_scope();
            result?;
        }
        Ok(())
    }

    fn eval_module_call(
        &mut self,
        modifier: Option<Modifier>,
        name: &str,
        args: &[Arg],
        children: &[Spanned<Stmt>],
    ) -> EResult<Vec<Node>> {
        if modifier == Some(Modifier::Disable) {
            return Ok(Vec::new());
        }

        let node = self.dispatch_module(name, args, children)?;

        // `#` highlight / `%` background wrap the produced node so the preview can
        // render them specially — `#` translucent red (kept in exports), `%`
        // translucent gray and excluded from the fused/exported mesh.
        let node = match modifier {
            Some(Modifier::Highlight) if !matches!(node, Node::Empty) => {
                Node::Highlight(Box::new(node))
            }
            Some(Modifier::Background) if !matches!(node, Node::Empty) => {
                Node::Background(Box::new(node))
            }
            _ => node,
        };

        // Tag the produced geometry with this statement's source span so the
        // preview can map a picked face back to the code (and the editor cursor
        // to geometry). Transparent to the fused mesh, the geometry cache, and
        // all mesh I/O — only the provenance partition pass reads the span. The
        // outermost (call-site) wrapper wins when partitioning, so a user module
        // call highlights its call site rather than the module body. Skipped when
        // no main-source span is available (spliced `include`/`use` statements)
        // or the call produced nothing.
        let node = match &self.cur_span {
            Some(span) if !matches!(node, Node::Empty) => Node::Provenance {
                span: span.clone(),
                child: Box::new(node),
            },
            _ => node,
        };

        if modifier == Some(Modifier::Root) {
            self.root = Some(node.clone());
        }
        if matches!(node, Node::Empty) {
            Ok(Vec::new())
        } else {
            Ok(vec![node])
        }
    }

    /// `color(c, alpha)` — wrap children in a [`Node::Color`]. `c` is a name/hex
    /// string or an `[r,g,b(,a)]` vector; an optional second positional / `alpha=`
    /// overrides the alpha. An unrecognized color warns and renders in the default.
    fn b_color(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let m = self.bind_named(&["c", "alpha"], args)?;
        let child = Node::group(self.eval_children(children)?);
        if matches!(child, Node::Empty) {
            return Ok(Node::Empty);
        }
        let alpha = m.get("alpha").and_then(Value::as_number);
        let rgba = m.get("c").and_then(|c| color::parse_color(c, alpha));
        match rgba {
            Some(rgba) => Ok(Node::Color {
                rgba,
                child: Box::new(child),
            }),
            None => {
                if let Some(c) = m.get("c") {
                    self.warn(format!("color: unknown color {}", c.repr()));
                }
                // Honor a bare `color(alpha=…)` against the default color.
                match alpha {
                    Some(a) => Ok(Node::Color {
                        rgba: [0.961, 0.647, 0.137, a as f32],
                        child: Box::new(child),
                    }),
                    None => Ok(child),
                }
            }
        }
    }

    fn dispatch_module(
        &mut self,
        name: &str,
        args: &[Arg],
        children: &[Spanned<Stmt>],
    ) -> EResult<Node> {
        match name {
            "cube" => self.b_cube(args),
            "sphere" => self.b_sphere(args),
            "cylinder" => self.b_cylinder(args),
            "polyhedron" => self.b_polyhedron(args),
            "square" => self.b_square(args),
            "circle" => self.b_circle(args),
            "polygon" => self.b_polygon(args),
            "text" => self.b_text(args),
            "linear_extrude" => self.b_linear_extrude(args, children),
            "rotate_extrude" => self.b_rotate_extrude(args, children),
            "offset" => self.b_offset(args, children),
            "projection" => {
                let m = self.bind_named(&["cut"], args)?;
                let cut = m.get("cut").map(Value::truthy).unwrap_or(false);
                Ok(Node::Projection {
                    cut,
                    child: Box::new(Node::group(self.eval_children(children)?)),
                })
            }
            "translate" => self.transform(args, children, TransformKind::Translate),
            "rotate" => self.transform(args, children, TransformKind::Rotate),
            "scale" => self.transform(args, children, TransformKind::Scale),
            "mirror" => self.transform(args, children, TransformKind::Mirror),
            "multmatrix" => self.b_multmatrix(args, children),
            "resize" => self.b_resize(args, children),
            // `color()` tints the preview; geometry is unaffected. `render()` is
            // a plain passthrough.
            "color" => self.b_color(args, children),
            "render" => Ok(Node::group(self.eval_children(children)?)),
            "union" => Ok(Node::Union(self.eval_children(children)?)),
            "difference" => Ok(Node::Difference(self.eval_children(children)?)),
            "intersection" => Ok(Node::Intersection(self.eval_children(children)?)),
            "intersection_for" => self.b_intersection_for(args, children),
            "hull" => Ok(Node::Hull(self.eval_children(children)?)),
            "minkowski" => Ok(Node::Minkowski(self.eval_children(children)?)),
            "import" => self.b_import(args),
            "surface" => self.b_surface(args),
            "group" => Ok(Node::group(self.eval_children(children)?)),
            "echo" => self.b_echo(args, children),
            "assert" => self.b_assert(args, children),
            "children" => self.b_children(args),
            _ => {
                if let Some(def) = self.lookup_module(name) {
                    self.instantiate_module(name, &def, args, children)
                } else {
                    self.warn(format!("Ignoring unknown module '{name}'"));
                    Ok(Node::Empty)
                }
            }
        }
    }

    fn eval_children(&mut self, children: &[Spanned<Stmt>]) -> EResult<Vec<Node>> {
        self.push_scope();
        let r = self.eval_stmts(children);
        self.pop_scope();
        r
    }

    fn instantiate_module(
        &mut self,
        name: &str,
        def: &Rc<ModClosure>,
        args: &[Arg],
        children: &[Spanned<Stmt>],
    ) -> EResult<Node> {
        // Guard against unbounded module recursion (shared budget with function
        // calls) so a runaway library errors gracefully instead of overflowing
        // the stack and aborting the process.
        if self.depth >= MAX_CALL_DEPTH {
            return err("maximum module recursion depth exceeded");
        }
        self.depth += 1;
        // OpenSCAD exposes the callee through parent_module() while evaluating
        // its arguments/defaults, even though `$parent_modules` still resolves
        // dynamically from the caller's special-variable frame until the body
        // frame below is pushed.
        self.module_stack.push(name.to_string());
        // Arguments are evaluated in the caller's scope; the body runs in the
        // module's captured (lexical) environment.
        let bound = match self.bind_params(&def.params, args, &def.env) {
            Ok(bound) => bound,
            Err(error) => {
                self.module_stack.pop();
                self.depth -= 1;
                return Err(error);
            }
        };
        let caller_scopes = self.scopes.clone();
        let saved = std::mem::replace(&mut self.scopes, def.env.clone());
        self.push_scope();
        for (k, v) in bound {
            self.set_var(&k, v);
        }
        // `$children` counts child *slots*, not raw statements: assignments and
        // definitions inside the child block are scoped locals, and bare blocks
        // are transparent (see `collect_child_slots`).
        let mut slots = Vec::new();
        collect_child_slots(children, &mut slots);
        self.set_var("$children", Value::Number(slots.len() as f64));
        self.children_stack
            .push((children.to_vec(), caller_scopes, self.in_main));
        self.set_var(
            "$parent_modules",
            Value::Number(self.module_stack.len() as f64),
        );
        // The body's spans index into the file the module was defined in, so only
        // attribute diagnostics to them when that file is the main source.
        let prev_main = std::mem::replace(&mut self.in_main, def.is_main);
        let r = self.eval_stmts(&def.body);
        self.in_main = prev_main;
        self.module_stack.pop();
        self.children_stack.pop();
        self.pop_scope();
        self.scopes = saved;
        self.depth -= 1;
        Ok(Node::group(r?))
    }

    /// `children()` / `children(i)` / `children([indices|range])`. Children are
    /// evaluated in the caller's lexical scope (where they were written).
    fn b_children(&mut self, args: &[Arg]) -> EResult<Node> {
        // Pop our own frame while evaluating the children: a `children()` call
        // *inside* those children must resolve to the grandparent's children
        // (one level up), matching OpenSCAD — and preventing infinite recursion
        // when a module forwards `children()` through another module's children.
        let Some(frame) = self.children_stack.pop() else {
            return Ok(Node::Empty);
        };
        let (kids, caller_scopes, caller_main) = frame.clone();
        let result = self.eval_children_frame(&kids, caller_scopes, caller_main, args);
        self.children_stack.push(frame);
        result
    }

    fn eval_children_frame(
        &mut self,
        kids: &[Spanned<Stmt>],
        caller_scopes: Vec<ScopeRef>,
        caller_main: bool,
        args: &[Arg],
    ) -> EResult<Node> {
        let kids = kids.to_vec();
        // The index selector is evaluated in the module's scope.
        let idxs: Option<Vec<usize>> = if args.is_empty() {
            None
        } else {
            let sel = self.first_positional(args)?;
            Some(match sel {
                Value::Number(n) => vec![n as usize],
                Value::Vector(ref v) => v
                    .iter()
                    .filter_map(Value::as_number)
                    .map(|n| n as usize)
                    .collect(),
                Value::Range { .. } => iter_values(&sel)?
                    .iter()
                    .filter_map(Value::as_number)
                    .map(|n| n as usize)
                    .collect(),
                _ => Vec::new(),
            })
        };
        // Evaluate the child geometry in the caller's lexical environment, with
        // the caller's main-ness so diagnostic spans attribute to the call site.
        let saved = std::mem::replace(&mut self.scopes, caller_scopes);
        let prev_main = std::mem::replace(&mut self.in_main, caller_main);
        self.push_scope();
        let result = (|| -> EResult<Node> {
            match idxs {
                None => Ok(Node::group(self.eval_stmts(&kids)?)),
                Some(idxs) => {
                    // Indexed `children(i)` skips the whole-block evaluation, so
                    // first hoist the block's scoped locals (assignments and
                    // definitions) — a selected child may reference a variable
                    // defined earlier in the same child block, e.g. BOSL2's
                    // `attachable(){ x = ..; shape(x); children(); }`.
                    self.eval_defs_and_assigns(&kids)?;
                    let mut slots = Vec::new();
                    collect_child_slots(&kids, &mut slots);
                    let mut out = Vec::new();
                    for i in idxs {
                        if let Some(stmt) = slots.get(i) {
                            out.extend(self.eval_geom(&stmt.node)?);
                        }
                    }
                    Ok(Node::group(out))
                }
            }
        })();
        self.pop_scope();
        self.scopes = saved;
        self.in_main = prev_main;
        result
    }

    // ---- builtin modules ----------------------------------------------

    fn b_cube(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["size", "center"], args)?;
        let size = match m.get("size") {
            Some(Value::Number(n)) => [*n, *n, *n],
            Some(v @ Value::Vector(_)) => v.as_vec3().unwrap_or([1.0, 1.0, 1.0]),
            _ => [1.0, 1.0, 1.0],
        };
        let center = m.get("center").map(Value::truthy).unwrap_or(false);
        Ok(Node::Cube { size, center })
    }

    fn b_sphere(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["r"], args)?;
        let r = if let Some(d) = m.get("d").and_then(Value::as_number) {
            d / 2.0
        } else {
            m.get("r").and_then(Value::as_number).unwrap_or(1.0)
        };
        Ok(Node::Sphere {
            r,
            frags: self.frag_spec(&m),
        })
    }

    fn b_cylinder(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["h", "r1", "r2", "center"], args)?;
        let h = m.get("h").and_then(Value::as_number).unwrap_or(1.0);

        // r / d apply to both ends; r1/r2/d1/d2 override per end.
        let base_r = m
            .get("d")
            .and_then(Value::as_number)
            .map(|d| d / 2.0)
            .or_else(|| m.get("r").and_then(Value::as_number));

        let r1 = m
            .get("d1")
            .and_then(Value::as_number)
            .map(|d| d / 2.0)
            .or_else(|| m.get("r1").and_then(Value::as_number))
            .or(base_r)
            .unwrap_or(1.0);
        let r2 = m
            .get("d2")
            .and_then(Value::as_number)
            .map(|d| d / 2.0)
            .or_else(|| m.get("r2").and_then(Value::as_number))
            .or(base_r)
            .unwrap_or(1.0);

        let center = m.get("center").map(Value::truthy).unwrap_or(false);
        Ok(Node::Cylinder {
            h,
            r1,
            r2,
            center,
            frags: self.frag_spec(&m),
        })
    }

    fn b_import(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["file"], args)?;
        let path = match m.get("file") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Ok(Node::Empty),
        };
        let format = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        match self.resolver.load_bytes(&path, &self.cur_dir) {
            Some(data) => Ok(Node::Import { data, format }),
            None => {
                self.warn(format!("Can't open import file '{path}'"));
                Ok(Node::Empty)
            }
        }
    }

    fn b_surface(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["file", "center", "convexity", "invert"], args)?;
        let path = match m.get("file") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Ok(Node::Empty),
        };
        let center = m.get("center").map(Value::truthy).unwrap_or(false);
        let invert = m.get("invert").map(Value::truthy).unwrap_or(false);
        let is_png = path.to_ascii_lowercase().ends_with(".png");
        let rows = if is_png {
            let Some(bytes) = self.resolver.load_bytes(&path, &self.cur_dir) else {
                self.warn(format!("Can't open surface file '{path}'"));
                return Ok(Node::Empty);
            };
            match png_heightmap(&bytes, invert) {
                Ok(rows) => rows,
                Err(e) => {
                    self.warn(format!("surface(): {e} in '{path}'"));
                    return Ok(Node::Empty);
                }
            }
        } else {
            let Some(lf) = self.resolver.load(&path, &self.cur_dir) else {
                self.warn(format!("Can't open surface file '{path}'"));
                return Ok(Node::Empty);
            };
            // Whitespace-separated rows of z-values; `#` lines are comments.
            lf.source
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| {
                    l.split_whitespace()
                        .filter_map(|s| s.parse::<f64>().ok())
                        .collect()
                })
                .filter(|r: &Vec<f64>| !r.is_empty())
                .collect()
        };
        Ok(surface_polyhedron(&rows, center, is_png))
    }

    fn b_polyhedron(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["points", "faces", "convexity"], args)?;
        let points: Vec<Vec3> = match m.get("points") {
            Some(Value::Vector(v)) => v.iter().map(value_to_point3).collect(),
            _ => Vec::new(),
        };
        // `faces` (current) or `triangles` (legacy).
        let faces_val = m.get("faces").or_else(|| m.get("triangles"));
        let faces: Vec<Vec<u32>> = match faces_val {
            Some(Value::Vector(v)) => v.iter().map(value_to_face).collect(),
            _ => Vec::new(),
        };
        Ok(Node::Polyhedron { points, faces })
    }

    fn b_square(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["size", "center"], args)?;
        let size = match m.get("size") {
            Some(Value::Number(n)) => [*n, *n],
            Some(Value::Vector(v)) => {
                let g = |i: usize| v.get(i).and_then(Value::as_number).unwrap_or(0.0);
                [g(0), g(1)]
            }
            _ => [1.0, 1.0],
        };
        let center = m.get("center").map(Value::truthy).unwrap_or(false);
        Ok(Node::Square { size, center })
    }

    fn b_circle(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["r"], args)?;
        let r = if let Some(d) = m.get("d").and_then(Value::as_number) {
            d / 2.0
        } else {
            m.get("r").and_then(Value::as_number).unwrap_or(1.0)
        };
        Ok(Node::Circle {
            r,
            frags: self.frag_spec(&m),
        })
    }

    fn b_text(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(
            &[
                "text",
                "size",
                "font",
                "direction",
                "language",
                "script",
                "halign",
                "valign",
                "spacing",
            ],
            args,
        )?;
        let text = m.get("text").map(Value::to_str).unwrap_or_default();
        let size = m.get("size").and_then(Value::as_number).unwrap_or(10.0);
        // Resolve `font` against the shared font database: the bundled Liberation
        // family (Sans/Serif/Mono × Regular/Bold/Italic/BoldItalic — the exact
        // files OpenSCAD ships, always available and byte-for-byte identical)
        // plus any system fonts the host registered. An unknown *family* falls
        // back to Liberation Sans with a warning.
        let font = m.get("font").map(Value::to_str).unwrap_or_default();
        let sopt = |k: &str, d: &str| m.get(k).map(Value::to_str).unwrap_or_else(|| d.to_string());
        let halign = sopt("halign", "left");
        let valign = sopt("valign", "baseline");
        let spacing = m.get("spacing").and_then(Value::as_number).unwrap_or(1.0);
        let direction = sopt("direction", "ltr");
        // Curve resolution follows `$fn` (like other curved primitives).
        let fn_ = self.lookup_var("$fn").as_number().unwrap_or(0.0);
        let segments = if fn_ >= 3.0 {
            ((fn_ / 4.0).ceil() as usize).max(2)
        } else {
            8
        };

        let rendered = text::render_text(
            &font,
            &text::TextParams {
                text: &text,
                size,
                halign: &halign,
                valign: &valign,
                spacing,
                direction: &direction,
                segments,
            },
        );
        // LuaCAD deviation: no fonts are bundled, so resolution can come up
        // empty. Warn and emit no geometry rather than aborting the render.
        let Some((points, paths, family_known)) = rendered else {
            self.warn("text(): no font found, and no fallback is installed".to_string());
            return Ok(Node::Empty);
        };
        if !font.is_empty() && !family_known {
            self.warn(format!(
                "text(): font {font:?} not available; using an installed fallback"
            ));
        }
        Ok(Node::Polygon {
            points,
            paths: Some(paths),
        })
    }

    fn b_polygon(&mut self, args: &[Arg]) -> EResult<Node> {
        let m = self.bind_named(&["points", "paths"], args)?;
        let points: Vec<[f64; 2]> = match m.get("points") {
            Some(Value::Vector(v)) => v
                .iter()
                .map(|p| {
                    if let Value::Vector(c) = p {
                        let g = |i: usize| c.get(i).and_then(Value::as_number).unwrap_or(0.0);
                        [g(0), g(1)]
                    } else {
                        [0.0, 0.0]
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let paths = match m.get("paths") {
            Some(Value::Vector(v)) => Some(
                v.iter()
                    .map(|p| match p {
                        Value::Vector(idx) => idx
                            .iter()
                            .filter_map(Value::as_number)
                            .map(|n| n as u32)
                            .collect(),
                        _ => Vec::new(),
                    })
                    .collect(),
            ),
            _ => None,
        };
        Ok(Node::Polygon { points, paths })
    }

    fn b_linear_extrude(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let m = self.bind_named(&["height"], args)?;
        // OpenSCAD's `linear_extrude` only accepts `height` (named or first
        // positional) — `h` is NOT an alias. Passing `h=` leaves height unset,
        // so it falls back to the default of 100 (OpenSCAD warns "variable h not
        // specified as parameter" and does the same). Accepting `h` here would
        // silently disagree with OpenSCAD, e.g. BOSL2-style `linear_extrude(h=x)`.
        let height = m.get("height").and_then(Value::as_number).unwrap_or(100.0);
        let center = m.get("center").map(Value::truthy).unwrap_or(false);
        let twist = m.get("twist").and_then(Value::as_number).unwrap_or(0.0);
        let scale = match m.get("scale") {
            Some(Value::Number(n)) => [*n, *n],
            Some(Value::Vector(v)) => {
                let g = |i: usize| v.get(i).and_then(Value::as_number).unwrap_or(1.0);
                [g(0), g(1)]
            }
            _ => [1.0, 1.0],
        };
        let slices = m
            .get("slices")
            .and_then(Value::as_number)
            .map(|s| s as u32)
            .unwrap_or_else(|| {
                if twist == 0.0 {
                    1
                } else {
                    (twist.abs() / 15.0).ceil().max(1.0) as u32
                }
            })
            .max(1);
        let child = Box::new(Node::group(self.eval_children(children)?));
        // `v`: extrude the profile along a direction vector instead of straight
        // up Z. OpenSCAD places the top profile at `height * normalize(v)`,
        // forming an oblique prism — equivalent to a straight extrude of the
        // vector's z-extent followed by a z→xy shear.
        let v = m.get("v").and_then(|val| match val {
            Value::Vector(c) => {
                let g = |i: usize| c.get(i).and_then(Value::as_number).unwrap_or(0.0);
                Some([g(0), g(1), g(2)])
            }
            _ => None,
        });
        if let Some(v) = v {
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if len > 1e-12 {
                let d = [
                    height * v[0] / len,
                    height * v[1] / len,
                    height * v[2] / len,
                ];
                let hz = d[2];
                if hz.abs() > 1e-9 {
                    let ext = Node::LinearExtrude {
                        height: hz,
                        center,
                        twist,
                        scale,
                        slices,
                        child,
                    };
                    // Shear: a point at height z is displaced in xy by
                    // (dx, dy) * (z / hz), so the top (z = hz) lands at (dx, dy).
                    let m4 = [
                        [1.0, 0.0, d[0] / hz, 0.0],
                        [0.0, 1.0, d[1] / hz, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ];
                    return Ok(Node::MultMatrix {
                        m: m4,
                        child: Box::new(ext),
                    });
                }
            }
        }
        Ok(Node::LinearExtrude {
            height,
            center,
            twist,
            scale,
            slices,
            child,
        })
    }

    fn b_offset(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let m = self.bind_named(&["r", "delta"], args)?;
        let r = m.get("r").and_then(Value::as_number);
        let delta = m.get("delta").and_then(Value::as_number);
        let chamfer = m.get("chamfer").map(Value::truthy).unwrap_or(false);
        // `r` takes precedence; default to r=1 if neither given.
        let (r, delta) = match (r, delta) {
            (Some(r), _) => (r, 0.0),
            (None, Some(d)) => (0.0, d),
            (None, None) => (1.0, 0.0),
        };
        let child = Box::new(Node::group(self.eval_children(children)?));
        Ok(Node::Offset {
            r,
            delta,
            chamfer,
            frags: self.frag_spec(&m),
            child,
        })
    }

    fn b_rotate_extrude(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let m = self.bind_named(&["angle"], args)?;
        let angle = m.get("angle").and_then(Value::as_number).unwrap_or(360.0);
        let start = m.get("start").and_then(Value::as_number).unwrap_or(0.0);
        let child = Box::new(Node::group(self.eval_children(children)?));
        let node = Node::RotateExtrude {
            angle,
            frags: self.frag_spec(&m),
            child,
        };
        // `start` (OpenSCAD 2023.x+) offsets where a partial sweep begins: the
        // profile is swept from `start` to `start + angle` about Z. That is the
        // plain `[0, angle]` extrude rotated by `start` degrees about Z.
        if start != 0.0 {
            let (s, c) = start.to_radians().sin_cos();
            return Ok(Node::MultMatrix {
                m: [
                    [c, -s, 0.0, 0.0],
                    [s, c, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                child: Box::new(node),
            });
        }
        Ok(node)
    }

    /// Resolve the fragment spec from call-site `$fn/$fa/$fs` args, falling back
    /// to the ambient special variables.
    fn frag_spec(&self, m: &FastMap<String, Value>) -> FragmentSpec {
        let pick = |key: &str, default: f64| -> f64 {
            m.get(key)
                .and_then(Value::as_number)
                .unwrap_or_else(|| self.lookup_var(key).as_number().unwrap_or(default))
        };
        FragmentSpec {
            fn_: pick("$fn", 0.0),
            fa: pick("$fa", 12.0),
            fs: pick("$fs", 2.0),
        }
    }

    fn transform(
        &mut self,
        args: &[Arg],
        children: &[Spanned<Stmt>],
        kind: TransformKind,
    ) -> EResult<Node> {
        let child = Node::group(self.eval_children(children)?);
        if matches!(child, Node::Empty) {
            return Ok(Node::Empty);
        }
        let child = Box::new(child);
        // Bind by real signatures (named, positional, or mixed) rather than
        // reading a single positional — `translate(v=…)`, `rotate(a=…, v=…)`,
        // etc. all resolve correctly.
        let node = match kind {
            TransformKind::Translate => {
                let m = self.bind_named(&["v"], args)?;
                let v = m
                    .get("v")
                    .and_then(Value::as_vec3)
                    .unwrap_or([0.0, 0.0, 0.0]);
                Node::Translate { v, child }
            }
            TransformKind::Scale => {
                let m = self.bind_named(&["v"], args)?;
                let v = m.get("v").map(scale_vec3).unwrap_or([1.0, 1.0, 1.0]);
                Node::Scale { v, child }
            }
            TransformKind::Mirror => {
                let m = self.bind_named(&["v"], args)?;
                let v = m
                    .get("v")
                    .and_then(Value::as_vec3)
                    .unwrap_or([1.0, 0.0, 0.0]);
                Node::Mirror { v, child }
            }
            TransformKind::Rotate => {
                let m = self.bind_named(&["a", "v"], args)?;
                let axis = m.get("v").and_then(Value::as_vec3);
                match (m.get("a"), axis) {
                    // `rotate(a, v)`: axis-angle about a non-degenerate axis.
                    // Lower to an affine matrix via Rodrigues — the existing
                    // `MultMatrix` node already renders and caches, so no new
                    // IR variant is needed.
                    (Some(Value::Number(angle)), Some(axis))
                        if axis[0].hypot(axis[1]).hypot(axis[2]) > 1e-9 =>
                    {
                        Node::MultMatrix {
                            m: axis_angle_matrix(axis, *angle),
                            child,
                        }
                    }
                    // `rotate(a)`: scalar → Z rotation; vector → Euler X,Y,Z.
                    // A missing or zero-length axis falls back here per the manual.
                    (a, _) => {
                        let deg = match a {
                            Some(Value::Number(n)) => [0.0, 0.0, *n],
                            Some(v) => v.as_vec3().unwrap_or([0.0, 0.0, 0.0]),
                            None => [0.0, 0.0, 0.0],
                        };
                        Node::Rotate { deg, child }
                    }
                }
            }
        };
        Ok(node)
    }

    fn b_multmatrix(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let child = Node::group(self.eval_children(children)?);
        if matches!(child, Node::Empty) {
            return Ok(Node::Empty);
        }
        let args = self.bind_named(&["m"], args)?;
        let m = args
            .get("m")
            .map(matrix_from_value)
            .unwrap_or_else(|| matrix_from_value(&Value::Undef));
        Ok(Node::MultMatrix {
            m,
            child: Box::new(child),
        })
    }

    fn b_resize(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        let child = Node::group(self.eval_children(children)?);
        if matches!(child, Node::Empty) {
            return Ok(Node::Empty);
        }
        let m = self.bind_named(&["newsize", "auto"], args)?;
        let new = m
            .get("newsize")
            .and_then(Value::as_vec3)
            .unwrap_or([0.0, 0.0, 0.0]);
        let auto = match m.get("auto") {
            Some(Value::Bool(b)) => [*b, *b, *b],
            Some(Value::Vector(v)) => {
                let g = |i: usize| v.get(i).map(Value::truthy).unwrap_or(false);
                [g(0), g(1), g(2)]
            }
            _ => [false, false, false],
        };
        Ok(Node::Resize {
            new,
            auto,
            child: Box::new(child),
        })
    }

    /// Format and record an `echo(...)`; shared by the module and expression forms.
    fn do_echo(&mut self, args: &[Arg]) -> EResult<()> {
        let mut parts = Vec::new();
        for a in args {
            let v = self.eval_expr(&a.value)?;
            match &a.name {
                Some(n) => parts.push(format!("{} = {}", n, v.repr())),
                None => parts.push(v.repr()),
            }
        }
        self.echoes.push(format!("ECHO: {}", parts.join(", ")));
        Ok(())
    }

    fn b_echo(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        self.do_echo(args)?;
        Ok(Node::group(self.eval_children(children)?))
    }

    /// Assert semantics shared by the module and expression forms; returns
    /// whether the assertion passed (errors on failure).
    fn do_assert(&mut self, args: &[Arg]) -> EResult<()> {
        // Count every assert that executes (passing or failing) so the BOSL2
        // oracle can reject vacuous passes.
        self.asserts_run += 1;
        let cond = self.first_positional(args).unwrap_or(Value::Undef).truthy();
        if !cond {
            let msg = args
                .get(1)
                .map(|a| self.eval_expr(&a.value))
                .transpose()?
                .map(|v| v.to_str())
                .unwrap_or_default();
            return err(format!("Assertion failed: {msg}"));
        }
        Ok(())
    }

    fn b_assert(&mut self, args: &[Arg], children: &[Spanned<Stmt>]) -> EResult<Node> {
        self.do_assert(args)?;
        Ok(Node::group(self.eval_children(children)?))
    }

    // ---- argument binding ---------------------------------------------

    /// Bind arguments by the given positional parameter names, honoring named
    /// args (including out-of-band `$fn`-style and `d`/`r1` overrides).
    fn bind_named(&mut self, positional: &[&str], args: &[Arg]) -> EResult<FastMap<String, Value>> {
        let mut map = FastMap::default();
        let mut pos = 0;
        for a in args {
            let v = self.eval_expr(&a.value)?;
            match &a.name {
                Some(n) => {
                    map.insert(n.clone(), v);
                }
                None => {
                    if let Some(name) = positional.get(pos) {
                        map.insert((*name).to_string(), v);
                    }
                    pos += 1;
                }
            }
        }
        Ok(map)
    }

    fn bind_params(
        &mut self,
        params: &[Param],
        args: &[Arg],
        definition_env: &[ScopeRef],
    ) -> EResult<FastMap<String, Value>> {
        let mut map = FastMap::default();

        // Explicit arguments are eager and evaluated left-to-right in the
        // caller's scope. Named arguments that aren't declared parameters still
        // become locals (with an OpenSCAD warning omitted here), so retain them.
        let mut pos = 0;
        for a in args {
            let v = self.eval_expr(&a.value)?;
            match &a.name {
                Some(n) => {
                    map.insert(n.clone(), v);
                }
                None => {
                    if let Some(p) = params.get(pos) {
                        map.insert(p.name.clone(), v);
                    }
                    pos += 1;
                }
            }
        }

        self.fill_param_defaults(params, definition_env, &mut map)?;
        Ok(map)
    }

    /// Fill only omitted parameters, evaluating their defaults in the closure's
    /// lexical environment. `$` variables remain dynamic because only the
    /// ordinary scope chain is swapped. A missing parameter without a default is
    /// explicitly bound to `undef`, so it still shadows an outer name in the body.
    fn fill_param_defaults(
        &mut self,
        params: &[Param],
        definition_env: &[ScopeRef],
        bound: &mut FastMap<String, Value>,
    ) -> EResult<()> {
        let saved = std::mem::replace(&mut self.scopes, definition_env.to_vec());
        let result = (|| -> EResult<()> {
            for p in params {
                if bound.contains_key(&p.name) {
                    continue;
                }
                let value = match &p.default {
                    Some(default) => self.eval_expr(default)?,
                    None => Value::Undef,
                };
                bound.insert(p.name.clone(), value);
            }
            Ok(())
        })();
        self.scopes = saved;
        result
    }

    fn first_positional(&mut self, args: &[Arg]) -> EResult<Value> {
        for a in args {
            if a.name.is_none() {
                return self.eval_expr(&a.value);
            }
        }
        Ok(Value::Undef)
    }

    // ---- expressions ---------------------------------------------------

    fn eval_expr(&mut self, expr: &Expr) -> EResult<Value> {
        // One fuel unit per expression node evaluated — bounds comprehension
        // element evaluation, recursion through expressions, and huge folds.
        self.burn()?;
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Undef => Ok(Value::Undef),
            Expr::Ident(name) => Ok(self.lookup_var(name)),
            Expr::Vector(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    self.eval_list_elem(e, &mut out)?;
                }
                Ok(value::vector(out))
            }
            Expr::Range { start, step, end } => {
                let s = self.eval_expr(start)?.as_number().unwrap_or(f64::NAN);
                let e = self.eval_expr(end)?.as_number().unwrap_or(f64::NAN);
                match step {
                    // 3-arg range: kept as written (empty if step direction
                    // contradicts start/end).
                    Some(x) => {
                        let st = self.eval_expr(x)?.as_number().unwrap_or(1.0);
                        Ok(Value::Range {
                            start: s,
                            step: st,
                            end: e,
                        })
                    }
                    // 2-arg range: OpenSCAD normalizes to ascending, step 1
                    // (so `[5:2]` becomes `[2:1:5]`).
                    None => {
                        let (lo, hi) = if s <= e { (s, e) } else { (e, s) };
                        Ok(Value::Range {
                            start: lo,
                            step: 1.0,
                            end: hi,
                        })
                    }
                }
            }
            Expr::Unary { op, expr } => {
                let v = self.eval_expr(expr)?;
                Ok(value::unary(*op, v))
            }
            // `&&` / `||` short-circuit (the right side may assert or error).
            Expr::Binary {
                op: BinOp::And,
                lhs,
                rhs,
            } => {
                if !self.eval_expr(lhs)?.truthy() {
                    Ok(Value::Bool(false))
                } else {
                    Ok(Value::Bool(self.eval_expr(rhs)?.truthy()))
                }
            }
            Expr::Binary {
                op: BinOp::Or,
                lhs,
                rhs,
            } => {
                if self.eval_expr(lhs)?.truthy() {
                    Ok(Value::Bool(true))
                } else {
                    Ok(Value::Bool(self.eval_expr(rhs)?.truthy()))
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let l = self.eval_expr(lhs)?;
                let r = self.eval_expr(rhs)?;
                Ok(value::binary(*op, l, r))
            }
            Expr::Ternary { cond, then, els } => {
                if self.eval_expr(cond)?.truthy() {
                    self.eval_expr(then)
                } else {
                    self.eval_expr(els)
                }
            }
            Expr::Index { base, index } => {
                let b = self.eval_expr(base)?;
                let i = self.eval_expr(index)?;
                Ok(index_value(&b, &i))
            }
            Expr::Member { base, field } => {
                let b = self.eval_expr(base)?;
                let idx = match field.as_str() {
                    "x" => 0,
                    "y" => 1,
                    "z" => 2,
                    _ => return Ok(Value::Undef),
                };
                Ok(index_value(&b, &Value::Number(idx as f64)))
            }
            Expr::Let { bindings, body } => {
                self.push_scope();
                for (n, e) in bindings {
                    let v = self.eval_expr(e)?;
                    self.set_var(n, v);
                }
                let r = self.eval_expr(body);
                self.pop_scope();
                r
            }
            Expr::Call { name, args } => self.eval_call(name, args),
            Expr::FunctionLiteral { params, body } => Ok(Value::Function(Rc::new(FnClosure {
                params: params.clone(),
                body: (**body).clone(),
                env: self.scopes.clone(),
                name: None,
                chunk: OnceCell::new(),
            }))),
            Expr::CallValue { callee, args } => {
                let c = self.eval_expr(callee)?;
                if let Value::Function(f) = c {
                    self.call_function(&f, args)
                } else {
                    Ok(Value::Undef)
                }
            }
            Expr::Echo { args, body } => {
                self.do_echo(args)?;
                self.eval_expr(body)
            }
            Expr::Assert { args, body } => {
                self.do_assert(args)?;
                self.eval_expr(body)
            }
        }
    }

    /// Call a function closure: arguments are bound in the caller's scope, then
    /// the body runs in the closure's captured lexical environment.
    ///
    /// Self-tail-calls are eliminated: when the body reduces (through ternaries
    /// and `let`s) to a call of the same function in tail position, the frame is
    /// reused in a loop instead of recursing, so accumulator-style recursion
    /// runs to arbitrary depth without overflowing the (small, on wasm) stack.
    fn call_function(&mut self, f: &Rc<FnClosure>, args: &[Arg]) -> EResult<Value> {
        // Positional fast path for compiled functions: evaluate arguments
        // straight into the VM's local frame, skipping the per-call binding map.
        if args.iter().all(|a| a.name.is_none()) && args.len() <= f.params.len() {
            if let Some(chunk) = f
                .chunk
                .get_or_init(|| vm::compile_fn(f).map(Rc::new))
                .clone()
            {
                let mut locals = vec![Value::Undef; chunk.n_locals()];
                // Explicit arguments are evaluated first in the caller's scope.
                for (i, a) in args.iter().enumerate() {
                    locals[i] = self.eval_expr(&a.value)?;
                }

                // Only omitted defaults run, and they resolve ordinary names in
                // the function's captured lexical environment. Special `$`
                // variables remain dynamic because `specials` is not swapped.
                let saved = std::mem::replace(&mut self.scopes, f.env.clone());
                let defaults = (|| -> EResult<()> {
                    for (i, p) in f.params.iter().enumerate().skip(args.len()) {
                        if let Some(default) = &p.default {
                            locals[i] = self.eval_expr(default)?;
                        }
                    }
                    Ok(())
                })();
                self.scopes = saved;
                defaults?;
                return self.run_chunk(f, chunk, locals);
            }
        }
        let bound = self.bind_params(&f.params, args, &f.env)?;
        self.run_bound(f, bound)
    }

    /// Run a compiled chunk with a prepared local frame (depth guard + scope
    /// swap to the closure's captured environment).
    fn run_chunk(
        &mut self,
        f: &Rc<FnClosure>,
        chunk: Rc<vm::Chunk>,
        locals: Vec<Value>,
    ) -> EResult<Value> {
        if self.depth >= MAX_CALL_DEPTH {
            return err("maximum call depth exceeded");
        }
        self.depth += 1;
        let saved = std::mem::replace(&mut self.scopes, f.env.clone());
        let r = vm::run(self, &chunk, locals, f);
        self.scopes = saved;
        self.depth -= 1;
        r
    }

    /// Call a function with already-evaluated positional argument values
    /// (used by the bytecode VM). Named args and defaults are handled by binding
    /// positionally against the parameter list.
    fn call_function_values(&mut self, f: &Rc<FnClosure>, argv: Vec<Value>) -> EResult<Value> {
        let mut bound = FastMap::default();
        for (i, v) in argv.into_iter().enumerate() {
            if let Some(p) = f.params.get(i) {
                bound.insert(p.name.clone(), v);
            }
        }
        self.fill_param_defaults(&f.params, &f.env, &mut bound)?;
        self.run_bound(f, bound)
    }

    /// Resolve a name to a user function / function-valued variable / builtin
    /// and call it with pre-evaluated positional values (VM call path). Mirrors
    /// [`Self::eval_call`] but with values instead of argument expressions.
    fn call_named_values(&mut self, name: &str, argv: Vec<Value>) -> EResult<Value> {
        if let Some(def) = self.lookup_func(name) {
            return self.call_function_values(&def, argv);
        }
        if let Value::Function(f) = self.lookup_var(name) {
            return self.call_function_values(&f, argv);
        }
        Ok(self.call_builtin_fn(name, &argv))
    }

    /// Invoke a builtin function, routing any warnings it emits through `warn`
    /// so they pick up the current statement's span.
    fn call_builtin_fn(&mut self, name: &str, args: &[Value]) -> Value {
        if name == "parent_module" {
            let index = match args.first() {
                None => 1isize,
                Some(Value::Number(n)) => *n as isize,
                Some(_) => return Value::Undef,
            };
            if index < 0 {
                return Value::Undef;
            }
            return self
                .module_stack
                .iter()
                .rev()
                .nth(index as usize)
                .cloned()
                .map(Value::Str)
                .unwrap_or(Value::Undef);
        }

        let mut ws: Vec<String> = Vec::new();
        let v = builtin_fn(name, args, &mut ws);
        for m in ws {
            self.warn(m);
        }
        v
    }

    /// Run a function whose parameters are already bound (by name → value),
    /// dispatching to the bytecode VM when the body compiled, else the tree-walk
    /// (which also handles tail-call elimination). Shared by all call paths.
    fn run_bound(
        &mut self,
        f: &Rc<FnClosure>,
        mut bound: FastMap<String, Value>,
    ) -> EResult<Value> {
        // Fast path: a compiled chunk uses slot-based locals (no per-call maps).
        let compiled = f
            .chunk
            .get_or_init(|| vm::compile_fn(f).map(Rc::new))
            .clone();
        if let Some(chunk) = compiled {
            let mut locals = vec![Value::Undef; chunk.n_locals()];
            for (i, p) in f.params.iter().enumerate() {
                if let Some(v) = bound.remove(&p.name) {
                    locals[i] = v;
                }
            }
            return self.run_chunk(f, chunk, locals);
        }

        // Tree-walk fallback with self-tail-call elimination.
        if self.depth >= MAX_CALL_DEPTH {
            return err("maximum call depth exceeded");
        }
        self.depth += 1;
        let mut iters = 0usize;
        let result = loop {
            let saved = std::mem::replace(&mut self.scopes, f.env.clone());
            self.push_scope();
            for (k, v) in bound.drain() {
                self.set_var(&k, v);
            }
            let tail = self.eval_tail(&f.body, f);
            self.pop_scope();
            self.scopes = saved;
            match tail {
                Err(e) => break Err(e),
                Ok(TailResult::Value(v)) => break Ok(v),
                Ok(TailResult::TailCall(next)) => {
                    bound = next;
                    iters += 1;
                    if iters > MAX_RANGE_ITERS {
                        break err("tail recursion exceeded iteration limit");
                    }
                }
            }
        };
        self.depth -= 1;
        result
    }

    /// Evaluate `expr` in tail position relative to function `f`. Returns either
    /// a final value or a request to tail-call `f` with fresh arguments (already
    /// evaluated in the current frame).
    fn eval_tail(&mut self, expr: &Expr, f: &Rc<FnClosure>) -> EResult<TailResult> {
        match expr {
            Expr::Ternary { cond, then, els } => {
                let branch = if self.eval_expr(cond)?.truthy() {
                    then
                } else {
                    els
                };
                self.eval_tail(branch, f)
            }
            Expr::Let { bindings, body } => {
                self.push_scope();
                for (n, e) in bindings {
                    let v = self.eval_expr(e)?;
                    self.set_var(n, v);
                }
                let r = self.eval_tail(body, f);
                self.pop_scope();
                r
            }
            Expr::Echo { args, body } => {
                self.do_echo(args)?;
                self.eval_tail(body, f)
            }
            Expr::Assert { args, body } => {
                self.do_assert(args)?;
                self.eval_tail(body, f)
            }
            Expr::Call { name, args } => {
                // A self-call in tail position becomes a loop iteration.
                if let Some(g) = self.lookup_func(name) {
                    if Rc::ptr_eq(&g, f) {
                        let next = self.bind_params(&f.params, args, &f.env)?;
                        return Ok(TailResult::TailCall(next));
                    }
                }
                Ok(TailResult::Value(self.eval_expr(expr)?))
            }
            _ => Ok(TailResult::Value(self.eval_expr(expr)?)),
        }
    }

    fn eval_list_elem(&mut self, el: &ListElem, out: &mut Vec<Value>) -> EResult<()> {
        match el {
            ListElem::Item(e) => {
                out.push(self.eval_expr(e)?);
            }
            ListElem::Each(inner) => {
                // Evaluate the operand element, then splice each produced value.
                let mut temp = Vec::new();
                self.eval_list_elem(inner, &mut temp)?;
                for v in temp {
                    match v {
                        Value::Vector(xs) => out.extend(xs.iter().cloned()),
                        r @ Value::Range { .. } => out.extend(iter_values(&r)?),
                        Value::Undef => {}
                        other => out.push(other),
                    }
                }
            }
            ListElem::For { bindings, body } => {
                self.lc_for_rec(bindings, body, out)?;
            }
            ListElem::CFor {
                init,
                cond,
                update,
                body,
            } => {
                self.push_scope();
                for (n, e) in init {
                    let v = self.eval_expr(e)?;
                    self.set_var(n, v);
                }
                let mut iters = 0usize;
                loop {
                    if !self.eval_expr(cond)?.truthy() {
                        break;
                    }
                    self.eval_list_elem(body, out)?;
                    // Updates are applied sequentially, each seeing prior updates
                    // in the same clause (matches OpenSCAD's accumulator form).
                    for (n, e) in update {
                        let v = self.eval_expr(e)?;
                        self.set_var(n, v);
                    }
                    iters += 1;
                    if iters > MAX_RANGE_ITERS {
                        self.pop_scope();
                        return err("C-style for exceeded iteration limit");
                    }
                }
                self.pop_scope();
            }
            ListElem::Let { bindings, body } => {
                self.push_scope();
                for (n, e) in bindings {
                    let val = self.eval_expr(e)?;
                    self.set_var(n, val);
                }
                let r = self.eval_list_elem(body, out);
                self.pop_scope();
                r?;
            }
            ListElem::If { cond, then, els } => {
                if self.eval_expr(cond)?.truthy() {
                    self.eval_list_elem(then, out)?;
                } else if let Some(e) = els {
                    self.eval_list_elem(e, out)?;
                }
            }
        }
        Ok(())
    }

    fn lc_for_rec(
        &mut self,
        bindings: &[(String, Expr)],
        body: &ListElem,
        out: &mut Vec<Value>,
    ) -> EResult<()> {
        if bindings.is_empty() {
            return self.eval_list_elem(body, out);
        }
        let (name, expr) = &bindings[0];
        let vals = iter_values(&self.eval_expr(expr)?)?;
        for v in vals {
            self.push_scope();
            self.set_var(name, v);
            let r = self.lc_for_rec(&bindings[1..], body, out);
            self.pop_scope();
            r?;
        }
        Ok(())
    }

    fn eval_call(&mut self, name: &str, args: &[Arg]) -> EResult<Value> {
        // User-defined function?
        if let Some(def) = self.lookup_func(name) {
            return self.call_function(&def, args);
        }
        // A variable holding a function value?
        if let Value::Function(f) = self.lookup_var(name) {
            return self.call_function(&f, args);
        }
        // Builtins.
        let vals: Vec<Value> = args
            .iter()
            .map(|a| self.eval_expr(&a.value))
            .collect::<EResult<_>>()?;
        Ok(self.call_builtin_fn(name, &vals))
    }
}

enum TransformKind {
    Translate,
    Rotate,
    Scale,
    Mirror,
}

/// Build a 4x4 affine matrix from a value (list of rows), padding from identity.
fn matrix_from_value(v: &Value) -> [[f64; 4]; 4] {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    if let Value::Vector(rows) = v {
        for (i, row) in rows.iter().enumerate().take(4) {
            if let Value::Vector(cols) = row {
                for (j, c) in cols.iter().enumerate().take(4) {
                    if let Some(n) = c.as_number() {
                        m[i][j] = n;
                    }
                }
            }
        }
    }
    m
}

fn scale_vec3(v: &Value) -> Vec3 {
    match v {
        Value::Number(n) => [*n, *n, *n],
        Value::Vector(xs) => {
            let get = |i: usize| xs.get(i).and_then(Value::as_number).unwrap_or(1.0);
            [get(0), get(1), get(2)]
        }
        _ => [1.0, 1.0, 1.0],
    }
}

/// Rodrigues rotation of `angle_deg` about `axis` as a row-major 4x4 affine
/// matrix (the `MultMatrix` convention: rotation in the 3x3 block, zero
/// translation column). A pure rotation has determinant +1, so the renderer's
/// negative-determinant winding flip never triggers. The caller guarantees a
/// non-degenerate axis.
fn axis_angle_matrix(axis: Vec3, angle_deg: f64) -> [[f64; 4]; 4] {
    let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let [x, y, z] = [axis[0] / len, axis[1] / len, axis[2] / len];
    let t = angle_deg.to_radians();
    let (c, s, k) = (t.cos(), t.sin(), 1.0 - t.cos());
    [
        [c + x * x * k, x * y * k - z * s, x * z * k + y * s, 0.0],
        [y * x * k + z * s, c + y * y * k, y * z * k - x * s, 0.0],
        [z * x * k - y * s, z * y * k + x * s, c + z * z * k, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn value_to_point3(v: &Value) -> Vec3 {
    match v {
        Value::Vector(c) => {
            let g = |i: usize| c.get(i).and_then(Value::as_number).unwrap_or(0.0);
            [g(0), g(1), g(2)]
        }
        _ => [0.0, 0.0, 0.0],
    }
}

fn value_to_face(v: &Value) -> Vec<u32> {
    match v {
        Value::Vector(idx) => idx
            .iter()
            .filter_map(Value::as_number)
            .map(|n| n as u32)
            .collect(),
        _ => Vec::new(),
    }
}

/// Push a triangle `(a,b,c)` to a polyhedron face list, oriented so its normal
/// points along `out`. `polyhedron()` reverses each face on triangulation, so we
/// emit `[v0, v2, v1]` where `(v0,v1,v2)` is the outward-facing order.
fn surf_face(faces: &mut Vec<Vec<u32>>, pts: &[Vec3], a: u32, b: u32, c: u32, out: Vec3) {
    let (pa, pb, pc) = (pts[a as usize], pts[b as usize], pts[c as usize]);
    let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    // Degenerate (zero-area) triangle — e.g. a wall where the height is 0.
    if n[0] * n[0] + n[1] * n[1] + n[2] * n[2] < 1e-18 {
        return;
    }
    let dot = n[0] * out[0] + n[1] * out[1] + n[2] * out[2];
    let (v0, v1, v2) = if dot >= 0.0 { (a, b, c) } else { (a, c, b) };
    faces.push(vec![v0, v2, v1]);
}

/// Decode a PNG into a heightmap grid for `surface()`. Each pixel's height is
/// its Rec.709 luma scaled to 0..100 (white = 100), matching OpenSCAD; `invert`
/// flips brightness (height → 100 - height). Rows are returned bottom-to-top so
/// they feed `surface_polyhedron`'s row-r→y=r convention (OpenSCAD places the
/// image's top row at the maximum Y).
fn png_heightmap(bytes: &[u8], invert: bool) -> Result<Vec<Vec<f64>>, String> {
    // LuaCAD deviation: upstream builds on png 0.17, which decodes straight from
    // a `&[u8]`. LuaCAD is on 0.18, where `Decoder` wants `BufRead + Seek`, so
    // the slice is wrapped rather than pinning a second copy of png in the tree.
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    // png 0.18 returns None when the frame's buffer size would overflow usize.
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "surface(): PNG is too large to decode".to_string())?;
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let chans = info.color_type.samples();
    let bytes_per_sample = match info.bit_depth {
        png::BitDepth::Sixteen => 2,
        _ => 1,
    };
    let sample = |base: usize| -> f64 {
        // Read one channel sample as 0..255. 16-bit samples are reduced to 8-bit
        // by their most-significant byte (libpng's strip_16), matching OpenSCAD.
        let i = base * bytes_per_sample;
        buf[i] as f64
    };
    let stride = w * chans;
    let mut rows: Vec<Vec<f64>> = Vec::with_capacity(h);
    for y in 0..h {
        let mut row = Vec::with_capacity(w);
        for x in 0..w {
            let base = y * stride + x * chans;
            // Grayscale (1-2 chans) uses the single value; RGB(A) uses Rec.709.
            let luma = if chans <= 2 {
                sample(base)
            } else {
                0.2126 * sample(base) + 0.7152 * sample(base + 1) + 0.0722 * sample(base + 2)
            };
            let mut z = luma / 2.55; // 0..255 -> 0..100
            if invert {
                z = 100.0 - z;
            }
            row.push(z);
        }
        rows.push(row);
    }
    rows.reverse(); // image top row -> largest y
    Ok(rows)
}

/// Build the `surface()` solid from a heightmap grid: the top follows the
/// heights, the bottom is flat at z=0, joined by vertical walls. Matches
/// OpenSCAD (row r → y=r, col c → x=c; `center` shifts to the origin).
fn surface_polyhedron(rows: &[Vec<f64>], center: bool, png: bool) -> Node {
    let nr = rows.len();
    let nc = rows.iter().map(Vec::len).max().unwrap_or(0);
    if nr < 2 || nc < 2 {
        return Node::Empty;
    }
    let h = |r: usize, c: usize| rows[r].get(c).copied().unwrap_or(0.0);
    let ox = if center { (nc - 1) as f64 / 2.0 } else { 0.0 };
    let oy = if center { (nr - 1) as f64 / 2.0 } else { 0.0 };
    // Bottom plane. OpenSCAD's two source paths differ: a text (.dat) surface
    // uses z=0 when all heights ≥1 (else min−1), while a PNG surface always
    // drops the base to min−1.
    let min_h = (0..nr)
        .flat_map(|r| (0..nc).map(move |c| (r, c)))
        .map(|(r, c)| h(r, c))
        .fold(f64::INFINITY, f64::min);
    let bottom_z = if png {
        min_h - 1.0
    } else {
        (min_h - 1.0).min(0.0)
    };

    let mut points: Vec<Vec3> = Vec::with_capacity(nr * nc * 2 + (nr - 1) * (nc - 1));
    for r in 0..nr {
        for c in 0..nc {
            points.push([c as f64 - ox, r as f64 - oy, h(r, c)]);
        }
    }
    for r in 0..nr {
        for c in 0..nc {
            points.push([c as f64 - ox, r as f64 - oy, bottom_z]);
        }
    }
    // A center vertex per top cell (average height): OpenSCAD fan-triangulates
    // each cell around it, so a non-planar cell's volume is exact.
    let cbase = 2 * nr * nc;
    for r in 0..nr - 1 {
        for c in 0..nc - 1 {
            let z = (h(r, c) + h(r, c + 1) + h(r + 1, c) + h(r + 1, c + 1)) / 4.0;
            points.push([c as f64 + 0.5 - ox, r as f64 + 0.5 - oy, z]);
        }
    }
    let top = |r: usize, c: usize| (r * nc + c) as u32;
    let bot = |r: usize, c: usize| (nr * nc + r * nc + c) as u32;
    let mid = |r: usize, c: usize| (cbase + r * (nc - 1) + c) as u32;
    let mut faces: Vec<Vec<u32>> = Vec::new();

    for r in 0..nr - 1 {
        for c in 0..nc - 1 {
            // Top: fan the 4 cell edges around the center vertex.
            let m = mid(r, c);
            surf_face(
                &mut faces,
                &points,
                top(r, c),
                top(r, c + 1),
                m,
                [0.0, 0.0, 1.0],
            );
            surf_face(
                &mut faces,
                &points,
                top(r, c + 1),
                top(r + 1, c + 1),
                m,
                [0.0, 0.0, 1.0],
            );
            surf_face(
                &mut faces,
                &points,
                top(r + 1, c + 1),
                top(r + 1, c),
                m,
                [0.0, 0.0, 1.0],
            );
            surf_face(
                &mut faces,
                &points,
                top(r + 1, c),
                top(r, c),
                m,
                [0.0, 0.0, 1.0],
            );
            // Bottom stays a flat quad (z=0 → volume-neutral either way).
            surf_face(
                &mut faces,
                &points,
                bot(r, c),
                bot(r, c + 1),
                bot(r + 1, c + 1),
                [0.0, 0.0, -1.0],
            );
            surf_face(
                &mut faces,
                &points,
                bot(r, c),
                bot(r + 1, c + 1),
                bot(r + 1, c),
                [0.0, 0.0, -1.0],
            );
        }
    }
    // Front (y=0) and back (y=max) walls.
    for c in 0..nc - 1 {
        surf_face(
            &mut faces,
            &points,
            top(0, c),
            top(0, c + 1),
            bot(0, c + 1),
            [0.0, -1.0, 0.0],
        );
        surf_face(
            &mut faces,
            &points,
            top(0, c),
            bot(0, c + 1),
            bot(0, c),
            [0.0, -1.0, 0.0],
        );
        surf_face(
            &mut faces,
            &points,
            top(nr - 1, c),
            top(nr - 1, c + 1),
            bot(nr - 1, c + 1),
            [0.0, 1.0, 0.0],
        );
        surf_face(
            &mut faces,
            &points,
            top(nr - 1, c),
            bot(nr - 1, c + 1),
            bot(nr - 1, c),
            [0.0, 1.0, 0.0],
        );
    }
    // Left (x=0) and right (x=max) walls.
    for r in 0..nr - 1 {
        surf_face(
            &mut faces,
            &points,
            top(r, 0),
            top(r + 1, 0),
            bot(r + 1, 0),
            [-1.0, 0.0, 0.0],
        );
        surf_face(
            &mut faces,
            &points,
            top(r, 0),
            bot(r + 1, 0),
            bot(r, 0),
            [-1.0, 0.0, 0.0],
        );
        surf_face(
            &mut faces,
            &points,
            top(r, nc - 1),
            top(r + 1, nc - 1),
            bot(r + 1, nc - 1),
            [1.0, 0.0, 0.0],
        );
        surf_face(
            &mut faces,
            &points,
            top(r, nc - 1),
            bot(r + 1, nc - 1),
            bot(r, nc - 1),
            [1.0, 0.0, 0.0],
        );
    }
    Node::Polyhedron { points, faces }
}

fn index_value(base: &Value, index: &Value) -> Value {
    match (base, index) {
        // Rust casts NaN to zero; OpenSCAD instead rejects it as an index.
        (_, Value::Number(n)) if n.is_nan() => Value::Undef,
        (Value::Vector(v), Value::Number(n)) => {
            let i = *n as isize;
            if i >= 0 && (i as usize) < v.len() {
                v[i as usize].clone()
            } else {
                Value::Undef
            }
        }
        (Value::Str(s), Value::Number(n)) => {
            let i = *n as isize;
            if i >= 0 {
                s.chars()
                    .nth(i as usize)
                    .map(|c| Value::Str(c.to_string()))
                    .unwrap_or(Value::Undef)
            } else {
                Value::Undef
            }
        }
        // A range indexes as [start, step, end] (OpenSCAD); BOSL2's is_range()
        // relies on this.
        (Value::Range { start, step, end }, Value::Number(n)) => match *n as isize {
            0 => Value::Number(*start),
            1 => Value::Number(*step),
            2 => Value::Number(*end),
            _ => Value::Undef,
        },
        _ => Value::Undef,
    }
}

/// `chr()` — turn code point(s) into a string.
fn chr(args: &[Value]) -> Value {
    fn one(n: f64) -> String {
        u32::try_from(n as i64)
            .ok()
            .filter(|&c| c != 0) // OpenSCAD ignores code point 0
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default()
    }
    fn append(value: &Value, s: &mut String) {
        match value {
            Value::Number(n) => s.push_str(&one(*n)),
            Value::Vector(v) => {
                for e in v.iter() {
                    if let Some(n) = e.as_number() {
                        s.push_str(&one(n));
                    }
                }
            }
            r @ Value::Range { .. } => {
                if let Ok(vals) = iter_values(r) {
                    for e in vals {
                        if let Some(n) = e.as_number() {
                            s.push_str(&one(n));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut s = String::new();
    for arg in args {
        append(arg, &mut s);
    }
    Value::Str(s)
}

/// Expand a value into the sequence a `for`/comprehension iterates over.
fn iter_values(v: &Value) -> EResult<Vec<Value>> {
    match v {
        // An undefined iterable contributes no loop/comprehension iterations.
        Value::Undef => Ok(Vec::new()),
        Value::Vector(xs) => Ok(xs.to_vec()),
        // Iterating a string yields its characters (OpenSCAD semantics).
        Value::Str(s) => Ok(s.chars().map(|c| Value::Str(c.to_string())).collect()),
        Value::Range { start, step, end } => {
            let mut out = Vec::new();
            let (start, step, end) = (*start, *step, *end);
            if step == 0.0 || start.is_nan() || end.is_nan() || step.is_nan() {
                return Ok(out);
            }
            let mut i = 0usize;
            if step > 0.0 {
                let mut x = start;
                while x <= end + 1e-12 {
                    out.push(Value::Number(x));
                    i += 1;
                    if i > MAX_RANGE_ITERS {
                        return err("range too large");
                    }
                    x = start + step * i as f64;
                }
            } else {
                let mut x = start;
                while x >= end - 1e-12 {
                    out.push(Value::Number(x));
                    i += 1;
                    if i > MAX_RANGE_ITERS {
                        return err("range too large");
                    }
                    x = start + step * i as f64;
                }
            }
            Ok(out)
        }
        // A scalar iterates once.
        other => Ok(vec![other.clone()]),
    }
}

/// Built-in expression functions.
fn builtin_fn(name: &str, args: &[Value], warnings: &mut Vec<String>) -> Value {
    let num = |i: usize| args.get(i).and_then(Value::as_number);
    let one = |f: fn(f64) -> f64| num(0).map(|x| Value::Number(f(x))).unwrap_or(Value::Undef);

    match name {
        "abs" => one(f64::abs),
        "sign" => one(|x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "round" => one(f64::round),
        "sqrt" => one(f64::sqrt),
        "exp" => one(f64::exp),
        "ln" => one(f64::ln),
        "log" => one(f64::log10),
        "sin" => one(sin_deg),
        "cos" => one(cos_deg),
        "tan" => one(tan_deg),
        "asin" => one(|x| x.asin().to_degrees()),
        "acos" => one(|x| x.acos().to_degrees()),
        "atan" => one(|x| x.atan().to_degrees()),
        "atan2" => match (num(0), num(1)) {
            (Some(y), Some(x)) => Value::Number(y.atan2(x).to_degrees()),
            _ => Value::Undef,
        },
        "pow" => match (num(0), num(1)) {
            (Some(b), Some(e)) => Value::Number(b.powf(e)),
            _ => Value::Undef,
        },
        "max" => reduce_num(args, f64::max),
        "min" => reduce_num(args, f64::min),
        "len" => match args.first() {
            Some(Value::Vector(v)) => Value::Number(v.len() as f64),
            Some(Value::Str(s)) => Value::Number(s.chars().count() as f64),
            _ => Value::Undef,
        },
        "norm" => match args.first() {
            Some(Value::Vector(v)) => {
                let Some(nums): Option<Vec<f64>> = v.iter().map(Value::as_number).collect() else {
                    return Value::Undef;
                };
                Value::Number(nums.iter().map(|x| x * x).sum::<f64>().sqrt())
            }
            _ => Value::Undef,
        },
        "cross" => cross(args),
        "concat" => {
            let mut out = Vec::new();
            for a in args {
                match a {
                    Value::Vector(v) => out.extend(v.iter().cloned()),
                    other => out.push(other.clone()),
                }
            }
            value::vector(out)
        }
        "is_undef" => Value::Bool(matches!(args.first(), Some(Value::Undef) | None)),
        // `is_num(nan)` is false in OpenSCAD (a NaN is not a usable number), but
        // `is_num(inf)` is true. Match that: a finite-or-infinite Number, not NaN.
        "is_num" => Value::Bool(matches!(args.first(), Some(Value::Number(n)) if !n.is_nan())),
        "is_bool" => Value::Bool(matches!(args.first(), Some(Value::Bool(_)))),
        "is_string" => Value::Bool(matches!(args.first(), Some(Value::Str(_)))),
        "is_list" => Value::Bool(matches!(args.first(), Some(Value::Vector(_)))),
        "is_function" => Value::Bool(matches!(args.first(), Some(Value::Function(_)))),
        "is_range" => Value::Bool(matches!(args.first(), Some(Value::Range { .. }))),
        "rands" => rands_fn(args),
        "version" => value::vector(vec![
            Value::Number(2021.0),
            Value::Number(1.0),
            Value::Number(0.0),
        ]),
        "version_num" => match args.first() {
            None => Value::Number(20210100.0),
            Some(Value::Vector(v)) if v.len() == 2 || v.len() == 3 => {
                let Some(year) = v.first().and_then(Value::as_number) else {
                    return Value::Undef;
                };
                let Some(month) = v.get(1).and_then(Value::as_number) else {
                    return Value::Undef;
                };
                let day = match v.get(2) {
                    Some(v) => {
                        let Some(day) = v.as_number() else {
                            return Value::Undef;
                        };
                        day
                    }
                    None => 0.0,
                };
                Value::Number(year * 10_000.0 + month * 100.0 + day)
            }
            _ => Value::Undef,
        },
        "lookup" => lookup(args),
        "search" => search(args),
        "str" => {
            let mut s = String::new();
            for a in args {
                s.push_str(&a.to_str());
            }
            Value::Str(s)
        }
        "chr" => chr(args),
        "ord" => match args.first() {
            Some(Value::Str(s)) => match s.chars().next() {
                Some(c) => Value::Number(c as u32 as f64),
                None => Value::Undef,
            },
            _ => Value::Undef,
        },
        _ => {
            warnings.push(format!("Ignoring unknown function '{name}'"));
            Value::Undef
        }
    }
}

thread_local! {
    /// The global unseeded-`rands` PRNG state. Advances across calls (like
    /// OpenSCAD's global generator) so two `rands(a,b,n)` calls yield different
    /// sequences; starts from a fixed value each process, so runs are still
    /// reproducible. A `rands(...,seed)` call reseeds it, matching OpenSCAD.
    static RNG_STATE: std::cell::Cell<u64> = const { std::cell::Cell::new(0x9E37_79B9_7F4A_7C15) };
}

/// `rands(min, max, count, seed=undef)` — a list of pseudo-random numbers.
/// Uses an xorshift PRNG. Values won't match OpenSCAD's PRNG bit-for-bit, but
/// range/distribution are correct and, crucially, consecutive unseeded calls
/// differ (BOSL2's geometry tests rely on that). Deterministic per process.
fn rands_fn(args: &[Value]) -> Value {
    let min = args.first().and_then(Value::as_number).unwrap_or(0.0);
    let max = args.get(1).and_then(Value::as_number).unwrap_or(1.0);
    let count = args
        .get(2)
        .and_then(Value::as_number)
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let count = count.min(1_000_000);
    let seed = args.get(3).and_then(Value::as_number);
    // Seeded: reset the global generator; unseeded: continue from where it left.
    let mut state: u64 = match seed {
        Some(s) => s.to_bits() ^ 0x2545_F491_4F6C_DD1D,
        None => RNG_STATE.with(|c| c.get()),
    };
    if state == 0 {
        state = 1;
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let u = (state >> 11) as f64 / (1u64 << 53) as f64; // [0, 1)
        out.push(Value::Number(min + u * (max - min)));
    }
    RNG_STATE.with(|c| c.set(state)); // persist so the next call advances
    value::vector(out)
}

/// Degree trig that returns exact 0/±1 at multiples of 90°, matching OpenSCAD.
fn norm_deg(x: f64) -> f64 {
    let a = x % 360.0;
    if a < 0.0 {
        a + 360.0
    } else {
        a
    }
}
fn sin_deg(x: f64) -> f64 {
    match norm_deg(x) {
        0.0 | 180.0 => 0.0,
        90.0 => 1.0,
        270.0 => -1.0,
        a => a.to_radians().sin(),
    }
}
fn cos_deg(x: f64) -> f64 {
    match norm_deg(x) {
        0.0 => 1.0,
        90.0 | 270.0 => 0.0,
        180.0 => -1.0,
        a => a.to_radians().cos(),
    }
}
fn tan_deg(x: f64) -> f64 {
    // Exact values at quadrant / 45-degree angles (matching OpenSCAD, which
    // returns exactly ±1 and ±inf here rather than the ~1e-16 error a naive
    // radian `tan` leaves — BOSL2 compares with exact `!=`).
    match norm_deg(x) {
        0.0 | 180.0 => 0.0,
        45.0 | 225.0 => 1.0,
        135.0 | 315.0 => -1.0,
        90.0 => f64::INFINITY,
        270.0 => f64::NEG_INFINITY,
        a => a.to_radians().tan(),
    }
}

fn reduce_num(args: &[Value], f: fn(f64, f64) -> f64) -> Value {
    // max(v) over a single vector, or max(a,b,c,...) over scalars.
    let nums: Option<Vec<f64>> = if let [Value::Vector(v)] = args {
        v.iter().map(Value::as_number).collect()
    } else {
        args.iter().map(Value::as_number).collect()
    };
    let Some(nums) = nums else {
        return Value::Undef;
    };
    match nums.split_first() {
        Some((first, rest)) => Value::Number(rest.iter().fold(*first, |a, b| f(a, *b))),
        None => Value::Undef,
    }
}

/// `lookup(key, table)` — linear interpolation over a `[[k, v], ...]` table.
fn lookup(args: &[Value]) -> Value {
    let Some(key) = args.first().and_then(Value::as_number) else {
        return Value::Undef;
    };
    let Some(Value::Vector(table)) = args.get(1) else {
        return Value::Undef;
    };
    let mut pairs: Vec<(f64, f64)> = table
        .iter()
        .filter_map(|e| {
            if let Value::Vector(p) = e {
                Some((p.first()?.as_number()?, p.get(1)?.as_number()?))
            } else {
                None
            }
        })
        .collect();
    if pairs.is_empty() {
        return Value::Undef;
    }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if key <= pairs[0].0 {
        return Value::Number(pairs[0].1);
    }
    let last = *pairs.last().unwrap();
    if key >= last.0 {
        return Value::Number(last.1);
    }
    for w in pairs.windows(2) {
        let (k0, v0) = w[0];
        let (k1, v1) = w[1];
        if key >= k0 && key <= k1 {
            if k1 == k0 {
                return Value::Number(v0);
            }
            let t = (key - k0) / (k1 - k0);
            return Value::Number(v0 + t * (v1 - v0));
        }
    }
    Value::Undef
}

/// `search(find, list, num_returns=1, index=0)`.
/// Collect the child *instantiations* a child block exposes to its parent
/// module (for `$children` counting and `children(i)` indexing), matching
/// OpenSCAD: assignments and `module`/`function`/`use`/`include` statements are
/// scoped locals rather than children, and a bare `{ ... }` block is
/// transparent — its statements splice into the parent's child list. Everything
/// else (`module` calls, `for`, `if`, `let`) counts as exactly one child.
fn collect_child_slots<'a>(kids: &'a [Spanned<Stmt>], out: &mut Vec<&'a Spanned<Stmt>>) {
    for k in kids {
        match &k.node {
            Stmt::Assign { .. }
            | Stmt::ModuleDef { .. }
            | Stmt::FunctionDef { .. }
            | Stmt::Use { .. }
            | Stmt::Include { .. } => {}
            Stmt::Block(inner) => collect_child_slots(inner, out),
            _ => out.push(k),
        }
    }
}

fn search(args: &[Value]) -> Value {
    let Some(find) = args.first() else {
        return Value::Undef;
    };
    let Some(list) = args.get(1) else {
        return Value::Undef;
    };
    let num_returns = args.get(2).and_then(Value::as_number).unwrap_or(1.0) as usize;
    let index = args.get(3).and_then(Value::as_number).unwrap_or(0.0) as usize;
    // An explicitly-passed `undef` counts as "not given" (OpenSCAD semantics),
    // so only a real numeric column index forces column matching.
    let index_given = args.get(3).and_then(Value::as_number).is_some();

    let entries: Vec<Value> = match list {
        Value::Vector(v) => v.to_vec(),
        Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
        _ => return Value::Undef,
    };
    let compare_val = |entry: &Value| -> Value {
        match entry {
            Value::Vector(row) => row.get(index).cloned().unwrap_or(Value::Undef),
            other => other.clone(),
        }
    };
    let match_indices = |needle: &Value| -> Vec<usize> {
        // OpenSCAD: with no explicit `index_col_num`, a *list* needle is matched
        // against the whole row (finding a vector in a list of vectors), while a
        // scalar needle matches against column 0. An explicit index always
        // matches against that column.
        let whole_row = !index_given && matches!(needle, Value::Vector(_));
        let mut out = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let target = if whole_row { e.clone() } else { compare_val(e) };
            if value::value_eq(&target, needle) {
                out.push(i);
                if num_returns != 0 && out.len() >= num_returns {
                    break;
                }
            }
        }
        out
    };
    // With the default num_returns == 1, OpenSCAD collapses each per-element
    // result to a single index (or an empty list when there is no match);
    // otherwise each result is the full list of indices.
    let pack = |idxs: Vec<usize>| -> Value {
        if num_returns == 1 {
            match idxs.first() {
                Some(i) => Value::Number(*i as f64),
                None => value::vector(Vec::new()),
            }
        } else {
            value::vector(idxs.into_iter().map(|i| Value::Number(i as f64)).collect())
        }
    };

    match find {
        // A single scalar returns a flat list of indices.
        Value::Number(_) | Value::Bool(_) => value::vector(
            match_indices(find)
                .into_iter()
                .map(|i| Value::Number(i as f64))
                .collect(),
        ),
        // A string searches per character; a list searches per element.
        // With the default `num_returns == 1`, OpenSCAD collapses a string
        // search to a flat list of first-match indices and *omits* characters
        // that are not found (`search("abe","abcdabcd") == [0,1]`, the missing
        // `e` produces no entry) — unlike a list needle, which keeps an empty
        // `[]` placeholder. Any other `num_returns` keeps the per-char sublists.
        Value::Str(s) if num_returns == 1 => value::vector(
            s.chars()
                .filter_map(|c| {
                    match_indices(&Value::Str(c.to_string()))
                        .first()
                        .map(|i| Value::Number(*i as f64))
                })
                .collect(),
        ),
        Value::Str(s) => value::vector(
            s.chars()
                .map(|c| pack(match_indices(&Value::Str(c.to_string()))))
                .collect(),
        ),
        Value::Vector(vs) => value::vector(vs.iter().map(|n| pack(match_indices(n))).collect()),
        _ => Value::Undef,
    }
}

fn cross(args: &[Value]) -> Value {
    // OpenSCAD: cross of two 3-vectors is a 3-vector; cross of two 2-vectors is
    // the scalar z-component; anything else (mismatched lengths, non-numeric
    // elements) is undef.
    let (Some(Value::Vector(a)), Some(Value::Vector(b))) = (args.first(), args.get(1)) else {
        return Value::Undef;
    };
    let nums = |v: &[Value]| -> Option<Vec<f64>> { v.iter().map(Value::as_number).collect() };
    let (Some(a), Some(b)) = (nums(a), nums(b)) else {
        return Value::Undef;
    };
    match (a.len(), b.len()) {
        (3, 3) => value::vector(vec![
            Value::Number(a[1] * b[2] - a[2] * b[1]),
            Value::Number(a[2] * b[0] - a[0] * b[2]),
            Value::Number(a[0] * b[1] - a[1] * b[0]),
        ]),
        (2, 2) => Value::Number(a[0] * b[1] - a[1] * b[0]),
        _ => Value::Undef,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrscad_syntax::parse;

    /// Malformed and adversarial inputs must fail gracefully (parse error or
    /// eval error), never panic or overflow the stack — this is what keeps the
    /// browser engine from taking down the tab. Guards against regressions.
    #[test]
    fn adversarial_inputs_never_panic() {
        let deep_open = "(".repeat(3000);
        let deep_list = format!("echo({}1{});", "[".repeat(1500), "]".repeat(1500));
        let cases: Vec<String> = vec![
            "x=1e999999; echo(x);".into(),
            "echo(1/0); echo(0/0); echo(5%0);".into(),
            "function f(x)=f(x); echo(f(1));".into(), // infinite tail recursion
            "function g(n)=n<=0?0:1+g(n-1); echo(g(100000));".into(), // deep non-tail
            "a=[]; echo(a[999999999999]);".into(),
            "echo(chr(-1), chr(1114112)); echo(\"abc\"[-5]);".into(),
            "echo([for(i=[0:1:1e9]) i]);".into(), // huge range (capped)
            "echo(2^2^2^2^2);".into(),
            "echo(\"日本語\"[1]); echo(len(\"🎉\"));".into(),
            "polygon([[0,0]]); linear_extrude(-5) square(10);".into(),
            "echo(concat(), str(), lookup(5,[]));".into(),
            "for(i=[0:-1:10]) cube(1);".into(),
            deep_open, // unbalanced parens
            deep_list, // deeply nested list
            "".into(), // empty
            "\u{0}\u{1}garbage \" unterminated".into(),
        ];
        // Run on a production-sized stack (the CLI uses 256 MiB, wasm 64 MiB);
        // the default 2 MiB test thread is smaller than any real deployment.
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || {
                for src in &cases {
                    // parse may Err; if it parses, eval may Err — neither panics.
                    if let Ok(prog) = parse(src) {
                        let _ = eval_program(&prog);
                    }
                }
            })
            .unwrap()
            .join()
            .expect("adversarial inputs must not panic or overflow the stack");
    }

    /// Recursively remove the transparent [`Node::Provenance`] wrappers the
    /// evaluator now inserts around every module call, so the structural
    /// assertions below match the underlying geometry tree.
    fn strip_provenance(node: Node) -> Node {
        use Node::*;
        let b = |n: Node| Box::new(strip_provenance(n));
        let each = |cs: Vec<Node>| cs.into_iter().map(strip_provenance).collect();
        match node {
            Provenance { child, .. } => strip_provenance(*child),
            Group(cs) => Group(each(cs)),
            Union(cs) => Union(each(cs)),
            Difference(cs) => Difference(each(cs)),
            Intersection(cs) => Intersection(each(cs)),
            Hull(cs) => Hull(each(cs)),
            Minkowski(cs) => Minkowski(each(cs)),
            Translate { v, child } => Translate {
                v,
                child: b(*child),
            },
            Rotate { deg, child } => Rotate {
                deg,
                child: b(*child),
            },
            Scale { v, child } => Scale {
                v,
                child: b(*child),
            },
            Mirror { v, child } => Mirror {
                v,
                child: b(*child),
            },
            MultMatrix { m, child } => MultMatrix {
                m,
                child: b(*child),
            },
            Resize { new, auto, child } => Resize {
                new,
                auto,
                child: b(*child),
            },
            LinearExtrude {
                height,
                center,
                twist,
                scale,
                slices,
                child,
            } => LinearExtrude {
                height,
                center,
                twist,
                scale,
                slices,
                child: b(*child),
            },
            RotateExtrude {
                angle,
                frags,
                child,
            } => RotateExtrude {
                angle,
                frags,
                child: b(*child),
            },
            Offset {
                r,
                delta,
                chamfer,
                frags,
                child,
            } => Offset {
                r,
                delta,
                chamfer,
                frags,
                child: b(*child),
            },
            Projection { cut, child } => Projection {
                cut,
                child: b(*child),
            },
            Color { rgba, child } => Color {
                rgba,
                child: b(*child),
            },
            Highlight(child) => Highlight(b(*child)),
            Background(child) => Background(b(*child)),
            leaf => leaf,
        }
    }

    fn eval(src: &str) -> EvalOutput {
        let mut out = eval_program(&parse(src).unwrap()).unwrap();
        out.node = strip_provenance(out.node);
        out
    }

    #[test]
    fn budget_bounds_runaway_evaluation() {
        // Nested loops multiply to 10^12 iterations — the per-construct
        // MAX_RANGE_ITERS limit does not stop this; a fuel budget must.
        let prog = parse("for(i=[0:999999]) for(j=[0:999999]) cube(1);").unwrap();
        let err = eval_program_with_budget(&prog, &NullResolver, ".", 100_000).unwrap_err();
        assert!(
            err.message.contains("budget exhausted"),
            "expected a budget-exhausted error, got: {:?}",
            err.message
        );
    }

    #[test]
    fn budget_generous_matches_unbudgeted() {
        // A realistic small program completes well within a modest budget and
        // produces exactly the unbudgeted result (fuel is otherwise invisible).
        let src = "for(i=[0:9]) translate([i,0,0]) cube(1); echo([for(i=[0:100]) i*i]);";
        let prog = parse(src).unwrap();
        let budgeted = eval_program_with_budget(&prog, &NullResolver, ".", 10_000_000).unwrap();
        assert_eq!(budgeted.echoes, eval_program(&prog).unwrap().echoes);
    }

    #[test]
    fn single_cube() {
        let out = eval("cube(10);");
        assert_eq!(
            out.node,
            Node::Cube {
                size: [10.0, 10.0, 10.0],
                center: false
            }
        );
    }

    // ---- transform argument binding (A1) ------------------------------

    /// Named args on the transforms must bind — the old `first_positional`
    /// path silently dropped them, leaving the child untransformed.
    #[test]
    fn transforms_bind_named_args() {
        match eval("translate(v=[1,2,3]) cube(1);").node {
            Node::Translate { v, .. } => assert_eq!(v, [1.0, 2.0, 3.0]),
            other => panic!("translate(v=): {other:?}"),
        }
        match eval("scale(v=2) cube(1);").node {
            Node::Scale { v, .. } => assert_eq!(v, [2.0, 2.0, 2.0]),
            other => panic!("scale(v=): {other:?}"),
        }
        match eval("mirror(v=[1,0,0]) cube(1);").node {
            Node::Mirror { v, .. } => assert_eq!(v, [1.0, 0.0, 0.0]),
            other => panic!("mirror(v=): {other:?}"),
        }
        match eval("rotate(a=45) cube(1);").node {
            Node::Rotate { deg, .. } => assert_eq!(deg, [0.0, 0.0, 45.0]),
            other => panic!("rotate(a=): {other:?}"),
        }
    }

    /// Existing positional/scalar/2-element forms keep working.
    #[test]
    fn transforms_positional_forms_unchanged() {
        match eval("translate([1,2]) cube(1);").node {
            Node::Translate { v, .. } => assert_eq!(v, [1.0, 2.0, 0.0]),
            other => panic!("translate([x,y]): {other:?}"),
        }
        match eval("scale([2,3]) cube(1);").node {
            Node::Scale { v, .. } => assert_eq!(v, [2.0, 3.0, 1.0]),
            other => panic!("scale([x,y]): {other:?}"),
        }
        match eval("rotate([45,0,0]) cube(1);").node {
            Node::Rotate { deg, .. } => assert_eq!(deg, [45.0, 0.0, 0.0]),
            other => panic!("rotate([x,y,z]): {other:?}"),
        }
    }

    /// `rotate(a, v)` (positional or named) lowers to an affine matrix. The
    /// old code dropped `v` and treated the angle as Euler `[a,0,0]`.
    #[test]
    fn rotate_axis_angle_lowers_to_multmatrix() {
        // 90° about X == the hand-computed Rodrigues matrix, and equals the
        // Euler rotate([90,0,0]) rotation about the same axis.
        let m = match eval("rotate(90, [1,0,0]) cube(1);").node {
            Node::MultMatrix { m, .. } => m,
            other => panic!("rotate(90,[1,0,0]): {other:?}"),
        };
        let expect = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (m[r][c] - expect[r][c]).abs() < 1e-12,
                    "m[{r}][{c}]={}",
                    m[r][c]
                );
            }
        }
        assert_eq!(
            [m[0][3], m[1][3], m[2][3]],
            [0.0, 0.0, 0.0],
            "no translation"
        );
        assert_eq!(m[3], [0.0, 0.0, 0.0, 1.0], "affine bottom row");

        // Named form is identical to positional.
        assert_eq!(
            eval("rotate(a=90, v=[1,0,0]) cube(1);").node,
            eval("rotate(90, [1,0,0]) cube(1);").node,
        );

        // The rotation axis is invariant: R * axis == axis (an independent
        // check that this really is a rotation about [1,1,0]).
        let m = match eval("rotate(45, [1,1,0]) cube(1);").node {
            Node::MultMatrix { m, .. } => m,
            other => panic!("rotate(45,[1,1,0]): {other:?}"),
        };
        let axis = [1.0, 1.0, 0.0];
        let mapped = [
            m[0][0] * axis[0] + m[0][1] * axis[1] + m[0][2] * axis[2],
            m[1][0] * axis[0] + m[1][1] * axis[1] + m[1][2] * axis[2],
            m[2][0] * axis[0] + m[2][1] * axis[1] + m[2][2] * axis[2],
        ];
        for i in 0..3 {
            assert!(
                (mapped[i] - axis[i]).abs() < 1e-12,
                "axis not fixed: {mapped:?}"
            );
        }
    }

    /// A zero-length axis is not a rotation — fall back to Euler/Z per the manual.
    #[test]
    fn rotate_zero_axis_falls_back_to_euler() {
        match eval("rotate(45, [0,0,0]) cube(1);").node {
            Node::Rotate { deg, .. } => assert_eq!(deg, [0.0, 0.0, 45.0]),
            other => panic!("rotate(45,[0,0,0]): {other:?}"),
        }
    }

    #[test]
    fn difference_tree() {
        let out = eval("difference() { cube(10, center=true); sphere(6); }");
        match out.node {
            Node::Difference(children) => assert_eq!(children.len(), 2),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn for_loop_produces_group() {
        let out = eval("for (i = [0:2]) translate([i*10, 0, 0]) cube(1);");
        match out.node {
            Node::Group(children) => assert_eq!(children.len(), 3),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn intersection_for_uses_dependent_cartesian_bindings() {
        let out = eval(
            "intersection_for(i=[1,2], j=[i,i+10]) \
             translate([i,j,0]) cube(1);",
        );
        let Node::Intersection(operands) = out.node else {
            panic!("expected intersection_for to lower to Intersection");
        };
        let translations: Vec<[f64; 3]> = operands
            .into_iter()
            .map(|node| match node {
                Node::Translate { v, .. } => v,
                other => panic!("expected translated operand, got {other:?}"),
            })
            .collect();
        assert_eq!(
            translations,
            [
                [1.0, 1.0, 0.0],
                [1.0, 11.0, 0.0],
                [2.0, 2.0, 0.0],
                [2.0, 12.0, 0.0],
            ]
        );
    }

    #[test]
    fn last_assignment_wins() {
        // x is hoisted: cube should see x = 2.
        let out = eval("x = 1; cube(x); x = 2;");
        assert_eq!(
            out.node,
            Node::Cube {
                size: [2.0, 2.0, 2.0],
                center: false
            }
        );
    }

    #[test]
    fn user_function() {
        let out = eval("function sq(a) = a * a; cube(sq(3));");
        assert_eq!(
            out.node,
            Node::Cube {
                size: [9.0, 9.0, 9.0],
                center: false
            }
        );
    }

    #[test]
    fn user_module() {
        let out = eval("module box(s) { cube(s, center=true); } box(4);");
        assert_eq!(
            out.node,
            Node::Cube {
                size: [4.0, 4.0, 4.0],
                center: true
            }
        );
    }

    #[test]
    fn echo_collected() {
        let out = eval("echo(\"hello\", 1 + 2);");
        assert_eq!(out.echoes, vec!["ECHO: \"hello\", 3".to_string()]);
    }

    #[test]
    fn recursion() {
        let out = eval("function fib(n) = n < 2 ? n : fib(n-1) + fib(n-2); cube(fib(10));");
        assert_eq!(
            out.node,
            Node::Cube {
                size: [55.0, 55.0, 55.0],
                center: false
            }
        );
    }

    #[test]
    fn if_else() {
        let out = eval("if (1 > 2) cube(1); else sphere(3);");
        assert!(matches!(out.node, Node::Sphere { .. }));
    }

    #[test]
    fn cylinder_d_and_center() {
        let out = eval("cylinder(h=10, d=8, center=true);");
        match out.node {
            Node::Cylinder {
                h, r1, r2, center, ..
            } => {
                assert_eq!(h, 10.0);
                assert_eq!(r1, 4.0);
                assert_eq!(r2, 4.0);
                assert!(center);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn cylinder_binds_fourth_positional_center() {
        // The fourth positional cylinder argument is `center`.
        match eval("cylinder(10, 2, 3, true);").node {
            Node::Cylinder {
                h, r1, r2, center, ..
            } => {
                assert_eq!((h, r1, r2), (10.0, 2.0, 3.0));
                assert!(center);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn multmatrix_binds_named_m() {
        // multmatrix accepts both its positional form and the documented `m=`
        // form; both must lower to the same transform.
        let matrix = "[[1,0,0,4],[0,1,0,5],[0,0,1,6],[0,0,0,1]]";
        assert_eq!(
            eval(&format!("multmatrix({matrix}) cube(1);")).node,
            eval(&format!("multmatrix(m={matrix}) cube(1);")).node
        );
    }

    #[test]
    fn text_binds_full_positional_signature_and_named_forms() {
        // Positional text arguments follow OpenSCAD's complete signature. The
        // equivalent named call also proves that those forms remain supported.
        let Some(family) = installed_family() else {
            eprintln!("skipped: no fonts installed");
            return;
        };
        let positional = format!(
            "text(\"AB\", 12, \"{family}\", \"rtl\", \"en\", \"latin\", \"center\", \"top\", 1.25);"
        );
        let named = format!(
            "text(text=\"AB\", size=12, font=\"{family}\", direction=\"rtl\", \
             language=\"en\", script=\"latin\", halign=\"center\", valign=\"top\", \
             spacing=1.25);"
        );
        let positional = eval(&positional);
        let named = eval(&named);
        assert_eq!(positional.node, named.node);
        assert!(positional.warnings.is_empty());
        assert!(named.warnings.is_empty());
    }

    fn echoes(src: &str) -> Vec<String> {
        eval(src).echoes
    }

    /// An installed font family that can draw Latin letters, or `None` when the
    /// machine has none.
    ///
    /// LuaCAD deviation from upstream OpenRSCAD: upstream bundles the Liberation
    /// family, so its `text()` tests name it directly and assert on its exact
    /// glyph metrics. LuaCAD bundles no fonts, so these tests use whatever is
    /// installed, assert on shape rather than on metrics, and skip when nothing
    /// is installed at all.
    ///
    /// Latin coverage has to be checked rather than assumed: a font database is
    /// not all text faces, and on macOS it opens with private-use ones
    /// (".Al Bayan PUA") that render "A" as nothing at all.
    fn installed_family() -> Option<String> {
        text::register_system_fonts();
        text::font_completions()
            .into_iter()
            .map(|c| c.value)
            .filter(|v| !v.contains(":style="))
            .find(|v| draws_latin(v))
    }

    /// Whether `family` turns "AB" into actual contours.
    fn draws_latin(family: &str) -> bool {
        let params = text::TextParams {
            text: "AB",
            size: 10.0,
            halign: "left",
            valign: "baseline",
            spacing: 1.0,
            direction: "ltr",
            segments: 8,
        };
        text::render_text(family, &params).is_some_and(|(_, paths, _)| !paths.is_empty())
    }

    #[test]
    fn function_literal_serializes_like_openscad() {
        // `str()`/`echo` of a function value render it as source: `function(params)
        // body`, with binary/ternary sub-expressions parenthesized and calls,
        // vectors, ranges, indexing, and literals left bare. BOSL2's fnliterals
        // suite asserts on these exact strings. Verified against OpenSCAD 2024.12.
        assert_eq!(
            echoes(concat!(
                "echo(str(function(x) x));\n",
                "echo(str(function(x, y) x + y * 2));\n",
                "echo(str(function(x) -x));\n",
                "echo(str(function(a, b) a == undef ? b : a));\n",
                "echo(str(function(x) f(x, 2)));\n",
                "echo(str(function(x) g(a=x, b=2)));\n",
                "echo(str(function(x) [1, x, 3]));\n",
                "echo(str(function(x) x[0]));\n",
                "echo(str(function(x = 5) x));\n",
                "echo(str(function(x) function(y) x + y));\n",
            )),
            vec![
                "ECHO: \"function(x) x\"",
                "ECHO: \"function(x, y) (x + (y * 2))\"",
                "ECHO: \"function(x) -x\"",
                "ECHO: \"function(a, b) ((a == undef) ? b : a)\"",
                "ECHO: \"function(x) f(x, 2)\"",
                "ECHO: \"function(x) g(a = x, b = 2)\"",
                "ECHO: \"function(x) [1, x, 3]\"",
                "ECHO: \"function(x) x[0]\"",
                "ECHO: \"function(x = 5) x\"",
                "ECHO: \"function(x) function(y) (x + y)\"",
            ]
        );
    }

    #[test]
    fn is_num_rejects_nan_but_accepts_inf() {
        // OpenSCAD: is_num(nan) is false, is_num(inf) is true.
        assert_eq!(
            echoes("echo(is_num(0/0), is_num(1/0), is_num(3));"),
            vec!["ECHO: false, true, true"]
        );
    }

    #[test]
    fn nan_truthiness_and_indexing_match_openscad() {
        // Although is_num(nan) is false, OpenSCAD's boolean conversion treats
        // NaN as true. It is not, however, a valid list/string/range index.
        assert_eq!(
            echoes("echo(!(0/0), (0/0) ? 1 : 2, (0/0) && true, (0/0) || false);"),
            vec!["ECHO: false, 1, true, true"]
        );
        assert_eq!(
            echoes("echo([10,20][0/0], \"ab\"[0/0], [1:2:5][0/0]);"),
            vec!["ECHO: undef, undef, undef"]
        );
    }

    #[test]
    fn tan_is_exact_at_45_degree_multiples() {
        // OpenSCAD returns exact ±1 / ±inf at these angles (BOSL2 compares with
        // exact `!=`), where a naive radian `tan` leaves a ~1e-16 error.
        assert_eq!(
            echoes("echo(tan(45), tan(135), tan(225), tan(315), tan(90), tan(270));"),
            vec!["ECHO: 1, -1, 1, -1, inf, -inf"]
        );
    }

    // The following cases pin OpenSCAD's last-write-wins scope semantics, each
    // verified against the OpenSCAD 2024.12 echo oracle. See `corpus/echo/
    // assign_hoist.scad` for the blessed end-to-end golden.

    #[test]
    fn assignment_last_write_wins_is_visible_to_earlier_reads() {
        // A read of a variable that is reassigned later sees the *final* value,
        // not the intermediate one: `p = 1; q = p; p = 5;` gives q == 5. This is
        // the core divergence the fix closes (was q == 1).
        assert_eq!(
            echoes("p = 1; q = p; p = 5; echo(q, p);"),
            vec!["ECHO: 5, 5"]
        );
        // Multiple rewrites: the last expression wins regardless of position.
        assert_eq!(
            echoes("p = 1; p = 2; q = p; p = 3; echo(q, p);"),
            vec!["ECHO: 3, 3"]
        );
    }

    #[test]
    fn assignment_has_no_forward_references() {
        // A read of a variable introduced *later* does not see it; at top level
        // it is undef (OpenSCAD warns "Ignoring unknown variable").
        assert_eq!(echoes("y = x; x = 5; echo(y, x);"), vec!["ECHO: undef, 5"]);
        assert_eq!(
            echoes("a = b + 1; b = c * 2; c = 10; echo(a, b, c);"),
            vec!["ECHO: undef, undef, 10"]
        );
    }

    #[test]
    fn assignment_final_expr_referencing_later_var_is_undef() {
        // The final expression is evaluated at the variable's first-introduction
        // point, so it cannot see a variable introduced afterwards: in
        // `a = 1; c = a; b = 2; a = b;` the surviving `a = b` reads `b` before it
        // exists (undef), and `c = a` then reads that undef.
        assert_eq!(
            echoes("a = 1; c = a; b = 2; a = b; echo(a, b, c);"),
            vec!["ECHO: undef, 2, undef"]
        );
        // Self-reference in the surviving assignment is likewise undef.
        assert_eq!(echoes("x = x + 1; echo(x);"), vec!["ECHO: undef"]);
    }

    #[test]
    fn assignment_overwritten_rhs_is_not_evaluated() {
        // Only the last assignment's RHS runs; an overwritten one is discarded
        // entirely, side effects included (no echo from the dead assignment).
        assert_eq!(
            echoes("x = echo(\"dead\") 1; x = 2; echo(x);"),
            vec!["ECHO: 2"]
        );
    }

    #[test]
    fn assignment_hoisting_falls_through_to_outer_scope() {
        // In a nested scope, a read before the local is introduced falls through
        // to the outer binding (not undef): outer x == 99, so `y = x` sees 99
        // while the later local `x = 5` shadows it afterwards.
        assert_eq!(
            echoes("x = 99; module m() { y = x; x = 5; echo(y, x); } m();"),
            vec!["ECHO: 99, 5"]
        );
    }

    fn warnings_of(src: &str) -> Vec<String> {
        eval(src).warnings.into_iter().map(|w| w.message).collect()
    }

    #[test]
    fn overwritten_assignment_warns() {
        // Reassigning a name in a scope warns on each dead (earlier) write.
        assert_eq!(
            warnings_of("z = 1; z = 2; z = 3; echo(z);"),
            vec![
                "variable 'z' is assigned again later; this assignment is overwritten",
                "variable 'z' is assigned again later; this assignment is overwritten",
            ]
        );
        // A single assignment does not warn.
        assert!(warnings_of("z = 1; echo(z);").is_empty());
    }

    #[test]
    fn overwritten_warning_dedups_across_module_reentry() {
        // A module body re-runs its assignment phase on each call; the dead-write
        // lint fires once per source site, not once per invocation.
        assert_eq!(
            warnings_of("module m() { z = 1; z = 2; echo(z); } m(); m();"),
            vec!["variable 'z' is assigned again later; this assignment is overwritten"]
        );
    }

    #[test]
    fn bare_assert_is_undef_not_unknown_function() {
        // `assert(cond)` in value position (no trailing expression) checks the
        // condition and yields undef — BOSL2 relies on this. Must not warn.
        let out = eval("y = assert(1 < 2, \"ok\"); echo(y);");
        assert_eq!(out.echoes, vec!["ECHO: undef"]);
        assert!(
            out.warnings.is_empty(),
            "unexpected warnings: {:?}",
            out.warnings
        );
        // A failing bare assert still aborts.
        let prog = parse("z = assert(1 > 2); echo(z);").unwrap();
        assert!(eval_program(&prog).is_err(), "failing assert should error");
    }

    #[test]
    fn omitted_param_shadows_global_through_assert_guard() {
        // An omitted parameter is `undef` in the body, shadowing any global of
        // the same name — even when the body is an `assert(...) expr` guard,
        // which can't compile to the slot-based VM path and so relies on the
        // interpreted binding map. Regression for BOSL2 gears.scad, where a
        // top-level `circ_pitch` used to leak into `circular_pitch()`'s omitted
        // `circ_pitch` param and trip its `one_defined` assert.
        let out = eval(
            "function f(p, mod) = assert(true) is_undef(p);\n\
             p = 9;\n\
             echo(f(mod = 2));",
        );
        assert_eq!(out.echoes, vec!["ECHO: true"]);
    }

    #[test]
    fn defaults_are_lazy_lexical_and_keep_specials_dynamic() {
        // Oracle-derived ordering and scope rules, exercised through a compiled
        // function (`f`), tree-walk fallback (`g`), and a module (`m`):
        // supplied args run first in caller scope; only missing defaults run;
        // ordinary names come from the definition scope, while `$d` is dynamic.
        assert_eq!(
            echoes(concat!(
                "x=10; $d=1;",
                "function f(a=echo(\"f-default\")x,b=$d,c=echo(\"f-dead\")3)=[a,b,c];",
                "function g(a=echo(\"g-default\")x,b=$d,c=echo(\"g-dead\")3)=",
                "assert(true)[a,b,c];",
                "module m(a=echo(\"m-default\")x,b=$d,c=echo(\"m-dead\")3){",
                "echo(\"m\",a,b,c);}",
                "module caller(){x=20;$d=7;",
                "echo(\"f\",f(c=echo(\"f-arg\")x));",
                "echo(\"g\",g(c=echo(\"g-arg\")x));",
                "m(c=echo(\"m-arg\")x);}",
                "caller();",
            )),
            vec![
                "ECHO: \"f-arg\"",
                "ECHO: \"f-default\"",
                "ECHO: \"f\", [10, 7, 20]",
                "ECHO: \"g-arg\"",
                "ECHO: \"g-default\"",
                "ECHO: \"g\", [10, 7, 20]",
                "ECHO: \"m-arg\"",
                "ECHO: \"m-default\"",
                "ECHO: \"m\", 10, 7, 20",
            ]
        );
    }

    #[test]
    fn module_defaults_observe_dynamic_instantiation_context() {
        // During binding, `$parent_modules` is inherited from the caller frame,
        // while parent_module(0) already identifies the callee being instantiated.
        assert_eq!(
            echoes(concat!(
                "module a(){b();}",
                "module b(depth=$parent_modules,current=parent_module(0)){",
                "echo(depth,current,$parent_modules,parent_module(0));}",
                "a();",
            )),
            vec!["ECHO: 1, \"b\", 2, \"b\""]
        );
    }

    #[test]
    fn cross_2d_is_scalar_3d_is_vector() {
        // OpenSCAD: 2D cross -> scalar z, 3D cross -> vector, mismatch -> undef.
        assert_eq!(echoes("echo(cross([1,2],[3,4]));"), vec!["ECHO: -2"]);
        assert_eq!(
            echoes("echo(cross([2,3,4],[5,6,7]));"),
            vec!["ECHO: [-3, 6, -3]"]
        );
        assert_eq!(echoes("echo(cross([1,2],[3,4,5]));"), vec!["ECHO: undef"]);
    }

    #[test]
    fn unseeded_rands_advances() {
        // Two consecutive unseeded rands() calls must differ (BOSL2 geometry
        // tests build distinct random points this way).
        let out = echoes("a=rands(0,1,3); b=rands(0,1,3); echo(a==b);");
        assert_eq!(out, vec!["ECHO: false"]);
        // A seeded call is reproducible.
        assert_eq!(
            echoes("echo(rands(0,1,2,42)==rands(0,1,2,42));"),
            vec!["ECHO: true"]
        );
    }

    #[test]
    fn comprehensions() {
        assert_eq!(
            echoes("echo([for(i=[0:4]) i*i]);"),
            vec!["ECHO: [0, 1, 4, 9, 16]"]
        );
        assert_eq!(
            echoes("echo([for(i=[0:5]) if(i%2==0) i]);"),
            vec!["ECHO: [0, 2, 4]"]
        );
        assert_eq!(
            echoes("echo([for(i=[1:3]) let(sq=i*i) sq]);"),
            vec!["ECHO: [1, 4, 9]"]
        );
        assert_eq!(
            echoes("echo([each [1,2], each [3,4]]);"),
            vec!["ECHO: [1, 2, 3, 4]"]
        );
        assert_eq!(
            echoes("echo([for(i=[0:2], j=[0:2]) i*10+j]);"),
            vec!["ECHO: [0, 1, 2, 10, 11, 12, 20, 21, 22]"]
        );
    }

    // The bytecode VM is a transparent fast path: these assert it produces the
    // same results as the tree-walk across the constructs it compiles, and that
    // unsupported constructs (comprehensions/closures in a body) fall back
    // correctly.
    #[test]
    fn parameter_overrides() {
        use openrscad_syntax::parse;
        // A top-level assignment is overridden (the override wins).
        let prog = parse("w=10; h=20; echo(w, h);").unwrap();
        let out = eval_program_with_params(
            &prog,
            &NullResolver,
            ".",
            &[("w".to_string(), Value::Number(30.0))],
        )
        .unwrap();
        assert_eq!(out.echoes, vec!["ECHO: 30, 20"]);

        // Overrides only touch main-file top-level vars — not a `let` local of
        // the same name inside a function.
        let prog2 = parse("function f()=let(w=5) w; w=1; echo(f(), w);").unwrap();
        let out2 = eval_program_with_params(
            &prog2,
            &NullResolver,
            ".",
            &[("w".to_string(), Value::Number(99.0))],
        )
        .unwrap();
        assert_eq!(out2.echoes, vec!["ECHO: 5, 99"]);
    }

    #[test]
    fn vm_recursion_and_tce() {
        // Non-tail recursion (factorial).
        assert_eq!(
            echoes("function f(n)=n<2?1:n*f(n-1); echo(f(6));"),
            vec!["ECHO: 720"]
        );
        // Tail-recursive accumulator to large depth must not overflow.
        assert_eq!(
            echoes("function s(n,a=0)=n==0?a:s(n-1,a+n); echo(s(100000));"),
            vec!["ECHO: 5.00005e+9"]
        );
        // Mutual (non-self) recursion: g calls h and vice versa.
        assert_eq!(
            echoes("function h(n)=n==0?0:g(n-1); function g(n)=n==0?1:h(n-1); echo(g(7), h(7));"),
            vec!["ECHO: 0, 1"]
        );
    }

    #[test]
    fn vm_internal_call_defaults_use_callee_definition_scope() {
        // The direct `inner()` call exercises the positional VM fast path.
        // `outer` compiles to a VM call of `inner`, so that invocation's zero
        // pre-evaluated arguments flow through `call_function_values` instead.
        assert_eq!(
            echoes(concat!(
                "x=10; $d=1; function inner(a=x,b=$d)=[a,b]; function outer()=inner();",
                "module caller(){x=20; $d=7; echo(inner(),outer());} caller();",
            )),
            vec!["ECHO: [10, 7], [10, 7]"]
        );
    }

    #[test]
    fn vm_paths_do_not_run_supplied_defaults() {
        // `fast(...)` uses the direct positional VM path; `via_vm()` invokes
        // `callee(...)` from bytecode with an already-evaluated argument.
        assert_eq!(
            echoes(concat!(
                "function fast(a=echo(\"fast-dead\")1)=a;",
                "function callee(a=echo(\"callee-dead\")1)=a;",
                "function via_vm()=callee(echo(\"vm-arg\")3);",
                "echo(fast(echo(\"fast-arg\")2)); echo(via_vm());",
            )),
            vec![
                "ECHO: \"fast-arg\"",
                "ECHO: 2",
                "ECHO: \"vm-arg\"",
                "ECHO: 3",
            ]
        );
    }

    #[test]
    fn tree_walk_tco_refills_omitted_defaults() {
        // `assert` forces tree-walk, and the self-tail-call omits `step`; this
        // must reuse the frame while re-evaluating the default each iteration.
        assert_eq!(
            echoes(concat!(
                "function down(n,step=1)=assert(true) ",
                "n<=0 ? 0 : down(n-step); echo(down(20000));",
            )),
            vec!["ECHO: 0"]
        );
    }

    #[test]
    fn vm_let_shadowing_and_free_vars() {
        // let bindings, sequential visibility, and shadowing a parameter.
        assert_eq!(
            echoes("function f(x)=let(a=x+1,b=a*2) let(x=b) x+a; echo(f(3));"),
            vec!["ECHO: 12"] // a=4, b=8, inner x=8 -> 8+4
        );
        // A function reads a module-level (free) variable and a $ special.
        assert_eq!(
            echoes("K=10; function f(x)=x+K; echo(f(5));"),
            vec!["ECHO: 15"]
        );
        assert_eq!(
            echoes("$q=7; function f(x)=x+$q; echo(f(1));"),
            vec!["ECHO: 8"]
        );
    }

    #[test]
    fn vm_vectors_ranges_index() {
        assert_eq!(
            echoes("function f(v)=v[0]+v.y+v[2]; echo(f([1,2,3]));"),
            vec!["ECHO: 6"]
        );
        assert_eq!(
            echoes("function mk(a,b)=[a,b,a+b]; echo(mk(2,3));"),
            vec!["ECHO: [2, 3, 5]"]
        );
        // A range built inside a function, consumed by a comprehension outside.
        assert_eq!(
            echoes("function r(n)=[0:n]; echo([for(i=r(3)) i]);"),
            vec!["ECHO: [0, 1, 2, 3]"]
        );
    }

    #[test]
    fn vm_falls_back_for_unsupported_bodies() {
        // Comprehension in a function body: compiler bails, tree-walk handles it.
        assert_eq!(
            echoes("function sq(n)=[for(i=[1:n]) i*i]; echo(sq(4));"),
            vec!["ECHO: [1, 4, 9, 16]"]
        );
        // A function literal captured inside a body: bail + correct closure.
        assert_eq!(
            echoes("function adder(k)=function(x) x+k; echo(adder(10)(5));"),
            vec!["ECHO: 15"]
        );
        // Short-circuit && / || inside a compiled body.
        assert_eq!(
            echoes("function t(a,b)=a && b; echo(t(true,false), t(true,true));"),
            vec!["ECHO: false, true"]
        );
    }

    #[test]
    fn use_inside_included_file_resolves_relative() {
        // main includes lib/a.scad, which `use`s a sibling b.scad. The `use`
        // must resolve relative to a.scad's directory (`lib/`), not the main
        // file's — the bug that kept BOSL2's builtins.scad from loading.
        struct R;
        impl FileResolver for R {
            fn load(&self, path: &str, from: &str) -> Option<LoadedFile> {
                let files = [
                    ("lib/a.scad", "use <b.scad>\nfunction val() = sz();"),
                    ("lib/b.scad", "function sz() = 7;"),
                ];
                let full = if from.is_empty() || from == "." {
                    path.to_string()
                } else {
                    format!("{from}/{path}")
                };
                files
                    .iter()
                    .find(|(k, _)| *k == full || *k == path)
                    .map(|(k, v)| {
                        let dir = k
                            .rsplit_once('/')
                            .map(|(d, _)| d.to_string())
                            .unwrap_or_default();
                        LoadedFile {
                            key: k.to_string(),
                            source: v.to_string(),
                            dir,
                        }
                    })
            }
        }
        let prog = openrscad_syntax::parse("include <lib/a.scad>\necho(val());").unwrap();
        let out = eval_program_with(&prog, &R, ".").unwrap();
        assert_eq!(out.echoes, vec!["ECHO: 7"]);
    }

    #[test]
    fn nested_children_forwarding() {
        // `children()` used inside a module's own children must forward to the
        // grandparent's children, not re-read the same frame (which would loop
        // forever). BOSL2's attachment system relies on this.
        let out = eval(
            "module outer() { children(); }\n\
             module wrap() { outer() children(); }\n\
             wrap() cube(2);",
        );
        assert_ne!(out.node, Node::Empty, "forwarded cube was lost");
    }

    #[test]
    fn parent_module_stack_tracks_nested_user_modules() {
        assert_eq!(
            echoes(concat!(
                "echo(\"top\", $parent_modules, parent_module(0));",
                "module a() { translate([0,0,0]) b(); }",
                "module b() { echo(\"nested\", $parent_modules, parent_module(0), ",
                "parent_module(1), parent_module()); }",
                "a();",
            )),
            vec![
                "ECHO: \"top\", undef, undef",
                "ECHO: \"nested\", 2, \"b\", \"a\", \"a\"",
            ]
        );
    }

    #[test]
    fn parent_module_stack_survives_children_forwarding() {
        assert_eq!(
            echoes(concat!(
                "module direct() { children(); }",
                "module forward() { direct() children(); }",
                "module outer() { forward() children(); }",
                "outer() echo(\"child\", $parent_modules, parent_module(0), ",
                "parent_module(1), parent_module(2), parent_module(3));",
            )),
            vec!["ECHO: \"child\", 3, \"direct\", \"forward\", \"outer\", undef"]
        );
    }

    #[test]
    fn text_glyph_outline() {
        // text() produces a closed polygon placed by `valign`. Upstream can pin
        // the exact bbox because it bundles the font; here the face varies by
        // machine, so this asserts what holds whatever the face is: a
        // non-degenerate "A" roughly as tall as the requested size, resting on
        // the baseline for `valign="baseline"` and hanging below the line top
        // for `valign="top"`. How far above the baseline the glyph starts is
        // the font's business — plenty of faces do not put "A" exactly on it.
        let Some(family) = installed_family() else {
            eprintln!("skipped: no fonts installed");
            return;
        };
        let extent = |valign: &str| {
            let out = eval(&format!(
                "text(\"A\", size = 10, font = \"{family}\", valign = \"{valign}\");"
            ));
            let Node::Polygon { points, paths } = out.node else {
                panic!("expected polygon, got {:?}", out.node);
            };
            assert!(!paths.unwrap().is_empty());
            let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
            for p in &points {
                x0 = x0.min(p[0]);
                x1 = x1.max(p[0]);
                y0 = y0.min(p[1]);
                y1 = y1.max(p[1]);
            }
            (x0, x1, y0, y1)
        };

        let (x0, x1, y0, y1) = extent("baseline");
        // A cap-height "A" at size 10 lands well inside this range for any face.
        assert!((3.0..20.0).contains(&(x1 - x0)), "width {}", x1 - x0);
        assert!((5.0..20.0).contains(&(y1 - y0)), "height {}", y1 - y0);
        assert!(
            y0 > -0.01,
            "an \"A\" should not dip below the baseline, got {y0}"
        );

        // `valign="top"` puts the face's ascender at y = 0, so the glyph moves
        // wholly underneath it, keeping its size and its horizontal placement.
        let (tx0, tx1, ty0, ty1) = extent("top");
        assert!(
            ty1 < 0.01,
            "top-aligned text should sit below y=0, got {ty1}"
        );
        assert!(
            (tx0, tx1) == (x0, x1),
            "valign must not move the text sideways"
        );
        assert!(
            ((ty1 - ty0) - (y1 - y0)).abs() < 1e-9,
            "valign must not resize the text"
        );
    }

    #[test]
    fn viewport_vars_report_final_values() {
        // A script assigning $vp* is observable on the output.
        let out = eval_program(&parse("$vpd = 99; $vpr = [1,2,3]; cube(1);").unwrap()).unwrap();
        assert_eq!(out.viewport.vpd, Some(99.0));
        assert_eq!(out.viewport.vpr, Some([1.0, 2.0, 3.0]));
        // Defaults when the script doesn't touch them.
        let out = eval_program(&parse("cube(1);").unwrap()).unwrap();
        assert_eq!(out.viewport.vpd, Some(140.0));
        assert_eq!(out.viewport.vpt, Some([0.0, 0.0, 0.0]));
    }

    #[test]
    fn text_font_warns_when_unavailable() {
        let Some(_) = installed_family() else {
            eprintln!("skipped: no fonts installed");
            return;
        };
        // An unavailable font warns (spanned) but still renders.
        let src = "text(\"A\", font=\"No Such Family 4Q7X\");";
        let out = eval_program(&parse(src).unwrap()).unwrap();
        assert!(matches!(strip_provenance(out.node), Node::Polygon { .. }));
        let w = out
            .warnings
            .iter()
            .find(|w| w.message.contains("font"))
            .expect("font warning");
        let span = w.span.clone().expect("warning should carry a span");
        assert_eq!(&src[span], src);
        // Every family/style the machine actually has renders without warning.
        let available: Vec<String> = text::font_completions()
            .into_iter()
            .map(|c| c.value)
            .take(8)
            .collect();
        for font in available {
            let src = format!("text(\"A\", font=\"{font}\");");
            let out = eval_program(&parse(&src).unwrap()).unwrap();
            assert!(out.warnings.is_empty(), "{font}: {:?}", out.warnings);
        }
    }

    #[test]
    fn range_indexing() {
        // A range indexes as [start, step, end] (BOSL2's is_range relies on it).
        assert_eq!(
            echoes("r=[1:2:9]; echo(r[0], r[1], r[2], r[3]);"),
            vec!["ECHO: 1, 2, 9, undef"]
        );
        // 2-arg range normalizes to step 1.
        assert_eq!(
            echoes("r=[3:7]; echo(r[0], r[1], r[2]);"),
            vec!["ECHO: 3, 1, 7"]
        );
    }

    #[test]
    fn undef_iterable_has_no_iterations() {
        assert_eq!(
            echoes("echo([for (i=undef) i]); for (i=undef) echo(i);"),
            vec!["ECHO: []"]
        );
    }

    #[test]
    fn matrix_multiplication() {
        // OpenSCAD linear-algebra `*`: dot, matrix·vector, vector·matrix, matrix·matrix.
        assert_eq!(echoes("echo([1,2,3]*[4,5,6]);"), vec!["ECHO: 32"]);
        assert_eq!(echoes("echo([[1,2],[3,4]]*[5,6]);"), vec!["ECHO: [17, 39]"]);
        assert_eq!(echoes("echo([1,2]*[[5,6],[7,8]]);"), vec!["ECHO: [19, 22]"]);
        assert_eq!(
            echoes("echo([[1,2],[3,4]]*[[5,6],[7,8]]);"),
            vec!["ECHO: [[19, 22], [43, 50]]"]
        );
        // Dimension mismatch → undef.
        assert_eq!(echoes("echo([1,2,3]*[[1,2],[3,4]]);"), vec!["ECHO: undef"]);
    }

    #[test]
    fn search_vector_needle_and_undef_index() {
        // A list needle with no index matches the whole row (find a vector in a
        // list of vectors) — the case BOSL2's in_list relies on.
        assert_eq!(
            echoes("echo(search([[0,0,1]],[[0,0,1],[1,0,0],[0,1,0]],num_returns_per_match=1));"),
            vec!["ECHO: [0]"]
        );
        // An explicitly-passed undef index behaves like no index.
        assert_eq!(
            echoes("echo(search([[1,0,0]],[[0,0,1],[1,0,0]],1,undef));"),
            vec!["ECHO: [1]"]
        );
        // A scalar needle still matches column 0 of a table.
        assert_eq!(
            echoes("echo(search([3],[[0,3],[1,4]]));"),
            vec!["ECHO: [[]]"]
        );
    }

    #[test]
    fn string_repr_and_builtins() {
        assert_eq!(echoes("echo(\"hi\");"), vec!["ECHO: \"hi\""]);
        assert_eq!(
            echoes("echo(chr(65), ord(\"A\"));"),
            vec!["ECHO: \"A\", 65"]
        );
        assert_eq!(
            echoes("echo(str(\"n=\", 5, true));"),
            vec!["ECHO: \"n=5true\""]
        );
        assert_eq!(
            echoes("echo([\"a\", \"b\"]);"),
            vec!["ECHO: [\"a\", \"b\"]"]
        );
        assert_eq!(echoes("s=\"abc\"; echo(s[1]);"), vec!["ECHO: \"b\""]);
    }

    #[test]
    fn strict_numeric_reducers_and_extended_builtins() {
        // OpenSCAD rejects the whole min/max/norm input when any element cannot
        // be converted to a number; it does not silently skip bad elements.
        assert_eq!(
            echoes(concat!(
                "echo(max([1,\"x\",3]), min([1,undef,3]), max(1,\"x\",3));",
                "echo(norm([3,\"x\",4]), norm([3,undef,4]), norm([]));",
            )),
            vec!["ECHO: undef, undef, undef", "ECHO: undef, undef, 0"]
        );

        // chr() concatenates every scalar/vector/range argument. version_num()
        // accepts a two- or three-component version vector.
        assert_eq!(
            echoes(concat!(
                "echo(chr(65,66), chr([65,\"x\",66],67), chr([65:67],68));",
                "echo(version_num([1,2]), version_num([1,2,3]), version_num([1]));",
            )),
            vec![
                "ECHO: \"AB\", \"ABC\", \"ABCD\"",
                "ECHO: 10200, 10203, undef"
            ]
        );
    }

    #[test]
    fn number_formatting() {
        assert_eq!(
            echoes("echo(1/3, 1e10, 1000000, 3.0, 0);"),
            vec!["ECHO: 0.333333, 1e+10, 1e+6, 3, 0"]
        );
        assert_eq!(
            echoes("echo(sign(-4), sign(0), sign(4));"),
            vec!["ECHO: -1, 0, 1"]
        );
    }

    #[test]
    fn function_literals() {
        assert_eq!(echoes("f = function(x) x*x; echo(f(5));"), vec!["ECHO: 25"]);
        assert_eq!(
            echoes("g = function(a,b) a+b; echo(g(3,4), is_function(g), is_function(3));"),
            vec!["ECHO: 7, true, false"]
        );
        assert_eq!(
            echoes("f = function(x) x*x; echo([for(i=[1:4]) f(i)]);"),
            vec!["ECHO: [1, 4, 9, 16]"]
        );
        // recursion through the bound name
        assert_eq!(
            echoes("h = function(n) n<=1 ? 1 : n*h(n-1); echo(h(5));"),
            vec!["ECHO: 120"]
        );
    }

    struct MapResolver(std::collections::HashMap<String, String>);
    impl FileResolver for MapResolver {
        fn load(&self, path: &str, _from: &str) -> Option<LoadedFile> {
            self.0.get(path).map(|s| LoadedFile {
                key: path.to_string(),
                source: s.clone(),
                dir: ".".to_string(),
            })
        }
    }

    #[test]
    fn include_and_use() {
        let mut files = std::collections::HashMap::new();
        files.insert(
            "lib.scad".to_string(),
            "function sqr(x)=x*x; K=7; echo(\"libran\");".to_string(),
        );
        let resolver = MapResolver(files);

        // `use` imports definitions only (no top-level echo, no variables).
        let prog = openrscad_syntax::parse("use <lib.scad>\necho(sqr(5), is_undef(K));").unwrap();
        let out = eval_program_with(&prog, &resolver, ".").unwrap();
        assert_eq!(out.echoes, vec!["ECHO: 25, true"]);

        // `include` splices everything (top-level echo runs; variables visible).
        let prog = openrscad_syntax::parse("include <lib.scad>\necho(sqr(4), K);").unwrap();
        let out = eval_program_with(&prog, &resolver, ".").unwrap();
        assert_eq!(out.echoes, vec!["ECHO: \"libran\"", "ECHO: 16, 7"]);
    }

    // ---- diagnostic spans (for inline editor squiggles) ----------------

    #[test]
    fn eval_error_carries_main_statement_span() {
        let src = "cube(1);\nassert(false);";
        let prog = openrscad_syntax::parse(src).unwrap();
        let e = eval_program(&prog).unwrap_err();
        let span = e.span.expect("eval error should carry a span");
        assert_eq!(&src[span], "assert(false);");
    }

    #[test]
    fn warning_carries_main_statement_span() {
        let src = "nope();";
        let prog = openrscad_syntax::parse(src).unwrap();
        let out = eval_program(&prog).unwrap();
        let w = out
            .warnings
            .iter()
            .find(|w| w.message.contains("nope"))
            .expect("unknown-module warning");
        let span = w.span.clone().expect("warning should carry a span");
        assert_eq!(&src[span], "nope();");
    }

    #[test]
    fn library_error_attributes_to_main_call_site() {
        // A `use`d module that fails: the error must be squiggled at the main
        // call site, never at a (meaningless) offset into the library source.
        let mut files = std::collections::HashMap::new();
        files.insert(
            "lib.scad".to_string(),
            "module boom() { assert(false); }".to_string(),
        );
        let resolver = MapResolver(files);
        let src = "use <lib.scad>\nboom();";
        let prog = openrscad_syntax::parse(src).unwrap();
        let e = eval_program_with(&prog, &resolver, ".").unwrap_err();
        let span = e.span.expect("should attribute to the main call site");
        assert_eq!(&src[span], "boom();");
    }

    #[test]
    fn diagnostics_json_serializes_error_and_warnings() {
        let err = Diagnostic::from_span("error", "boom".into(), &Some(3..8));
        let warns = vec![Warning {
            message: "w".into(),
            span: None,
        }];
        let json = diagnostics_json(Some(&err), &warns);
        assert!(json.contains("\"severity\":\"error\""), "{json}");
        assert!(json.contains("\"start\":3"), "{json}");
        assert!(json.contains("\"end\":8"), "{json}");
        assert!(json.contains("\"severity\":\"warning\""), "{json}");
        assert!(json.contains("\"start\":-1"), "{json}");
    }

    #[test]
    fn lexical_scoping() {
        // A global function sees the global variable, not the caller's local.
        assert_eq!(
            echoes("a=10; function f()=a; module m(){a=20; echo(f(),a);} m();"),
            vec!["ECHO: 10, 20"]
        );
        assert_eq!(
            echoes("a=10; function g(x)=x+a; module n(a){echo(g(1));} n(99);"),
            vec!["ECHO: 11"]
        );
        // A function literal closes over its defining scope.
        assert_eq!(
            echoes("x=5; lit=function() x; module m(){x=99; echo(lit());} m();"),
            vec!["ECHO: 5"]
        );
        // `$` variables are dynamically scoped through module calls.
        assert_eq!(
            echoes("$fn=8; module r(){echo($fn);} module s(){$fn=16; r();} s(); r();"),
            vec!["ECHO: 16", "ECHO: 8"]
        );
    }

    #[test]
    fn power_operator() {
        // right-associative, binds tighter than unary and *
        assert_eq!(
            echoes("echo(2^10, -2^2, 2^-3, 2^3^2, 3*2^2);"),
            vec!["ECHO: 1024, -4, 0.125, 512, 12"]
        );
    }

    #[test]
    fn cstyle_for() {
        assert_eq!(
            echoes("echo([for(k=0,s=0;k<=3;k=k+1,s=s+k) s]);"),
            vec!["ECHO: [0, 1, 3, 6]"]
        );
        assert_eq!(
            echoes("echo([for(a=1,b=1;a<=5;a=a+1,b=b*a) b]);"),
            vec!["ECHO: [1, 2, 6, 24, 120]"]
        );
    }

    #[test]
    fn polyhedron_node() {
        let out = eval(
            "polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], \
             faces=[[0,1,2],[0,1,3],[1,2,3],[0,2,3]]);",
        );
        match out.node {
            Node::Polyhedron { points, faces } => {
                assert_eq!(points.len(), 4);
                assert_eq!(faces.len(), 4);
                assert_eq!(points[1], [1.0, 0.0, 0.0]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn module_children() {
        let out = eval("module m() { translate([1,0,0]) children(); } m() { cube(2); sphere(3); }");
        match out.node {
            Node::Translate { child, .. } => match *child {
                Node::Group(c) => assert_eq!(c.len(), 2),
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn children_count_and_index() {
        let out = eval(
            "module m() { echo($children); children(1); } m() { cube(1); sphere(2); cube(3); }",
        );
        assert_eq!(out.echoes, vec!["ECHO: 3"]);
        assert!(matches!(out.node, Node::Sphere { .. }));
    }

    #[test]
    fn list_builtins() {
        assert_eq!(echoes("echo(search(3,[1,2,3,4]));"), vec!["ECHO: [2]"]);
        assert_eq!(echoes("echo(search(\"b\",\"abcabc\"));"), vec!["ECHO: [1]"]);
        assert_eq!(
            echoes("echo(lookup(2.5,[[0,0],[1,10],[2,20],[3,30]]));"),
            vec!["ECHO: 25"]
        );
        assert_eq!(
            echoes("echo(is_undef(undef), is_list([1]), is_num(1), is_string(\"s\"));"),
            vec!["ECHO: true, true, true, true"]
        );
    }

    #[test]
    fn color_wraps_child() {
        // color() wraps its child in a Color node (geometry unchanged inside).
        let out = eval("color(\"red\") cube(2);");
        match out.node {
            Node::Color { rgba, child } => {
                assert_eq!(rgba, [1.0, 0.0, 0.0, 1.0]);
                assert_eq!(
                    *child,
                    Node::Cube {
                        size: [2.0, 2.0, 2.0],
                        center: false
                    }
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn color_vector_and_alpha() {
        let out = eval("color([0,0,1], 0.5) cube(1);");
        match out.node {
            Node::Color { rgba, .. } => assert_eq!(rgba, [0.0, 0.0, 1.0, 0.5]),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn unknown_color_warns_and_passes_through() {
        let out = eval("color(\"chartreusey\") cube(1);");
        // Falls back to the bare child (no Color wrapper) and warns.
        assert!(matches!(out.node, Node::Cube { .. }));
        assert!(out
            .warnings
            .iter()
            .any(|w| w.message.contains("unknown color")));
    }

    #[test]
    fn highlight_and_background_wrap() {
        match eval("#cube(1);").node {
            Node::Highlight(c) => assert!(matches!(*c, Node::Cube { .. })),
            other => panic!("unexpected: {other:?}"),
        }
        match eval("%sphere(2);").node {
            Node::Background(c) => assert!(matches!(*c, Node::Sphere { .. })),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn disable_modifier() {
        let out = eval("union() { cube(1); *sphere(5); }");
        match out.node {
            Node::Union(children) => assert_eq!(children.len(), 1),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
