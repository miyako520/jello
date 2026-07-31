use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenClass {
    Plain,
    Punctuation,
    String,
    Number,
    Keyword,
    Identifier,
    Comment,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSpan {
    pub range: Range<usize>,
    pub class: TokenClass,
}

pub fn token_spans(source: &str) -> Vec<TokenSpan> {
    let bytes = source.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let class = match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                index += 1;
                while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
                TokenClass::Plain
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                index += 1;
                TokenClass::Punctuation
            }
            b'"' | b'\'' => {
                let quote = bytes[index];
                index += 1;
                let mut escaped = false;
                let mut terminated = false;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == quote {
                        terminated = true;
                        break;
                    }
                }
                if terminated {
                    TokenClass::String
                } else {
                    TokenClass::Invalid
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && !matches!(bytes[index], b'\r' | b'\n') {
                    index += 1;
                }
                TokenClass::Comment
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut terminated = false;
                while index + 1 < bytes.len() {
                    if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                        index += 2;
                        terminated = true;
                        break;
                    }
                    index += 1;
                }
                if !terminated {
                    index = bytes.len();
                    TokenClass::Invalid
                } else {
                    TokenClass::Comment
                }
            }
            b'-' | b'+' | b'.' | b'0'..=b'9' => {
                index += 1;
                while index < bytes.len()
                    && matches!(
                        bytes[index],
                        b'0'..=b'9'
                            | b'a'..=b'f'
                            | b'A'..=b'F'
                            | b'x'
                            | b'X'
                            | b'+'
                            | b'-'
                            | b'.'
                    )
                {
                    index += 1;
                }
                TokenClass::Number
            }
            _ => {
                let first = source[index..].chars().next().unwrap();
                index += first.len_utf8();
                while index < bytes.len() {
                    let ch = source[index..].chars().next().unwrap();
                    if !(ch == '_' || ch == '$' || ch.is_alphanumeric()) {
                        break;
                    }
                    index += ch.len_utf8();
                }
                match &source[start..index] {
                    "true" | "false" | "null" => TokenClass::Keyword,
                    word if word
                        .chars()
                        .all(|ch| ch == '_' || ch == '$' || ch.is_alphanumeric()) =>
                    {
                        TokenClass::Identifier
                    }
                    _ => TokenClass::Invalid,
                }
            }
        };
        spans.push(TokenSpan {
            range: start..index,
            class,
        });
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::{TokenClass, token_spans};

    #[test]
    fn highlighter_recognizes_json_tokens_without_changing_text() {
        let source = r#"{"name":"Ada","age":42,"ready":true,"empty":null}"#;
        let spans = token_spans(source);

        assert_eq!(spans[0].class, TokenClass::Punctuation);
        assert!(spans.iter().any(|span| span.class == TokenClass::String));
        assert!(spans.iter().any(|span| span.class == TokenClass::Number));
        assert!(spans.iter().any(|span| span.class == TokenClass::Keyword));
        assert_eq!(
            spans
                .iter()
                .map(|span| &source[span.range.clone()])
                .collect::<String>(),
            source
        );
    }

    #[test]
    fn unterminated_string_is_kept_as_a_single_visible_span() {
        let source = r#"{"name":"Ada}"#;
        let spans = token_spans(source);

        assert_eq!(spans.last().unwrap().class, TokenClass::Invalid);
        assert_eq!(
            spans
                .iter()
                .map(|span| &source[span.range.clone()])
                .collect::<String>(),
            source
        );
    }
}
