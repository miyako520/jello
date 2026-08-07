use std::path::PathBuf;
use std::time::{Duration, Instant};

use jello::{Diagnostic, RepairDecision, RepairDecisionSetId, RepairEvaluation, RepairGroupId};

use crate::i18n::UiLanguage;
use crate::review::ReviewState;
use crate::schema_engine::SchemaState;
use crate::worker::{AnalysisRequest, AnalysisResult};

const ANALYSIS_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct AppModel {
    pub language: UiLanguage,
    pub source_path: Option<PathBuf>,
    pub source_snapshot: Option<String>,
    pub saved_path: Option<PathBuf>,
    pub saved_snapshot: Option<Vec<u8>>,
    pub source: String,
    pub preview: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub review: Option<ReviewState>,
    pub schema_path: Option<PathBuf>,
    pub schema_state: SchemaState,
    pub status: Option<String>,
    last_edit: Option<Instant>,
    analysis_pending: bool,
    reevaluation_queued: bool,
    analysis_generation: u64,
    applied_generation: u64,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            language: UiLanguage::default(),
            source_path: None,
            source_snapshot: None,
            saved_path: None,
            saved_snapshot: None,
            source: String::new(),
            preview: None,
            diagnostics: Vec::new(),
            review: None,
            schema_path: None,
            schema_state: SchemaState::NotLoaded,
            status: None,
            last_edit: None,
            analysis_pending: false,
            reevaluation_queued: false,
            analysis_generation: 0,
            applied_generation: 0,
        }
    }
}

impl AppModel {
    pub fn mark_edited(&mut self, now: Instant) {
        self.analysis_generation = self.analysis_generation.saturating_add(1);
        self.last_edit = Some(now);
        self.analysis_pending = true;
        self.reevaluation_queued = false;
        self.preview = None;
        self.diagnostics.clear();
        self.review = None;
        self.schema_state = SchemaState::NotLoaded;
        self.status = None;
    }

    pub fn request_analysis_now(&mut self) {
        self.analysis_generation = self.analysis_generation.saturating_add(1);
        self.last_edit = None;
        self.analysis_pending = true;
        self.reevaluation_queued = false;
    }

    pub fn analysis_generation(&self) -> u64 {
        self.analysis_generation
    }

    pub fn analysis_in_flight(&self) -> bool {
        self.analysis_pending
            || self.applied_generation < self.analysis_generation
            || self
                .review
                .as_ref()
                .is_some_and(ReviewState::evaluation_pending)
    }

    pub fn can_save(&self) -> bool {
        self.source_path.is_some()
            && self.preview.is_some()
            && (self.schema_path.is_none() || matches!(self.schema_state, SchemaState::Valid))
            && self.review.as_ref().is_none_or(ReviewState::can_save)
    }

    pub fn verify_save_target(&self) -> std::io::Result<()> {
        if let Some(path) = &self.saved_path {
            let Some(expected) = self.saved_snapshot.as_deref() else {
                return Err(std::io::Error::other(
                    "saved content is unavailable; reopen the file",
                ));
            };
            if std::fs::read(path)? != expected {
                return Err(std::io::Error::other(
                    "saved file changed after it was written; reopen it before saving",
                ));
            }
            return Ok(());
        }
        self.verify_source_unchanged()
    }

    pub fn verify_source_unchanged(&self) -> std::io::Result<()> {
        let Some(path) = self.source_path.as_deref() else {
            return Ok(());
        };
        let Some(snapshot) = self.source_snapshot.as_deref() else {
            return Err(std::io::Error::other(
                "source snapshot is unavailable; reopen the file",
            ));
        };
        let current = jello::read_utf8_file_stable(path)?;
        if current != snapshot {
            return Err(std::io::Error::other(
                "source file changed after it was opened; reopen it before saving",
            ));
        }
        Ok(())
    }

