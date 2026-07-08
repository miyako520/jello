use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    En,
    Zh,
}

impl Language {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "en" => Ok(Self::En),
            "zh" => Ok(Self::Zh),
            other => Err(format!(
                "unsupported language `{}`; expected `zh` or `en`",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    EmptyInput,
    InvalidArgument(String),
    Io(String),
    InvalidCharacter(char),
    UnterminatedString,
    InvalidEscape(char),
    InvalidUnicodeEscape,
    InvalidNumber,
    Expected(String),
    UnexpectedToken(String),
    TrailingInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub kind: DiagnosticKind,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn new(code: &'static str, kind: DiagnosticKind, span: Option<Span>) -> Self {
        Self { code, kind, span }
    }

    pub fn message(&self, lang: Language) -> String {
        match (&self.kind, lang) {
            (DiagnosticKind::EmptyInput, Language::En) => "input is empty".to_string(),
            (DiagnosticKind::EmptyInput, Language::Zh) => "输入为空".to_string(),
            (DiagnosticKind::InvalidArgument(msg), Language::En) => msg.clone(),
            (DiagnosticKind::InvalidArgument(msg), Language::Zh) => format!("参数错误：{}", msg),
            (DiagnosticKind::Io(msg), Language::En) => format!("I/O error: {}", msg),
            (DiagnosticKind::Io(msg), Language::Zh) => format!("I/O 错误：{}", msg),
            (DiagnosticKind::InvalidCharacter(ch), Language::En) => {
                format!("invalid character `{}`", ch)
            }
            (DiagnosticKind::InvalidCharacter(ch), Language::Zh) => {
                format!("非法字符 `{}`", ch)
            }
            (DiagnosticKind::UnterminatedString, Language::En) => {
                "unterminated string literal".to_string()
            }
            (DiagnosticKind::UnterminatedString, Language::Zh) => "字符串没有闭合".to_string(),
            (DiagnosticKind::InvalidEscape(ch), Language::En) => {
                format!("invalid escape sequence `\\{}`", ch)
            }
            (DiagnosticKind::InvalidEscape(ch), Language::Zh) => {
                format!("非法转义序列 `\\{}`", ch)
            }
            (DiagnosticKind::InvalidUnicodeEscape, Language::En) => {
                "invalid unicode escape; expected four hex digits".to_string()
            }
            (DiagnosticKind::InvalidUnicodeEscape, Language::Zh) => {
                "非法 Unicode 转义；需要四个十六进制数字".to_string()
            }
            (DiagnosticKind::InvalidNumber, Language::En) => "invalid number".to_string(),
            (DiagnosticKind::InvalidNumber, Language::Zh) => "非法数字".to_string(),
            (DiagnosticKind::Expected(expected), Language::En) => {
                format!("expected {}", expected)
            }
            (DiagnosticKind::Expected(expected), Language::Zh) => {
                format!("期望出现 {}", expected)
            }
            (DiagnosticKind::UnexpectedToken(found), Language::En) => {
                format!("unexpected token {}", found)
            }
            (DiagnosticKind::UnexpectedToken(found), Language::Zh) => {
                format!("意外的 token {}", found)
            }
            (DiagnosticKind::TrailingInput, Language::En) => {
                "unexpected input after JSON value".to_string()
            }
            (DiagnosticKind::TrailingInput, Language::Zh) => "JSON 值之后存在多余输入".to_string(),
        }
    }
}

pub fn render_diagnostic(diag: &Diagnostic, source: &str, lang: Language) -> String {
    let mut out = format!("error[{}]: {}\n", diag.code, diag.message(lang));
    let Some(span) = diag.span else {
        return out;
    };

    let line_number = span.start.line.max(1);
    let line = source.lines().nth(line_number - 1).unwrap_or("");
    out.push_str(&format!(
        " --> line {}, column {}\n",
        line_number, span.start.column
    ));
    out.push_str(&format!("{:>4} | {}\n", line_number, line));
    out.push_str("     | ");
    let spaces = span.start.column.saturating_sub(1);
    out.push_str(&" ".repeat(spaces));
    out.push('^');
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{Position, Span};

    #[test]
    fn renders_english_snippet() {
        let span = Span::new(Position::new(1, 1, 2), Position::new(2, 1, 3));
        let diag = Diagnostic::new("E001", DiagnosticKind::InvalidCharacter('x'), Some(span));

        let rendered = render_diagnostic(&diag, "{x}", Language::En);

        assert!(rendered.contains("invalid character `x`"));
        assert!(rendered.contains("^"));
    }

    #[test]
    fn renders_chinese_message() {
        let diag = Diagnostic::new("E000", DiagnosticKind::EmptyInput, None);

        let rendered = render_diagnostic(&diag, "", Language::Zh);

        assert!(rendered.contains("输入为空"));
    }
}
