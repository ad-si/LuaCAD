//! logos-based lexer for the OpenSCAD language (M0 subset).

use logos::Logos;

/// Process a double-quoted string literal into its unescaped contents.
fn unescape(lex: &mut logos::Lexer<Token>) -> String {
    let s = lex.slice();
    // strip surrounding quotes
    let inner = &s[1..s.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*[^*]*\*+([^/*][^*]*\*+)*/")]
pub enum Token {
    // literals
    #[regex(r"(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    Number(f64),

    #[regex(r#""(?:[^"\\]|\\.)*""#, unescape)]
    Str(String),

    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("undef")]
    Undef,

    // keywords
    #[token("module")]
    Module,
    #[token("function")]
    Function,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("for")]
    For,
    #[token("let")]
    Let,
    #[token("include")]
    Include,
    #[token("use")]
    Use,

    // identifiers (including `$special` variables)
    #[regex(r"\$?[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_owned())]
    Ident(String),

    // punctuation
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,

    // operators
    #[token("=")]
    Assign,
    #[token("==")]
    Eq,
    #[token("!=")]
    Ne,
    #[token("<")]
    Lt,
    #[token("<=")]
    Le,
    #[token(">")]
    Gt,
    #[token(">=")]
    Ge,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("^")]
    Caret,
    #[token("!")]
    Bang,
    #[token("#")]
    Hash,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("?")]
    Question,
}

/// A token together with its source byte span.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: std::ops::Range<usize>,
}

/// Lex an entire source string. Returns an error at the first invalid token.
pub fn lex(src: &str) -> Result<Vec<SpannedToken>, crate::SyntaxError> {
    let mut out = Vec::new();
    let mut lexer = Token::lexer(src);
    // The contents of an include/use path are not OpenSCAD tokens: punctuation
    // such as `@`, `~`, and `\` is part of the filename. Once the opening `<`
    // has been lexed in that context, skip verbatim to the next `>` and emit a
    // synthetic closing delimiter. Keeping both delimiter spans means the
    // parser can recover the exact path from the original source.
    let mut expect_angle_path = false;
    while let Some(res) = lexer.next() {
        match res {
            Ok(token) => {
                let span = lexer.span();
                // `<=` must also enter path mode: in `include <=name.scad>`,
                // the `=` is raw filename content, not the comparison token
                // Logos would normally produce.
                let starts_angle_path = expect_angle_path && matches!(token, Token::Lt | Token::Le);
                expect_angle_path = matches!(token, Token::Include | Token::Use);

                if starts_angle_path {
                    let path_start = span.start + 1;
                    out.push(SpannedToken {
                        token: Token::Lt,
                        span: span.start..path_start,
                    });

                    if let Some(relative_end) = src[path_start..].find('>') {
                        let close_start = path_start + relative_end;
                        // `bump` extends the current Logos token internally, but
                        // its opening `<` span was captured above. Account for
                        // any `=` Logos already consumed as part of a `<=`.
                        lexer.bump(close_start + 1 - span.end);
                        out.push(SpannedToken {
                            token: Token::Gt,
                            span: close_start..close_start + 1,
                        });
                    } else {
                        // Let the parser report the more useful unterminated-path
                        // diagnostic, even if the raw path contains characters
                        // that the ordinary lexer would reject.
                        lexer.bump(src.len() - span.end);
                    }
                } else {
                    out.push(SpannedToken { token, span });
                }
            }
            Err(_) => {
                return Err(crate::SyntaxError::new(
                    format!("unexpected character(s) `{}`", lexer.slice()),
                    lexer.span(),
                ))
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Token> {
        lex(src).unwrap().into_iter().map(|s| s.token).collect()
    }

    #[test]
    fn numbers() {
        assert_eq!(
            toks("1 2.5 .5 1e3 2.0e-2"),
            vec![
                Token::Number(1.0),
                Token::Number(2.5),
                Token::Number(0.5),
                Token::Number(1000.0),
                Token::Number(0.02),
            ]
        );
    }

    #[test]
    fn special_vars() {
        assert_eq!(
            toks("$fn $fa x"),
            vec![
                Token::Ident("$fn".into()),
                Token::Ident("$fa".into()),
                Token::Ident("x".into()),
            ]
        );
    }

    #[test]
    fn strings_and_escapes() {
        assert_eq!(toks(r#""a\nb""#), vec![Token::Str("a\nb".into())]);
    }

    #[test]
    fn comments_skipped() {
        assert_eq!(
            toks("1 // line\n2 /* block */ 3"),
            vec![Token::Number(1.0), Token::Number(2.0), Token::Number(3.0),]
        );
    }

    #[test]
    fn block_comment_with_stars() {
        assert_eq!(
            toks("1 /** doc ** star */ 2"),
            vec![Token::Number(1.0), Token::Number(2.0),]
        );
    }

    #[test]
    fn include_path_contents_are_lexed_verbatim() {
        assert_eq!(
            toks(r"include <=@scope/my library/~user\part.scad>"),
            vec![Token::Include, Token::Lt, Token::Gt],
        );
    }

    #[test]
    fn angle_comparison_tokens_are_unchanged() {
        assert_eq!(
            toks("x < 2 && x > 0"),
            vec![
                Token::Ident("x".into()),
                Token::Lt,
                Token::Number(2.0),
                Token::And,
                Token::Ident("x".into()),
                Token::Gt,
                Token::Number(0.0),
            ]
        );
    }
}