    pub fn take_analysis_request(&mut self, now: Instant) -> Option<AnalysisRequest> {
        if self.reevaluation_queued {
            self.reevaluation_queued = false;
            if let Some(review) = &self.review {
                return Some(AnalysisRequest::Evaluate {
                    generation: self.analysis_generation,
                    selection_version: review.selection_version(),
                    plan: review.plan().clone(),
                    selection: review.selection().clone(),
                    schema_path: self.schema_path.clone(),
                });
            }
        }
        if !self.analysis_pending {
            return None;
        }
        if let Some(last_edit) = self.last_edit
            && now.saturating_duration_since(last_edit) < ANALYSIS_DEBOUNCE
        {
            return None;
        }
        self.analysis_pending = false;
        self.last_edit = None;
        Some(AnalysisRequest::Analyze {
            generation: self.analysis_generation,
            source: self.source.clone(),
            schema_path: self.schema_path.clone(),
        })
    }

    pub fn apply_result(&mut self, result: AnalysisResult) -> bool {
        if result.generation != self.analysis_generation {
            return false;
        }
        let AnalysisResult {
            generation,
            selection_version,
            plan,
            evaluation,
            diagnostics,
            schema_state,
        } = result;
        match (plan, evaluation) {
            (Some(plan), Some(evaluation)) if selection_version == 0 => {
                if self
                    .review
                    .as_ref()
                    .is_some_and(|review| selection_version != review.selection_version())
                {
                    return false;
                }
                self.applied_generation = generation;
                self.preview = evaluation_preview(&evaluation);
                let selection = plan.default_selection();
                let merged_diagnostics = merge_invalid_diagnostics(diagnostics, &evaluation);
                self.review = Some(ReviewState::new(plan, selection, evaluation));
                self.diagnostics = merged_diagnostics;
                self.schema_state = schema_state;
                self.reevaluation_queued = false;
                true
            }
            (None, Some(evaluation)) => {
                let Some(review) = self.review.as_mut() else {
                    return false;
                };
                if selection_version != review.selection_version() {
                    return false;
                }
                self.applied_generation = generation;
                self.preview = evaluation_preview(&evaluation);
                self.diagnostics = merge_invalid_diagnostics(diagnostics, &evaluation);
                review.replace_evaluation(evaluation);
                self.schema_state = schema_state;
                self.reevaluation_queued = false;
                true
            }
            (None, None) if selection_version == 0 => {
                self.applied_generation = generation;
                self.preview = None;
                self.review = None;
                self.diagnostics = diagnostics;
                self.schema_state = schema_state;
                self.reevaluation_queued = false;
                true
            }
            _ => false,
        }
    }

    pub fn decide_repair(&mut self, set_id: RepairDecisionSetId, decision: RepairDecision) -> bool {
        let Some(review) = self.review.as_mut() else {
            return false;
        };
        if !review.decide(set_id, decision) {
            return false;
        }
        self.reevaluation_queued = true;
        true
    }

    pub fn set_all_repairs(&mut self, decision: RepairDecision) {
        if let Some(review) = self.review.as_mut() {
            review.set_all(decision);
            self.reevaluation_queued = true;
        }
    }

    pub fn select_repair(&mut self, group_id: Option<RepairGroupId>) {
        if let Some(review) = self.review.as_mut() {
            review.set_selected_group(group_id);
        }
    }

    pub fn open_source(&mut self, path: PathBuf, source: String) {
        self.source_path = Some(path);
        self.source_snapshot = Some(source.clone());
        self.saved_path = None;
        self.saved_snapshot = None;
        self.source = source;
        self.preview = None;
        self.diagnostics.clear();
        self.review = None;
        self.schema_state = SchemaState::NotLoaded;
        self.status = None;
        self.request_analysis_now();
    }

    pub fn select_schema(&mut self, path: PathBuf) {
        self.schema_path = Some(path);
        self.schema_state = SchemaState::NotLoaded;
        if let Some(review) = self.review.as_mut() {
            self.analysis_generation = self.analysis_generation.saturating_add(1);
            self.last_edit = None;
            self.analysis_pending = false;
            self.reevaluation_queued = true;
            review.mark_evaluation_pending();
        } else {
            self.request_analysis_now();
        }
    }
}

fn evaluation_preview(evaluation: &RepairEvaluation) -> Option<String> {
    match evaluation {
        RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => {
            Some(candidate.output.clone())
        }
        RepairEvaluation::Invalid { .. } => None,
    }
}

