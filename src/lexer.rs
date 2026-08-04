use std::ops::Range;

use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::repair_plan::{RecordedRepair, RepairKind};
use crate::span::{Position, Span};

pub const MAX_DIAGNOSTICS: usize = 64;
pub const MAX_TOKENS: usize = 250_000;
pub const MAX_REPAIR_EDITS: usize = 10_000;

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

#[cfg(test)]
fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_with_mode(source, InputMode::Json)
}

pub fn lex_with_mode(source: &str, mode: InputMode) -> (Vec<Token>, Vec<Diagnostic>) {
    let (tokens, diagnostics, _) = Lexer::new(source, mode, false).lex();
    (tokens, diagnostics)
}

pub(crate) fn lex_for_repair(
    source: &str,
    mode: InputMode,
) -> (Vec<Token>, Vec<Diagnostic>, Vec<RecordedRepair>) {
    Lexer::new(source, mode, true).lex()
}

struct Lexer<'a> {
    source: &'a str,
    mode: InputMode,
    index: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    previous_was_cr: bool,
    diagnostics: Vec<Diagnostic>,
    audit_normalizations: bool,
    edits: Vec<RecordedRepair>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, mode: InputMode, audit_normalizations: bool) -> Self {
        Self {
            source,
            mode,
            index: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            previous_was_cr: false,
            diagnostics: Vec::new(),
            audit_normalizations,
            edits: Vec::new(),
        }
    }

    fn lex(mut self) -> (Vec<Token>, Vec<Diagnostic>, Vec<RecordedRepair>) {
        if self
            .diagnostics
            .try_reserve_exact(MAX_DIAGNOSTICS + 1)
            .is_err()
        {
            return (
                Vec::new(),
                vec![Diagnostic::new(
                    "E020",
                    DiagnosticKind::AllocationFailed,
                    None,
                )],
                Vec::new(),
            );
        }
        let token_capacity = self
            .source
            .len()
            .saturating_add(1)
            .min(MAX_TOKENS.saturating_add(1));
        if self.tokens.try_reserve_exact(token_capacity).is_err() {
            self.diagnostics.push(Diagnostic::new(
                "E020",
                DiagnosticKind::AllocationFailed,
                None,
            ));
            return (self.tokens, self.diagnostics, self.edits);
        }
        if self.audit_normalizations {
            let edit_capacity = self.source.len().min(MAX_REPAIR_EDITS);
            if self.edits.try_reserve_exact(edit_capacity).is_err() {
                self.diagnostics.push(Diagnostic::new(
                    "E020",
                    DiagnosticKind::AllocationFailed,
                    None,
                ));
                return (self.tokens, self.diagnostics, self.edits);
            }
        }

        while let Some(ch) = self.peek() {
            match ch {
                c if is_whitespace(c, self.mode) => self.lex_whitespace(),
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

            if self.diagnostics.len() >= MAX_DIAGNOSTICS && self.peek().is_some() {
                self.diagnostics.push(Diagnostic::new(
                    "E017",
                    DiagnosticKind::TooManyErrors {
                        max_errors: MAX_DIAGNOSTICS,
                    },
                    Some(Span::new(self.position(), self.position())),
                ));
                break;
            }
            if self.tokens.len() >= MAX_TOKENS && self.peek().is_some() {
                self.diagnostics.push(Diagnostic::new(
                    "E018",
                    DiagnosticKind::TooManyTokens {
                        max_tokens: MAX_TOKENS,
                    },
                    Some(Span::new(self.position(), self.position())),
                ));
                break;
            }
        }

        let pos = self.position();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(pos, pos),
        });
        (self.tokens, self.diagnostics, self.edits)
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
        let keyword_end = self.index.saturating_add(expected.len());
        let has_boundary = self
            .source
            .get(keyword_end..)
            .and_then(|suffix| suffix.chars().next())
            .map(|ch| !is_identifier_continue(ch))
            .unwrap_or(true);
        if self.source[self.index..].starts_with(expected) && has_boundary {
            for _ in expected.chars() {
                self.bump();
            }
            self.tokens.push(Token {
                kind,
                span: Span::new(start, self.position()),
            });
        } else {
            let ch = self.peek().unwrap_or('\0');
            while matches!(self.peek(), Some(current) if is_identifier_continue(current)) {
                self.bump();
            }
            self.diagnostics.push(Diagnostic::new(
                "E001",
                DiagnosticKind::InvalidCharacter(ch),
                Some(Span::new(start, self.position())),
            ));
        }
    }

    fn lex_whitespace(&mut self) {
        let start = self.position();
        let start_index = self.index;
        let mut normalization = false;
        while let Some(ch) = self.peek() {
            if !is_whitespace(ch, self.mode) {
                break;
            }
            if self.mode == InputMode::Json5 && !is_whitespace(ch, InputMode::Json) {
                normalization = true;
            }
            self.bump();
        }
        if normalization {
            self.record_normalization(
                Span::new(start, self.position()),
                start_index..self.index,
                "",
                "removed JSON5-only whitespace",
            );
        }
    }
    fn lex_comment(&mut self) {
        let start = self.position();
        let start_index = self.index;
        self.bump();
        match self.peek() {
            Some('/') => {
                self.bump();
                while !matches!(
                    self.peek(),
                    None | Some('\n' | '\r' | '\u{2028}' | '\u{2029}')
                ) {
                    self.bump();
                }
                self.record_normalization(
                    Span::new(start, self.position()),
                    start_index..self.index,
                    "",
                    "removed JSON5 line comment",
                );
            }
            Some('*') => {
                self.bump();
                let mut terminated = false;
                loop {
                    match (self.peek(), self.peek_next()) {
                        (Some('*'), Some('/')) => {
                            self.bump();
                            self.bump();
                            terminated = true;
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
                if terminated {
                    self.record_normalization(
                        Span::new(start, self.position()),
                        start_index..self.index,
                        "",
                        "removed JSON5 block comment",
                    );
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
                    let escape_start = self.position();
                    let escape_start_index = self.index;
                    self.bump();
                    match self.bump() {
                        Some('"') => value.push('"'),
                        Some('\'') if self.mode == InputMode::Json5 => {
                            if quote == '"' {
                                self.record_normalization(
                                    Span::new(escape_start, self.position()),
                                    escape_start_index..self.index,
                                    "'",
                                    "normalized JSON5 apostrophe escape",
                                );
                            }
                            value.push('\'');
                        }
                        Some('\\') => value.push('\\'),
                        Some('/') => value.push('/'),
                        Some('b') => value.push('\u{0008}'),
                        Some('f') => value.push('\u{000C}'),
                        Some('n') => value.push('\n'),
                        Some('r') => value.push('\r'),
                        Some('t') => value.push('\t'),
                        Some('\n' | '\u{2028}' | '\u{2029}') if self.mode == InputMode::Json5 => {
                            self.record_normalization(
                                Span::new(escape_start, self.position()),
                                escape_start_index..self.index,
                                "",
                                "removed JSON5 string line continuation",
                            );
                        }
                        Some('\r') if self.mode == InputMode::Json5 => {
                            if self.peek() == Some('\n') {
                                self.bump();
                            }
                            self.record_normalization(
                                Span::new(escape_start, self.position()),
                                escape_start_index..self.index,
                                "",
                                "removed JSON5 string line continuation",
                            );
                        }
                        Some('u') => match self.read_unicode_escape() {
                            Some(decoded) => value.push(decoded),
                            None => {
                                self.diagnostics.push(Diagnostic::new(
                                    "E004",
                                    DiagnosticKind::InvalidUnicodeEscape,
                                    Some(Span::new(escape_start, self.position())),
                                ));
                                self.synchronize_string(quote);
                                return;
                            }
                        },
                        Some(other) => {
                            self.diagnostics.push(Diagnostic::new(
                                "E003",
                                DiagnosticKind::InvalidEscape(other),
                                Some(Span::new(escape_start, self.position())),
                            ));
                            self.synchronize_string(quote);
                            return;
                        }
                        None => break,
                    }
                }
                control @ '\u{0000}'..='\u{001F}' => {
                    let control_start = self.position();
                    self.bump();
                    self.diagnostics.push(Diagnostic::new(
                        "E016",
                        DiagnosticKind::UnescapedControlCharacter(control),
                        Some(Span::new(control_start, self.position())),
                    ));
                    self.synchronize_string(quote);
                    return;
                }
                separator @ ('\u{2028}' | '\u{2029}') => {
                    let separator_start = self.position();
                    let separator_start_index = self.index;
                    value.push(separator);
                    self.bump();
                    self.record_normalization(
                        Span::new(separator_start, self.position()),
                        separator_start_index..self.index,
                        match separator {
                            '\u{2028}' => "\\u2028",
                            '\u{2029}' => "\\u2029",
                            _ => unreachable!(),
                        },
                        "escaped Unicode line separator in formatted output",
                    );
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

    fn synchronize_string(&mut self, quote: char) {
        let mut escaped = false;
        while let Some(ch) = self.peek() {
            self.bump();
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                return;
            }
        }
    }

    fn read_unicode_escape(&mut self) -> Option<char> {
        let high = self.read_hex_code_unit()?;
        if (0xD800..=0xDBFF).contains(&high) {
            if self.peek() != Some('\\') {
                return None;
            }
            self.bump();
            if self.peek() != Some('u') {
                return None;
            }
            self.bump();
            let low = self.read_hex_code_unit()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return None;
            }
            let scalar = 0x10000 + (((high as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00);
            return char::from_u32(scalar);
        }
        if (0xDC00..=0xDFFF).contains(&high) {
            return None;
        }
        char::from_u32(high as u32)
    }

    fn read_hex_code_unit(&mut self) -> Option<u16> {
        let mut digits = String::new();
        for _ in 0..4 {
            let ch = self.peek()?;
            if !ch.is_ascii_hexdigit() {
                return None;
            }
            digits.push(ch);
            self.bump();
        }
        u16::from_str_radix(&digits, 16).ok()
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
            self.record_normalization(
                Span::new(start, self.position()),
                start_index..self.index,
                &text,
                "normalized JSON5 hexadecimal number",
            );
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
        if text.as_str() != &self.source[start_index..self.index] {
            self.record_normalization(
                Span::new(start, self.position()),
                start_index..self.index,
                &text,
                "normalized JSON5 number",
            );
        }
        self.tokens.push(Token {
            kind: TokenKind::Number(text),
            span: Span::new(start, self.position()),
        });
    }

    fn record_normalization(
        &mut self,
        span: Span,
        byte_range: Range<usize>,
        replacement: &str,
        description: &'static str,
    ) {
        if !self.audit_normalizations {
            return;
        }
        if self.edits.len() >= MAX_REPAIR_EDITS {
            self.diagnostics.push(Diagnostic::new(
                "E021",
                DiagnosticKind::TooManyRepairs {
                    max_repairs: MAX_REPAIR_EDITS,
                },
                Some(span),
            ));
            self.audit_normalizations = false;
            return;
        }
        let mut replacement_text = String::new();
        if replacement_text
            .try_reserve_exact(replacement.len())
            .is_err()
            || self.edits.try_reserve(1).is_err()
        {
            self.diagnostics.push(Diagnostic::new(
                "E020",
                DiagnosticKind::AllocationFailed,
                Some(span),
            ));
            self.audit_normalizations = false;
            return;
        }
        replacement_text.push_str(replacement);
        self.edits.push(RecordedRepair::replace(
            RepairKind::Json5Normalization,
            "F005",
            description,
            span,
            byte_range,
            replacement_text,
        ));
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
        match ch {
            '\r' => {
                self.line += 1;
                self.column = 1;
                self.previous_was_cr = true;
            }
            '\n' if self.previous_was_cr => {
                self.column = 1;
                self.previous_was_cr = false;
            }
            '\n' | '\u{2028}' | '\u{2029}' => {
                self.line += 1;
                self.column = 1;
                self.previous_was_cr = false;
            }
            _ => {
                self.column += 1;
                self.previous_was_cr = false;
            }
        }
        Some(ch)
    }

    fn position(&self) -> Position {
        Position::new(self.index, self.line, self.column)
    }
}

pub(crate) fn is_whitespace(ch: char, mode: InputMode) -> bool {
    match mode {
        InputMode::Json => matches!(ch, ' ' | '\t' | '\r' | '\n'),
        InputMode::Json5 => matches!(
            ch,
            '\u{0009}'..='\u{000D}'
                | '\u{0020}'
                | '\u{00A0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
        ),
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

    #[test]
    fn strict_mode_rejects_non_json_whitespace() {
        let (_, diagnostics) = lex("\u{00a0}{}");

        assert!(matches!(
            diagnostics[0].kind,
            DiagnosticKind::InvalidCharacter('\u{00a0}')
        ));
    }

    #[test]
    fn strict_mode_rejects_raw_control_characters_in_strings() {
        let (_, diagnostics) = lex("\"before\tafter\"");

        assert!(matches!(
            diagnostics[0].kind,
            DiagnosticKind::UnescapedControlCharacter('\t')
        ));
    }

    #[test]
    fn decodes_unicode_surrogate_pairs() {
        let (tokens, diagnostics) = lex(r#""\uD83D\uDE00""#);

        assert!(diagnostics.is_empty());
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::String("😀".into())));
    }

    #[test]
    fn rejects_lone_unicode_surrogates() {
        for source in [r#""\uD83D""#, r#""\uDE00""#] {
            let (_, diagnostics) = lex(source);

            assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidUnicodeEscape);
        }
    }

    #[test]
    fn tracks_json_and_json5_line_terminators() {
        for (source, mode, byte) in [
            ("\r@", InputMode::Json, 1),
            ("\r\n@", InputMode::Json, 2),
            ("\u{2028}@", InputMode::Json5, 3),
            ("\u{2029}@", InputMode::Json5, 3),
        ] {
            let (_, diagnostics) = lex_with_mode(source, mode);
            let start = diagnostics[0].span.unwrap().start;

            assert_eq!(
                start,
                Position::new(byte, 2, 1),
                "wrong position for {source:?}"
            );
        }
    }
    #[test]
    fn caps_diagnostics_for_hostile_input() {
        let source = "@".repeat(MAX_DIAGNOSTICS + 10);
        let (_, diagnostics) = lex(&source);
        assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
        assert_eq!(
            diagnostics.last().unwrap().kind,
            DiagnosticKind::TooManyErrors {
                max_errors: MAX_DIAGNOSTICS
            }
        );
    }

    #[test]
    fn caps_tokens_for_hostile_input() {
        let source = "[".repeat(MAX_TOKENS + 1);
        let (_, diagnostics) = lex(&source);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.kind
            == DiagnosticKind::TooManyTokens {
                max_tokens: MAX_TOKENS
            }));
    }

    #[test]
    fn json5_uses_its_own_whitespace_table() {
        assert!(lex_with_mode("\u{feff}{}", InputMode::Json5).1.is_empty());
        assert!(lex_with_mode("\u{0085}{}", InputMode::Json5)
            .1
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::InvalidCharacter('\u{0085}')));
    }

    #[test]
    fn json5_line_comments_end_at_unicode_line_terminators() {
        for terminator in ['\u{2028}', '\u{2029}'] {
            let source = format!("// comment{terminator}{{}}");
            let (tokens, diagnostics) = lex_with_mode(&source, InputMode::Json5);
            assert!(diagnostics.is_empty());
            assert!(tokens
                .iter()
                .any(|token| token.kind == TokenKind::LeftBrace));
        }
    }

    #[test]
    fn json5_supports_unicode_line_continuations() {
        for terminator in ['\u{2028}', '\u{2029}'] {
            let source = format!("'a\\{terminator}b'");
            let (tokens, diagnostics) = lex_with_mode(&source, InputMode::Json5);
            assert!(diagnostics.is_empty());
            assert!(tokens
                .iter()
                .any(|token| token.kind == TokenKind::String("ab".into())));
        }
    }

    #[test]
    fn repair_lexer_records_exact_json5_normalizations() {
        let cases = [
            ("{\u{00a0}\"a\":1}", "\u{00a0}", ""),
            ("{/*x*/\"a\":1}", "/*x*/", ""),
            ("{\"a\":0x10}", "0x10", "16"),
            ("{\"a\":+2}", "+2", "2"),
            ("{\"a\":.5}", ".5", "0.5"),
            (r#"{"a":"it\'s"}"#, r#"\'"#, "'"),
        ];
        for (source, before, after) in cases {
            let (_, diagnostics, records) = lex_for_repair(source, InputMode::Json5);
            assert!(diagnostics.is_empty(), "{source}");
            assert!(
                records.iter().any(|record| {
                    &source[record.byte_range()] == before && record.replacement() == after
                }),
                "missing exact record for {source}"
            );
        }
    }

    #[test]
    fn repair_lexer_records_a_string_line_continuation_as_one_range() {
        let source = "\"a\\\nb\"";
        let (_, diagnostics, records) = lex_for_repair(source, InputMode::Json5);

        assert!(diagnostics.is_empty());
        assert!(records.iter().any(|record| {
            &source[record.byte_range()] == "\\\n" && record.replacement().is_empty()
        }));
    }

    #[test]
    fn string_errors_do_not_cascade_and_point_at_the_bad_character() {
        let (_, unicode_diagnostics) = lex(r#""\u12x""#);
        assert_eq!(unicode_diagnostics.len(), 1);
        assert_eq!(
            unicode_diagnostics[0].kind,
            DiagnosticKind::InvalidUnicodeEscape
        );

        let (_, control_diagnostics) = lex("\"before\tafter\"");
        let span = control_diagnostics[0].span.unwrap();
        assert_eq!(span.start.column, 8);
        assert_eq!(span.end.column, 9);
    }
    #[test]
    fn strict_keywords_require_an_identifier_boundary() {
        let (tokens, diagnostics) = lex("truefalse");

        assert!(!diagnostics.is_empty());
        assert!(!tokens
            .iter()
            .any(|token| matches!(token.kind, TokenKind::True | TokenKind::False)));
    }
}
