use crate::ast::Value;
use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::formatter::{format_json, FormatError};
use crate::lexer::InputMode;
use crate::parser::{parse_repair, parse_strict_repair_output, parse_with_mode};
use crate::repair_plan::{RepairDecision, RepairEvaluation, RepairPlan};
use crate::span::Position;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixEdit {
    pub byte: usize,
    pub line: usize,
    pub column: usize,
    pub code: &'static str,
    pub description: String,
}

impl FixEdit {
    pub(crate) fn at(code: &'static str, description: &str, position: Position) -> Self {
        Self {
            byte: position.byte,
            line: position.line,
            column: position.column,
            code,
            description: description.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FixResult {
    pub(crate) value: Value,
    pub output: String,
    pub edits: Vec<FixEdit>,
}

pub(crate) fn fix(source: &str, mode: InputMode) -> Result<FixResult, Vec<Diagnostic>> {
    let plan = plan(source, mode)?;
    let mut selection = plan.default_selection();
    selection.set_all(RepairDecision::Accepted);
    match plan.evaluate(&selection) {
        RepairEvaluation::Ready(candidate) => {
            let value = parse_strict_repair_output(&candidate.output)?;
            Ok(FixResult {
                value,
                output: candidate.output,
                edits: candidate.edits,
            })
        }
        RepairEvaluation::Preview(_) | RepairEvaluation::Invalid { .. } => {
            Err(invalid_plan_diagnostic())
        }
    }
}

pub(crate) fn plan(source: &str, mode: InputMode) -> Result<RepairPlan, Vec<Diagnostic>> {
    if mode == InputMode::Json {
        if let Ok(value) = parse_with_mode(source, mode) {
            let output = format_json(&value).map_err(format_diagnostic)?;
            return RepairPlan::valid(source, output);
        }
    }
    let (_value, edits, records) = parse_repair(source, mode)?;
    RepairPlan::from_records(source, edits, records)
}

fn format_diagnostic(error: FormatError) -> Vec<Diagnostic> {
    let (code, kind) = match error {
        FormatError::OutputTooLarge { max_bytes } => {
            ("E019", DiagnosticKind::OutputTooLarge { max_bytes })
        }
        FormatError::AllocationFailed => ("E020", DiagnosticKind::AllocationFailed),
    };
    vec![Diagnostic::new(code, kind, None)]
}

fn invalid_plan_diagnostic() -> Vec<Diagnostic> {
    vec![Diagnostic::new(
        "E022",
        DiagnosticKind::InvalidRepairPlan,
        None,
    )]
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Value;

    fn fix_json5(source: &str) -> Result<FixResult, Vec<Diagnostic>> {
        fix(source, InputMode::Json5)
    }

    fn assert_round_trip(result: &FixResult) -> Value {
        crate::parser::parse(&result.output).expect("repaired output must be strict JSON")
    }

    #[test]
    fn converts_single_quoted_strings() {
        let fixed = fix_json5("{'name':'Ada'}").unwrap();

        assert_round_trip(&fixed);
        assert_eq!(
            fixed
                .edits
                .iter()
                .filter(|edit| edit.code == "F001")
                .count(),
            2
        );
    }

    #[test]
    fn quotes_unquoted_keys() {
        let fixed = fix_json5("{name:\"Ada\"}").unwrap();

        assert_round_trip(&fixed);
        assert!(fixed.edits.iter().any(|edit| edit.code == "F002"));
    }

    #[test]
    fn removes_trailing_commas() {
        let fixed = fix_json5("{\"a\":1,}").unwrap();

        assert_round_trip(&fixed);
        assert!(fixed.edits.iter().any(|edit| edit.code == "F003"));
    }

    #[test]
    fn inserts_missing_commas() {
        let fixed = fix_json5("[1 2 true]").unwrap();

        assert_round_trip(&fixed);
        assert_eq!(
            fixed
                .edits
                .iter()
                .filter(|edit| edit.code == "F004")
                .count(),
            2
        );
    }

    #[test]
    fn combines_missing_comma_and_unquoted_key_repairs() {
        let fixed = fix_json5("{a:1 b:2}").unwrap();

        assert_round_trip(&fixed);
        assert_eq!(
            fixed
                .edits
                .iter()
                .filter(|edit| edit.code == "F002")
                .count(),
            2
        );
        assert!(fixed.edits.iter().any(|edit| edit.code == "F004"));
    }

    #[test]
    fn repairs_escaped_apostrophe_without_creating_invalid_json_escape() {
        let fixed = fix_json5("{message: 'it\\'s'}").unwrap();

        let parsed = assert_round_trip(&fixed);
        assert_eq!(
            parsed,
            Value::Object(vec![("message".into(), Value::String("it's".into()),)])
        );
        assert!(fixed.edits.iter().any(|edit| edit.code == "F001"));
        assert!(fixed.edits.iter().any(|edit| edit.code == "F002"));
    }

    #[test]
    fn does_not_quote_identifiers_in_array_value_position() {
        let errors = fix_json5("[a:1]").unwrap_err();

        assert!(!errors.is_empty());
    }

    #[test]
    fn repairs_missing_commas_in_nested_containers() {
        let fixed = fix_json5(r#"{"a":[1 2],"b":{"c":true "d":null}}"#).unwrap();

        assert_round_trip(&fixed);
        assert_eq!(
            fixed
                .edits
                .iter()
                .filter(|edit| edit.code == "F004")
                .count(),
            2
        );
    }

    #[test]
    fn repairs_trailing_commas_with_source_positions() {
        let fixed = fix_json5("{\n  a: [1,],\n}").unwrap();

        assert_round_trip(&fixed);
        assert!(fixed
            .edits
            .iter()
            .any(|edit| edit.code == "F003" && edit.line == 2 && edit.column > 1));
    }

    #[test]
    fn missing_colon_is_unrepairable() {
        let errors = fix_json5(r#"{"a" 1}"#).unwrap_err();

        assert!(errors.iter().any(|diagnostic| {
            matches!(
                diagnostic.kind,
                crate::diagnostic::DiagnosticKind::Expected(_)
            )
        }));
    }
    #[test]
    fn strict_fix_does_not_implicitly_accept_json5_lexical_syntax() {
        assert!(fix("{a: 0x10}", InputMode::Json).is_err());
    }

    #[test]
    fn json5_fix_audits_normalization_alongside_other_repairs() {
        let fixed = fix("{a:/* comment */0x10}", InputMode::Json5).unwrap();
        assert!(fixed.edits.iter().any(|edit| edit.code == "F002"));
        assert!(
            fixed
                .edits
                .iter()
                .filter(|edit| edit.code == "F005")
                .count()
                >= 2
        );
    }

    #[test]
    fn json5_fix_audits_non_json_whitespace() {
        let fixed = fix_json5("{\u{00a0}\"a\":1}").unwrap();

        assert!(fixed
            .edits
            .iter()
            .any(|edit| { edit.code == "F005" && edit.description.contains("whitespace") }));
    }

    #[test]
    fn json5_fix_audits_escaped_apostrophe_in_double_quoted_string() {
        let fixed = fix_json5(r#"{"a":"it\'s"}"#).unwrap();

        assert!(fixed
            .edits
            .iter()
            .any(|edit| { edit.code == "F005" && edit.description.contains("escape") }));
    }
    #[test]
    fn missing_comma_repair_rejects_adjacent_tokens_without_trivia() {
        for source in ["[truefalse]", "[1-2]"] {
            assert!(fix(source, InputMode::Json).is_err(), "{source}");
        }
        for source in ["[1.2.3]", "[1+2]", "{a:1b:2}"] {
            assert!(fix(source, InputMode::Json5).is_err(), "{source}");
        }
    }

    #[test]
    fn missing_comma_repair_accepts_tokens_separated_by_trivia() {
        for (source, mode) in [
            ("[1 2]", InputMode::Json),
            ("[1 /* comment */ 2]", InputMode::Json5),
            ("{a:1 b:2}", InputMode::Json5),
        ] {
            let fixed = fix(source, mode).unwrap();
            assert!(fixed.edits.iter().any(|edit| edit.code == "F004"));
        }
    }

    #[test]
    fn plan_exposes_structured_groups_for_every_repair_code() {
        let plan = plan("{name:'Ada' values:[1,]}", InputMode::Json5).unwrap();
        let codes: Vec<_> = plan.groups().iter().map(|group| group.code()).collect();
        assert!(codes.contains(&"F001"));
        assert!(codes.contains(&"F002"));
        assert!(codes.contains(&"F003"));
        assert!(codes.contains(&"F004"));
    }

    #[test]
    fn string_wide_repair_links_inner_normalization_to_one_decision_set() {
        let plan = plan("['a\\\nb']", InputMode::Json5).unwrap();
        assert_eq!(
            plan.groups()
                .iter()
                .filter(|group| group.code() == "F001")
                .count(),
            1
        );
        assert_eq!(
            plan.groups()
                .iter()
                .filter(|group| group.code() == "F005")
                .count(),
            1
        );
        assert_eq!(plan.decision_sets().len(), 1);
    }

    #[test]
    fn missing_comma_links_json5_gap_normalization_to_the_same_set() {
        let plan = plan("[1\u{00a0}2]", InputMode::Json5).unwrap();
        let comma = plan
            .groups()
            .iter()
            .find(|group| group.code() == "F004")
            .unwrap();
        let whitespace = plan
            .groups()
            .iter()
            .find(|group| group.code() == "F005")
            .unwrap();
        assert_eq!(comma.decision_set_id(), whitespace.decision_set_id());
    }
}
