//! Line-level diff between two texts.
//!
//! The engine is a compact-trace Myers algorithm with explicit bounds:
//! inputs above [`MAX_DIFF_LINES`] per side, or edits above the internal D
//! limit, produce `None` so callers can degrade gracefully instead of
//! allocating a full edit trace.

/// Maximum number of lines per side before line diffing is skipped.
pub const MAX_DIFF_LINES: usize = 20_000;

/// Maximum edit steps (the Myers D bound) before line diffing is skipped.
const MAX_DIFF_D: usize = 2_000;

/// One row of an edit script. Line numbers are zero-based and refer to the
/// original `before`/`after` texts, so callers can fetch the text themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOp {
    Equal { before: usize, after: usize },
    Delete { before: usize },
    Insert { after: usize },
}

/// Compute a line-level diff, or `None` when the inputs are too large to diff
/// cheaply. Lines are normalized via [`str::lines`], so CRLF and LF inputs
/// compare equal.
pub fn diff_lines(before: &str, after: &str) -> Option<Vec<DiffOp>> {
    let before: Vec<&str> = before.lines().collect();
    let after: Vec<&str> = after.lines().collect();
    if before.len() > MAX_DIFF_LINES || after.len() > MAX_DIFF_LINES {
        return None;
    }
    myers(&before, &after)
}

fn myers(before: &[&str], after: &[&str]) -> Option<Vec<DiffOp>> {
    let n = before.len() as isize;
    let m = after.len() as isize;
    let max_d = (n + m).min(MAX_DIFF_D as isize);
    let mut trace: Vec<Vec<usize>> = Vec::with_capacity(max_d as usize + 1);

    // Layer d stores V[k] for k = -d, -d+2, .., d at index (k + d). V[k] is
    // the furthest x reachable on diagonal k in d edits. For the boundary
    // diagonals the only predecessor direction exists; for the rest the
    // direction with the larger x is preferred, exactly like the classic
    // algorithm, but reads always stay inside the previous layer.
    for d in 0..=max_d {
        let width = (2 * d + 1) as usize;
        let mut layer = vec![0; width];
        if d == 0 {
            let mut x = 0_isize;
            let mut y = 0_isize;
            while x < n && y < m && before[x as usize] == after[y as usize] {
                x += 1;
                y += 1;
            }
            layer[0] = x as usize;
            if x == n && y == m {
                trace.push(layer);
                return Some(backtrack(&trace, before.len(), after.len()));
            }
        } else {
            let prev = &trace[d as usize - 1];
            for i in (0..=2 * d as usize).step_by(2) {
                let k = i as isize - d;
                let x = if i == 0 {
                    prev[0]
                } else if i == 2 * d as usize {
                    prev[i - 2] + 1
                } else if prev[i - 2] < prev[i] {
                    prev[i]
                } else {
                    prev[i - 2] + 1
                };
                let mut x = x as isize;
                let mut y = x - k;
                while x < n && y >= 0 && y < m && before[x as usize] == after[y as usize] {
                    x += 1;
                    y += 1;
                }
                layer[i] = x as usize;
                if x == n && y == m {
                    trace.push(layer);
                    return Some(backtrack(&trace, before.len(), after.len()));
                }
            }
        }
        trace.push(layer);
    }
    None
}

fn backtrack(trace: &[Vec<usize>], n: usize, m: usize) -> Vec<DiffOp> {
    let mut ops = Vec::new();
    let mut x = n as isize;
    let mut y = m as isize;
    for d in (1..trace.len()).rev() {
        let prev = &trace[d - 1];
        let k = x - y;
        let i = k + d as isize;
        let from_insert = i == 0 || (i < 2 * d as isize && prev[i as usize - 2] < prev[i as usize]);
        let (prev_x, prev_k) = if from_insert {
            (prev[i as usize], k + 1)
        } else {
            (prev[i as usize - 2], k - 1)
        };
        let prev_x = prev_x as isize;
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(DiffOp::Equal {
                before: x as usize,
                after: y as usize,
            });
        }
        if from_insert {
            y -= 1;
            ops.push(DiffOp::Insert { after: y as usize });
        } else {
            x -= 1;
            ops.push(DiffOp::Delete { before: x as usize });
        }
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        ops.push(DiffOp::Equal {
            before: x as usize,
            after: y as usize,
        });
    }
    ops.reverse();
    ops
}

