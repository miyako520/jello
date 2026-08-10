use jello::{parse, plan_repair_json5, MAX_NESTING_DEPTH, MAX_TOKENS};

#[test]
fn public_parser_accepts_the_nesting_limit_and_rejects_one_more_level() {
    let at_limit = format!(
        "{}0{}",
        "[".repeat(MAX_NESTING_DEPTH),
        "]".repeat(MAX_NESTING_DEPTH)
    );
    let above_limit = format!(
        "{}0{}",
        "[".repeat(MAX_NESTING_DEPTH + 1),
        "]".repeat(MAX_NESTING_DEPTH + 1)
    );

    assert!(parse(&at_limit).is_ok());
    assert!(parse(&above_limit).is_err());
}

#[test]
fn public_parser_rejects_inputs_above_the_token_limit() {
    let source = "[".repeat(MAX_TOKENS + 1);

    assert!(parse(&source).is_err());
    assert!(plan_repair_json5(&source).is_err());
}

#[test]
fn repair_changes_stay_on_utf8_boundaries_after_multibyte_text() {
    let source = "{名字:'值',}";
    let plan = plan_repair_json5(source).expect("supported JSON5 must produce a repair plan");

    for group in plan.groups() {
        for change in group.changes() {
            let range = change.byte_range();
            assert!(source.is_char_boundary(range.start));
            assert!(source.is_char_boundary(range.end));
            let _ = &source[range];
        }
    }
}
