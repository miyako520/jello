use std::fmt;

use crate::ast::Value;

pub const MAX_INDENT_WIDTH: usize = 16;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    OutputTooLarge { max_bytes: usize },
    AllocationFailed,
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputTooLarge { max_bytes } => {
                write!(
                    formatter,
                    "formatted output exceeds the {max_bytes} byte limit"
                )
            }
            Self::AllocationFailed => formatter.write_str("unable to allocate formatted output"),
        }
    }
}

impl std::error::Error for FormatError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    indent: usize,
    compact: bool,
}

impl FormatOptions {
    pub fn pretty(indent: usize) -> Result<Self, String> {
        if indent > MAX_INDENT_WIDTH {
            return Err(format!(
                "indent width must be between 0 and {}",
                MAX_INDENT_WIDTH
            ));
        }
        Ok(Self {
            indent,
            compact: false,
        })
    }

    pub const fn compact() -> Self {
        Self {
            indent: 0,
            compact: true,
        }
    }

    pub const fn indent_width(self) -> usize {
        self.indent
    }

    pub const fn is_compact(self) -> bool {
        self.compact
    }
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent: 2,
            compact: false,
        }
    }
}

pub(crate) fn format_json(value: &Value) -> Result<String, FormatError> {
    format_json_with_options(value, FormatOptions::default())
}

pub(crate) fn format_json_with_options(
    value: &Value,
    options: FormatOptions,
) -> Result<String, FormatError> {
    let mut out = LimitedOutput::new(MAX_OUTPUT_BYTES);
    write_value(value, 0, options, &mut out)?;
    Ok(out.finish())
}

struct LimitedOutput {
    value: String,
    limit: usize,
}

impl LimitedOutput {
    fn new(limit: usize) -> Self {
        Self {
            value: String::new(),
            limit,
        }
    }

    fn reserve(&mut self, additional: usize) -> Result<(), FormatError> {
        let Some(required) = self.value.len().checked_add(additional) else {
            return Err(FormatError::OutputTooLarge {
                max_bytes: self.limit,
            });
        };
        if required > self.limit {
            return Err(FormatError::OutputTooLarge {
                max_bytes: self.limit,
            });
        }
        self.value
            .try_reserve(additional)
            .map_err(|_| FormatError::AllocationFailed)
    }

    fn push_str(&mut self, value: &str) -> Result<(), FormatError> {
        self.reserve(value.len())?;
        self.value.push_str(value);
        Ok(())
    }

    fn push_char(&mut self, value: char) -> Result<(), FormatError> {
        self.reserve(value.len_utf8())?;
        self.value.push(value);
        Ok(())
    }

    fn finish(self) -> String {
        self.value
    }
}

fn write_value(
    value: &Value,
    depth: usize,
    options: FormatOptions,
    out: &mut LimitedOutput,
) -> Result<(), FormatError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(text) => out.push_str(text),
        Value::String(text) => write_string(text, out),
        Value::Array(values) => write_array(values, depth, options, out),
        Value::Object(pairs) => write_object(pairs, depth, options, out),
    }
}

fn write_array(
    values: &[Value],
    depth: usize,
    options: FormatOptions,
    out: &mut LimitedOutput,
) -> Result<(), FormatError> {
    if values.is_empty() {
        return out.push_str("[]");
    }

    out.push_char('[')?;
    if options.compact {
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                out.push_char(',')?;
            }
            write_value(value, depth + 1, options, out)?;
        }
        return out.push_char(']');
    }

    out.push_char('\n')?;
    for (index, value) in values.iter().enumerate() {
        indent(depth + 1, options.indent, out)?;
        write_value(value, depth + 1, options, out)?;
        if index + 1 != values.len() {
            out.push_char(',')?;
        }
        out.push_char('\n')?;
    }
    indent(depth, options.indent, out)?;
    out.push_char(']')
}

