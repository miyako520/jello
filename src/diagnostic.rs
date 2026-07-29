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
    InvalidArgument { en: Box<str>, zh: Box<str> },
    Io(String),
    InvalidUtf8,
    InvalidCharacter(char),
    UnescapedControlCharacter(char),
    UnterminatedString,
    InvalidEscape(char),
    InvalidUnicodeEscape,
    InvalidNumber,
    UnterminatedBlockComment,
    NonFiniteNumber,
    InputTooLarge { max_bytes: usize },
    OutputTooLarge { max_bytes: usize },
    TooManyErrors { max_errors: usize },
    TooManyTokens { max_tokens: usize },
    TooManyRepairs { max_repairs: usize },
    AllocationFailed,
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
            (DiagnosticKind::InvalidArgument { en, .. }, Language::En) => en.to_string(),
            (DiagnosticKind::InvalidArgument { zh, .. }, Language::Zh) => {
                format!("参数错误：{}", zh)
            }
            (DiagnosticKind::Io(msg), Language::En) => format!("I/O error: {}", msg),
            (DiagnosticKind::Io(msg), Language::Zh) => format!("I/O 错误：{}", msg),
            (DiagnosticKind::InvalidUtf8, Language::En) => "input is not valid UTF-8".to_string(),
            (DiagnosticKind::InvalidUtf8, Language::Zh) => "输入不是有效的 UTF-8".to_string(),
            (DiagnosticKind::InvalidCharacter(ch), Language::En) => {
                format!("invalid character `{}`", ch)
            }
            (DiagnosticKind::InvalidCharacter(ch), Language::Zh) => {
                format!("非法字符 `{}`", ch)
            }
            (DiagnosticKind::UnescapedControlCharacter(ch), Language::En) => {
                format!("unescaped control character U+{:04X}", *ch as u32)
            }
            (DiagnosticKind::UnescapedControlCharacter(ch), Language::Zh) => {
                format!("未转义的控制字符 U+{:04X}", *ch as u32)
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
            (DiagnosticKind::OutputTooLarge { max_bytes }, Language::En) => {
                format!("formatted output exceeds the {} byte limit", max_bytes)
            }
            (DiagnosticKind::OutputTooLarge { max_bytes }, Language::Zh) => {
                format!("格式化输出超过 {} 字节限制", max_bytes)
            }
            (DiagnosticKind::TooManyErrors { max_errors }, Language::En) => {
                format!("too many errors; stopped after {}", max_errors)
            }
            (DiagnosticKind::TooManyErrors { max_errors }, Language::Zh) => {
                format!("错误过多；已在 {} 个错误后停止", max_errors)
            }
            (DiagnosticKind::TooManyTokens { max_tokens }, Language::En) => {
                format!("input exceeds the {} token limit", max_tokens)
            }
            (DiagnosticKind::TooManyTokens { max_tokens }, Language::Zh) => {
                format!("输入超过 {} 个 token 的限制", max_tokens)
            }
            (DiagnosticKind::TooManyRepairs { max_repairs }, Language::En) => {
                format!("repair requires more than {} edits", max_repairs)
            }
            (DiagnosticKind::TooManyRepairs { max_repairs }, Language::Zh) => {
                format!("修复需要超过 {} 项编辑", max_repairs)
            }
            (DiagnosticKind::AllocationFailed, Language::En) => {
                "unable to allocate memory for the operation".to_string()
            }
            (DiagnosticKind::AllocationFailed, Language::Zh) => "无法为操作分配内存".to_string(),
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
    let safe_code = escape_terminal_text(diag.code);
    let code = style(&safe_code, "33", color);
    let message = escape_terminal_text(&diag.message(lang));
    let mut out = format!("{}[{}]: {}\n", error, code, message);
    let Some(span) = diag.span else {
        return out;
    };

    let line_number = span.start.line.max(1);
    let lines = source_lines(source);
    let gutter_width = line_number.max(lines.len()).to_string().len();
    let arrow = style("-->", "34", color);
    out.push_str(&format!(
        " {} {}:{}:{}\n",
        arrow,
        escape_terminal_text(source_label),
        line_number,
        span.start.column
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
        let (safe_line, column_offsets) = escape_source_line(line);
        let gutter = style(
            &format!("{:>width$} |", number, width = gutter_width),
            "34",
            color,
        );
        out.push_str(&format!("{} {}\n", gutter, safe_line));
        if number == line_number {
            let marker_start =
                rendered_column(&column_offsets, span.start.column.saturating_sub(1));
            let marker_end = if span.end.line == span.start.line {
                rendered_column(&column_offsets, span.end.column.saturating_sub(1))
            } else {
                *column_offsets.last().unwrap_or(&0)
            };
            let marker_width = marker_end.saturating_sub(marker_start).max(1);
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
                " ".repeat(marker_start),
                marker
            ));
        }
    }
    out
}

fn escape_terminal_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        push_terminal_safe_char(&mut escaped, ch);
    }
    escaped
}

fn escape_source_line(line: &str) -> (String, Vec<usize>) {
    let mut escaped = String::with_capacity(line.len());
    let mut column_offsets = Vec::with_capacity(line.chars().count() + 1);
    let mut rendered_column = 0;

    for ch in line.chars() {
        column_offsets.push(rendered_column);
        let before = escaped.len();
        push_terminal_safe_char(&mut escaped, ch);
        rendered_column += escaped[before..].chars().count();
    }
    column_offsets.push(rendered_column);
    (escaped, column_offsets)
}

