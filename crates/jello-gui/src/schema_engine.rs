use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use jello::{SchemaIssue, SchemaValidator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaState {
    NotLoaded,
    Valid,
    Invalid(Vec<SchemaIssue>),
    LoadError(String),
}

/// A thin cancellation-aware wrapper around the core [`SchemaValidator`].
/// The core checks the generation between bounded parsing, reference loading,
/// cache validation, compilation, and issue collection steps. Results for a
/// generation superseded inside one third-party call are discarded afterward.
pub struct SchemaEngine {
    validator: SchemaValidator,
}

impl Default for SchemaEngine {
    fn default() -> Self {
        Self {
            validator: SchemaValidator::new(),
        }
    }
}

impl SchemaEngine {
    pub fn validate(
        &mut self,
        schema_path: &Path,
        instance_json: &str,
        latest_generation: &Arc<AtomicU64>,
        generation: u64,
    ) -> Option<SchemaState> {
        if is_cancelled(latest_generation, generation) {
            return None;
        }
        let cancellation_generation = latest_generation.clone();
        match self
            .validator
            .validate_with_cancel(schema_path, instance_json, move || {
                is_cancelled(&cancellation_generation, generation)
            }) {
            Ok(Some(issues)) if issues.is_empty() => Some(SchemaState::Valid),
            Ok(Some(issues)) => Some(SchemaState::Invalid(issues)),
            Ok(None) => None,
            Err(error) => Some(SchemaState::LoadError(error)),
        }
    }
}

fn is_cancelled(latest_generation: &Arc<AtomicU64>, generation: u64) -> bool {
    latest_generation.load(Ordering::Acquire) != generation
}

#[cfg(test)]
pub fn validate(schema_path: &Path, instance_json: &str) -> SchemaState {
    let latest_generation = Arc::new(AtomicU64::new(0));
    SchemaEngine::default()
        .validate(schema_path, instance_json, &latest_generation, 0)
        .unwrap_or(SchemaState::NotLoaded)
}
