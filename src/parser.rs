use crate::ast::Value;
use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::fixer::FixEdit;
use crate::lexer::{
    is_whitespace, lex_for_repair, lex_with_mode, InputMode, Token, TokenKind, MAX_REPAIR_EDITS,
};

pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_NESTING_DEPTH: usize = 256;

pub fn parse(source: &str) -> Result<Value, Vec<Diagnostic>> {
    parse_with_mode(source, InputMode::Json)
}

pub fn parse_with_mode(source: &str, mode: InputMode) -> Result<Value, Vec<Diagnostic>> {
    parse_internal(source, mode, false).map(|(value, _)| value)
}

pub(crate) fn parse_repair(
    source: &str,
    mode: InputMode,
) -> Result<(Value, Vec<FixEdit>), Vec<Diagnostic>> {
    parse_internal(source, mode, true)
}

fn parse_internal(
    source: &str,
    mode: InputMode,
    repair: bool,
) -> Result<(Value, Vec<FixEdit>), Vec<Diagnostic>> {
    if source.len() > MAX_INPUT_BYTES {
        return Err(vec![Diagnostic::new(
            "E014",
            DiagnosticKind::InputTooLarge {
                max_bytes: MAX_INPUT_BYTES,
            },
            None,
        )]);
    }
    if is_empty_input(source, mode) {
        return Err(vec![Diagnostic::new(
            "E000",
            DiagnosticKind::EmptyInput,
            None,
        )]);
    }

    let (tokens, lex_errors, lex_edits) = if repair {
        lex_for_repair(source, mode)
    } else {
        let (tokens, diagnostics) = lex_with_mode(source, mode);
        (tokens, diagnostics, Vec::new())
    };
    if !lex_errors.is_empty() {
        return Err(lex_errors);
    }
    let edits = lex_edits
        .into_iter()
        .map(|record| FixEdit::at("F005", record.description(), record.span().start))
        .collect();

    let mut parser = Parser {
        source,
        tokens,
        index: 0,
        mode,
        repair,
        edits,
        diagnostics: Vec::new(),
    };
    let value = parser.parse_value(0);

    if parser.diagnostics.is_empty() && !matches!(parser.current().kind, TokenKind::Eof) {
        let span = parser.current().span;
        parser.diagnostics.push(Diagnostic::new(
            "E008",
            DiagnosticKind::TrailingInput,
            Some(span),
        ));
    }

    if parser.diagnostics.is_empty() {
        Ok((value.unwrap_or(Value::Null), parser.edits))
    } else {
        Err(parser.diagnostics)
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    index: usize,
    mode: InputMode,
    repair: bool,
    edits: Vec<FixEdit>,
    diagnostics: Vec<Diagnostic>,
}

impl Parser<'_> {
    fn parse_value(&mut self, depth: usize) -> Option<Value> {
        if depth >= MAX_NESTING_DEPTH
            && matches!(
                self.current().kind,
                TokenKind::LeftBrace | TokenKind::LeftBracket
            )
        {
            let span = self.current().span;
            self.diagnostics.push(Diagnostic::new(
                "E015",
                DiagnosticKind::NestingTooDeep {
                    max_depth: MAX_NESTING_DEPTH,
                },
                Some(span),
            ));
            return None;
        }
        match &self.current().kind {
            TokenKind::Null => {
                self.advance();
                Some(Value::Null)
            }
            TokenKind::True => {
                self.advance();
                Some(Value::Bool(true))
            }
            TokenKind::False => {
                self.advance();
                Some(Value::Bool(false))
            }
            TokenKind::Number(text) => {
                let text = text.clone();
                self.advance();
                Some(Value::Number(text))
            }
            TokenKind::String(text) => {
                let text = text.clone();
                let span = self.current().span;
                self.record_single_quote(span);
                self.advance();
                Some(Value::String(text))
            }
            TokenKind::LeftBrace => self.parse_object(depth),
            TokenKind::LeftBracket => self.parse_array(depth),
            other => {
                let span = self.current().span;
                self.diagnostics.push(Diagnostic::new(
                    "E007",
                    DiagnosticKind::UnexpectedToken(describe_token(other)),
                    Some(span),
                ));
                None
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Option<Value> {
        self.advance();
        let mut pairs = Vec::new();

        if self.consume_if(TokenDiscriminant::RightBrace) {
            return Some(Value::Object(pairs));
        }

        loop {
            if pairs.try_reserve(1).is_err() {
                self.allocation_failed();
                return None;
            }
            let key = match &self.current().kind {
                TokenKind::String(key) => {
                    let key = key.clone();
                    let span = self.current().span;
                    self.record_single_quote(span);
                    self.advance();
                    key
                }
                TokenKind::Identifier(key) if self.mode == InputMode::Json5 => {
                    let key = key.clone();
                    let position = self.current().span.start;
                    if self.repair {
                        self.record_edit("F002", "quoted unquoted object key", position);
                    }
                    self.advance();
                    key
                }
                TokenKind::True | TokenKind::False | TokenKind::Null
                    if self.mode == InputMode::Json5 =>
                {
                    let key = match self.current().kind {
                        TokenKind::True => "true",
                        TokenKind::False => "false",
                        TokenKind::Null => "null",
                        _ => unreachable!(),
                    };
                    let position = self.current().span.start;
                    if self.repair {
                        self.record_edit("F002", "quoted unquoted object key", position);
                    }
                    self.advance();
                    key.to_string()
                }
                _ => {
                    self.expected("object key string");
                    return None;
                }
            };

            if !self.consume_if(TokenDiscriminant::Colon) {
                self.expected("`:`");
                return None;
            }

            let value = self.parse_value(depth + 1)?;
            pairs.push((key, value));

            let comma_position = self.current().span.start;
            if self.consume_if(TokenDiscriminant::Comma) {
                if (self.mode == InputMode::Json5 || self.repair)
                    && self.consume_if(TokenDiscriminant::RightBrace)
                {
                    if self.repair {
                        self.record_edit("F003", "removed trailing comma", comma_position);
                    }
                    break;
                }
                continue;
            }
            if self.consume_if(TokenDiscriminant::RightBrace) {
                break;
            }
            if self.repair
                && self.has_trivia_before_current()
                && can_start_object_key(&self.current().kind)
            {
                let position = self.current().span.start;
                self.record_edit("F004", "inserted missing comma", position);
                continue;
            }
            self.expected("`,` or `}`");
            return None;
        }

        Some(Value::Object(pairs))
    }

    fn parse_array(&mut self, depth: usize) -> Option<Value> {
        self.advance();
        let mut values = Vec::new();

        if self.consume_if(TokenDiscriminant::RightBracket) {
            return Some(Value::Array(values));
        }

        loop {
            if values.try_reserve(1).is_err() {
                self.allocation_failed();
                return None;
            }
            values.push(self.parse_value(depth + 1)?);

            let comma_position = self.current().span.start;
            if self.consume_if(TokenDiscriminant::Comma) {
                if (self.mode == InputMode::Json5 || self.repair)
                    && self.consume_if(TokenDiscriminant::RightBracket)
                {
                    if self.repair {
                        self.record_edit("F003", "removed trailing comma", comma_position);
                    }
                    break;
                }
                continue;
            }
            if self.consume_if(TokenDiscriminant::RightBracket) {
                break;
            }
            if self.repair
                && self.has_trivia_before_current()
                && can_start_value(&self.current().kind)
            {
                let position = self.current().span.start;
                self.record_edit("F004", "inserted missing comma", position);
                continue;
            }
            self.expected("`,` or `]`");
            return None;
        }

        Some(Value::Array(values))
    }

    fn has_trivia_before_current(&self) -> bool {
        self.index
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map(|previous| previous.span.end.byte < self.current().span.start.byte)
            .unwrap_or(false)
    }

    fn allocation_failed(&mut self) {
        self.diagnostics.push(Diagnostic::new(
            "E020",
            DiagnosticKind::AllocationFailed,
            Some(self.current().span),
        ));
    }

    fn record_single_quote(&mut self, span: crate::span::Span) {
        if self.repair && self.source.as_bytes().get(span.start.byte).copied() == Some(b'\'') {
            self.record_edit("F001", "converted single-quoted string", span.start);
        }
    }

    fn record_edit(
        &mut self,
        code: &'static str,
        description: &str,
        position: crate::span::Position,
    ) {
        if self.edits.len() >= MAX_REPAIR_EDITS {
            if !self
                .diagnostics
                .iter()
                .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::TooManyRepairs { .. }))
            {
                self.diagnostics.push(Diagnostic::new(
                    "E021",
                    DiagnosticKind::TooManyRepairs {
                        max_repairs: MAX_REPAIR_EDITS,
                    },
                    Some(crate::span::Span::new(position, position)),
                ));
            }
            return;
        }
        self.edits.push(FixEdit::at(code, description, position));
    }

    fn consume_if(&mut self, expected: TokenDiscriminant) -> bool {
        if token_matches(&self.current().kind, expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expected(&mut self, expected: &str) {
        let span = self.current().span;
        self.diagnostics.push(Diagnostic::new(
            "E006",
            DiagnosticKind::Expected(expected.to_string()),
            Some(span),
        ));
    }

    fn advance(&mut self) {
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }
}

fn is_empty_input(source: &str, mode: InputMode) -> bool {
    source.is_empty() || source.chars().all(|ch| is_whitespace(ch, mode))
}

fn can_start_object_key(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::String(_)
            | TokenKind::Identifier(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
    )
}

fn can_start_value(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LeftBrace
            | TokenKind::LeftBracket
            | TokenKind::String(_)
            | TokenKind::Number(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
    )
}

#[derive(Debug, Clone, Copy)]
enum TokenDiscriminant {
    RightBrace,
    RightBracket,
    Colon,
    Comma,
}

fn token_matches(kind: &TokenKind, expected: TokenDiscriminant) -> bool {
    matches!(
        (kind, expected),
        (TokenKind::RightBrace, TokenDiscriminant::RightBrace)
            | (TokenKind::RightBracket, TokenDiscriminant::RightBracket)
            | (TokenKind::Colon, TokenDiscriminant::Colon)
            | (TokenKind::Comma, TokenDiscriminant::Comma)
    )
}

fn describe_token(kind: &TokenKind) -> String {
    match kind {
        TokenKind::LeftBrace => "`{`".to_string(),
        TokenKind::RightBrace => "`}`".to_string(),
        TokenKind::LeftBracket => "`[`".to_string(),
        TokenKind::RightBracket => "`]`".to_string(),
        TokenKind::Colon => "`:`".to_string(),
        TokenKind::Comma => "`,`".to_string(),
        TokenKind::String(_) => "string".to_string(),
        TokenKind::Identifier(_) => "identifier".to_string(),
        TokenKind::Number(_) => "number".to_string(),
        TokenKind::True => "`true`".to_string(),
        TokenKind::False => "`false`".to_string(),
        TokenKind::Null => "`null`".to_string(),
        TokenKind::Eof => "end of input".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::format_json;
    use crate::lexer::InputMode;

    #[test]
    fn parses_nested_json() {
        let parsed = parse(r#"{"name":"Ada","items":[1,false,null]}"#).unwrap();

        assert_eq!(
            parsed,
            Value::Object(vec![
                ("name".into(), Value::String("Ada".into())),
                (
                    "items".into(),
                    Value::Array(vec![
                        Value::Number("1".into()),
                        Value::Bool(false),
                        Value::Null,
                    ])
                )
            ])
        );
    }

    #[test]
    fn rejects_missing_colon() {
        let errors = parse(r#"{"name" "Ada"}"#).unwrap_err();

        assert!(matches!(errors[0].kind, DiagnosticKind::Expected(_)));
    }

    #[test]
    fn parses_json5_keys_numbers_and_trailing_commas() {
        let parsed =
            parse_with_mode("{name: 'Ada', values: [0x10, +.5, 5.,],}", InputMode::Json5).unwrap();

        assert_eq!(
            parsed,
            Value::Object(vec![
                ("name".into(), Value::String("Ada".into())),
                (
                    "values".into(),
                    Value::Array(vec![
                        Value::Number("16".into()),
                        Value::Number("0.5".into()),
                        Value::Number("5.0".into()),
                    ])
                ),
            ])
        );
    }

    #[test]
    fn json5_output_round_trips_through_strict_parser() {
        let value = parse_with_mode(
            "// config\n{message: 'hello', enabled: true,}",
            InputMode::Json5,
        )
        .unwrap();
        let formatted = format_json(&value).unwrap();

        assert_eq!(parse(&formatted).unwrap(), value);
    }

    #[test]
    fn strict_parser_rejects_json5_keys() {
        assert!(parse("{name: 'Ada'}").is_err());
    }

    #[test]
    fn rejects_input_over_size_limit() {
        let input = " ".repeat(MAX_INPUT_BYTES + 1);

        let errors = parse(&input).unwrap_err();

        assert_eq!(
            errors[0].kind,
            DiagnosticKind::InputTooLarge {
                max_bytes: MAX_INPUT_BYTES
            }
        );
    }

    #[test]
    fn rejects_nesting_over_depth_limit() {
        let depth = MAX_NESTING_DEPTH + 1;
        let input = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));

        let errors = parse(&input).unwrap_err();

        assert_eq!(
            errors[0].kind,
            DiagnosticKind::NestingTooDeep {
                max_depth: MAX_NESTING_DEPTH
            }
        );
    }

    #[test]
    fn parses_json5_keyword_object_keys() {
        let parsed = parse_with_mode("{true: 1, false: 2, null: 3}", InputMode::Json5).unwrap();

        assert_eq!(
            parsed,
            Value::Object(vec![
                ("true".into(), Value::Number("1".into())),
                ("false".into(), Value::Number("2".into())),
                ("null".into(), Value::Number("3".into())),
            ])
        );
    }
    #[test]
    fn repairs_missing_comma_before_json5_keyword_object_keys() {
        let (parsed, edits) =
            parse_repair("{a: 1 true: 2 false: 3 null: 4}", InputMode::Json5).unwrap();
        assert!(matches!(parsed, Value::Object(_)));
        assert_eq!(edits.iter().filter(|edit| edit.code == "F004").count(), 3);
    }
}
