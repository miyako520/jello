use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{self, Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
#[cfg(test)]
use std::time::Duration;

use jello::{Diagnostic, RepairEvaluation, RepairPlan, RepairSelection};

use crate::schema_engine::{SchemaEngine, SchemaState};

#[derive(Debug)]
pub enum AnalysisRequest {
    Analyze {
        generation: u64,
        source: String,
        schema_path: Option<PathBuf>,
    },
    Evaluate {
        generation: u64,
        selection_version: u64,
        plan: Arc<RepairPlan>,
        selection: RepairSelection,
        schema_path: Option<PathBuf>,
    },
}

impl AnalysisRequest {
    pub fn generation(&self) -> u64 {
        match self {
            Self::Analyze { generation, .. } | Self::Evaluate { generation, .. } => *generation,
        }
    }
}

#[derive(Debug)]
pub struct AnalysisResult {
    pub generation: u64,
    pub selection_version: u64,
    pub plan: Option<Arc<RepairPlan>>,
    pub evaluation: Option<RepairEvaluation>,
    pub diagnostics: Vec<Diagnostic>,
    pub schema_state: SchemaState,
}

pub struct AnalysisWorker {
    requests: Arc<LatestRequestSlot>,
    results: Receiver<AnalysisResult>,
    latest_generation: Arc<AtomicU64>,
}

impl AnalysisWorker {
    pub fn new(context: eframe::egui::Context) -> Self {
        let requests = Arc::new(LatestRequestSlot::default());
        let worker_requests = requests.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let latest_generation = Arc::new(AtomicU64::new(0));
        let worker_generation = latest_generation.clone();

        thread::Builder::new()
            .name("jello-analysis".to_string())
            .spawn(move || run_worker(worker_requests, result_tx, context, worker_generation))
            .expect("failed to start Jello analysis worker");

        Self {
            requests,
            results: result_rx,
            latest_generation,
        }
    }

    pub fn invalidate(&self, generation: u64) {
        self.latest_generation
            .fetch_max(generation, Ordering::Release);
    }

    pub fn submit(&self, request: AnalysisRequest) -> Result<(), SendError<AnalysisRequest>> {
        self.invalidate(request.generation());
        self.requests.submit(request)
    }

    pub fn try_recv(&self) -> Result<AnalysisResult, TryRecvError> {
        self.results.try_recv()
    }

    #[cfg(test)]
    pub fn recv_timeout(&self, timeout: Duration) -> Result<AnalysisResult, RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }
}

impl Drop for AnalysisWorker {
    fn drop(&mut self) {
        self.requests.close();
    }
}

#[derive(Default)]
struct LatestRequestSlot {
    state: Mutex<LatestRequestState>,
    ready: Condvar,
}

#[derive(Default)]
struct LatestRequestState {
    pending: Option<AnalysisRequest>,
    closed: bool,
}

impl LatestRequestSlot {
    fn submit(&self, request: AnalysisRequest) -> Result<(), SendError<AnalysisRequest>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Err(SendError(request));
        }
        state.pending = Some(request);
        self.ready.notify_one();
        Ok(())
    }

    fn recv(&self) -> Option<AnalysisRequest> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.pending.is_none() && !state.closed {
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        state.pending.take()
    }

    fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        state.pending = None;
        self.ready.notify_one();
    }
}

fn run_worker(
    requests: Arc<LatestRequestSlot>,
    results: Sender<AnalysisResult>,
    context: eframe::egui::Context,
    latest_generation: Arc<AtomicU64>,
) {
    let mut schema_engine = SchemaEngine::default();
    while let Some(request) = requests.recv() {
        let Some(result) = analyze(request, &mut schema_engine, &latest_generation) else {
            continue;
        };
        if results.send(result).is_err() {
            break;
        }
        context.request_repaint();
    }
}

fn analyze(
    request: AnalysisRequest,
    schema_engine: &mut SchemaEngine,
    latest_generation: &Arc<AtomicU64>,
) -> Option<AnalysisResult> {
    let generation = request.generation();
    if is_cancelled(latest_generation, generation) {
        return None;
    }
    match request {
        AnalysisRequest::Analyze {
            source,
            schema_path,
            ..
        } => finish_analyze(
            generation,
            jello::plan_repair_json5(&source),
            schema_path,
            schema_engine,
            latest_generation,
        ),
        AnalysisRequest::Evaluate {
            selection_version,
            plan,
            selection,
            schema_path,
            ..
        } => {
            let evaluation = plan.evaluate(&selection);
            evaluated_result(
                generation,
                selection_version,
                None,
                evaluation,
                schema_path,
                schema_engine,
                latest_generation,
            )
        }
    }
}

fn finish_analyze(
    generation: u64,
    planned: Result<RepairPlan, Vec<Diagnostic>>,
    schema_path: Option<PathBuf>,
    schema_engine: &mut SchemaEngine,
    latest_generation: &Arc<AtomicU64>,
) -> Option<AnalysisResult> {
    if is_cancelled(latest_generation, generation) {
        return None;
    }
    let plan = match planned {
        Ok(plan) => Arc::new(plan),
        Err(diagnostics) => {
            return Some(AnalysisResult {
                generation,
                selection_version: 0,
                plan: None,
                evaluation: None,
                diagnostics,
                schema_state: SchemaState::NotLoaded,
            });
        }
    };
    let selection = plan.default_selection();
    let evaluation = plan.evaluate(&selection);
    evaluated_result(
        generation,
        0,
        Some(plan),
        evaluation,
        schema_path,
        schema_engine,
        latest_generation,
    )
}

