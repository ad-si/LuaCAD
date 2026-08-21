//! Typed AST for the OpenSCAD language (M0 subset).

/// A syntax node together with its source byte span. Statement lists carry these
/// so the evaluator can attribute a runtime error/warning to the offending
/// statement's source range (used for inline editor diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: std::ops::Range<usize>,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: std::ops::Range<usize>) -> Self {
        Spanned { node, span }
    }
}

/// Binary operators, in the OpenSCAD sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
}

/// Expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Bool(bool),
    Str(String),
    Undef,
    /// A variable reference.
    Ident(String),
    /// `[a, b, c]` — also the surface for list comprehensions, whose elements
    /// are [`ListElem`]s (a plain expression is `ListElem::Item`).
    Vector(Vec<ListElem>),
    /// `[start : end]` or `[start : step : end]`
    Range {
        start: Box<Expr>,
        step: Option<Box<Expr>>,
        end: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// `cond ? a : b`
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// `base[index]`
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `base.x` / `.y` / `.z` (sugar for indexing 0/1/2)
    Member {
        base: Box<Expr>,
        field: String,
    },
    /// A function call `name(args)`.
    Call {
        name: String,
        args: Vec<Arg>,
    },
    /// `let(a = 1, b = 2) body`
    Let {
        bindings: Vec<(String, Expr)>,
        body: Box<Expr>,
    },
    /// An anonymous function literal: `function (params) body`.
    FunctionLiteral {
        params: Vec<Param>,
        body: Box<Expr>,
    },
    /// Calling the result of an expression: `expr(args)`.
    CallValue {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    /// `echo(args) body` — echo as an expression prefix (2019.05); echoes then
    /// evaluates to `body`.
    Echo {
        args: Vec<Arg>,
        body: Box<Expr>,
    },
    /// `assert(cond, msg) body` — assert as an expression prefix.
    Assert {
        args: Vec<Arg>,
        body: Box<Expr>,
    },
}

/// An element of a vector / list comprehension.
#[derive(Debug, Clone, PartialEq)]
pub enum ListElem {
    /// A plain expression element.
    Item(Expr),
    /// `each elem` — splice the produced list(s) in place (the operand is itself
    /// a comprehension element, so `each if (c) xs` is valid).
    Each(Box<ListElem>),
    /// `for (bindings) body` — range/cartesian form.
    For {
        bindings: Vec<(String, Expr)>,
        body: Box<ListElem>,
    },
    /// C-style `for (init; cond; update) body` (2019.05 accumulator form).
    CFor {
        init: Vec<(String, Expr)>,
        cond: Expr,
        update: Vec<(String, Expr)>,
        body: Box<ListElem>,
    },
    /// `if (cond) then [else els]`
    If {
        cond: Expr,
        then: Box<ListElem>,
        els: Option<Box<ListElem>>,
    },
    /// `let (bindings) body`
    Let {
        bindings: Vec<(String, Expr)>,
        body: Box<ListElem>,
    },
}

/// A call argument, optionally named.
#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

/// A module / function parameter declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
}

/// Debug modifiers that can prefix a module instantiation: `* ! # %`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// `*` — disable this subtree.
    Disable,
    /// `!` — show only this subtree (root modifier).
    Root,
    /// `#` — highlight.
    Highlight,
    /// `%` — background / transparent.
    Background,
}

/// Statements.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// A variable assignment `name = expr;`
    Assign { name: String, value: Expr },
    /// A module instantiation, e.g. `translate([1,0,0]) cube(2);`
    ModuleCall {
        modifier: Option<Modifier>,
        name: String,
        args: Vec<Arg>,
        children: Vec<Spanned<Stmt>>,
    },
    /// `module name(params) body`
    ModuleDef {
        name: String,
        params: Vec<Param>,
        body: Vec<Spanned<Stmt>>,
    },
    /// `function name(params) = expr;`
    FunctionDef {
        name: String,
        params: Vec<Param>,
        body: Expr,
    },
    /// `for (var = range) body` (possibly nested over multiple bindings)
    For {
        bindings: Vec<(String, Expr)>,
        body: Vec<Spanned<Stmt>>,
    },
    /// `if (cond) then [else els]`
    If {
        cond: Expr,
        then: Vec<Spanned<Stmt>>,
        els: Vec<Spanned<Stmt>>,
    },
    /// `let (bindings) body` at statement level — binds variables for children.
    Let {
        bindings: Vec<(String, Expr)>,
        body: Vec<Spanned<Stmt>>,
    },
    /// A bare `{ ... }` block.
    Block(Vec<Spanned<Stmt>>),
    /// `include <path>` — splice the file's top-level statements here.
    Include { path: String },
    /// `use <path>` — import only the file's module/function definitions.
    Use { path: String },
}

/// A parsed program: a sequence of top-level statements, each with its span.
pub type Program = Vec<Spanned<Stmt>>;
