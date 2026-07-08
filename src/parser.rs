use crate::ast::Value;
use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::lexer::{lex, Token, TokenKind};

pub fn parse(source: &str) -> Result<Value, Vec<Diagnostic>> {
    if source.trim().is_empty() {
        return Err(vec![Diagnostic::new(
            "E000",
            DiagnosticKind::EmptyInput,
            None,
        )]);
    }

    let (tokens, lex_errors) = lex(source);
    if !lex_errors.is_empty() {
        return Err(lex_errors);
    }

    let mut parser = Parser {
        tokens,
        index: 0,
        diagnostics: Vec::new(),
    };
    let value = parser.parse_value();

    if parser.diagnostics.is_empty() && !matches!(parser.current().kind, TokenKind::Eof) {
        let span = parser.current().span;
        parser.diagnostics.push(Diagnostic::new(
            "E008",
            DiagnosticKind::TrailingInput,
            Some(span),
        ));
    }

    if parser.diagnostics.is_empty() {
        Ok(value.unwrap_or(Value::Null))
    } else {
        Err(parser.diagnostics)
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn parse_value(&mut self) -> Option<Value> {
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
                self.advance();
                Some(Value::String(text))
            }
            TokenKind::LeftBrace => self.parse_object(),
            TokenKind::LeftBracket => self.parse_array(),
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

    fn parse_object(&mut self) -> Option<Value> {
        self.advance();
        let mut pairs = Vec::new();

        if self.consume_if(TokenDiscriminant::RightBrace) {
            return Some(Value::Object(pairs));
        }

        loop {
            let key = match &self.current().kind {
                TokenKind::String(key) => {
                    let key = key.clone();
                    self.advance();
                    key
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

            let value = self.parse_value()?;
            pairs.push((key, value));

            if self.consume_if(TokenDiscriminant::Comma) {
                continue;
            }
            if self.consume_if(TokenDiscriminant::RightBrace) {
                break;
            }
            self.expected("`,` or `}`");
            return None;
        }

        Some(Value::Object(pairs))
    }

    fn parse_array(&mut self) -> Option<Value> {
        self.advance();
        let mut values = Vec::new();

        if self.consume_if(TokenDiscriminant::RightBracket) {
            return Some(Value::Array(values));
        }

        loop {
            values.push(self.parse_value()?);

            if self.consume_if(TokenDiscriminant::Comma) {
                continue;
            }
            if self.consume_if(TokenDiscriminant::RightBracket) {
                break;
            }
            self.expected("`,` or `]`");
            return None;
        }

        Some(Value::Array(values))
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
}