fn merge_invalid_diagnostics(
    mut diagnostics: Vec<Diagnostic>,
    evaluation: &RepairEvaluation,
) -> Vec<Diagnostic> {
    if let RepairEvaluation::Invalid {
        diagnostics: invalid,
        ..
    } = evaluation
    {
        diagnostics.extend(invalid.iter().cloned());
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use std::sync::Arc;

    use jello::{RepairDecision, RepairEvaluation};

    use crate::review::ReviewState;
    use crate::schema_engine::SchemaState;
    use crate::worker::{AnalysisRequest, AnalysisResult};

    use super::AppModel;

    fn analyzed_result(source: &str, generation: u64) -> AnalysisResult {
        let plan = Arc::new(jello::plan_repair_json5(source).unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        AnalysisResult {
            generation,
            selection_version: 0,
            plan: Some(plan),
            evaluation: Some(evaluation),
            diagnostics: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        }
    }

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("jello-model-{name}-{}.json", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn analysis_is_submitted_only_after_250_ms_without_edits() {
        let start = Instant::now();
        let mut model = AppModel {
            source: "{\"value\": 1}".to_string(),
            ..Default::default()
        };
        model.mark_edited(start);

        assert!(
            model
                .take_analysis_request(start + Duration::from_millis(249))
                .is_none()
        );
        let request = model
            .take_analysis_request(start + Duration::from_millis(250))
            .unwrap();
        let AnalysisRequest::Analyze {
            generation, source, ..
        } = request
        else {
            panic!("source edits must queue analysis");
        };
        assert_eq!(generation, 1);
        assert_eq!(source, "{\"value\": 1}");
    }

    #[test]
    fn stale_worker_results_do_not_replace_the_latest_preview() {
        let start = Instant::now();
        let mut model = AppModel {
            source: "{}".to_string(),
            ..Default::default()
        };
        model.mark_edited(start);
        model
            .take_analysis_request(start + Duration::from_millis(250))
            .unwrap();
        model.source = "{\"new\": true}".to_string();
        model.mark_edited(start + Duration::from_millis(300));
        model
            .take_analysis_request(start + Duration::from_millis(550))
            .unwrap();
        assert!(model.apply_result(analyzed_result("{new:true}", 2)));
        model.preview = Some("latest".to_string());
        model.schema_state = SchemaState::Valid;

        let applied = model.apply_result(analyzed_result("{}", 1));

        assert!(!applied);
        assert_eq!(model.preview.as_deref(), Some("latest"));
        assert!(matches!(model.schema_state, SchemaState::Valid));
        assert_eq!(model.review.as_ref().unwrap().plan().source(), "{new:true}");
    }

    #[test]
    fn editing_immediately_invalidates_the_saveable_preview() {
        let start = Instant::now();
        let mut model = AppModel {
            source_path: Some("example.json".into()),
            source: "{}".to_string(),
            ..Default::default()
        };
        model.request_analysis_now();
        let request = model.take_analysis_request(start).unwrap();
        assert!(model.apply_result(analyzed_result("{}", request.generation())));
        assert!(model.preview.is_some());

        model.source.push(' ');
        model.mark_edited(start + Duration::from_millis(1));

        assert!(
            model.preview.is_none(),
            "an edited source must not retain a preview that can be saved"
        );
    }

    #[test]
    fn editing_immediately_clears_the_review_state() {
        let start = Instant::now();
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        let review = ReviewState::new(plan, selection, evaluation);
        let mut model = AppModel {
            source_path: Some("example.json".into()),
            source: "{name:'Ada'}".to_string(),
            preview: Some("{\n  \"name\": \"Ada\"\n}\n".to_string()),
            review: Some(review),
            ..Default::default()
        };

        model.mark_edited(start);

        assert!(model.review.is_none());
        assert!(model.preview.is_none());
    }

    #[test]
    fn review_blocks_save_while_evaluation_or_decisions_are_pending() {
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let selection = plan.default_selection();
        let evaluation = plan.evaluate(&selection);
        let mut review = ReviewState::new(plan, selection, evaluation);
        let mut model = AppModel {
            source_path: Some("example.json".into()),
            preview: Some("{\n  \"name\": \"Ada\"\n}\n".to_string()),
            review: Some(review.clone()),
            ..Default::default()
        };

        assert!(!model.can_save());

        review.set_all(RepairDecision::Accepted);
        model.review = Some(review.clone());
        assert!(!model.can_save());

        review.replace_evaluation(review.plan().evaluate(review.selection()));
        model.review = Some(review);
        assert!(model.can_save());
    }

    #[test]
    fn successful_analysis_installs_review_and_preview() {
        let mut model = AppModel::default();

        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));

        assert!(model.preview.is_some());
        let review = model.review.as_ref().unwrap();
        assert_eq!(review.selection_version(), 0);
        assert!(matches!(review.evaluation(), RepairEvaluation::Preview(_)));
    }

    #[test]
    fn opening_a_source_resets_the_session_saved_output() {
        let mut model = AppModel {
            saved_path: Some(PathBuf::from("data.fixed.json")),
            saved_snapshot: Some(b"{}\n".to_vec()),
            ..Default::default()
        };

        model.open_source(PathBuf::from("data.json"), "{}".to_string());

        assert!(model.saved_path.is_none());
        assert!(model.saved_snapshot.is_none());
        assert_eq!(model.source_snapshot.as_deref(), Some("{}"));
    }

    #[test]
    fn a_loaded_schema_must_be_valid_before_saving() {
        let mut model = AppModel {
            source_path: Some("example.json".into()),
            preview: Some("{}\n".to_string()),
            ..Default::default()
        };

        assert!(model.can_save());

        model.schema_path = Some(PathBuf::from("schema.json"));
        model.schema_state = SchemaState::Invalid(Vec::new());
        assert!(!model.can_save());

        model.schema_state = SchemaState::LoadError("boom".to_string());
        assert!(!model.can_save());

        model.schema_state = SchemaState::Valid;
        assert!(model.can_save());
    }

    #[test]
    fn a_decision_queues_immediate_evaluation_without_source_debounce() {
        let now = Instant::now();
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let decision_set = model.review.as_ref().unwrap().plan().decision_sets()[0].id();

        assert!(model.decide_repair(decision_set, RepairDecision::Accepted));

        let request = model.take_analysis_request(now).unwrap();
        let AnalysisRequest::Evaluate {
            generation,
            selection_version,
            selection,
            ..
        } = request
        else {
            panic!("repair decisions must queue evaluation");
        };
        assert_eq!(generation, 0);
        assert_eq!(selection_version, 1);
        assert_eq!(
            selection.decision(decision_set),
            Some(RepairDecision::Accepted)
        );
    }

    #[test]
    fn stale_selection_results_do_not_replace_review_preview_or_schema() {
        let now = Instant::now();
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let decision_set = model.review.as_ref().unwrap().plan().decision_sets()[0].id();

        assert!(model.decide_repair(decision_set, RepairDecision::Accepted));
        let AnalysisRequest::Evaluate {
            plan, selection, ..
        } = model.take_analysis_request(now).unwrap()
        else {
            panic!("first decision must queue evaluation");
        };
        assert!(model.decide_repair(decision_set, RepairDecision::Rejected));
        model.preview = Some("latest".to_string());
        model.schema_state = SchemaState::Valid;
        let stale_evaluation = plan.evaluate(&selection);

        let applied = model.apply_result(AnalysisResult {
            generation: 0,
            selection_version: 1,
            plan: None,
            evaluation: Some(stale_evaluation),
            diagnostics: Vec::new(),
            schema_state: SchemaState::Invalid(Vec::new()),
        });

        assert!(!applied);
        assert_eq!(model.preview.as_deref(), Some("latest"));
        assert!(matches!(model.schema_state, SchemaState::Valid));
        let review = model.review.as_ref().unwrap();
        assert_eq!(
            review.selection().decision(decision_set),
            Some(RepairDecision::Rejected)
        );
        assert_eq!(review.selection_version(), 2);
        assert!(review.evaluation_pending());
    }

    #[test]
    fn delayed_analyze_result_preserves_newer_same_generation_selection() {
        let now = Instant::now();
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let decision_set = model.review.as_ref().unwrap().plan().decision_sets()[0].id();
        model.request_analysis_now();

        let AnalysisRequest::Analyze { generation: 1, .. } =
            model.take_analysis_request(now).unwrap()
        else {
            panic!("explicit reanalysis must queue a new analysis generation");
        };
        assert!(model.decide_repair(decision_set, RepairDecision::Accepted));

        assert!(!model.apply_result(analyzed_result("{name:'Ada'}", 1)));
        let review = model.review.as_ref().unwrap();
        assert_eq!(review.selection_version(), 1);
        assert_eq!(
            review.selection().decision(decision_set),
            Some(RepairDecision::Accepted)
        );

        let AnalysisRequest::Evaluate {
            generation,
            selection_version,
            plan,
            selection,
            ..
        } = model.take_analysis_request(now).unwrap()
        else {
            panic!("rejecting delayed analysis must preserve queued evaluation");
        };
        let evaluation = plan.evaluate(&selection);
        assert!(model.apply_result(AnalysisResult {
            generation,
            selection_version,
            plan: None,
            evaluation: Some(evaluation),
            diagnostics: Vec::new(),
            schema_state: SchemaState::Valid,
        }));
        let review = model.review.as_ref().unwrap();
        assert_eq!(review.selection_version(), 1);
        assert_eq!(
            review.selection().decision(decision_set),
            Some(RepairDecision::Accepted)
        );
        assert!(!review.evaluation_pending());
        assert!(matches!(model.schema_state, SchemaState::Valid));
    }

    #[test]
    fn selecting_schema_re_evaluates_the_current_review_selection() {
        let now = Instant::now();
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let decision_set = model.review.as_ref().unwrap().plan().decision_sets()[0].id();
        assert!(model.decide_repair(decision_set, RepairDecision::Accepted));
        let AnalysisRequest::Evaluate {
            generation,
            selection_version,
            plan,
            selection,
            ..
        } = model.take_analysis_request(now).unwrap()
        else {
            panic!("repair decision must queue evaluation");
        };
        assert!(model.apply_result(AnalysisResult {
            generation,
            selection_version,
            plan: None,
            evaluation: Some(plan.evaluate(&selection)),
            diagnostics: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        }));
        let original_plan = model.review.as_ref().unwrap().plan().clone();

        let schema_path = PathBuf::from("schema.json");
        model.select_schema(schema_path.clone());
        assert!(model.review.as_ref().unwrap().evaluation_pending());

        let AnalysisRequest::Evaluate {
            generation,
            selection_version,
            plan,
            selection,
            schema_path: request_schema_path,
        } = model.take_analysis_request(now).unwrap()
        else {
            panic!("schema changes with a review must queue evaluation");
        };
        assert_eq!(generation, 1);
        assert_eq!(selection_version, 1);
        assert!(Arc::ptr_eq(&plan, &original_plan));
        assert_eq!(
            selection.decision(decision_set),
            Some(RepairDecision::Accepted)
        );
        assert_eq!(request_schema_path, Some(schema_path));

        assert!(model.apply_result(AnalysisResult {
            generation,
            selection_version,
            plan: None,
            evaluation: Some(plan.evaluate(&selection)),
            diagnostics: Vec::new(),
            schema_state: SchemaState::Valid,
        }));
        let review = model.review.as_ref().unwrap();
        assert_eq!(review.selection_version(), 1);
        assert_eq!(
            review.selection().decision(decision_set),
            Some(RepairDecision::Accepted)
        );
        assert!(!review.evaluation_pending());
        assert!(matches!(model.schema_state, SchemaState::Valid));
    }

    #[test]
    fn selecting_a_repair_updates_the_review_focus() {
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let group = model.review.as_ref().unwrap().plan().groups()[0].id();

        model.select_repair(Some(group));

        assert_eq!(model.review.as_ref().unwrap().selected_group(), Some(group));

        model.select_repair(None);

        assert_eq!(model.review.as_ref().unwrap().selected_group(), None);
    }

    #[test]
    fn a_result_from_before_the_latest_edit_is_rejected_during_debounce() {
        let start = Instant::now();
        let mut model = AppModel {
            source: "{}".to_string(),
            ..Default::default()
        };
        model.request_analysis_now();
        let old_request = model.take_analysis_request(start).unwrap();

        model.source = "{\"changed\": true}".to_string();
        model.mark_edited(start + Duration::from_millis(1));
        assert!(
            model
                .take_analysis_request(start + Duration::from_millis(100))
                .is_none()
        );

        let applied = model.apply_result(analyzed_result("{}", old_request.generation()));

        assert!(!applied);
        assert!(model.preview.is_none());
    }

    #[test]
    fn decided_repairs_stay_in_flight_until_re_evaluated() {
        let now = Instant::now();
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        assert!(!model.analysis_in_flight());

        let decision_set = model.review.as_ref().unwrap().plan().decision_sets()[0].id();
        assert!(model.decide_repair(decision_set, RepairDecision::Accepted));
        assert!(
            model.analysis_in_flight(),
            "a queued re-evaluation must show as analyzing"
        );

        let AnalysisRequest::Evaluate {
            generation,
            selection_version,
            plan,
            selection,
            ..
        } = model.take_analysis_request(now).unwrap()
        else {
            panic!("decisions must queue evaluation");
        };
        let evaluation = plan.evaluate(&selection);
        assert!(model.apply_result(AnalysisResult {
            generation,
            selection_version,
            plan: None,
            evaluation: Some(evaluation),
            diagnostics: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        }));
        assert!(!model.analysis_in_flight());
    }

    #[test]
    fn invalid_review_diagnostics_are_exposed_to_the_problems_tab() {
        let now = Instant::now();
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let decision_set = model.review.as_ref().unwrap().plan().decision_sets()[0].id();
        assert!(model.decide_repair(decision_set, RepairDecision::Rejected));
        let AnalysisRequest::Evaluate {
            generation,
            selection_version,
            plan,
            selection,
            ..
        } = model.take_analysis_request(now).unwrap()
        else {
            panic!("decisions must queue evaluation");
        };
        let evaluation = plan.evaluate(&selection);
        let RepairEvaluation::Invalid { diagnostics, .. } = &evaluation else {
            panic!("rejecting a required repair must invalidate the plan");
        };
        let invalid_len = diagnostics.len();

        assert!(model.apply_result(AnalysisResult {
            generation,
            selection_version,
            plan: None,
            evaluation: Some(evaluation),
            diagnostics: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        }));
        assert!(
            model.diagnostics.len() >= invalid_len,
            "invalid review diagnostics must be listed in the problems tab"
        );
    }

    #[test]
    fn editing_clears_the_status_message() {
        let start = Instant::now();
        let mut model = AppModel {
            status: Some("saved".to_string()),
            ..Default::default()
        };

        model.mark_edited(start);

        assert!(model.status.is_none());
    }

    #[test]
    fn analysis_in_flight_tracks_submission_and_application() {
        let start = Instant::now();
        let mut model = AppModel::default();
        assert!(!model.analysis_in_flight());

        model.mark_edited(start);
        assert!(model.analysis_in_flight());

        let request = model
            .take_analysis_request(start + Duration::from_millis(250))
            .unwrap();
        assert!(
            model.analysis_in_flight(),
            "a submitted but not yet applied result must stay in flight"
        );

        assert!(model.apply_result(analyzed_result("{}", request.generation())));
        assert!(!model.analysis_in_flight());
    }

    #[test]
    fn analysis_in_flight_clears_when_an_unrepairable_result_arrives() {
        let start = Instant::now();
        let mut model = AppModel {
            source: "{".to_string(),
            ..Default::default()
        };
        model.request_analysis_now();
        let request = model.take_analysis_request(start).unwrap();
        let generation = request.generation();
        assert!(model.analysis_in_flight());

        assert!(model.apply_result(AnalysisResult {
            generation,
            selection_version: 0,
            plan: None,
            evaluation: None,
            diagnostics: jello::plan_repair_json5("{").unwrap_err(),
            schema_state: SchemaState::NotLoaded,
        }));
        assert!(!model.analysis_in_flight());
    }

    #[test]
    fn save_verification_accepts_an_unchanged_session_output() {
        let source = temp_file("save-ok-source", b"{}\n");
        let saved = temp_file("save-ok-saved", b"{\n  \"a\": 1\n}\n");
        let model = AppModel {
            source_path: Some(source.clone()),
            source_snapshot: Some("{}\n".to_string()),
            saved_path: Some(saved.clone()),
            saved_snapshot: Some(b"{\n  \"a\": 1\n}\n".to_vec()),
            ..Default::default()
        };

        assert!(model.verify_save_target().is_ok());
        assert!(model.verify_source_unchanged().is_ok());
        std::fs::remove_file(source).unwrap();
        std::fs::remove_file(saved).unwrap();
    }

    #[test]
    fn save_verification_refuses_a_deleted_session_output() {
        let source = temp_file("save-deleted-source", b"{}\n");
        let saved = std::env::temp_dir().join(format!(
            "jello-model-save-deleted-saved-{}.json",
            std::process::id()
        ));
        let model = AppModel {
            source_path: Some(source),
            source_snapshot: Some("{}\n".to_string()),
            saved_path: Some(saved),
            saved_snapshot: Some(b"{\n  \"a\": 1\n}\n".to_vec()),
            ..Default::default()
        };

        assert!(model.verify_save_target().is_err());
        assert!(model.verify_source_unchanged().is_ok());
        std::fs::remove_file(model.source_path.as_ref().unwrap()).unwrap();
    }

    #[test]
    fn save_verification_refuses_a_modified_session_output() {
        let source = temp_file("save-modified-source", b"{}\n");
        let saved = temp_file("save-modified-saved", b"new content");
        let model = AppModel {
            source_path: Some(source.clone()),
            source_snapshot: Some("{}\n".to_string()),
            saved_path: Some(saved.clone()),
            saved_snapshot: Some(b"expected content".to_vec()),
            ..Default::default()
        };

        assert!(model.verify_save_target().is_err());
        assert!(model.verify_source_unchanged().is_ok());
        std::fs::remove_file(source).unwrap();
        std::fs::remove_file(saved).unwrap();
    }

    #[test]
    fn save_as_verification_ignores_a_missing_session_output() {
        let source = temp_file("save-as-source", b"{}\n");
        let saved = std::env::temp_dir().join(format!(
            "jello-model-save-as-saved-{}.json",
            std::process::id()
        ));
        let model = AppModel {
            source_path: Some(source.clone()),
            source_snapshot: Some("{}\n".to_string()),
            saved_path: Some(saved),
            saved_snapshot: Some(b"{\n  \"a\": 1\n}\n".to_vec()),
            ..Default::default()
        };

        assert!(model.verify_save_target().is_err());
        assert!(model.verify_source_unchanged().is_ok());
        std::fs::remove_file(source).unwrap();
    }

    #[test]
    fn save_verification_refuses_a_modified_source() {
        let source = temp_file("save-source-modified", b"changed");
        let model = AppModel {
            source_path: Some(source.clone()),
            source_snapshot: Some("original".to_string()),
            ..Default::default()
        };

        assert!(model.verify_save_target().is_err());
        assert!(model.verify_source_unchanged().is_err());
        std::fs::remove_file(source).unwrap();
    }

    #[test]
    fn save_verification_falls_back_to_the_source_when_nothing_was_saved() {
        let source = temp_file("save-fallback-source", b"{}\n");
        let model = AppModel {
            source_path: Some(source.clone()),
            source_snapshot: Some("{}\n".to_string()),
            ..Default::default()
        };

        assert!(model.verify_save_target().is_ok());
        std::fs::remove_file(source).unwrap();
    }

    #[test]
    fn save_verification_requires_a_saved_snapshot_when_adopting_a_path() {
        let source = temp_file("save-snapshot-source", b"{}\n");
        let saved = temp_file("save-snapshot-saved", b"{}");
        let model = AppModel {
            source_path: Some(source.clone()),
            source_snapshot: Some("{}\n".to_string()),
            saved_path: Some(saved.clone()),
            saved_snapshot: None,
            ..Default::default()
        };

        assert!(model.verify_save_target().is_err());
        assert!(model.verify_source_unchanged().is_ok());
        std::fs::remove_file(source).unwrap();
        std::fs::remove_file(saved).unwrap();
    }

    #[test]
    fn save_verification_is_lenient_without_any_session_files() {
        let model = AppModel::default();

        assert!(model.verify_save_target().is_ok());
        assert!(model.verify_source_unchanged().is_ok());
    }
}
