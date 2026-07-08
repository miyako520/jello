use crate::ast::Value;

pub fn format_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out
}

fn write_value(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(text) => out.push_str(text),
        Value::String(text) => {
            out.push('"');
            out.push_str(&escape_string(text));
            out.push('"');
        }
        Value::Array(values) => write_array(values, depth, out),
        Value::Object(pairs) => write_object(pairs, depth, out),
    }
}

fn write_array(values: &[Value], depth: usize, out: &mut String) {
    if values.is_empty() {
        out.push_str("[]");
        return;
    }

    out.push('[');
    out.push('\n');
    for (index, value) in values.iter().enumerate() {
        indent(depth + 1, out);
        write_value(value, depth + 1, out);
        if index + 1 != values.len() {
            out.push(',');
        }
        out.push('\n');
    }
    indent(depth, out);
    out.push(']');
}

fn write_object(pairs: &[(String, Value)], depth: usize, out: &mut String) {
    if pairs.is_empty() {
        out.push_str("{}");
        return;
    }

    out.push('{');
    out.push('\n');
    for (index, (key, value)) in pairs.iter().enumerate() {
        indent(depth + 1, out);
        out.push('"');
        out.push_str(&escape_string(key));
        out.push_str("\": ");
        write_value(value, depth + 1, out);
        if index + 1 != pairs.len() {
            out.push(',');
        }
        out.push('\n');
    }
    indent(depth, out);
    out.push('}');
}

fn indent(depth: usize, out: &mut String) {
    out.push_str(&"  ".repeat(depth));
}

fn escape_string(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_objects_with_two_spaces() {
        let value = Value::Object(vec![
            ("name".into(), Value::String("Ada".into())),
            ("ok".into(), Value::Bool(true)),
        ]);

        let formatted = format_json(&value);

        assert_eq!(formatted, "{\n  \"name\": \"Ada\",\n  \"ok\": true\n}");
    }
}
