//! Recursive-descent parser producing the typed AST.

use crate::ast::*;
use crate::lexer::{SpannedToken, Token};
use crate::SyntaxError;

pub struct Parser<'a> {
    tokens: Vec<SpannedToken>,
    pos: usize,
    /// Source text, for reconstructing `<include paths>` and EOF spans.
    src: &'a str,
}

type PResult<T> = Result<T, SyntaxError>;

/// Human-readable description of a token for diagnostics.
fn describe(tok: Option<&Token>) -> String {
    match tok {
        None => "end of input".to_string(),
        Some(t) => format!("{t:?}"),
    }
}

impl<'a> Parser<'a> {
    pub fn new(tokens: Vec<SpannedToken>, src: &'a str) -> Self {
        Parser {
            tokens,
            pos: 0,
            src,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|s| &s.token)
    }

    fn peek2(&self) -> Option<&Token> {
        self.tokens.get(self.pos + 1).map(|s| &s.token)
    }

    fn span_here(&self) -> std::ops::Range<usize> {
        self.tokens
            .get(self.pos)
            .map(|s| s.span.clone())
            .unwrap_or(self.src.len()..self.src.len())
    }

    /// End byte offset of the most recently consumed token (0 at the very start).
    fn prev_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map(|s| s.span.end)
            .unwrap_or(0)
    }

    /// Parse a statement, recording its source byte span (first token start ..
    /// last token end) so the evaluator can attribute diagnostics to it.
    fn parse_statement_spanned(&mut self) -> PResult<Spanned<Stmt>> {
        let start = self.span_here().start;
        let node = self.parse_statement()?;
        let end = self.prev_end().max(start);
        Ok(Spanned::new(node, start..end))
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn advance(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).map(|s| s.token.clone());
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, tok: &Token) -> bool {
        if self.peek() == Some(tok) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Token) -> PResult<()> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(SyntaxError::new(
                format!("expected `{:?}`, found {}", tok, describe(self.peek())),
                self.span_here(),
            ))
        }
    }

    /// Parse an `<include/use path>` by taking the raw source between the `<`
    /// and the next `>` (paths aren't ordinary token sequences).
    fn expect_angle_path(&mut self) -> PResult<String> {
        let lt = self
            .tokens
            .get(self.pos)
            .filter(|s| s.token == Token::Lt)
            .ok_or_else(|| {
                SyntaxError::new("expected `<` before include path".into(), self.span_here())
            })?;
        let start = lt.span.end;
        self.pos += 1;
        loop {
            match self.tokens.get(self.pos) {
                Some(s) if s.token == Token::Gt => {
                    let path = self.src[start..s.span.start].to_string();
                    self.pos += 1;
                    return Ok(path);
                }
                Some(_) => self.pos += 1,
                None => {
                    return Err(SyntaxError::new(
                        "unterminated include path (missing `>`)".into(),
                        self.span_here(),
                    ))
                }
            }
        }
    }

    fn expect_ident(&mut self) -> PResult<String> {
        match self.advance() {
            Some(Token::Ident(name)) => Ok(name),
            other => Err(SyntaxError::new(
                format!("expected identifier, found {}", describe(other.as_ref())),
                self.span_here(),
            )),
        }
    }

    // ---- statements ----------------------------------------------------

    pub fn parse_program(&mut self) -> PResult<Program> {
        let mut stmts = Vec::new();
        while !self.at_end() {
            if self.eat(&Token::Semi) {
                continue;
            }
            stmts.push(self.parse_statement_spanned()?);
        }
        Ok(stmts)
    }

    fn parse_statement(&mut self) -> PResult<Stmt> {
        match self.peek() {
            Some(Token::LBrace) => Ok(Stmt::Block(self.parse_block()?)),
            Some(Token::Include) => {
                self.advance();
                let path = self.expect_angle_path()?;
                Ok(Stmt::Include { path })
            }
            Some(Token::Use) => {
                self.advance();
                let path = self.expect_angle_path()?;
                Ok(Stmt::Use { path })
            }
            Some(Token::Module) => self.parse_module_def(),
            Some(Token::Function) => self.parse_function_def(),
            Some(Token::If) => self.parse_if(),
            Some(Token::For) => self.parse_for(),
            Some(Token::Let) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let bindings = self.parse_bindings()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_child_body()?;
                Ok(Stmt::Let { bindings, body })
            }
            Some(Token::Star | Token::Bang | Token::Hash | Token::Percent) => {
                let modifier = match self.advance().unwrap() {
                    Token::Star => Modifier::Disable,
                    Token::Bang => Modifier::Root,
                    Token::Hash => Modifier::Highlight,
                    Token::Percent => Modifier::Background,
                    _ => unreachable!(),
                };
                self.parse_module_call(Some(modifier))
            }
            Some(Token::Ident(_)) => {
                if self.peek2() == Some(&Token::Assign) {
                    self.parse_assign()
                } else {
                    self.parse_module_call(None)
                }
            }
            other => Err(SyntaxError::new(
                format!(
                    "unexpected token in statement position: {}",
                    describe(other)
                ),
                self.span_here(),
            )),
        }
    }

    /// Parse the "body" that follows a construct like `translate(...)`, `if(...)`,
    /// `for(...)`, or `module foo()`: either a `{ block }`, a single statement, or
    /// an empty `;`.
    fn parse_child_body(&mut self) -> PResult<Vec<Spanned<Stmt>>> {
        if self.peek() == Some(&Token::LBrace) {
            self.parse_block()
        } else if self.eat(&Token::Semi) {
            Ok(Vec::new())
        } else {
            Ok(vec![self.parse_statement_spanned()?])
        }
    }

    fn parse_block(&mut self) -> PResult<Vec<Spanned<Stmt>>> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while self.peek() != Some(&Token::RBrace) {
            if self.at_end() {
                return Err(SyntaxError::new(
                    "unterminated block".into(),
                    self.span_here(),
                ));
            }
            if self.eat(&Token::Semi) {
                continue;
            }
            stmts.push(self.parse_statement_spanned()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_assign(&mut self) -> PResult<Stmt> {
        let name = self.expect_ident()?;
        self.expect(&Token::Assign)?;
        let value = self.parse_expr()?;
        self.expect(&Token::Semi)?;
        Ok(Stmt::Assign { name, value })
    }

    fn parse_module_call(&mut self, modifier: Option<Modifier>) -> PResult<Stmt> {
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let args = self.parse_args()?;
        self.expect(&Token::RParen)?;
        let children = self.parse_child_body()?;
        Ok(Stmt::ModuleCall {
            modifier,
            name,
            args,
            children,
        })
    }

    fn parse_module_def(&mut self) -> PResult<Stmt> {
        self.expect(&Token::Module)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_child_body()?;
        Ok(Stmt::ModuleDef { name, params, body })
    }

    fn parse_function_def(&mut self) -> PResult<Stmt> {
        self.expect(&Token::Function)?;
        let name = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;
        self.expect(&Token::Assign)?;
        let body = self.parse_expr()?;
        self.expect(&Token::Semi)?;
        Ok(Stmt::FunctionDef { name, params, body })
    }

    fn parse_if(&mut self) -> PResult<Stmt> {
        self.expect(&Token::If)?;
        self.expect(&Token::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&Token::RParen)?;
        let then = self.parse_child_body()?;
        let els = if self.eat(&Token::Else) {
            self.parse_child_body()?
        } else {
            Vec::new()
        };
        Ok(Stmt::If { cond, then, els })
    }

    fn parse_for(&mut self) -> PResult<Stmt> {
        self.expect(&Token::For)?;
        self.expect(&Token::LParen)?;
        let bindings = self.parse_bindings()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_child_body()?;
        Ok(Stmt::For { bindings, body })
    }

    fn parse_bindings(&mut self) -> PResult<Vec<(String, Expr)>> {
        let mut out = Vec::new();
        // Empty binding lists: `let()`/`for()`, and either the init or the
        // update clause of a C-style `for(; cond; )` — BOSL2 writes the empty
        // init form when the loop variable comes from the enclosing scope.
        if matches!(self.peek(), Some(Token::RParen | Token::Semi)) {
            return Ok(out);
        }
        loop {
            let name = self.expect_ident()?;
            self.expect(&Token::Assign)?;
            let value = self.parse_expr()?;
            out.push((name, value));
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(out)
    }

    fn parse_params(&mut self) -> PResult<Vec<Param>> {
        let mut out = Vec::new();
        while self.peek() != Some(&Token::RParen) {
            let name = self.expect_ident()?;
            let default = if self.eat(&Token::Assign) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            out.push(Param { name, default });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(out)
    }

    fn parse_args(&mut self) -> PResult<Vec<Arg>> {
        let mut out = Vec::new();
        while self.peek() != Some(&Token::RParen) {
            let name = if matches!(self.peek(), Some(Token::Ident(_)))
                && self.peek2() == Some(&Token::Assign)
            {
                let n = self.expect_ident()?;
                self.expect(&Token::Assign)?;
                Some(n)
            } else {
                None
            };
            let value = self.parse_expr()?;
            out.push(Arg { name, value });
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(out)
    }

    /// Whether the current token can begin an expression (used to detect the
    /// trailing body of an `echo(...)`/`assert(...)` expression prefix).
    fn at_expr_start(&self) -> bool {
        matches!(
            self.peek(),
            Some(
                Token::Number(_)
                    | Token::Str(_)
                    | Token::True
                    | Token::False
                    | Token::Undef
                    | Token::Ident(_)
                    | Token::Let
                    | Token::Function
                    | Token::LParen
                    | Token::LBracket
                    | Token::Minus
                    | Token::Plus
                    | Token::Bang
            )
        )
    }

    // ---- expressions ---------------------------------------------------

    pub fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> PResult<Expr> {
        let cond = self.parse_binary(1)?;
        if self.eat(&Token::Question) {
            let then = self.parse_ternary()?;
            self.expect(&Token::Colon)?;
            let els = self.parse_ternary()?;
            Ok(Expr::Ternary {
                cond: Box::new(cond),
                then: Box::new(then),
                els: Box::new(els),
            })
        } else {
            Ok(cond)
        }
    }

    fn peek_binop(&self) -> Option<(BinOp, u8)> {
        let op = match self.peek()? {
            Token::Or => (BinOp::Or, 1),
            Token::And => (BinOp::And, 2),
            Token::Eq => (BinOp::Eq, 3),
            Token::Ne => (BinOp::Ne, 3),
            Token::Lt => (BinOp::Lt, 4),
            Token::Le => (BinOp::Le, 4),
            Token::Gt => (BinOp::Gt, 4),
            Token::Ge => (BinOp::Ge, 4),
            Token::Plus => (BinOp::Add, 5),
            Token::Minus => (BinOp::Sub, 5),
            Token::Star => (BinOp::Mul, 6),
            Token::Slash => (BinOp::Div, 6),
            Token::Percent => (BinOp::Mod, 6),
            _ => return None,
        };
        Some(op)
    }

    fn parse_binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        while let Some((op, prec)) = self.peek_binop() {
            if prec < min_prec {
                break;
            }
            self.advance();
            let rhs = self.parse_binary(prec + 1)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            Some(Token::Minus) => Some(UnOp::Neg),
            Some(Token::Plus) => Some(UnOp::Pos),
            Some(Token::Bang) => Some(UnOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let expr = self.parse_unary()?;
            Ok(Expr::Unary {
                op,
                expr: Box::new(expr),
            })
        } else {
            self.parse_power()
        }
    }

    /// `base ^ exp` — exponentiation binds tighter than unary and `*`, and is
    /// right-associative (`2^3^2 == 2^(3^2)`; the exponent may be unary).
    fn parse_power(&mut self) -> PResult<Expr> {
        let base = self.parse_postfix()?;
        if self.eat(&Token::Caret) {
            let exp = self.parse_unary()?;
            Ok(Expr::Binary {
                op: BinOp::Pow,
                lhs: Box::new(base),
                rhs: Box::new(exp),
            })
        } else {
            Ok(base)
        }
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut base = self.parse_primary()?;
        loop {
            if self.eat(&Token::LBracket) {
                let index = self.parse_expr()?;
                self.expect(&Token::RBracket)?;
                base = Expr::Index {
                    base: Box::new(base),
                    index: Box::new(index),
                };
            } else if self.eat(&Token::Dot) {
                let field = self.expect_ident()?;
                base = Expr::Member {
                    base: Box::new(base),
                    field,
                };
            } else if self.peek() == Some(&Token::LParen) {
                // calling the result of an expression, e.g. `funcs[0](3)`
                self.advance();
                let args = self.parse_args()?;
                self.expect(&Token::RParen)?;
                base = Expr::CallValue {
                    callee: Box::new(base),
                    args,
                };
            } else {
                break;
            }
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.advance();
                Ok(Expr::Number(n))
            }
            Some(Token::Str(s)) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Some(Token::Undef) => {
                self.advance();
                Ok(Expr::Undef)
            }
            Some(Token::Let) => self.parse_let(),
            Some(Token::Function) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let params = self.parse_params()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_expr()?;
                Ok(Expr::FunctionLiteral {
                    params,
                    body: Box::new(body),
                })
            }
            Some(Token::Ident(name)) => {
                self.advance();
                if self.eat(&Token::LParen) {
                    let args = self.parse_args()?;
                    self.expect(&Token::RParen)?;
                    // `echo(...) expr` / `assert(...) expr` are expression
                    // prefixes; a bare `assert(cond)` / `echo(...)` with no
                    // trailing expression evaluates to `undef` (OpenSCAD does
                    // this — BOSL2 relies on bare `assert(...)` in value
                    // position, e.g. `x = assert(cond, msg);`).
                    if name == "echo" || name == "assert" {
                        let body = if self.at_expr_start() {
                            Box::new(self.parse_expr()?)
                        } else {
                            Box::new(Expr::Undef)
                        };
                        if name == "echo" {
                            Ok(Expr::Echo { args, body })
                        } else {
                            Ok(Expr::Assert { args, body })
                        }
                    } else {
                        Ok(Expr::Call { name, args })
                    }
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            Some(Token::LParen) => {
                self.advance();
                let e = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            Some(Token::LBracket) => self.parse_bracket(),
            other => Err(SyntaxError::new(
                format!(
                    "unexpected token in expression: {}",
                    describe(other.as_ref())
                ),
                self.span_here(),
            )),
        }
    }

    fn parse_let(&mut self) -> PResult<Expr> {
        self.expect(&Token::Let)?;
        self.expect(&Token::LParen)?;
        let bindings = self.parse_bindings()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_expr()?;
        Ok(Expr::Let {
            bindings,
            body: Box::new(body),
        })
    }

    /// `[ ... ]` — an empty vector, a range, or a vector / list comprehension.
    fn parse_bracket(&mut self) -> PResult<Expr> {
        self.expect(&Token::LBracket)?;
        if self.eat(&Token::RBracket) {
            return Ok(Expr::Vector(Vec::new()));
        }

        // A leading comprehension keyword means the whole bracket is a list of
        // comprehension elements.
        if self.at_list_keyword() {
            let elems = self.parse_list_elements()?;
            self.expect(&Token::RBracket)?;
            return Ok(Expr::Vector(elems));
        }

        let first = self.parse_expr()?;
        if self.eat(&Token::Colon) {
            // range: [start:end] or [start:step:end]
            let mid = self.parse_expr()?;
            let (step, end) = if self.eat(&Token::Colon) {
                let end = self.parse_expr()?;
                (Some(Box::new(mid)), end)
            } else {
                (None, mid)
            };
            self.expect(&Token::RBracket)?;
            Ok(Expr::Range {
                start: Box::new(first),
                step,
                end: Box::new(end),
            })
        } else {
            let mut elems = vec![ListElem::Item(first)];
            while self.eat(&Token::Comma) {
                if self.peek() == Some(&Token::RBracket) {
                    break;
                }
                elems.push(self.parse_list_element()?);
            }
            self.expect(&Token::RBracket)?;
            Ok(Expr::Vector(elems))
        }
    }

    fn at_list_keyword(&self) -> bool {
        matches!(self.peek(), Some(Token::For | Token::Let | Token::If))
            || matches!(self.peek(), Some(Token::Ident(n)) if n == "each")
    }

    fn parse_list_elements(&mut self) -> PResult<Vec<ListElem>> {
        let mut elems = vec![self.parse_list_element()?];
        while self.eat(&Token::Comma) {
            if self.peek() == Some(&Token::RBracket) {
                break;
            }
            elems.push(self.parse_list_element()?);
        }
        Ok(elems)
    }

    fn parse_list_element(&mut self) -> PResult<ListElem> {
        match self.peek() {
            Some(Token::For) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let init = self.parse_bindings()?;
                if self.eat(&Token::Semi) {
                    // C-style: for (init; cond; update) body
                    let cond = self.parse_expr()?;
                    self.expect(&Token::Semi)?;
                    let update = self.parse_bindings()?;
                    self.expect(&Token::RParen)?;
                    let body = Box::new(self.parse_list_element()?);
                    Ok(ListElem::CFor {
                        init,
                        cond,
                        update,
                        body,
                    })
                } else {
                    self.expect(&Token::RParen)?;
                    let body = Box::new(self.parse_list_element()?);
                    Ok(ListElem::For {
                        bindings: init,
                        body,
                    })
                }
            }
            Some(Token::Let) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let bindings = self.parse_bindings()?;
                self.expect(&Token::RParen)?;
                let body = Box::new(self.parse_list_element()?);
                Ok(ListElem::Let { bindings, body })
            }
            Some(Token::If) => {
                self.advance();
                self.expect(&Token::LParen)?;
                let cond = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                let then = Box::new(self.parse_list_element()?);
                let els = if self.eat(&Token::Else) {
                    Some(Box::new(self.parse_list_element()?))
                } else {
                    None
                };
                Ok(ListElem::If { cond, then, els })
            }
            Some(Token::Ident(n)) if n == "each" => {
                self.advance();
                Ok(ListElem::Each(Box::new(self.parse_list_element()?)))
            }
            // A parenthesized comprehension element, e.g. `(each a)`.
            Some(Token::LParen)
                if matches!(self.peek2(), Some(Token::For | Token::Let | Token::If))
                    || matches!(self.peek2(), Some(Token::Ident(n)) if n == "each") =>
            {
                self.advance(); // (
                let elem = self.parse_list_element()?;
                self.expect(&Token::RParen)?;
                Ok(elem)
            }
            _ => Ok(ListElem::Item(self.parse_expr()?)),
        }
    }
}