/// Render a unified diff with `context` lines of surrounding context per
/// hunk. Returns `None` when the inputs are too large to diff cheaply, and an
/// empty string when the texts are equal.
pub fn unified_diff(before: &str, after: &str, context: usize) -> Option<String> {
    let ops = diff_lines(before, after)?;
    if ops.iter().all(|op| matches!(op, DiffOp::Equal { .. })) {
        return Some(String::new());
    }
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let mut ranges: Vec<(usize, usize)> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, DiffOp::Equal { .. }))
        .map(|(pos, _)| {
            (
                pos.saturating_sub(context),
                (pos + context + 1).min(ops.len()),
            )
        })
        .collect();
    ranges.sort_unstable();
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match hunks.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => hunks.push((start, end)),
        }
    }

    let mut output = String::new();
    for (start, end) in hunks {
        let mut before_start = usize::MAX;
        let mut after_start = usize::MAX;
        for op in &ops[start..end] {
            match op {
                DiffOp::Equal { before, after } => {
                    before_start = before_start.min(*before);
                    after_start = after_start.min(*after);
                }
                DiffOp::Delete { before } => before_start = before_start.min(*before),
                DiffOp::Insert { after } => after_start = after_start.min(*after),
            }
        }
        let before_start = before_start.min(before_lines.len().saturating_sub(1));
        let after_start = after_start.min(after_lines.len().saturating_sub(1));
        let before_count = ops[start..end]
            .iter()
            .filter(|op| !matches!(op, DiffOp::Insert { .. }))
            .count();
        let after_count = ops[start..end]
            .iter()
            .filter(|op| !matches!(op, DiffOp::Delete { .. }))
            .count();
        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            before_start + 1,
            before_count,
            after_start + 1,
            after_count
        ));
        for op in &ops[start..end] {
            match op {
                DiffOp::Equal { before, .. } => {
                    output.push_str("  ");
                    output.push_str(before_lines[*before]);
                    output.push('\n');
                }
                DiffOp::Delete { before } => {
                    output.push_str("- ");
                    output.push_str(before_lines[*before]);
                    output.push('\n');
                }
                DiffOp::Insert { after } => {
                    output.push_str("+ ");
                    output.push_str(after_lines[*after]);
                    output.push('\n');
                }
            }
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_inputs_produce_only_equal_rows() {
        let ops = diff_lines("a\nb\nc", "a\nb\nc").unwrap();
        assert_eq!(
            ops,
            vec![
                DiffOp::Equal {
                    before: 0,
                    after: 0
                },
                DiffOp::Equal {
                    before: 1,
                    after: 1
                },
                DiffOp::Equal {
                    before: 2,
                    after: 2
                },
            ]
        );
    }

    #[test]
    fn empty_inputs_produce_an_empty_edit_script() {
        assert!(diff_lines("", "").unwrap().is_empty());
        assert_eq!(
            diff_lines("", "x").unwrap(),
            vec![DiffOp::Insert { after: 0 }]
        );
        assert_eq!(
            diff_lines("x", "").unwrap(),
            vec![DiffOp::Delete { before: 0 }]
        );
    }

    #[test]
    fn insertion_deletion_and_replacement_rows() {
        assert_eq!(
            diff_lines("a", "a\nb").unwrap(),
            vec![
                DiffOp::Equal {
                    before: 0,
                    after: 0
                },
                DiffOp::Insert { after: 1 },
            ]
        );
        assert_eq!(
            diff_lines("a\nb", "a").unwrap(),
            vec![
                DiffOp::Equal {
                    before: 0,
                    after: 0
                },
                DiffOp::Delete { before: 1 },
            ]
        );
        assert_eq!(
            diff_lines("a", "b").unwrap(),
            vec![DiffOp::Delete { before: 0 }, DiffOp::Insert { after: 0 },]
        );
    }

    #[test]
    fn separate_hunks_keep_their_shared_rows() {
        assert_eq!(
            diff_lines("a\nb\nc\nd", "a\nx\nc\ny").unwrap(),
            vec![
                DiffOp::Equal {
                    before: 0,
                    after: 0
                },
                DiffOp::Delete { before: 1 },
                DiffOp::Insert { after: 1 },
                DiffOp::Equal {
                    before: 2,
                    after: 2
                },
                DiffOp::Delete { before: 3 },
                DiffOp::Insert { after: 3 },
            ]
        );
    }

    #[test]
    fn crlf_inputs_compare_equal_to_lf_inputs() {
        let ops = diff_lines("a\r\nb\r\n", "a\nb\n").unwrap();
        assert!(ops.iter().all(|op| matches!(op, DiffOp::Equal { .. })));
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn unified_diff_is_empty_for_equal_inputs() {
        assert_eq!(unified_diff("a\nb", "a\nb", 3).unwrap(), "");
    }

    #[test]
    fn unified_diff_renders_a_single_hunk_with_context() {
        let rendered = unified_diff("a\nb\nc\nd\ne", "a\nx\nc\nd\ne", 3).unwrap();
        assert!(rendered.starts_with("@@ -1,5 +1,5 @@\n"));
        assert!(rendered.contains("  a\n- b\n+ x\n  c\n"));
    }

    #[test]
    fn unified_diff_splits_far_apart_changes_into_separate_hunks() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
        let after = "X\n2\n3\n4\n5\n6\n7\n8\n9\nY";
        let rendered = unified_diff(before, after, 1).unwrap();
        assert_eq!(rendered.matches("@@ -").count(), 2);
        assert!(rendered.contains("@@ -1,2 +1,2 @@\n"));
        assert!(rendered.contains("@@ -9,2 +9,2 @@\n"));
    }

    #[test]
    fn unified_diff_handles_append_only_changes() {
        let rendered = unified_diff("a\nb", "a\nb\nc", 1).unwrap();
        assert!(rendered.contains("+ c\n"));
    }

    #[test]
    fn inputs_above_the_line_limit_are_skipped() {
        let huge = "x\n".repeat(MAX_DIFF_LINES + 1);
        assert!(diff_lines(&huge, "y").is_none());
        assert!(diff_lines("y", &huge).is_none());
    }

    #[test]
    fn edits_above_the_d_limit_are_skipped() {
        let before: String = (0..=MAX_DIFF_D / 2)
            .map(|index| format!("before-{index}\n"))
            .collect();
        let after: String = (0..=MAX_DIFF_D / 2)
            .map(|index| format!("after-{index}\n"))
            .collect();
        assert!(diff_lines(&before, &after).is_none());
    }

    #[test]
    fn a_small_diff_within_a_large_file_stays_cheap() {
        let mut before = String::new();
        let mut after = String::new();
        for index in 0..10_000 {
            before.push_str(&format!("line-{index}\n"));
            after.push_str(&format!("line-{index}\n"));
        }
        after.push_str("inserted\n");
        let ops = diff_lines(&before, &after).unwrap();
        assert_eq!(*ops.last().unwrap(), DiffOp::Insert { after: 10_000 });
    }
}