fn rendered_column(column_offsets: &[usize], source_column: usize) -> usize {
    column_offsets
        .get(source_column)
        .copied()
        .unwrap_or_else(|| *column_offsets.last().unwrap_or(&0))
}

fn push_terminal_safe_char(output: &mut String, ch: char) {
    match ch {
        '\n' => output.push_str(r"\n"),
        '\r' => output.push_str(r"\r"),
        '\t' => output.push_str(r"\t"),
        ch if ch.is_control() => {
            use std::fmt::Write;
            let _ = write!(output, "\\u{{{:04X}}}", ch as u32);
        }
        ch => output.push(ch),
    }
}

fn source_lines(source: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = source.char_indices().peekable();

    while let Some((index, ch)) = chars.next() {
        if !matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        lines.push(&source[start..index]);
        start = index + ch.len_utf8();
        if ch == '\r' {
            if let Some(&(next_index, '\n')) = chars.peek() {
                chars.next();
                start = next_index + 1;
            }
        }
    }
    lines.push(&source[start..]);
    lines
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
    fn renders_invalid_utf8_in_both_languages() {
        let diag = Diagnostic::new("E015", DiagnosticKind::InvalidUtf8, None);

        assert!(diag.message(Language::En).contains("valid UTF-8"));
        assert!(diag.message(Language::Zh).contains("UTF-8"));
    }

    #[test]
    fn renders_chinese_control_character() {
        let diag = Diagnostic::new(
            "E002",
            DiagnosticKind::UnescapedControlCharacter('\n'),
            None,
        );
        assert_eq!(diag.message(Language::Zh), "未转义的控制字符 U+000A");
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
    fn escapes_terminal_controls_in_untrusted_diagnostic_text() {
        let source = "a\u{1b}\u{7}\u{9b}z";
        let span = Span::new(Position::new(1, 1, 2), Position::new(5, 1, 5));
        let diag = Diagnostic::new(
            "E\u{1b}]8;;bad\u{7}",
            DiagnosticKind::Io("bad\u{1b}]52;c;payload\u{7}\npath".to_string()),
            Some(span),
        );

        let rendered = render_diagnostic(
            &diag,
            source,
            "input\u{1b}]0;owned\u{7}\n.json",
            Language::En,
            false,
        );

        assert!(rendered.contains(r"\u{001B}"));
        assert!(rendered.contains(r"\u{0007}"));
        assert!(rendered.contains(r"\u{009B}"));
        assert!(rendered.contains(r"\n"));
        assert!(rendered.chars().all(|ch| ch == '\n' || !ch.is_control()));
    }

    #[test]
    fn caret_positions_follow_visible_control_escapes() {
        let source = "a\u{1b}z";
        let span = Span::new(Position::new(2, 1, 3), Position::new(3, 1, 4));
        let diag = Diagnostic::new("E001", DiagnosticKind::InvalidCharacter('z'), Some(span));

        let rendered = render_diagnostic(&diag, source, "<stdin>", Language::En, false);
        let source_line = rendered
            .lines()
            .find(|line| line.contains(r"a\u{001B}z"))
            .unwrap();
        let marker_line = rendered.lines().find(|line| line.contains('^')).unwrap();

        assert_eq!(source_line.find('z'), marker_line.find('^'));
    }

    #[test]
    fn color_output_only_contains_renderer_ansi_sequences() {
        let span = Span::new(Position::new(0, 1, 1), Position::new(1, 1, 2));
        let diag = Diagnostic::new(
            "E001",
            DiagnosticKind::InvalidCharacter('\u{1b}'),
            Some(span),
        );

        let rendered = render_diagnostic(
            &diag,
            "\u{1b}",
            "input\u{1b}]52;c;payload\u{7}",
            Language::En,
            true,
        );

        assert!(rendered.contains("\u{1b}[31m"));
        assert!(rendered.contains(r"\u{001B}"));
        assert!(!rendered.contains("\u{1b}]52;"));
        assert!(!rendered.contains('\u{7}'));
    }

    #[test]
    fn resolves_color_policy_with_no_color_precedence() {
        assert!(color_enabled(ColorChoice::Auto, true, false));
        assert!(!color_enabled(ColorChoice::Auto, false, false));
        assert!(color_enabled(ColorChoice::Always, false, false));
        assert!(!color_enabled(ColorChoice::Always, true, true));
        assert!(!color_enabled(ColorChoice::Never, true, false));
    }

    #[test]
    fn renders_source_context_after_lone_carriage_return() {
        let source = "first\rbad";
        let span = Span::new(Position::new(6, 2, 1), Position::new(7, 2, 2));
        let diag = Diagnostic::new("E001", DiagnosticKind::InvalidCharacter('b'), Some(span));

        let rendered = render_diagnostic(&diag, source, "input.json", Language::En, false);

        assert!(rendered.contains("1 | first"));
        assert!(rendered.contains("2 | bad"));
        assert!(rendered.contains("--> input.json:2:1"));
    }
    #[test]
    fn renders_eof_on_the_empty_line_after_a_trailing_newline() {
        let position = crate::span::Position::new(4, 2, 1);
        let diagnostic = Diagnostic::new(
            "E006",
            DiagnosticKind::Expected("value".into()),
            Some(Span::new(position, position)),
        );
        let rendered = render_diagnostic(&diagnostic, "[1,\n", "<stdin>", Language::En, false);
        assert!(rendered.contains("2 | \n"));
        assert!(rendered.contains("| ^\n"));
    }
}
