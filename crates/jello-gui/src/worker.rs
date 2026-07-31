use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::mpsc::{self, Receiver, SendError, Sender, TryRecvError};
use std::thread;
#[cfg(test)]
use std::time::Duration;

use jello::{Diagnostic, FixEdit, RepairOutcome};

use crate::schema_engine::{SchemaEngine, SchemaState};

#[derive(Debug)]
pub struct AnalysisRequest {
    pub generation: u64,
    pub source: String,
    pub schema_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct AnalysisResult {
    pub generation: u64,
    pub preview: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub repairs: Vec<FixEdit>,
    pub schema_state: SchemaState,
}

pub struct AnalysisWorker {
    requests: Sender<AnalysisRequest>,
    results: Receiver<AnalysisResult>,
    latest_generation: Arc<AtomicU64>,
}

impl AnalysisWorker {
    pub fn new(context: eframe::egui::Context) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<AnalysisRequest>();
        let (result_tx, result_rx) = mpsc::channel();
        let latest_generation = Arc::new(AtomicU64::new(0));
        let worker_generation = latest_generation.clone();

        thread::Builder::new()
            .name("jello-analysis".to_string())
            .spawn(move || run_worker(request_rx, result_tx, context, worker_generation))
            .expect("failed to start Jello analysis worker");

        Self {
            requests: request_tx,
            results: result_rx,
            latest_generation,
        }
    }

    pub fn invalidate(&self, generation: u64) {
        self.latest_generation
            .fetch_max(generation, Ordering::Release);
    }

    pub fn submit(&self, request: AnalysisRequest) -> Result<(), SendError<AnalysisRequest>> {
        self.invalidate(request.generation);
        self.requests.send(request)
    }

    pub fn try_recv(&self) -> Result<AnalysisResult, TryRecvError> {
        self.results.try_recv()
    }

    #[cfg(test)]
    pub fn recv_timeout(&self, timeout: Duration) -> Result<AnalysisResult, RecvTimeoutError> {
        self.results.recv_timeout(timeout)
    }
}

fn run_worker(
    requests: Receiver<AnalysisRequest>,
    results: Sender<AnalysisResult>,
    context: eframe::egui::Context,
    latest_generation: Arc<AtomicU64>,
) {
    let mut schema_engine = SchemaEngine::default();
    while let Ok(mut request) = requests.recv() {
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }
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
    let AnalysisRequest {
        generation,
        source,
        schema_path,
    } = request;
    if is_cancelled(latest_generation, generation) {
        return None;
    }
    let outcome = jello::repair_json5(&source);
    if is_cancelled(latest_generation, generation) {
        return None;
    }
    match outcome {
        RepairOutcome::Valid(result) | RepairOutcome::Repaired(result) => {
            let schema_state = match schema_path.as_deref() {
                Some(path) => {
                    schema_engine.validate(path, &result.output, latest_generation, generation)?
                }
                None => SchemaState::NotLoaded,
            };
            if is_cancelled(latest_generation, generation) {
                return None;
            }
            Some(AnalysisResult {
                generation,
                preview: Some(result.output),
                diagnostics: Vec::new(),
                repairs: result.edits,
                schema_state,
            })
        }
        RepairOutcome::Unrepairable(diagnostics) => Some(AnalysisResult {
            generation,
            preview: None,
            diagnostics,
            repairs: Vec::new(),
            schema_state: SchemaState::NotLoaded,
        }),
    }
}

fn is_cancelled(latest_generation: &Arc<AtomicU64>, generation: u64) -> bool {
    latest_generation.load(Ordering::Acquire) != generation
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;
    use std::time::Duration;

    use crate::schema_engine::{SchemaEngine, SchemaState};

    use super::{AnalysisRequest, AnalysisWorker, analyze};

    #[test]
    fn repairable_json5_produces_a_strict_preview_and_repairs() {
        let worker = AnalysisWorker::new(eframe::egui::Context::default());
        worker
            .submit(AnalysisRequest {
                generation: 7,
                source: "{name: 'Ada', values: [1 2]}".to_string(),
                schema_path: None,
            })
            .unwrap();

        let result = worker.recv_timeout(Duration::from_secs(2)).unwrap();

        assert_eq!(result.generation, 7);
        assert_eq!(
            result.preview.as_deref(),
            Some("{\n  \"name\": \"Ada\",\n  \"values\": [\n    1,\n    2\n  ]\n}")
        );
        assert!(!result.repairs.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn unrepairable_input_preserves_diagnostics_and_has_no_preview() {
        let worker = AnalysisWorker::new(eframe::egui::Context::default());
        worker
            .submit(AnalysisRequest {
                generation: 9,
                source: "{\"name\" 1}".to_string(),
                schema_path: None,
            })
            .unwrap();

        let result = worker.recv_timeout(Duration::from_secs(2)).unwrap();

        assert_eq!(result.generation, 9);
        assert!(result.preview.is_none());
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
            .submit(AnalysisRequest {
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
            AnalysisRequest {
                generation: 1,
                source: "{}".to_string(),
                schema_path: None,
            },
            &mut SchemaEngine::default(),
            &latest,
        );

        assert!(result.is_none());
    }
}
