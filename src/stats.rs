use crate::ast::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub key_count: usize,
    pub max_depth: usize,
    pub leaf_count: usize,
    pub original_size: usize,
    pub formatted_size: usize,
    pub string_lengths: [usize; 6],
}

impl Stats {
    pub(crate) fn from_value(value: &Value, original_size: usize, formatted_size: usize) -> Self {
        let mut stats = Self {
            key_count: 0,
            max_depth: 0,
            leaf_count: 0,
            original_size,
            formatted_size,
            string_lengths: [0; 6],
        };
        stats.visit(value, 1);
        stats
    }

    pub fn compression_ratio(&self) -> f64 {
        if self.formatted_size == 0 {
            0.0
        } else {
            self.original_size as f64 / self.formatted_size as f64
        }
    }

    pub fn render(&self, was_valid: bool, fix_count: usize) -> String {
        let mut output = format!(
            "stats:\n  keys: {}\n  max_depth: {}\n  leaves: {}\n  size_ratio: {:.2}\n  valid_json: {}\n  fixes: {}",
            self.key_count,
            self.max_depth,
            self.leaf_count,
            self.compression_ratio(),
            was_valid,
            fix_count
        );
        output.push_str("\n  string_lengths:");
        let largest = self.string_lengths.iter().copied().max().unwrap_or(0);
        for (label, count) in ["0", "1-4", "5-8", "9-16", "17-32", "33+"]
            .iter()
            .zip(self.string_lengths)
        {
            let width = if count == 0 || largest == 0 {
                0
            } else {
                ((count as u128 * 24).div_ceil(largest as u128)) as usize
            };
            output.push_str(&format!(
                "\n  {:<4} | {:<24} {}",
                label,
                "#".repeat(width),
                count
            ));
        }
        output
    }

    fn visit(&mut self, value: &Value, depth: usize) {
        self.max_depth = self.max_depth.max(depth);
        match value {
            Value::Object(pairs) => {
                if pairs.is_empty() {
                    self.leaf_count += 1;
                }
                self.key_count += pairs.len();
                for (key, value) in pairs {
                    self.record_string_length(key);
                    self.visit(value, depth + 1);
                }
            }
            Value::Array(values) => {
                if values.is_empty() {
                    self.leaf_count += 1;
                }
                for value in values {
                    self.visit(value, depth + 1);
                }
            }
            Value::String(text) => {
                self.record_string_length(text);
                self.leaf_count += 1;
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                self.leaf_count += 1;
            }
        }
    }

    fn record_string_length(&mut self, text: &str) {
        let length = text.chars().count();
        let bucket = match length {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            9..=16 => 3,
            17..=32 => 4,
            _ => 5,
        };
        self.string_lengths[bucket] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_keys_depth_and_leaves() {
        let value = Value::Object(vec![
            ("a".into(), Value::Number("1".into())),
            (
                "b".into(),
                Value::Array(vec![Value::Bool(true), Value::Null]),
            ),
        ]);

        let stats = Stats::from_value(&value, 10, 20);

        assert_eq!(stats.key_count, 2);
        assert_eq!(stats.max_depth, 3);
        assert_eq!(stats.leaf_count, 3);
        assert_eq!(stats.compression_ratio(), 0.5);
    }

    #[test]
    fn buckets_key_and_value_lengths_by_unicode_characters() {
        let value = Value::Object(vec![
            ("".into(), Value::String("中文".into())),
            ("a".into(), Value::String("123456789".into())),
            ("abcde".into(), Value::Null),
            ("12345678901234567".into(), Value::Bool(true)),
            ("tail".into(), Value::String("x".repeat(33))),
        ]);

        let stats = Stats::from_value(&value, 1, 1);

        assert_eq!(stats.string_lengths, [1, 3, 1, 1, 1, 1]);
    }

    #[test]
    fn renders_scaled_string_length_histogram() {
        let value = Value::Object(vec![
            ("a".into(), Value::String("b".into())),
            ("five5".into(), Value::String("123456789".into())),
        ]);
        let rendered = Stats::from_value(&value, 1, 1).render(true, 0);

        assert!(rendered.contains("string_lengths:"));
        assert!(rendered.contains("1-4  | ######################## 2"));
        assert!(rendered.contains("5-8  | ############             1"));
        assert!(rendered.contains("33+  |                          0"));
    }
    #[test]
    fn rendering_public_stats_cannot_overflow() {
        let stats = Stats {
            key_count: 0,
            max_depth: 0,
            leaf_count: 0,
            original_size: 0,
            formatted_size: 0,
            string_lengths: [usize::MAX, 0, 0, 0, 0, 0],
        };
        assert!(stats.render(true, 0).contains(&"#".repeat(24)));
    }
}
