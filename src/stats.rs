use crate::ast::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub key_count: usize,
    pub max_depth: usize,
    pub leaf_count: usize,
    pub original_size: usize,
    pub formatted_size: usize,
}

impl Stats {
    pub fn from_value(value: &Value, original_size: usize, formatted_size: usize) -> Self {
        let mut stats = Self {
            key_count: 0,
            max_depth: 0,
            leaf_count: 0,
            original_size,
            formatted_size,
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
        format!(
            "stats:\n  keys: {}\n  max_depth: {}\n  leaves: {}\n  size_ratio: {:.2}\n  valid_json: {}\n  fixes: {}",
            self.key_count,
            self.max_depth,
            self.leaf_count,
            self.compression_ratio(),
            was_valid,
            fix_count
        )
    }

    fn visit(&mut self, value: &Value, depth: usize) {
        self.max_depth = self.max_depth.max(depth);
        match value {
            Value::Object(pairs) => {
                if pairs.is_empty() {
                    self.leaf_count += 1;
                }
                self.key_count += pairs.len();
                for (_, value) in pairs {
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
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                self.leaf_count += 1;
            }
        }
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
}
