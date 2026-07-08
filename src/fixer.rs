#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixEdit {
    pub position: usize,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixResult {
    pub output: String,
    pub edits: Vec<FixEdit>,
}

pub fn fix(source: &str) -> FixResult {
    let mut edits = Vec::new();
    let after_strings = convert_single_quoted_strings(source, &mut edits);
    let after_commas = insert_missing_commas(&after_strings, &mut edits);
    let after_keys = quote_unquoted_keys(&after_commas, &mut edits);
    let output = remove_trailing_commas(&after_keys, &mut edits);
    FixResult { output, edits }
}

fn convert_single_quoted_strings(source: &str, edits: &mut Vec<FixEdit>) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_double = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' && !is_escaped(&chars, i) {
            in_double = !in_double;
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '\'' && !in_double {
            edits.push(FixEdit {
                position: i,
                description: "converted single-quoted string".to_string(),
            });
            out.push('"');
            i += 1;
            while i < chars.len() {
                let inner = chars[i];
                if inner == '\'' && !is_escaped(&chars, i) {
                    out.push('"');
                    i += 1;
                    break;
                }
                if inner == '"' {
                    out.push_str("\\\"");
                } else {
                    out.push(inner);
                }
                i += 1;
            }
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

fn quote_unquoted_keys(source: &str, edits: &mut Vec<FixEdit>) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' && !is_escaped(&chars, i) {
            in_string = !in_string;
            out.push(ch);
            i += 1;
            continue;
        }

        if !in_string && (ch == '{' || ch == ',') {
            out.push(ch);
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                out.push(chars[i]);
                i += 1;
            }

            if i < chars.len() && is_ident_start(chars[i]) {
                let start = i;
                let mut end = i + 1;
                while end < chars.len() && is_ident_continue(chars[end]) {
                    end += 1;
                }
                let mut probe = end;
                while probe < chars.len() && chars[probe].is_whitespace() {
                    probe += 1;
                }
                if probe < chars.len() && chars[probe] == ':' {
                    edits.push(FixEdit {
                        position: start,
                        description: "quoted unquoted object key".to_string(),
                    });
                    out.push('"');
                    for key_ch in &chars[start..end] {
                        out.push(*key_ch);
                    }
                    out.push('"');
                    i = end;
                    continue;
                }
            }
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

fn remove_trailing_commas(source: &str, edits: &mut Vec<FixEdit>) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' && !is_escaped(&chars, i) {
            in_string = !in_string;
            out.push(ch);
            i += 1;
            continue;
        }

        if !in_string && ch == ',' {
            let mut probe = i + 1;
            while probe < chars.len() && chars[probe].is_whitespace() {
                probe += 1;
            }
            if probe < chars.len() && (chars[probe] == '}' || chars[probe] == ']') {
                edits.push(FixEdit {
                    position: i,
                    description: "removed trailing comma".to_string(),
                });
                i += 1;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    out
}

fn insert_missing_commas(source: &str, edits: &mut Vec<FixEdit>) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        let start = i;
        if chars[i] == '"' {
            out.push(chars[i]);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                out.push(ch);
                i += 1;
                if ch == '"' && !is_escaped(&chars, i - 1) {
                    break;
                }
            }
            maybe_insert_comma(&chars, i, start, edits, &mut out);
            continue;
        }

        if is_number_start(chars[i]) {
            out.push(chars[i]);
            i += 1;
            while i < chars.len() && is_number_continue(chars[i]) {
                out.push(chars[i]);
                i += 1;
            }
            maybe_insert_comma(&chars, i, start, edits, &mut out);
            continue;
        }

        if starts_with_word(&chars, i, "true")
            || starts_with_word(&chars, i, "false")
            || starts_with_word(&chars, i, "null")
        {
            let word_len = if starts_with_word(&chars, i, "true") {
                4
            } else if starts_with_word(&chars, i, "false") {
                5
            } else {
                4
            };
            for _ in 0..word_len {
                out.push(chars[i]);
                i += 1;
            }
            maybe_insert_comma(&chars, i, start, edits, &mut out);
            continue;
        }

        out.push(chars[i]);
        i += 1;
        if matches!(chars[start], '}' | ']') {
            maybe_insert_comma(&chars, i, start, edits, &mut out);
        }
    }

    out
}

fn maybe_insert_comma(
    chars: &[char],
    index: usize,
    position: usize,
    edits: &mut Vec<FixEdit>,
    out: &mut String,
) {
    let mut probe = index;
    while probe < chars.len() && chars[probe].is_whitespace() {
        probe += 1;
    }

    if probe >= chars.len() {
        return;
    }

    let next = chars[probe];
    if next == ':' || next == ',' || next == '}' || next == ']' {
        return;
    }

    if is_value_start(next) {
        edits.push(FixEdit {
            position,
            description: "inserted missing comma".to_string(),
        });
        out.push(',');
    }
}

fn is_escaped(chars: &[char], index: usize) -> bool {
    let mut count = 0;
    let mut i = index;
    while i > 0 {
        i -= 1;
        if chars[i] == '\\' {
            count += 1;
        } else {
            break;
        }
    }
    count % 2 == 1
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch == '-' || ch.is_ascii_alphanumeric()
}

fn is_number_start(ch: char) -> bool {
    ch == '-' || ch.is_ascii_digit()
}

fn is_number_continue(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-')
}

fn is_value_start(ch: char) -> bool {
    matches!(ch, '"' | '{' | '[' | '-' | '0'..='9' | 't' | 'f' | 'n') || is_ident_start(ch)
}

fn starts_with_word(chars: &[char], index: usize, word: &str) -> bool {
    for (offset, expected) in word.chars().enumerate() {
        if chars.get(index + offset) != Some(&expected) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_single_quoted_strings() {
        let fixed = fix("{'name':'Ada'}");

        assert_eq!(fixed.output, "{\"name\":\"Ada\"}");
    }

    #[test]
    fn quotes_unquoted_keys() {
        let fixed = fix("{name:\"Ada\"}");

        assert_eq!(fixed.output, "{\"name\":\"Ada\"}");
    }

    #[test]
    fn removes_trailing_commas() {
        let fixed = fix("{\"a\":1,}");

        assert_eq!(fixed.output, "{\"a\":1}");
    }

    #[test]
    fn inserts_missing_commas() {
        let fixed = fix("[1 2 true]");

        assert_eq!(fixed.output, "[1, 2, true]");
    }

    #[test]
    fn combines_missing_comma_and_unquoted_key_repairs() {
        let fixed = fix("{a:1 b:2}");

        assert_eq!(fixed.output, "{\"a\":1, \"b\":2}");
    }
}
