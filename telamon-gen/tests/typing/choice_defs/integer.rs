pub use super::utils::RcStr;

pub use super::telamon_gen::ast::*;
pub use super::telamon_gen::lexer::{Lexer, LexerPosition, Position, Spanned};
pub use super::telamon_gen::parser;

#[cfg(test)]
mod undefined {
    pub use super::*;

    /// Missing the set MySet from a Integer.
    #[test]
    fn parameter() {
        assert_eq!(
            parser::parse_ast(Lexer::new(
                b"define integer foo($arg in MySet): \"mycode\"
              end"
                .to_vec()
            ))
            .unwrap()
            .type_check()
            .err(),
            Some(TypeError::Undefined {
                object_name: Spanned {
                    beg: Position {
                        position: LexerPosition {
                            line: 0,
                            column: 15
                        },
                        ..Default::default()
                    },
                    end: Position {
                        position: LexerPosition {
                            line: 0,
                            column: 18
                        },
                        ..Default::default()
                    },
                    data: String::from("MySet"),
                }
            })
        );
    }
}

#[cfg(test)]
mod redefinition {
    pub use super::*;

    /// Redefinition of the foo Integer.
    #[test]
    fn integer() {
        assert_eq!(
            parser::parse_ast(Lexer::new(
                b"define integer foo(): \"mycode\"
              end
              define integer foo(): \"mycode\"
              end"
                .to_vec()
            ))
            .unwrap()
            .type_check()
            .err(),
            Some(TypeError::Redefinition {
                object_kind: Spanned {
                    beg: Position {
                        position: LexerPosition {
                            line: 0,
                            column: 15
                        },
                        ..Default::default()
                    },
                    end: Position {
                        position: LexerPosition {
                            line: 0,
                            column: 18
                        },
                        ..Default::default()
                    },
                    data: Hint::Integer,
                },
                object_name: Spanned {
                    beg: Position {
                        position: LexerPosition {
                            line: 2,
                            column: 29
                        },
                        ..Default::default()
                    },
                    end: Position {
                        position: LexerPosition {
                            line: 2,
                            column: 32
                        },
                        ..Default::default()
                    },
                    data: String::from("foo"),
                }
            })
        );
    }
}
