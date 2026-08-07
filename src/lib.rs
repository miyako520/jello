//! Handwritten JSON parsing, formatting, diagnostics, and conservative repair.

mod ast;
mod cli;
mod config;
mod diagnostic;
mod diff;
#[cfg(any(feature = "windows-drop", test))]
mod drop_mode;
mod fixer;
mod formatter;
mod input;
mod lexer;
mod output;
mod parser;
mod repair_plan;
#[cfg(feature = "schema")]
mod schema;
mod span;
mod stats;

pub use config::{load_language_config, save_language_config};
pub use diagnostic::{ColorChoice, Diagnostic, DiagnosticKind, Language};
pub use diff::{diff_lines, unified_diff, DiffOp, MAX_DIFF_LINES};
pub use fixer::FixEdit;
pub use formatter::{FormatError, FormatOptions, MAX_INDENT_WIDTH, MAX_OUTPUT_BYTES};
pub use input::read_utf8_file_stable;
pub use lexer::{MAX_DIAGNOSTICS, MAX_REPAIR_EDITS, MAX_TOKENS};
pub use output::{save_as_new, save_fixed, save_updated, CleanupWarning, SavedOutput};
pub use parser::{MAX_INPUT_BYTES, MAX_NESTING_DEPTH};
pub use repair_plan::{
    CandidateHighlight, RepairCandidate, RepairChange, RepairDecision, RepairDecisionSet,
    RepairDecisionSetId, RepairEvaluation, RepairGroup, RepairGroupId, RepairKind, RepairPlan,
    RepairSelection,
};
#[cfg(feature = "schema")]
pub use schema::{SchemaIssue, SchemaValidator, MAX_SCHEMA_FILES, MAX_SCHEMA_TOTAL_BYTES};
pub use span::{Position, Span};
pub use stats::Stats;

use ast::Value;
use lexer::InputMode;

/// A parsed JSON document.
///
/// Its syntax tree is intentionally opaque so callers cannot construct values
/// that violate JSON number or string invariants.
#[derive(Debug, Clone, PartialEq)]
pub struct Document(Value);

/// The result of a successful repair attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct RepairResult {
    pub document: Document,
    pub output: String,
    pub edits: Vec<FixEdit>,
}

/// Whether repair edits were needed or the input could not be recovered safely.
#[derive(Debug, Clone, PartialEq)]
pub enum RepairOutcome {
    /// The input was valid strict JSON and needed no repair edits.
    /// The canonical formatted output may still differ from the input.
    Valid(RepairResult),
    Repaired(RepairResult),
    Unrepairable(Vec<Diagnostic>),
}

/// Parse strict RFC 8259 JSON.
///
/// Inputs larger than [`MAX_INPUT_BYTES`] or nested beyond
/// [`MAX_NESTING_DEPTH`] are rejected.
pub fn parse(source: &str) -> Result<Document, Vec<Diagnostic>> {
    parser::parse(source).map(Document)
}

/// Parse the documented, finite-number JSON5 subset.
///
/// The accepted subset includes comments, quoted and unquoted keys, trailing
/// commas, hexadecimal numbers, leading plus signs, and string continuations.
pub fn parse_json5(source: &str) -> Result<Document, Vec<Diagnostic>> {
    parser::parse_with_mode(source, InputMode::Json5).map(Document)
}

/// Format a parsed document using validated options.
pub fn format(document: &Document, options: FormatOptions) -> Result<String, FormatError> {
    formatter::format_json_with_options(&document.0, options)
}

/// Conservatively repair common structural JSON mistakes.
///
/// Successful output is always parsed again by the repair path as strict JSON
/// before it is returned.
pub fn repair(source: &str) -> RepairOutcome {
    repair_with_mode(source, InputMode::Json)
}

/// Repair structural mistakes and normalize the supported JSON5 subset.
pub fn repair_json5(source: &str) -> RepairOutcome {
    repair_with_mode(source, InputMode::Json5)
}

