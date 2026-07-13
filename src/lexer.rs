use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::span::{Position, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Json,
    Json5,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    String(String),
    Identifier(String),
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
    lex_with_mode(source, InputMode::Json)
}

pub fn lex_with_mode(source: &str, mode: InputMode) -> (Vec<Token>, Vec<Diagnostic>) {
    Lexer::new(source, mode).lex()
}

struct Lexer<'a> {
    source: &'a str,
    mode: InputMode,
    index: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, mode: InputMode) -> Self {
        Self {
            source,
            mode,
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
                '/' if self.mode == InputMode::Json5 => self.lex_comment(),
                '{' => self.simple(TokenKind::LeftBrace),
                '}' => self.simple(TokenKind::RightBrace),
                '[' => self.simple(TokenKind::LeftBracket),
                ']' => self.simple(TokenKind::RightBracket),
                ':' => self.simple(TokenKind::Colon),
                ',' => self.simple(TokenKind::Comma),
                '"' => self.lex_string('"'),
                '\'' if self.mode == InputMode::Json5 => self.lex_string('\''),
                '+' | '.' if self.mode == InputMode::Json5 => self.lex_number(),
                '-' | '0'..='9' => self.lex_number(),
                c if self.mode == InputMode::Json5 && is_identifier_start(c) => {
                    self.lex_identifier()
                }
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

    fn lex_comment(&mut self) {
        let start = self.position();
        self.bump();
        match self.peek() {
            Some('/') => {
                self.bump();
                while !matches!(self.peek(), None | Some('\n' | '\r')) {
                    self.bump();
                }
            }
            Some('*') => {
                self.bump();
                loop {
                    match (self.peek(), self.peek_next()) {
                        (Some('*'), Some('/')) => {
                            self.bump();
                            self.bump();
                            break;
                        }
                        (Some(_), _) => {
                            self.bump();
                        }
                        (None, _) => {
                            self.diagnostics.push(Diagnostic::new(
                                "E012",
                                DiagnosticKind::UnterminatedBlockComment,
                                Some(Span::new(start, self.position())),
                            ));
                            break;
                        }
                    }
                }
            }
            _ => {
                self.diagnostics.push(Diagnostic::new(
                    "E001",
                    DiagnosticKind::InvalidCharacter('/'),
                    Some(Span::new(start, self.position())),
                ));
            }
        }
    }

    fn lex_identifier(&mut self) {
        let start = self.position();
        let start_index = self.index;
        self.bump();
        while matches!(self.peek(), Some(ch) if is_identifier_continue(ch)) {
            self.bump();
        }

        let text = &self.source[start_index..self.index];
        let kind = match text {
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            "Infinity" | "NaN" => {
                self.diagnostics.push(Diagnostic::new(
                    "E013",
                    DiagnosticKind::NonFiniteNumber,
                    Some(Span::new(start, self.position())),
                ));
                return;
            }
            _ => TokenKind::Identifier(text.to_string()),
        };
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.position()),
        });
    }

    fn lex_string(&mut self, quote: char) {
        let start = self.position();
        self.bump();
        let mut value = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                ch if ch == quote => {
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
                        Some('\'') if self.mode == InputMode::Json5 => value.push('\''),
                        Some('\\') => value.push('\\'),
                        Some('/') => value.push('/'),
                        Some('b') => value.push('\u{0008}'),
                        Some('f') => value.push('\u{000C}'),
                        Some('n') => value.push('\n'),
                        Some('r') => value.push('\r'),
                        Some('t') => value.push('\t'),
                        Some('\n') if self.mode == InputMode::Json5 => {}
                        Some('\r') if self.mode == InputMode::Json5 => {
                            if self.peek() == Some('\n') {
                                self.bump();
                            }
                        }
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

        if matches!(self.peek(), Some('-' | '+')) {
            if self.peek() == Some('+') && self.mode == InputMode::Json {
                self.invalid_number(start);
                return;
            }
            self.bump();
        }

        if self.mode == InputMode::Json5 {
            let non_finite = if self.source[self.index..].starts_with("Infinity") {
                Some("Infinity")
            } else if self.source[self.index..].starts_with("NaN") {
                Some("NaN")
            } else {
                None
            };
            if let Some(word) = non_finite {
                for _ in word.chars() {
                    self.bump();
                }
                self.diagnostics.push(Diagnostic::new(
                    "E013",
                    DiagnosticKind::NonFiniteNumber,
                    Some(Span::new(start, self.position())),
                ));
                return;
            }
        }

        if self.mode == InputMode::Json5
            && self.peek() == Some('0')
            && matches!(self.peek_next(), Some('x' | 'X'))
        {
            self.bump();
            self.bump();
            let digits_start = self.index;
            while matches!(self.peek(), Some(ch) if ch.is_ascii_hexdigit()) {
                self.bump();
            }
            if self.index == digits_start {
                self.invalid_number(start);
                return;
            }
            let digits = &self.source[digits_start..self.index];
            let Ok(value) = u128::from_str_radix(digits, 16) else {
                self.invalid_number(start);
                return;
            };
            let negative = self.source[start_index..].starts_with('-');
            let text = if negative {
                format!("-{}", value)
            } else {
                value.to_string()
            };
            self.tokens.push(Token {
                kind: TokenKind::Number(text),
                span: Span::new(start, self.position()),
            });
            return;
        }

        let integer_start = self.index;
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
        }
        let has_integer = self.index > integer_start;
        if self.index.saturating_sub(integer_start) > 1
            && self.source[integer_start..self.index].starts_with('0')
        {
            self.invalid_number(start);
            return;
        }
        let mut has_fraction = false;
        let mut has_fraction_digits = false;

        if self.peek() == Some('.') {
            has_fraction = true;
            self.bump();
            let fraction_start = self.index;
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
            has_fraction_digits = self.index > fraction_start;
            if self.index == fraction_start && self.mode == InputMode::Json {
                self.invalid_number(start);
                return;
            }
        }

        if !has_integer && (!has_fraction || !has_fraction_digits) {
            self.invalid_number(start);
            return;
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                self.invalid_number(start);
                return;
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.bump();
            }
        }

        let mut text = self.source[start_index..self.index].to_string();
        if self.mode == InputMode::Json5 {
            if text.starts_with('+') {
                text.remove(0);
            }
            if text.starts_with('.') {
                text.insert(0, '0');
            } else if text.starts_with("-.") {
                text.insert(1, '0');
            }
            if let Some(dot) = text.find('.') {
                let exponent_follows = text
                    .as_bytes()
                    .get(dot + 1)
                    .map(|byte| matches!(*byte, b'e' | b'E'))
                    .unwrap_or(false);
                if exponent_follows {
                    text.insert(dot + 1, '0');
                }
            }
            if text.ends_with('.') {
                text.push('0');
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::Number(text),
            span: Span::new(start, self.position()),
        });
    }

    fn invalid_number(&mut self, start: Position) {
        self.diagnostics.push(Diagnostic::new(
            "E005",
            DiagnosticKind::InvalidNumber,
            Some(Span::new(start, self.position())),
        ));
    }

    fn peek(&self) -> Option<char> {
        self.source[self.index..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.index..].chars();
        chars.next()?;
        chars.next()
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

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_alphanumeric()
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

    #[test]
    fn tokenizes_lossless_json5_syntax() {
        let source = "// heading\n{unquoted: 'value', hex: 0x10, plus: +.5, tail: 5., exp: 5.e2,}";
        let (tokens, diagnostics) = lex_with_mode(source, InputMode::Json5);

        assert!(diagnostics.is_empty());
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Identifier("unquoted".into())));
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::String("value".into())));
        for expected in ["16", "0.5", "5.0", "5.0e2"] {
            assert!(tokens
                .iter()
                .any(|token| token.kind == TokenKind::Number(expected.into())));
        }
    }

    #[test]
    fn supports_block_comments_and_string_line_continuations() {
        let source = "/* note */ {'message': 'hello\\\nworld'}";
        let (tokens, diagnostics) = lex_with_mode(source, InputMode::Json5);

        assert!(diagnostics.is_empty());
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::String("helloworld".into())));
    }

    #[test]
    fn strict_mode_rejects_json5_only_syntax() {
        let (_, diagnostics) = lex_with_mode("{key: 'value'}", InputMode::Json);

        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn reports_unterminated_block_comments() {
        let (_, diagnostics) = lex_with_mode("/* never closed", InputMode::Json5);

        assert_eq!(
            diagnostics[0].kind,
            DiagnosticKind::UnterminatedBlockComment
        );
        assert_eq!(diagnostics[0].span.unwrap().start.column, 1);
    }

    #[test]
    fn rejects_non_finite_json5_numbers() {
        for source in ["Infinity", "+Infinity", "-Infinity", "NaN", "+NaN", "-NaN"] {
            let (_, diagnostics) = lex_with_mode(source, InputMode::Json5);
            assert_eq!(diagnostics[0].kind, DiagnosticKind::NonFiniteNumber);
        }
    }
}
