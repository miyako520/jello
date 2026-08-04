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
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            language: UiLanguage::default(),
            source_path: None,
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

    pub fn can_save(&self) -> bool {
        self.source_path.is_some()
            && self.preview.is_some()
            && self.review.as_ref().is_none_or(ReviewState::can_save)
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
            selection_version,
            plan,
            evaluation,
            diagnostics,
            schema_state,
            ..
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
                self.preview = evaluation_preview(&evaluation);
                let selection = plan.default_selection();
                self.review = Some(ReviewState::new(plan, selection, evaluation));
                self.diagnostics = diagnostics;
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
                self.preview = evaluation_preview(&evaluation);
                review.replace_evaluation(evaluation);
                self.diagnostics = diagnostics;
                self.schema_state = schema_state;
                self.reevaluation_queued = false;
                true
            }
            (None, None) if selection_version == 0 => {
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn select_repair(&mut self, group_id: RepairGroupId) {
        if let Some(review) = self.review.as_mut() {
            review.set_selected_group(Some(group_id));
        }
    }

    pub fn open_source(&mut self, path: PathBuf, source: String) {
        self.source_path = Some(path);
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
        self.request_analysis_now();
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

#[cfg(test)]
mod tests {
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
        model.select_schema("schema.json".into());

        let AnalysisRequest::Analyze { generation: 1, .. } =
            model.take_analysis_request(now).unwrap()
        else {
            panic!("schema changes must queue a new analysis generation");
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
    fn selecting_a_repair_updates_the_review_focus() {
        let mut model = AppModel::default();
        assert!(model.apply_result(analyzed_result("{name:'Ada'}", 0)));
        let group = model.review.as_ref().unwrap().plan().groups()[0].id();

        model.select_repair(group);

        assert_eq!(model.review.as_ref().unwrap().selected_group(), Some(group));
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
}