fn write_object(
    pairs: &[(String, Value)],
    depth: usize,
    options: FormatOptions,
    out: &mut LimitedOutput,
) -> Result<(), FormatError> {
    if pairs.is_empty() {
        return out.push_str("{}");
    }

    out.push_char('{')?;
    if options.compact {
        for (index, (key, value)) in pairs.iter().enumerate() {
            if index != 0 {
                out.push_char(',')?;
            }
            write_string(key, out)?;
            out.push_char(':')?;
            write_value(value, depth + 1, options, out)?;
        }
        return out.push_char('}');
    }

    out.push_char('\n')?;
    for (index, (key, value)) in pairs.iter().enumerate() {
        indent(depth + 1, options.indent, out)?;
        write_string(key, out)?;
        out.push_str(": ")?;
        write_value(value, depth + 1, options, out)?;
        if index + 1 != pairs.len() {
            out.push_char(',')?;
        }
        out.push_char('\n')?;
    }
    indent(depth, options.indent, out)?;
    out.push_char('}')
}

fn indent(depth: usize, width: usize, out: &mut LimitedOutput) -> Result<(), FormatError> {
    let Some(mut remaining) = depth.checked_mul(width) else {
        return Err(FormatError::OutputTooLarge {
            max_bytes: out.limit,
        });
    };
    const SPACES: &str = "                                                                ";
    while remaining != 0 {
        let chunk = remaining.min(SPACES.len());
        out.push_str(&SPACES[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

fn write_string(input: &str, out: &mut LimitedOutput) -> Result<(), FormatError> {
    out.push_char('"')?;
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\"")?,
            '\\' => out.push_str("\\\\")?,
            '\n' => out.push_str("\\n")?,
            '\r' => out.push_str("\\r")?,
            '\t' => out.push_str("\\t")?,
            '\u{0008}' => out.push_str("\\b")?,
            '\u{000C}' => out.push_str("\\f")?,
            '\u{2028}' => out.push_str("\\u2028")?,
            '\u{2029}' => out.push_str("\\u2029")?,
            value if value.is_control() => write_unicode_escape(value, out)?,
            value => out.push_char(value)?,
        }
    }
    out.push_char('"')
}

fn write_unicode_escape(value: char, out: &mut LimitedOutput) -> Result<(), FormatError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let code = value as u32;
    out.push_str("\\u")?;
    for shift in [12, 8, 4, 0] {
        out.push_char(HEX[((code >> shift) & 0x0f) as usize] as char)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn object() -> Value {
        Value::Object(vec![
            ("name".into(), Value::String("Ada".into())),
            ("ok".into(), Value::Bool(true)),
        ])
    }

    #[test]
    fn formats_objects_with_two_spaces() {
        assert_eq!(
            format_json(&object()).unwrap(),
            "{\n  \"name\": \"Ada\",\n  \"ok\": true\n}"
        );
    }

    #[test]
    fn formats_compact_objects() {
        assert_eq!(
            format_json_with_options(&object(), FormatOptions::compact()).unwrap(),
            r#"{"name":"Ada","ok":true}"#
        );
    }

    #[test]
    fn validates_indent_width() {
        assert!(FormatOptions::pretty(0).is_ok());
        assert!(FormatOptions::pretty(MAX_INDENT_WIDTH).is_ok());
        assert!(FormatOptions::pretty(MAX_INDENT_WIDTH + 1).is_err());
    }
    #[test]
    fn bounded_output_reports_the_limit_instead_of_growing() {
        let mut output = LimitedOutput::new(4);
        output.push_str("1234").unwrap();
        assert_eq!(
            output.push_char('5').unwrap_err(),
            FormatError::OutputTooLarge { max_bytes: 4 }
        );
    }

    #[test]
    fn escapes_unicode_line_and_paragraph_separators() {
        let value = Value::String("a\u{2028}b\u{2029}c".into());
        assert_eq!(format_json(&value).unwrap(), r#""a\u2028b\u2029c""#);
    }
}
