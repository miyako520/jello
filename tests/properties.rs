use jello::{
    diff_lines, parse, repair_json5, unified_diff, DiffOp, RepairDecision, RepairEvaluation,
    RepairOutcome,
};
use proptest::prelude::*;

fn assert_diff_round_trip(before: &str, after: &str) {
    let before_lines: Vec<_> = before.lines().collect();
    let after_lines: Vec<_> = after.lines().collect();
    let ops = diff_lines(before, after).expect("small generated inputs must be diffable");
    let mut before_index = 0;
    let mut after_index = 0;
    let mut rebuilt_before = Vec::new();
    let mut rebuilt_after = Vec::new();

    for op in ops {
        match op {
            DiffOp::Equal { before, after } => {
                assert_eq!(before, before_index);
                assert_eq!(after, after_index);
                assert_eq!(before_lines[before], after_lines[after]);
                rebuilt_before.push(before_lines[before]);
                rebuilt_after.push(after_lines[after]);
                before_index += 1;
                after_index += 1;
            }
            DiffOp::Delete { before } => {
                assert_eq!(before, before_index);
                rebuilt_before.push(before_lines[before]);
                before_index += 1;
            }
            DiffOp::Insert { after } => {
                assert_eq!(after, after_index);
                rebuilt_after.push(after_lines[after]);
                after_index += 1;
            }
        }
    }

    assert_eq!(before_index, before_lines.len());
    assert_eq!(after_index, after_lines.len());
    assert_eq!(rebuilt_before, before_lines);
    assert_eq!(rebuilt_after, after_lines);
}

prop_compose! {
    fn json5_scalar()(value in prop_oneof![
        Just("null".to_owned()),
        Just("true".to_owned()),
        Just("false".to_owned()),
        (-1000i32..1000i32).prop_map(|value| value.to_string()),
        "[A-Za-z0-9 _-]{0,12}".prop_map(|value| format!("'{value}'")),
    ]) -> String {
        value
    }
}

prop_compose! {
    fn json5_array()(values in prop::collection::vec(json5_scalar(), 0..8), trailing in any::<bool>()) -> String {
        let suffix = if trailing { "," } else { "" };
        format!("[{}{}]", values.join(","), suffix)
    }
}

prop_compose! {
    fn repairable_json5()(name in "[A-Za-z][A-Za-z0-9_-]{0,8}", value in json5_scalar(), trailing in any::<bool>()) -> String {
        let suffix = if trailing { "," } else { "" };
        format!("{{{name}:{value}{suffix}}}")
    }
}

proptest! {
    #[test]
    fn parsing_and_repairing_arbitrary_utf8_never_panics(source in any::<String>()) {
        let _ = jello::parse(&source);
        let _ = jello::parse_json5(&source);
        let _ = jello::repair(&source);
        let _ = repair_json5(&source);
        let _ = jello::plan_repair(&source);
        let _ = jello::plan_repair_json5(&source);
    }

    #[test]
    fn successful_json5_repair_always_produces_strict_json(source in prop_oneof![
        json5_scalar(),
        json5_array(),
        repairable_json5(),
    ]) {
        let outcome = repair_json5(&source);
        if let RepairOutcome::Valid(result) | RepairOutcome::Repaired(result) = outcome {
            prop_assert!(parse(&result.output).is_ok(), "output was not strict JSON: {:?}", result.output);
        }
    }

    #[test]
    fn accepting_every_repair_matches_legacy_repair(source in repairable_json5()) {
        if let Ok(plan) = jello::plan_repair_json5(&source) {
            let mut selection = plan.default_selection();
            selection.set_all(RepairDecision::Accepted);
            if let RepairEvaluation::Ready(candidate) = plan.evaluate(&selection) {
                if let RepairOutcome::Repaired(legacy) = repair_json5(&source) {
                    prop_assert_eq!(candidate.output, legacy.output);
                    prop_assert_eq!(candidate.edits, legacy.edits);
                }
            }
        }
    }

    #[test]
    fn accepted_plan_candidates_are_strict_json(source in any::<String>()) {
        if let Ok(plan) = jello::plan_repair_json5(&source) {
            let mut selection = plan.default_selection();
            selection.set_all(RepairDecision::Accepted);
            if let RepairEvaluation::Ready(candidate) = plan.evaluate(&selection) {
                prop_assert!(parse(&candidate.output).is_ok(), "output was not strict JSON: {:?}", candidate.output);
            }
        }
    }

    #[test]
    fn diff_round_trips_arbitrary_line_sequences(
        left in prop::collection::vec("[\\PC]{0,24}", 0..16),
        right in prop::collection::vec("[\\PC]{0,24}", 0..16),
    ) {
        assert_diff_round_trip(&left.join("\n"), &right.join("\n"));
    }

    #[test]
    fn unified_diff_handles_arbitrary_unicode_lines(left in "[\\PC]{0,80}", right in "[\\PC]{0,80}") {
        let _ = unified_diff(&left, &right, 3);
    }
}