/// Plan conservative repairs for strict RFC 8259 JSON input.
pub fn plan_repair(source: &str) -> Result<RepairPlan, Vec<Diagnostic>> {
    fixer::plan(source, InputMode::Json)
}

/// Plan repairs and normalizations for the supported JSON5 subset.
pub fn plan_repair_json5(source: &str) -> Result<RepairPlan, Vec<Diagnostic>> {
    fixer::plan(source, InputMode::Json5)
}

fn repair_with_mode(source: &str, mode: InputMode) -> RepairOutcome {
    match fixer::fix(source, mode) {
        Ok(result) => {
            let public = RepairResult {
                document: Document(result.value),
                output: result.output,
                edits: result.edits,
            };
            if public.edits.is_empty() {
                RepairOutcome::Valid(public)
            } else {
                RepairOutcome::Repaired(public)
            }
        }
        Err(diagnostics) => RepairOutcome::Unrepairable(diagnostics),
    }
}

/// Calculate structural statistics for a parsed document.
pub fn statistics(document: &Document, original_size: usize, formatted_size: usize) -> Stats {
    Stats::from_value(&document.0, original_size, formatted_size)
}

/// Run the command-line interface.
///
/// This entry point is public only so the package's binary target can call it.
#[doc(hidden)]
pub fn run_cli() -> i32 {
    cli::run()
}