fn evaluated_result(
    generation: u64,
    selection_version: u64,
    plan: Option<Arc<RepairPlan>>,
    evaluation: RepairEvaluation,
    schema_path: Option<PathBuf>,
    schema_engine: &mut SchemaEngine,
    latest_generation: &Arc<AtomicU64>,
) -> Option<AnalysisResult> {
    if is_cancelled(latest_generation, generation) {
        return None;
    }
    let schema_state = match &evaluation {
        RepairEvaluation::Preview(candidate) | RepairEvaluation::Ready(candidate) => {
            match schema_path.as_deref() {
                Some(path) => schema_engine.validate(
                    path,
                    &candidate.output,
                    latest_generation,
                    generation,
                )?,
                None => SchemaState::NotLoaded,
            }
        }
        RepairEvaluation::Invalid { .. } => SchemaState::NotLoaded,
    };
    if is_cancelled(latest_generation, generation) {
        return None;
    }
    Some(AnalysisResult {
        generation,
        selection_version,
        plan,
        evaluation: Some(evaluation),
        diagnostics: Vec::new(),
        schema_state,
    })
}

fn is_cancelled(latest_generation: &Arc<AtomicU64>, generation: u64) -> bool {
    latest_generation.load(Ordering::Acquire) != generation
}

#[cfg(test)]
fn analyze_for_test(request: AnalysisRequest) -> AnalysisResult {
    let latest_generation = Arc::new(AtomicU64::new(request.generation()));
    analyze(request, &mut SchemaEngine::default(), &latest_generation)
        .expect("test analysis should not be cancelled")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    use jello::{RepairDecision, RepairEvaluation};

    use crate::schema_engine::{SchemaEngine, SchemaState};

    use super::{
        AnalysisRequest, AnalysisWorker, LatestRequestSlot, analyze, analyze_for_test,
        finish_analyze,
    };

    #[test]
    fn pending_request_slot_keeps_only_the_latest_request() {
        let slot = LatestRequestSlot::default();
        slot.submit(AnalysisRequest::Analyze {
            generation: 1,
            source: "old".to_string(),
            schema_path: None,
        })
        .unwrap();
        slot.submit(AnalysisRequest::Analyze {
            generation: 2,
            source: "new".to_string(),
            schema_path: None,
        })
        .unwrap();

        assert_eq!(slot.recv().unwrap().generation(), 2);
        slot.close();
        assert!(slot.recv().is_none());
    }

    #[test]
    fn repairable_json5_produces_a_pending_plan_and_strict_preview() {
        let result = analyze_for_test(AnalysisRequest::Analyze {
            generation: 7,
            source: "{name: 'Ada', values: [1 2]}".to_string(),
            schema_path: None,
        });
        assert!(
            result
                .plan
                .as_ref()
                .is_some_and(|plan| !plan.groups().is_empty())
        );
        assert!(matches!(
            result.evaluation,
            Some(RepairEvaluation::Preview(_))
        ));
    }

    #[test]
    fn rejecting_required_repairs_returns_invalid_without_schema_work() {
        let plan = Arc::new(jello::plan_repair_json5("{name:'Ada'}").unwrap());
        let mut selection = plan.default_selection();
        selection.set_all(RepairDecision::Rejected);
        let result = analyze_for_test(AnalysisRequest::Evaluate {
            generation: 9,
            selection_version: 1,
            plan,
            selection,
            schema_path: None,
        });
        assert!(matches!(
            result.evaluation,
            Some(RepairEvaluation::Invalid { .. })
        ));
        assert!(matches!(result.schema_state, SchemaState::NotLoaded));
    }

    #[test]
    fn unrepairable_input_preserves_diagnostics_and_has_no_preview() {
        let worker = AnalysisWorker::new(eframe::egui::Context::default());
        worker
            .submit(AnalysisRequest::Analyze {
                generation: 9,
                source: "{\"name\" 1}".to_string(),
                schema_path: None,
            })
            .unwrap();

        let result = worker.recv_timeout(Duration::from_secs(2)).unwrap();

        assert_eq!(result.generation, 9);
        assert!(result.evaluation.is_none());
        assert!(result.plan.is_none());
        assert!(!result.diagnostics.is_empty());
        assert_eq!(result.diagnostics[0].code, "E006");
    }

    #[test]
    fn schema_validation_runs_against_the_strict_preview() {
        let schema =
            std::env::temp_dir().join(format!("jello-worker-schema-{}.json", std::process::id()));
        std::fs::write(
            &schema,
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"age":{"type":"integer"}}}"#,
        )
        .unwrap();
        let worker = AnalysisWorker::new(eframe::egui::Context::default());
        worker
            .submit(AnalysisRequest::Analyze {
                generation: 11,
                source: "{age: 'old'}".to_string(),
                schema_path: Some(schema.clone()),
            })
            .unwrap();

        let result = worker.recv_timeout(Duration::from_secs(2)).unwrap();
        std::fs::remove_file(schema).unwrap();

        assert!(matches!(result.schema_state, SchemaState::Invalid(_)));
    }

    #[test]
    fn superseded_work_is_dropped_before_analysis() {
        let latest = Arc::new(AtomicU64::new(2));
        let result = analyze(
            AnalysisRequest::Analyze {
                generation: 1,
                source: "{}".to_string(),
                schema_path: None,
            },
            &mut SchemaEngine::default(),
            &latest,
        );

        assert!(result.is_none());
    }

    #[test]
    fn superseded_work_is_dropped_after_plan_creation() {
        let latest = Arc::new(AtomicU64::new(2));
        let result = finish_analyze(
            1,
            jello::plan_repair_json5("{}"),
            None,
            &mut SchemaEngine::default(),
            &latest,
        );

        assert!(result.is_none());
    }
}
