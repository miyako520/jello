use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::span::{Position, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    String(String),
    Number(String),
    True,
    False,
    Null,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    Lexer::new(source).lex()
}

struct Lexer<'a> {
    source: &'a str,
    index: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            index: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn lex(mut self) -> (Vec<Token>, Vec<Diagnostic>) {
        while let Some(ch) = self.peek() {
            match ch {
                c if c.is_whitespace() => {
                    self.bump();
                }
                '{' => self.simple(TokenKind::LeftBrace),
                '}' => self.simple(TokenKind::RightBrace),
                '[' => self.simple(TokenKind::LeftBracket),
                ']' => self.simple(TokenKind::RightBracket),
                ':' => self.simple(TokenKind::Colon),
                ',' => self.simple(TokenKind::Comma),
                '"' => self.lex_string(),
                '-' | '0'..='9' => self.lex_number(),
                't' => self.lex_keyword("true", TokenKind::True),
                'f' => self.lex_keyword("false", TokenKind::False),
                'n' => self.lex_keyword("null", TokenKind::Null),
                other => {
                    let start = self.position();
                    self.bump();
                    let span = Span::new(start, self.position());
                    self.diagnostics.push(Diagnostic::new(
                        "E001",
                        DiagnosticKind::InvalidCharacter(other),
                        Some(span),
                    ));
                }
            }
        }

        let pos = self.position();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(pos, pos),
        });
        (self.tokens, self.diagnostics)
    }

    fn simple(&mut self, kind: TokenKind) {
        let start = self.position();
        self.bump();
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.position()),
        });
    }

    fn lex_keyword(&mut self, expected: &str, kind: TokenKind) {
        let start = self.position();
        if self.source[self.index..].starts_with(expected) {
            for _ in expected.chars() {
                self.bump();
            }
            self.tokens.push(Token {
                kind,
                span: Span::new(start, self.position()),
            });
        } else {
            let ch = self.bump().unwrap_or('\0');
            self.diagnostics.push(Diagnostic::new(
                "E001",
                DiagnosticKind::InvalidCharacter(ch),
                Some(Span::new(start, self.position())),
            ));
        }
    }

    fn lex_string(&mut self) {
        let start = self.position();
        self.bump();
        let mut value = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::String(value),
                        span: Span::new(start, self.position()),
                    });
                    return;
                }
                '\\' => {
                    self.bump();
                    match self.bump() {
                        Some('"') => value.push('"'),
                        Some('\\') => value.push('\\'),
                        Some('/') => value.push('/'),
                        Some('b') => value.push('\u{0008}'),
                        Some('f') => value.push('\u{000C}'),
                        Some('n') => value.push('\n'),
                        Some('r') => value.push('\r'),
                        Some('t') => value.push('\t'),
                        Some('u') => match self.read_unicode_escape() {
                            Some(decoded) => value.push(decoded),
                            None => {
                                self.diagnostics.push(Diagnostic::new(
                                    "E004",
                                    DiagnosticKind::InvalidUnicodeEscape,
                                    Some(Span::new(start, self.position())),
                                ));
                                return;
                            }
                        },
                        Some(other) => {
                            self.diagnostics.push(Diagnostic::new(
                                "E003",
                                DiagnosticKind::InvalidEscape(other),
                                Some(Span::new(start, self.position())),
                            ));
                            return;
                        }
                        None => break,
                    }
                }
                '\n' | '\r' => {
                    self.diagnostics.push(Diagnostic::new(
                        "E002",
                        DiagnosticKind::UnterminatedString,
                        Some(Span::new(start, self.position())),
                    ));
                    return;
                }
                other => {
                    value.push(other);
                    self.bump();
                }
            }
        }

        self.diagnostics.push(Diagnostic::new(
            "E002",
            DiagnosticKind::UnterminatedString,
            Some(Span::new(start, self.position())),
        ));
    }

    fn read_unicode_escape(&mut self) -> Option<char> {
        let mut digits = String::new();
        for _ in 0..4 {
            let ch = self.peek()?;
            if !ch.is_ascii_hexdigit() {
                return None;
            }
            digits.push(ch);
            self.bump();
        }
        u32::from_str_radix(&digits, 16)
            .ok()
            .and_then(char::from_u32)
    }

    fn lex_number(&mut self) {
        let start = self.position();
        let start_index = self.index;

        if self.peek() == Some('-') {
            self.bump();
        }

        match self.peek() {
            Some('0') => {
                self.bump();
            }
            Some('1'..='9') => {
                self.bump();
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
            }
            _ => {
                self.diagnostics.push(Diagnostic::new(
                    "E005",
                    DiagnosticKind::InvalidNumber,
                    Some(Span::new(start, self.position())),
                ));
                return;
            }
        }

        if self.peek() == Some('.') {
            self.bump();
            if !matches!(self.peek(), Some('0'..='9')) {
                self.diagnostics.push(Diagnostic::new(
                    "E005",
                    DiagnosticKind::InvalidNumber,
                    Some(Span::new(start, self.position())),
                ));
                return;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                self.diagnostics.push(Diagnostic::new(
                    "E005",
                    DiagnosticKind::InvalidNumber,
                    Some(Span::new(start, self.position())),
                ));
                return;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }

        let text = self.source[start_index..self.index].to_string();
        self.tokens.push(Token {
            kind: TokenKind::Number(text),
            span: Span::new(start, self.position()),
        });
    }

    fn peek(&self) -> Option<char> {
        self.source[self.index..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn position(&self) -> Position {
        Position::new(self.index, self.line, self.column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_basic_json() {
        let (tokens, diagnostics) = lex(r#"{"name":"Ada","ok":true,"n":12.5}"#);

        assert!(diagnostics.is_empty());
        assert!(matches!(tokens[0].kind, TokenKind::LeftBrace));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::String("Ada".into())));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::True));
        assert!(tokens
            .iter()
            .any(|t| t.kind == TokenKind::Number("12.5".into())));
    }

    #[test]
    fn reports_unterminated_string() {
        let (_, diagnostics) = lex(r#""abc"#);

        assert_eq!(diagnostics[0].kind, DiagnosticKind::UnterminatedString);
    }
}