/// Run the Windows drag-and-drop helper.
///
/// This entry point is public only so the optional binary target can call it.
#[cfg(feature = "windows-drop")]
#[doc(hidden)]
pub fn run_drop_cli() -> i32 {
    drop_mode::run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_api_formats_with_default_and_custom_indentation() {
        let document = parse(r#"{"a":[1,true]}"#).unwrap();

        assert_eq!(
            format(&document, FormatOptions::default()).unwrap(),
            "{\n  \"a\": [\n    1,\n    true\n  ]\n}"
        );
        assert_eq!(
            format(&document, FormatOptions::pretty(4).unwrap()).unwrap(),
            "{\n    \"a\": [\n        1,\n        true\n    ]\n}"
        );
        assert!(FormatOptions::pretty(17).is_err());
    }

    #[test]
    fn public_api_formats_compact_json() {
        let document = parse(r#"{"a":[1,true]}"#).unwrap();

        assert_eq!(
            format(&document, FormatOptions::compact()).unwrap(),
            r#"{"a":[1,true]}"#
        );
    }

    #[test]
    fn public_api_converts_supported_json5_to_strict_json() {
        let document = parse_json5("{message: 'hello', values: [0x10,],}").unwrap();
        let output = format(&document, FormatOptions::compact()).unwrap();

        assert_eq!(output, r#"{"message":"hello","values":[16]}"#);
        assert!(parse(&output).is_ok());
    }

    #[test]
    fn public_api_reports_valid_input_without_claiming_unchanged_output() {
        let source = r#"{"a":1}"#;
        let RepairOutcome::Valid(result) = repair(source) else {
            panic!("valid strict JSON must not require repairs");
        };
        assert!(result.edits.is_empty());
        assert_ne!(result.output, source);

        assert!(matches!(repair("{a:1}"), RepairOutcome::Unrepairable(_)));
        assert!(matches!(repair_json5("{a:1}"), RepairOutcome::Repaired(_)));
        assert!(matches!(
            repair(r#"{"a" 1}"#),
            RepairOutcome::Unrepairable(_)
        ));
    }

    #[test]
    fn accepting_a_public_plan_matches_legacy_repair_json5() {
        let source = "{name:'Ada', values:[0x10,]}";
        let plan = plan_repair_json5(source).unwrap();
        let mut selection = plan.default_selection();
        selection.set_all(RepairDecision::Accepted);
        let RepairEvaluation::Ready(candidate) = plan.evaluate(&selection) else {
            panic!("accept-all plan must be ready");
        };
        let RepairOutcome::Repaired(legacy) = repair_json5(source) else {
            panic!("legacy repair must succeed");
        };
        assert_eq!(candidate.output, legacy.output);
        assert_eq!(candidate.edits, legacy.edits);
    }

    #[test]
    fn public_strict_plan_does_not_accept_json5_lexical_syntax() {
        assert!(plan_repair("{name:1}").is_err());
        assert!(plan_repair_json5("{name:1}").is_ok());
    }

    #[test]
    fn candidate_highlights_only_repair_tokens_not_pretty_print_whitespace() {
        let plan = plan_repair_json5("{name:'Ada'}").unwrap();
        let RepairEvaluation::Preview(candidate) = plan.evaluate(&plan.default_selection()) else {
            panic!("new plan must start pending");
        };
        assert!(!candidate.highlights.is_empty());
        for highlight in &candidate.highlights {
            let text = &candidate.output[highlight.range.clone()];
            assert!(!text.chars().all(char::is_whitespace));
        }
    }

    #[test]
    fn deletion_highlight_anchors_to_the_next_surviving_token() {
        let plan = plan_repair_json5("[1,]").unwrap();
        let RepairEvaluation::Preview(candidate) = plan.evaluate(&plan.default_selection()) else {
            panic!("new plan must start pending");
        };
        let highlight = candidate
            .highlights
            .iter()
            .find(|highlight| {
                plan.groups()[highlight.group.index()].kind() == RepairKind::TrailingComma
            })
            .expect("trailing-comma repair must be highlighted");
        assert!(highlight.anchor_only);
        assert_eq!(&candidate.output[highlight.range.clone()], "]");
    }

    #[test]
    fn eof_deletion_highlight_anchors_to_the_previous_surviving_token() {
        let plan = plan_repair_json5("[]// comment").unwrap();
        let RepairEvaluation::Preview(candidate) = plan.evaluate(&plan.default_selection()) else {
            panic!("new plan must start pending");
        };
        let highlight = candidate
            .highlights
            .first()
            .expect("EOF comment repair must be highlighted");
        assert!(highlight.anchor_only);
        assert_eq!(&candidate.output[highlight.range.clone()], "]");
    }

    #[test]
    fn deletion_inside_a_surviving_token_anchors_to_that_token() {
        let plan = plan_repair_json5("[\"a\\\nb\"]").unwrap();
        let RepairEvaluation::Preview(candidate) = plan.evaluate(&plan.default_selection()) else {
            panic!("new plan must start pending");
        };
        let highlight = candidate
            .highlights
            .first()
            .expect("string continuation repair must be highlighted");
        assert!(highlight.anchor_only);
        assert_eq!(&candidate.output[highlight.range.clone()], "\"ab\"");
    }

    #[test]
    fn folded_repairs_keep_independent_highlight_anchor_metadata() {
        let plan = plan_repair_json5("['li\\\nne']").unwrap();
        let RepairEvaluation::Preview(candidate) = plan.evaluate(&plan.default_selection()) else {
            panic!("new plan must start pending");
        };
        let highlight_for = |kind| {
            candidate
                .highlights
                .iter()
                .find(|highlight| plan.groups()[highlight.group.index()].kind() == kind)
                .expect("repair group must be highlighted")
        };

        let outer = highlight_for(RepairKind::SingleQuotedString);
        assert!(!outer.anchor_only);
        assert_eq!(&candidate.output[outer.range.clone()], "\"line\"");

        let folded = highlight_for(RepairKind::Json5Normalization);
        assert!(folded.anchor_only);
        assert_eq!(&candidate.output[folded.range.clone()], "\"line\"");
    }

    #[test]
    fn public_repair_reparses_formatted_output_larger_than_the_input_limit() {
        let mut source = "[".repeat(255);
        source.push_str(&"0,".repeat(39_999));
        source.push('0');
        source.push_str(&"]".repeat(255));
        assert!(source.len() < MAX_INPUT_BYTES);

        let outcome = repair(&source);
        let RepairOutcome::Valid(result) = outcome else {
            panic!("valid bounded source must remain repairable: {outcome:?}");
        };
        assert!(result.output.len() > MAX_INPUT_BYTES);
        assert!(result.output.len() <= MAX_OUTPUT_BYTES);
    }
}
