//! Lexing and parsing for the OpenSCAD language (M0 subset).
//!
//! Clean-room: grammar reconstructed from public documentation and black-box
//! observation of the OpenSCAD CLI. No OpenSCAD (GPL) source is consulted.

pub mod ast;
pub mod customizer;
pub mod lexer;
pub mod parser;

pub use ast::*;

/// A lexing or parsing error, carrying a source byte span.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct SyntaxError {
    pub message: String,
    pub span: std::ops::Range<usize>,
}

impl SyntaxError {
    pub fn new(message: String, span: std::ops::Range<usize>) -> Self {
        SyntaxError { message, span }
    }
}

/// Parse a complete program from source.
pub fn parse(src: &str) -> Result<Program, SyntaxError> {
    let tokens = lexer::lex(src)?;
    let mut parser = parser::Parser::new(tokens, src);
    parser.parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_primitive() {
        let prog = parse("cube(10);").unwrap();
        assert_eq!(prog.len(), 1);
        match &prog[0].node {
            Stmt::ModuleCall {
                name,
                args,
                children,
                ..
            } => {
                assert_eq!(name, "cube");
                assert_eq!(args.len(), 1);
                assert!(children.is_empty());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn nested_transforms() {
        let prog = parse("translate([1,2,3]) rotate([0,0,45]) cube(2, center=true);").unwrap();
        assert_eq!(prog.len(), 1);
        match &prog[0].node {
            Stmt::ModuleCall { name, children, .. } => {
                assert_eq!(name, "translate");
                assert_eq!(children.len(), 1);
                match &children[0].node {
                    Stmt::ModuleCall { name, .. } => assert_eq!(name, "rotate"),
                    other => panic!("unexpected: {other:?}"),
                }
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn boolean_block() {
        let prog = parse("difference() { cube(10); sphere(6); }").unwrap();
        match &prog[0].node {
            Stmt::ModuleCall { name, children, .. } => {
                assert_eq!(name, "difference");
                assert_eq!(children.len(), 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn for_and_if() {
        let prog = parse("for (i = [0:2:10]) if (i > 2) translate([i,0,0]) cube(1);").unwrap();
        match &prog[0].node {
            Stmt::For { bindings, body } => {
                assert_eq!(bindings.len(), 1);
                assert_eq!(bindings[0].0, "i");
                assert_eq!(body.len(), 1);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn expression_precedence() {
        // 1 + 2 * 3 == 7  should parse as 1 + (2*3)
        let prog = parse("x = 1 + 2 * 3;").unwrap();
        match &prog[0].node {
            Stmt::Assign { value, .. } => match value {
                Expr::Binary {
                    op: BinOp::Add,
                    rhs,
                    ..
                } => {
                    assert!(matches!(**rhs, Expr::Binary { op: BinOp::Mul, .. }));
                }
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn module_and_function_defs() {
        let prog =
            parse("function sq(x) = x * x; module ring(r) { cylinder(r=r, h=1); } ring(sq(2));")
                .unwrap();
        assert_eq!(prog.len(), 3);
        assert!(matches!(prog[0].node, Stmt::FunctionDef { .. }));
        assert!(matches!(prog[1].node, Stmt::ModuleDef { .. }));
    }

    #[test]
    fn statements_carry_spans() {
        // Top-level statement spans cover their source range; nested statements
        // carry their own (finer) spans.
        let src = "cube(1);\ntranslate([0,0,1]) sphere(2);";
        let prog = parse(src).unwrap();
        assert_eq!(prog.len(), 2);
        // Statement spans run from the first token to the end of the trailing `;`.
        assert_eq!(&src[prog[0].span.clone()], "cube(1);");
        assert_eq!(&src[prog[1].span.clone()], "translate([0,0,1]) sphere(2);");
        match &prog[1].node {
            Stmt::ModuleCall { children, .. } => {
                assert_eq!(&src[children[0].span.clone()], "sphere(2);");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn modifier_prefix() {
        let prog = parse("*cube(1); #sphere(2);").unwrap();
        match &prog[0].node {
            Stmt::ModuleCall { modifier, .. } => {
                assert_eq!(*modifier, Some(Modifier::Disable));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn include_and_use_paths_preserve_raw_contents() {
        let prog = parse(
            "include <=@scope/my library/~user\\part.scad>\nuse < C:\\Open SCAD\\lib@2.scad >",
        )
        .unwrap();

        assert!(matches!(
            &prog[0].node,
            Stmt::Include { path } if path == r"=@scope/my library/~user\part.scad"
        ));
        assert!(matches!(
            &prog[1].node,
            Stmt::Use { path } if path == r" C:\Open SCAD\lib@2.scad "
        ));
    }

    #[test]
    fn include_path_requires_opening_angle_bracket() {
        let err = parse("include library.scad>").unwrap_err();
        assert!(err.message.contains("expected `<` before include path"));
        assert_eq!(err.span, 8..15);
    }

    #[test]
    fn unterminated_raw_use_path_is_a_parser_error() {
        let src = r"use <@scope/~user\library file.scad";
        let err = parse(src).unwrap_err();
        assert!(err.message.contains("unterminated include path"));
        assert_eq!(err.span, src.len()..src.len());
    }
}
