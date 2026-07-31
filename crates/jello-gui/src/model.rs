use std::path::PathBuf;
use std::time::{Duration, Instant};

use jello::{Diagnostic, FixEdit};

use crate::i18n::UiLanguage;
use crate::schema_engine::SchemaState;
use crate::worker::{AnalysisRequest, AnalysisResult};

const ANALYSIS_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct AppModel {
    pub language: UiLanguage,
    pub source_path: Option<PathBuf>,
    pub source: String,
    pub preview: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub repairs: Vec<FixEdit>,
    pub schema_path: Option<PathBuf>,
    pub schema_state: SchemaState,
    pub status: Option<String>,
    last_edit: Option<Instant>,
    analysis_pending: bool,
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
            repairs: Vec::new(),
            schema_path: None,
            schema_state: SchemaState::NotLoaded,
            status: None,
            last_edit: None,
            analysis_pending: false,
            analysis_generation: 0,
        }
    }
}

impl AppModel {
    pub fn mark_edited(&mut self, now: Instant) {
        self.analysis_generation = self.analysis_generation.saturating_add(1);
        self.last_edit = Some(now);
        self.analysis_pending = true;
        self.preview = None;
        self.diagnostics.clear();
        self.repairs.clear();
        self.schema_state = SchemaState::NotLoaded;
    }

    pub fn request_analysis_now(&mut self) {
        self.analysis_generation = self.analysis_generation.saturating_add(1);
        self.last_edit = None;
        self.analysis_pending = true;
    }

    pub fn analysis_generation(&self) -> u64 {
        self.analysis_generation
    }

    pub fn can_save(&self) -> bool {
        self.source_path.is_some() && self.preview.is_some()
    }

    pub fn take_analysis_request(&mut self, now: Instant) -> Option<AnalysisRequest> {
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
        Some(AnalysisRequest {
            generation: self.analysis_generation,
            source: self.source.clone(),
            schema_path: self.schema_path.clone(),
        })
    }

    pub fn apply_result(&mut self, result: AnalysisResult) -> bool {
        if result.generation != self.analysis_generation {
            return false;
        }
        self.preview = result.preview;
        self.diagnostics = result.diagnostics;
        self.repairs = result.repairs;
        self.schema_state = result.schema_state;
        true
    }

    pub fn open_source(&mut self, path: PathBuf, source: String) {
        self.source_path = Some(path);
        self.source = source;
        self.preview = None;
        self.diagnostics.clear();
        self.repairs.clear();
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::schema_engine::SchemaState;
    use crate::worker::AnalysisResult;

    use super::AppModel;

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
        assert_eq!(request.generation, 1);
        assert_eq!(request.source, "{\"value\": 1}");
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
        model.preview = Some("latest".to_string());

        let applied = model.apply_result(AnalysisResult {
            generation: 1,
            preview: Some("stale".to_string()),
            diagnostics: Vec::new(),
            repairs: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        });

        assert!(!applied);
        assert_eq!(model.preview.as_deref(), Some("latest"));
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
        assert!(model.apply_result(AnalysisResult {
            generation: request.generation,
            preview: Some("{}\n".to_string()),
            diagnostics: Vec::new(),
            repairs: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        }));
        assert!(model.preview.is_some());

        model.source.push(' ');
        model.mark_edited(start + Duration::from_millis(1));

        assert!(
            model.preview.is_none(),
            "an edited source must not retain a preview that can be saved"
        );
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

        let applied = model.apply_result(AnalysisResult {
            generation: old_request.generation,
            preview: Some("stale".to_string()),
            diagnostics: Vec::new(),
            repairs: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        });

        assert!(!applied);
        assert!(model.preview.is_none());
    }
}
