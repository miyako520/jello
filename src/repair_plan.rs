use std::ops::Range;
use std::sync::Arc;

use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::fixer::FixEdit;
use crate::formatter::{self, FormatError, MAX_OUTPUT_BYTES};
use crate::lexer::{self, InputMode, Token, TokenKind, MAX_REPAIR_EDITS};
use crate::parser::{self, MAX_INPUT_BYTES};
use crate::span::{Position, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepairGroupId(pub(crate) usize);

impl RepairGroupId {
    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepairDecisionSetId(pub(crate) usize);

impl RepairDecisionSetId {
    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairDecision {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairKind {
    SingleQuotedString,
    UnquotedObjectKey,
    TrailingComma,
    MissingComma,
    Json5Normalization,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairChange {
    span: Span,
    byte_range: Range<usize>,
    replacement: String,
}

impl RepairChange {
    pub const fn span(&self) -> Span {
        self.span
    }
    pub fn byte_range(&self) -> Range<usize> {
        self.byte_range.clone()
    }
    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairGroup {
    id: RepairGroupId,
    decision_set: RepairDecisionSetId,
    kind: RepairKind,
    code: &'static str,
    description: &'static str,
    changes: Vec<RepairChange>,
}

impl RepairGroup {
    pub const fn id(&self) -> RepairGroupId {
        self.id
    }
    pub const fn decision_set(&self) -> RepairDecisionSetId {
        self.decision_set
    }
    pub const fn decision_set_id(&self) -> RepairDecisionSetId {
        self.decision_set
    }
    pub const fn kind(&self) -> RepairKind {
        self.kind
    }
    pub const fn code(&self) -> &'static str {
        self.code
    }
    pub const fn description(&self) -> &'static str {
        self.description
    }
    pub fn changes(&self) -> &[RepairChange] {
        &self.changes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairDecisionSet {
    id: RepairDecisionSetId,
    groups: Vec<RepairGroupId>,
}

impl RepairDecisionSet {
    pub const fn id(&self) -> RepairDecisionSetId {
        self.id
    }
    pub fn groups(&self) -> &[RepairGroupId] {
        &self.groups
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairSelection {
    decisions: Vec<RepairDecision>,
}

impl RepairSelection {
    pub fn decisions(&self) -> &[RepairDecision] {
        &self.decisions
    }
    pub fn decision(&self, id: RepairDecisionSetId) -> Option<RepairDecision> {
        self.decisions.get(id.0).copied()
    }
    pub fn set(&mut self, id: RepairDecisionSetId, decision: RepairDecision) -> bool {
        let Some(slot) = self.decisions.get_mut(id.0) else {
            return false;
        };
        *slot = decision;
        true
    }
    pub fn set_all(&mut self, decision: RepairDecision) {
        for slot in &mut self.decisions {
            *slot = decision;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanPatch {
    decision_set: RepairDecisionSetId,
    byte_range: Range<usize>,
    replacement: String,
    changes: Vec<PlanPatchChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanPatchChange {
    group: RepairGroupId,
    byte_range: Range<usize>,
    deletion_only: bool,
}

#[derive(Debug, Clone)]
pub struct RepairPlan {
    source: Arc<str>,
    source_fingerprint: u64,
    valid_output: Option<String>,
    groups: Vec<RepairGroup>,
    decision_sets: Vec<RepairDecisionSet>,
    edits: Vec<FixEdit>,
    patches: Vec<PlanPatch>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairCandidate {
    pub output: String,
    pub edits: Vec<FixEdit>,
    pub highlights: Vec<CandidateHighlight>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateHighlight {
    pub group: RepairGroupId,
    pub range: Range<usize>,
    pub anchor_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RepairEvaluation {
    Preview(RepairCandidate),
    Ready(RepairCandidate),
    Invalid {
        diagnostics: Vec<Diagnostic>,
        blocking_groups: Vec<RepairGroupId>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct RecordedRepair {
    kind: RepairKind,
    code: &'static str,
    description: &'static str,
    span: Span,
    audit_position: Position,
    byte_range: Range<usize>,
    decision_scope: Range<usize>,
    replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedChange {
    group: RepairGroupId,
    range: Range<usize>,
    anchor_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedCandidate {
    source: String,
    changes: Vec<AppliedChange>,
}

impl RecordedRepair {
    pub(crate) fn replace(
        kind: RepairKind,
        code: &'static str,
        description: &'static str,
        span: Span,
        byte_range: Range<usize>,
        replacement: String,
    ) -> Self {
        Self {
            kind,
            code,
            description,
            span,
            audit_position: span.start,
            decision_scope: byte_range.clone(),
            byte_range,
            replacement,
        }
    }
    #[allow(dead_code)]
    pub(crate) fn with_decision_scope(mut self, scope: Range<usize>) -> Self {
        self.decision_scope = scope;
        self
    }

    pub(crate) fn with_audit_position(mut self, audit_position: Position) -> Self {
        self.audit_position = audit_position;
        self
    }

    #[cfg(test)]
    pub(crate) const fn span(&self) -> Span {
        self.span
    }

    pub(crate) const fn audit_position(&self) -> Position {
        self.audit_position
    }

    pub(crate) const fn description(&self) -> &'static str {
        self.description
    }

    #[cfg(test)]
    pub(crate) fn byte_range(&self) -> Range<usize> {
        self.byte_range.clone()
    }

    #[cfg(test)]
    pub(crate) fn replacement(&self) -> &str {
        &self.replacement
    }
}

impl RepairPlan {
    pub(crate) fn from_records(
        source: &str,
        edits: Vec<FixEdit>,
        mut records: Vec<RecordedRepair>,
    ) -> Result<Self, Vec<Diagnostic>> {
        if source.len() > MAX_INPUT_BYTES {
            return Err(vec![input_too_large()]);
        }
        if records.len() > MAX_REPAIR_EDITS {
            return Err(vec![too_many_repairs()]);
        }
        for record in &records {
            if !valid_range(source, &record.byte_range)
                || !valid_range(source, &record.decision_scope)
            {
                return Err(vec![invalid_plan()]);
            }
        }
        records.sort_by(|left, right| {
            left.byte_range
                .start
                .cmp(&right.byte_range.start)
                .then(left.byte_range.end.cmp(&right.byte_range.end))
        });
        let mut parents: Vec<usize> = (0..records.len()).collect();
        let mut scopes: Vec<usize> = (0..records.len()).collect();
        scopes.sort_by(|left, right| {
            records[*left]
                .decision_scope
                .start
                .cmp(&records[*right].decision_scope.start)
                .then(
                    records[*left]
                        .decision_scope
                        .end
                        .cmp(&records[*right].decision_scope.end),
                )
        });
        let mut active_non_zero: Option<usize> = None;
        let mut previous_zero: Option<usize> = None;
        for index in scopes {
            if records[index].decision_scope.is_empty() {
                if let Some(previous) = previous_zero {
                    if records[previous].decision_scope == records[index].decision_scope {
                        union(&mut parents, previous, index);
                    }
                }
                previous_zero = Some(index);
                continue;
            }
            previous_zero = None;
            if let Some(previous) = active_non_zero {
                if ranges_overlap(
                    &records[previous].decision_scope,
                    &records[index].decision_scope,
                ) {
                    union(&mut parents, previous, index);
                }
                if records[index].decision_scope.end > records[previous].decision_scope.end {
                    active_non_zero = Some(index);
                }
            } else {
                active_non_zero = Some(index);
            }
        }
        let mut root_sets = vec![None; records.len()];
        let mut group_sets = Vec::with_capacity(records.len());
        let mut decision_sets = Vec::new();
        for index in 0..records.len() {
            let root = find(&mut parents, index);
            let set = match root_sets[root] {
                Some(set) => set,
                None => {
                    let set = RepairDecisionSetId(decision_sets.len());
                    root_sets[root] = Some(set);
                    decision_sets.push(RepairDecisionSet {
                        id: set,
                        groups: Vec::new(),
                    });
                    set
                }
            };
            group_sets.push(set);
            decision_sets[set.0].groups.push(RepairGroupId(index));
        }
        let mut patches: Vec<PlanPatch> = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let group = RepairGroupId(index);
            let decision_set = group_sets[index];
            let change = PlanPatchChange {
                group,
                byte_range: record.byte_range.clone(),
                deletion_only: record.replacement.is_empty(),
            };
            let mut containing_patch = None;
            let mut remove = Vec::new();
            for (patch_index, patch) in patches.iter().enumerate() {
                if !ranges_overlap(&patch.byte_range, &record.byte_range) {
                    continue;
                }
                if contains_range(&patch.byte_range, &record.byte_range) {
                    containing_patch = Some(patch_index);
                    break;
                }
                if contains_range(&record.byte_range, &patch.byte_range) {
                    remove.push(patch_index);
                    continue;
                }
                return Err(vec![invalid_plan()]);
            }
            if let Some(patch_index) = containing_patch {
                if patches[patch_index].decision_set != decision_set {
                    return Err(vec![invalid_plan()]);
                }
                patches[patch_index].changes.push(change);
                continue;
            }
            let mut patch_changes = vec![change];
            for patch_index in remove.into_iter().rev() {
                let removed = patches.remove(patch_index);
                if removed.decision_set != decision_set {
                    return Err(vec![invalid_plan()]);
                }
                patch_changes.extend(removed.changes);
            }
            patch_changes.sort_by_key(|change| change.group.index());
            patches.push(PlanPatch {
                decision_set,
                byte_range: record.byte_range.clone(),
                replacement: record.replacement.clone(),
                changes: patch_changes,
            });
        }
        patches.sort_by(|left, right| {
            left.byte_range
                .start
                .cmp(&right.byte_range.start)
                .then(left.byte_range.end.cmp(&right.byte_range.end))
        });
        let groups = records
            .into_iter()
            .enumerate()
            .map(|(index, record)| RepairGroup {
                id: RepairGroupId(index),
                decision_set: group_sets[index],
                kind: record.kind,
                code: record.code,
                description: record.description,
                changes: vec![RepairChange {
                    span: record.span,
                    byte_range: record.byte_range,
                    replacement: record.replacement,
                }],
            })
            .collect();
        Ok(Self {
            source: Arc::from(source),
            source_fingerprint: fingerprint(source),
            valid_output: None,
            groups,
            decision_sets,
            edits,
            patches,
        })
    }

    pub(crate) fn valid(source: &str, output: String) -> Result<Self, Vec<Diagnostic>> {
        if source.len() > MAX_INPUT_BYTES {
            return Err(vec![input_too_large()]);
        }
        if output.len() > MAX_OUTPUT_BYTES {
            return Err(vec![Diagnostic::new(
                "E019",
                DiagnosticKind::OutputTooLarge {
                    max_bytes: MAX_OUTPUT_BYTES,
                },
                None,
            )]);
        }
        Ok(Self {
            source: Arc::from(source),
            source_fingerprint: fingerprint(source),
            valid_output: Some(output),
            groups: Vec::new(),
            decision_sets: Vec::new(),
            edits: Vec::new(),
            patches: Vec::new(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }
    pub const fn source_fingerprint(&self) -> u64 {
        self.source_fingerprint
    }
    pub fn groups(&self) -> &[RepairGroup] {
        &self.groups
    }
    pub fn decision_sets(&self) -> &[RepairDecisionSet] {
        &self.decision_sets
    }
    pub fn default_selection(&self) -> RepairSelection {
        RepairSelection {
            decisions: vec![RepairDecision::Pending; self.decision_sets.len()],
        }
    }

    pub fn evaluate(&self, selection: &RepairSelection) -> RepairEvaluation {
        if selection.decisions.len() != self.decision_sets.len() {
            return self.invalid(
                vec![invalid_plan()],
                self.groups.iter().map(|group| group.id).collect(),
            );
        }
        if let Some(output) = &self.valid_output {
            return RepairEvaluation::Ready(RepairCandidate {
                output: output.clone(),
                edits: Vec::new(),
                highlights: Vec::new(),
            });
        }
        let has_pending = selection
            .decisions
            .iter()
            .any(|decision| *decision == RepairDecision::Pending);
        match self.apply(selection).and_then(|applied| {
            parser::parse_strict_repair_output(&applied.source)
                .and_then(|value| formatter::format_json(&value).map_err(format_diagnostic))
                .and_then(|output| {
                    let highlights = map_highlights(&applied.source, &output, &applied.changes)?;
                    Ok((output, highlights))
                })
        }) {
            Ok((output, highlights)) => {
                let candidate = RepairCandidate {
                    output,
                    edits: self.edits.clone(),
                    highlights,
                };
                if has_pending {
                    RepairEvaluation::Preview(candidate)
                } else {
                    RepairEvaluation::Ready(candidate)
                }
            }
            Err(diagnostics) => self.invalid(diagnostics, self.blocking_groups(selection)),
        }
    }

    fn apply(&self, selection: &RepairSelection) -> Result<AppliedCandidate, Vec<Diagnostic>> {
        let mut output = String::new();
        let mut changes = Vec::new();
        let mut cursor = 0;
        for patch in &self.patches {
            if selection.decisions[patch.decision_set.0] == RepairDecision::Rejected {
                continue;
            }
            if patch.byte_range.start < cursor {
                return Err(vec![invalid_plan()]);
            }
            append_limited(&mut output, &self.source[cursor..patch.byte_range.start])?;
            let start = output.len();
            append_limited(&mut output, &patch.replacement)?;
            let end = output.len();
            changes.try_reserve(patch.changes.len()).map_err(|_| {
                vec![Diagnostic::new(
                    "E020",
                    DiagnosticKind::AllocationFailed,
                    None,
                )]
            })?;
            for change in &patch.changes {
                let (range, anchor_only) = if change.deletion_only {
                    let anchor = projected_deletion_anchor(patch, change, start..end, &output)?;
                    (anchor..anchor, true)
                } else {
                    (start..end, false)
                };
                changes.push(AppliedChange {
                    group: change.group,
                    range,
                    anchor_only,
                });
            }
            cursor = patch.byte_range.end;
        }
        append_limited(&mut output, &self.source[cursor..])?;
        changes.sort_by_key(|change| change.group.index());
        Ok(AppliedCandidate {
            source: output,
            changes,
        })
    }

    fn blocking_groups(&self, selection: &RepairSelection) -> Vec<RepairGroupId> {
        self.groups
            .iter()
            .filter(|group| selection.decisions[group.decision_set.0] == RepairDecision::Rejected)
            .map(|group| group.id)
            .collect()
    }

    fn invalid(
        &self,
        diagnostics: Vec<Diagnostic>,
        blocking_groups: Vec<RepairGroupId>,
    ) -> RepairEvaluation {
        RepairEvaluation::Invalid {
            diagnostics,
            blocking_groups,
        }
    }
}

fn projected_deletion_anchor(
    patch: &PlanPatch,
    change: &PlanPatchChange,
    candidate_range: Range<usize>,
    candidate: &str,
) -> Result<usize, Vec<Diagnostic>> {
    if !contains_range(&patch.byte_range, &change.byte_range)
        || candidate_range.end > candidate.len()
    {
        return Err(vec![invalid_plan()]);
    }
    let relative = change.byte_range.start - patch.byte_range.start;
    let candidate_len = candidate_range.end - candidate_range.start;
    let offset = if candidate_len == 0 {
        0
    } else {
        relative.min(candidate_len - 1)
    };
    let mut anchor = candidate_range.start + offset;
    while anchor > candidate_range.start && !candidate.is_char_boundary(anchor) {
        anchor -= 1;
    }
    if !candidate.is_char_boundary(anchor) {
        return Err(vec![invalid_plan()]);
    }
    Ok(anchor)
}

fn map_highlights(
    strict_candidate: &str,
    formatted_output: &str,
    changes: &[AppliedChange],
) -> Result<Vec<CandidateHighlight>, Vec<Diagnostic>> {
    let strict_tokens = aligned_tokens(strict_candidate)?;
    let formatted_tokens = aligned_tokens(formatted_output)?;
    if strict_tokens.len() != formatted_tokens.len()
        || strict_tokens
            .iter()
            .zip(&formatted_tokens)
            .any(|(strict, formatted)| strict.kind != formatted.kind)
    {
        return Err(vec![invalid_plan()]);
    }

    let mut highlights = Vec::new();
    for change in changes {
        if !valid_range(strict_candidate, &change.range) {
            return Err(vec![invalid_plan()]);
        }
        if change.anchor_only {
            let ordinal = containing_token_ordinal(&strict_tokens, change.range.start)
                .or_else(|| next_token_ordinal(&strict_tokens, change.range.start))
                .or_else(|| previous_token_ordinal(&strict_tokens, change.range.start))
                .ok_or_else(|| vec![invalid_plan()])?;
            push_highlight(
                &mut highlights,
                change.group,
                &formatted_tokens[ordinal],
                true,
            )?;
            continue;
        }

        let first = first_token_ending_after(&strict_tokens, change.range.start);
        let mut matched = false;
        for (ordinal, token) in strict_tokens.iter().enumerate().skip(first) {
            if matches!(token.kind, TokenKind::Eof) || token.span.start.byte >= change.range.end {
                break;
            }
            let token_range = token.span.start.byte..token.span.end.byte;
            if ranges_overlap(&token_range, &change.range) {
                push_highlight(
                    &mut highlights,
                    change.group,
                    &formatted_tokens[ordinal],
                    false,
                )?;
                matched = true;
            }
        }
        if !matched {
            return Err(vec![invalid_plan()]);
        }
    }
    Ok(highlights)
}

fn aligned_tokens(source: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    let (tokens, diagnostics) = lexer::lex_with_mode(source, InputMode::Json);
    if !diagnostics.is_empty() {
        return Err(vec![invalid_plan()]);
    }
    Ok(tokens)
}

fn containing_token_ordinal(tokens: &[Token], position: usize) -> Option<usize> {
    let ordinal = first_token_ending_after(tokens, position);
    tokens.get(ordinal).and_then(|token| {
        if !matches!(token.kind, TokenKind::Eof) && token.span.start.byte <= position {
            Some(ordinal)
        } else {
            None
        }
    })
}

fn next_token_ordinal(tokens: &[Token], position: usize) -> Option<usize> {
    let token_count = non_eof_token_count(tokens);
    let mut low = 0;
    let mut high = token_count;
    while low < high {
        let middle = low + (high - low) / 2;
        if tokens[middle].span.start.byte < position {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    (low < token_count).then_some(low)
}

fn previous_token_ordinal(tokens: &[Token], position: usize) -> Option<usize> {
    let token_count = non_eof_token_count(tokens);
    let mut low = 0;
    let mut high = token_count;
    while low < high {
        let middle = low + (high - low) / 2;
        if tokens[middle].span.end.byte <= position {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low.checked_sub(1)
}

fn first_token_ending_after(tokens: &[Token], position: usize) -> usize {
    let token_count = non_eof_token_count(tokens);
    let mut low = 0;
    let mut high = token_count;
    while low < high {
        let middle = low + (high - low) / 2;
        if tokens[middle].span.end.byte <= position {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}

fn non_eof_token_count(tokens: &[Token]) -> usize {
    tokens
        .last()
        .filter(|token| matches!(token.kind, TokenKind::Eof))
        .map(|_| tokens.len() - 1)
        .unwrap_or(tokens.len())
}

fn push_highlight(
    highlights: &mut Vec<CandidateHighlight>,
    group: RepairGroupId,
    token: &Token,
    anchor_only: bool,
) -> Result<(), Vec<Diagnostic>> {
    if matches!(token.kind, TokenKind::Eof) || token.span.start.byte >= token.span.end.byte {
        return Err(vec![invalid_plan()]);
    }
    highlights.try_reserve(1).map_err(|_| {
        vec![Diagnostic::new(
            "E020",
            DiagnosticKind::AllocationFailed,
            None,
        )]
    })?;
    highlights.push(CandidateHighlight {
        group,
        range: token.span.start.byte..token.span.end.byte,
        anchor_only,
    });
    Ok(())
}

fn valid_range(source: &str, range: &Range<usize>) -> bool {
    range.start <= range.end
        && range.end <= source.len()
        && source.is_char_boundary(range.start)
        && source.is_char_boundary(range.end)
}
fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}
fn contains_range(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
fn find(parents: &mut [usize], index: usize) -> usize {
    if parents[index] != index {
        parents[index] = find(parents, parents[index]);
    }
    parents[index]
}
fn union(parents: &mut [usize], left: usize, right: usize) {
    let left = find(parents, left);
    let right = find(parents, right);
    if left != right {
        parents[right] = left;
    }
}
fn fingerprint(source: &str) -> u64 {
    let mut fingerprint = 0xcbf29ce484222325_u64;
    for byte in source.as_bytes() {
        fingerprint = (fingerprint ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    fingerprint
}
fn input_too_large() -> Diagnostic {
    Diagnostic::new(
        "E014",
        DiagnosticKind::InputTooLarge {
            max_bytes: MAX_INPUT_BYTES,
        },
        None,
    )
}
fn too_many_repairs() -> Diagnostic {
    Diagnostic::new(
        "E021",
        DiagnosticKind::TooManyRepairs {
            max_repairs: MAX_REPAIR_EDITS,
        },
        None,
    )
}
fn invalid_plan() -> Diagnostic {
    Diagnostic::new("E022", DiagnosticKind::InvalidRepairPlan, None)
}
fn format_diagnostic(error: FormatError) -> Vec<Diagnostic> {
    let diagnostic = match error {
        FormatError::OutputTooLarge { max_bytes } => {
            Diagnostic::new("E019", DiagnosticKind::OutputTooLarge { max_bytes }, None)
        }
        FormatError::AllocationFailed => {
            Diagnostic::new("E020", DiagnosticKind::AllocationFailed, None)
        }
    };
    vec![diagnostic]
}
fn append_limited(output: &mut String, value: &str) -> Result<(), Vec<Diagnostic>> {
    let Some(required) = output.len().checked_add(value.len()) else {
        return Err(vec![Diagnostic::new(
            "E019",
            DiagnosticKind::OutputTooLarge {
                max_bytes: MAX_OUTPUT_BYTES,
            },
            None,
        )]);
    };
    if required > MAX_OUTPUT_BYTES {
        return Err(vec![Diagnostic::new(
            "E019",
            DiagnosticKind::OutputTooLarge {
                max_bytes: MAX_OUTPUT_BYTES,
            },
            None,
        )]);
    }
    output.try_reserve(value.len()).map_err(|_| {
        vec![Diagnostic::new(
            "E020",
            DiagnosticKind::AllocationFailed,
            None,
        )]
    })?;
    output.push_str(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{Position, Span};

    fn span(start: usize, end: usize) -> Span {
        Span::new(
            Position::new(start, 1, start + 1),
            Position::new(end, 1, end + 1),
        )
    }

    fn key_record() -> RecordedRepair {
        RecordedRepair::replace(
            RepairKind::UnquotedObjectKey,
            "F002",
            "quoted unquoted object key",
            span(1, 2),
            1..2,
            "\"a\"".to_string(),
        )
    }

    #[test]
    fn pending_repairs_are_previewed_but_not_ready_to_save() {
        let plan = RepairPlan::from_records("{a:1}", Vec::new(), vec![key_record()]).unwrap();
        let evaluation = plan.evaluate(&plan.default_selection());
        let RepairEvaluation::Preview(candidate) = evaluation else {
            panic!("pending plan must produce a preview");
        };
        assert_eq!(candidate.output, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn accepting_every_set_produces_a_ready_candidate() {
        let plan = RepairPlan::from_records("{a:1}", Vec::new(), vec![key_record()]).unwrap();
        let mut selection = plan.default_selection();
        selection.set_all(RepairDecision::Accepted);
        assert!(matches!(
            plan.evaluate(&selection),
            RepairEvaluation::Ready(_)
        ));
    }

    #[test]
    fn rejecting_a_required_repair_is_invalid() {
        let plan = RepairPlan::from_records("{a:1}", Vec::new(), vec![key_record()]).unwrap();
        let mut selection = plan.default_selection();
        selection.set_all(RepairDecision::Rejected);
        assert!(matches!(
            plan.evaluate(&selection),
            RepairEvaluation::Invalid { .. }
        ));
    }

    #[test]
    fn overlapping_repairs_share_one_decision_set() {
        let outer = RecordedRepair::replace(
            RepairKind::SingleQuotedString,
            "F001",
            "converted single-quoted string",
            span(1, 8),
            1..8,
            "\"line\"".to_string(),
        );
        let inner = RecordedRepair::replace(
            RepairKind::Json5Normalization,
            "F005",
            "removed JSON5 string line continuation",
            span(3, 5),
            3..5,
            String::new(),
        );
        let plan =
            RepairPlan::from_records("['li\\\nne']", Vec::new(), vec![outer, inner]).unwrap();
        assert_eq!(plan.groups().len(), 2);
        assert_eq!(plan.decision_sets().len(), 1);
    }

    #[test]
    fn zero_width_scope_does_not_split_an_overlapping_scope_component() {
        let outer = RecordedRepair::replace(
            RepairKind::SingleQuotedString,
            "F001",
            "outer repair",
            span(0, 1),
            0..1,
            "0".to_string(),
        )
        .with_decision_scope(0..10);
        let insertion = RecordedRepair::replace(
            RepairKind::MissingComma,
            "F004",
            "insertion repair",
            span(5, 5),
            5..5,
            ",".to_string(),
        );
        let tail = RecordedRepair::replace(
            RepairKind::Json5Normalization,
            "F005",
            "tail repair",
            span(6, 7),
            6..7,
            "6".to_string(),
        );
        let plan = RepairPlan::from_records("0123456789", Vec::new(), vec![outer, insertion, tail])
            .unwrap();
        assert_eq!(plan.decision_sets().len(), 2);
        assert_eq!(
            plan.groups()[0].decision_set(),
            plan.groups()[2].decision_set()
        );
        assert_ne!(
            plan.groups()[0].decision_set(),
            plan.groups()[1].decision_set()
        );
    }

    #[test]
    fn token_alignment_mismatch_is_a_controlled_invalid_plan_error() {
        let changes = vec![AppliedChange {
            group: RepairGroupId(0),
            range: 0..1,
            anchor_only: false,
        }];
        let diagnostics = map_highlights("1", "2", &changes).unwrap_err();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "E022");
        assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidRepairPlan);
    }
}
