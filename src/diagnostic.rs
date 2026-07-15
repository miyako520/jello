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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            other => Err(format!(
                "unsupported color mode `{}`; expected `auto`, `always`, or `never`",
                other
            )),
        }
    }
}

pub fn color_enabled(choice: ColorChoice, is_terminal: bool, no_color: bool) -> bool {
    if no_color {
        return false;
    }
    match choice {
        ColorChoice::Auto => is_terminal,
        ColorChoice::Always => true,
        ColorChoice::Never => false,
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
    UnterminatedBlockComment,
    NonFiniteNumber,
    InputTooLarge { max_bytes: usize },
    NestingTooDeep { max_depth: usize },
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
            (DiagnosticKind::UnterminatedBlockComment, Language::En) => {
                "unterminated block comment".to_string()
            }
            (DiagnosticKind::UnterminatedBlockComment, Language::Zh) => {
                "块注释没有闭合".to_string()
            }
            (DiagnosticKind::NonFiniteNumber, Language::En) => {
                "non-finite numbers cannot be converted to standard JSON".to_string()
            }
            (DiagnosticKind::NonFiniteNumber, Language::Zh) => {
                "非有限数值无法无损转换为标准 JSON".to_string()
            }
            (DiagnosticKind::InputTooLarge { max_bytes }, Language::En) => {
                format!("input exceeds the {} byte limit", max_bytes)
            }
            (DiagnosticKind::InputTooLarge { max_bytes }, Language::Zh) => {
                format!("输入超过 {} 字节限制", max_bytes)
            }
            (DiagnosticKind::NestingTooDeep { max_depth }, Language::En) => {
                format!("JSON nesting exceeds the {} level limit", max_depth)
            }
            (DiagnosticKind::NestingTooDeep { max_depth }, Language::Zh) => {
                format!("JSON 嵌套超过 {} 层限制", max_depth)
            }
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

pub fn render_diagnostic(
    diag: &Diagnostic,
    source: &str,
    source_label: &str,
    lang: Language,
    color: bool,
) -> String {
    let error = style("error", "31", color);
    let code = style(diag.code, "33", color);
    let mut out = format!("{}[{}]: {}\n", error, code, diag.message(lang));
    let Some(span) = diag.span else {
        return out;
    };

    let line_number = span.start.line.max(1);
    let lines: Vec<&str> = source.lines().collect();
    let gutter_width = line_number.max(lines.len()).to_string().len();
    let arrow = style("-->", "34", color);
    out.push_str(&format!(
        " {} {}:{}:{}\n",
        arrow, source_label, line_number, span.start.column
    ));
    out.push_str(&format!(
        "{}\n",
        style(
            &format!("{:>width$} |", "", width = gutter_width),
            "34",
            color
        )
    ));

    let first = line_number.saturating_sub(2);
    let last = (line_number + 1).min(lines.len());
    for (index, line) in lines.iter().enumerate().take(last).skip(first) {
        let number = index + 1;
        let gutter = style(
            &format!("{:>width$} |", number, width = gutter_width),
            "34",
            color,
        );
        out.push_str(&format!("{} {}\n", gutter, line));
        if number == line_number {
            let marker_width = if span.end.line == span.start.line {
                span.end.column.saturating_sub(span.start.column).max(1)
            } else {
                line.chars()
                    .count()
                    .saturating_sub(span.start.column.saturating_sub(1))
                    .max(1)
            };
            let marker = style(
                &format!("^{}", "~".repeat(marker_width.saturating_sub(1))),
                "31",
                color,
            );
            let gutter = style(
                &format!("{:>width$} |", "", width = gutter_width),
                "34",
                color,
            );
            out.push_str(&format!(
                "{} {}{}\n",
                gutter,
                " ".repeat(span.start.column.saturating_sub(1)),
                marker
            ));
        }
    }
    out
}

fn style(text: &str, ansi_code: &str, enabled: bool) -> String {
    if enabled {
        format!("\u{1b}[{}m{}\u{1b}[0m", ansi_code, text)
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{Position, Span};

    #[test]
    fn renders_english_snippet() {
        let source = "first\nbad value\nthird";
        let span = Span::new(Position::new(10, 2, 5), Position::new(15, 2, 10));
        let diag = Diagnostic::new("E001", DiagnosticKind::InvalidCharacter('x'), Some(span));

        let rendered = render_diagnostic(&diag, source, "input.json", Language::En, false);

        assert!(rendered.starts_with("error[E001]:"));
        assert!(rendered.contains("invalid character `x`"));
        assert!(rendered.contains("--> input.json:2:5"));
        assert!(rendered.contains("1 | first"));
        assert!(rendered.contains("2 | bad value"));
        assert!(rendered.contains("3 | third"));
        assert!(rendered.contains("^~~~~"));
        assert!(!rendered.contains("\u{1b}["));
    }

    #[test]
    fn renders_chinese_message() {
        let diag = Diagnostic::new("E000", DiagnosticKind::EmptyInput, None);

        let rendered = render_diagnostic(&diag, "", "<stdin>", Language::Zh, false);

        assert!(rendered.contains("输入为空"));
    }

    #[test]
    fn renders_ansi_color_when_enabled() {
        let span = Span::new(Position::new(0, 1, 1), Position::new(1, 1, 2));
        let diag = Diagnostic::new("E001", DiagnosticKind::InvalidCharacter('x'), Some(span));

        let rendered = render_diagnostic(&diag, "x", "<stdin>", Language::En, true);

        assert!(rendered.contains("\u{1b}[31m"));
        assert!(rendered.contains("\u{1b}[34m"));
        assert!(rendered.contains("\u{1b}[0m"));
    }

    #[test]
    fn resolves_color_policy_with_no_color_precedence() {
        assert!(color_enabled(ColorChoice::Auto, true, false));
        assert!(!color_enabled(ColorChoice::Auto, false, false));
        assert!(color_enabled(ColorChoice::Always, false, false));
        assert!(!color_enabled(ColorChoice::Always, true, true));
        assert!(!color_enabled(ColorChoice::Never, true, false));
    }
}
