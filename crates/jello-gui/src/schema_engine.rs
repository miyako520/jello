use std::collections::{HashMap, hash_map::DefaultHasher};
use std::error::Error;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use jsonschema::{Retrieve, Uri, Validator};
use serde_json::Value;
use url::Url;

pub const MAX_SCHEMA_FILES: usize = 64;
pub const MAX_SCHEMA_TOTAL_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    pub message: String,
    pub instance_path: String,
    pub schema_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaState {
    NotLoaded,
    Valid,
    Invalid(Vec<SchemaIssue>),
    LoadError(String),
}

#[derive(Debug, Clone, Copy)]
struct SchemaLimits {
    max_files: usize,
    max_total_bytes: usize,
}

impl Default for SchemaLimits {
    fn default() -> Self {
        Self {
            max_files: MAX_SCHEMA_FILES,
            max_total_bytes: MAX_SCHEMA_TOTAL_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
struct FileStamp {
    path: PathBuf,
    bytes: usize,
    fingerprint: u64,
}

#[derive(Debug, Clone)]
struct LoadedSchema {
    value: Value,
    stamp: FileStamp,
}

#[derive(Debug)]
struct LoadState {
    root: PathBuf,
    files: HashMap<PathBuf, LoadedSchema>,
    total_bytes: usize,
    limits: SchemaLimits,
    latest_generation: Arc<AtomicU64>,
    generation: u64,
}

impl LoadState {
    fn is_cancelled(&self) -> bool {
        self.latest_generation.load(Ordering::Acquire) != self.generation
    }

    fn load_path(&mut self, path: &Path) -> Result<Value, String> {
        if self.is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }
        if let Some(loaded) = self.files.get(path) {
            return Ok(loaded.value.clone());
        }
        if self.files.len() >= self.limits.max_files {
            return Err(format!(
                "schema file count limit exceeded (maximum {})",
                self.limits.max_files
            ));
        }

        let source = jello::read_utf8_file_stable(path)
            .map_err(|error| format!("unable to read schema {}: {error}", path.display()))?;
        let next_total = self
            .total_bytes
            .checked_add(source.len())
            .ok_or_else(|| "schema total byte count overflowed".to_string())?;
        if next_total > self.limits.max_total_bytes {
            return Err(format!(
                "schema total byte limit exceeded (maximum {})",
                self.limits.max_total_bytes
            ));
        }
        if self.is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }

        let value: Value = serde_json::from_str(&source)
            .map_err(|error| format!("schema {} is not valid JSON: {error}", path.display()))?;
        let stamp = FileStamp {
            path: path.to_path_buf(),
            bytes: source.len(),
            fingerprint: fingerprint(source.as_bytes()),
        };
        self.total_bytes = next_total;
        self.files.insert(
            path.to_path_buf(),
            LoadedSchema {
                value: value.clone(),
                stamp,
            },
        );
        Ok(value)
    }

    fn stamps(&self) -> Vec<FileStamp> {
        self.files
            .values()
            .map(|loaded| loaded.stamp.clone())
            .collect()
    }
}

#[derive(Debug, Clone)]
struct LocalSchemaRetriever {
    state: Arc<Mutex<LoadState>>,
}

impl Retrieve for LocalSchemaRetriever {
    fn retrieve(&self, uri: &Uri<String>) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let url = Url::parse(uri.as_str())?;
        if url.scheme() != "file" {
            return Err(format!(
                "network references are disabled; only local schema files are allowed: {uri}"
            )
            .into());
        }
        let requested = url
            .to_file_path()
            .map_err(|_| format!("invalid local schema reference: {uri}"))?;
        let canonical = fs::canonicalize(&requested)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "schema loader state is unavailable")?;
        if !canonical.starts_with(&state.root) {
            return Err(format!(
                "schema reference is outside the schema directory: {}",
                requested.display()
            )
            .into());
        }
        state.load_path(&canonical).map_err(Into::into)
    }
}

struct CachedSchema {
    canonical_path: PathBuf,
    dependencies: Vec<FileStamp>,
    validator: Validator,
}

#[derive(Default)]
pub struct SchemaEngine {
    cached: Option<CachedSchema>,
    limits: SchemaLimits,
}

impl SchemaEngine {
    #[cfg(test)]
    fn with_limits(limits: SchemaLimits) -> Self {
        Self {
            cached: None,
            limits,
        }
    }

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
        Some(
            match self.validate_inner(schema_path, instance_json, latest_generation, generation) {
                Ok(state) => state,
                Err(error) => SchemaState::LoadError(error),
            },
        )
    }

    fn validate_inner(
        &mut self,
        schema_path: &Path,
        instance_json: &str,
        latest_generation: &Arc<AtomicU64>,
        generation: u64,
    ) -> Result<SchemaState, String> {
        let canonical_schema = fs::canonicalize(schema_path)
            .map_err(|error| format!("unable to open schema {}: {error}", schema_path.display()))?;
        let instance: Value = serde_json::from_str(instance_json)
            .map_err(|error| format!("formatted preview is not strict JSON: {error}"))?;

        let cache_is_current = self.cached.as_ref().is_some_and(|cached| {
            cached.canonical_path == canonical_schema
                && dependencies_are_current(&cached.dependencies, latest_generation, generation)
        });
        if is_cancelled(latest_generation, generation) {
            return Err("schema validation was cancelled".to_string());
        }
        if !cache_is_current {
            self.cached = None;
            let cached = self.compile(
                canonical_schema.clone(),
                latest_generation.clone(),
                generation,
            )?;
            self.cached = Some(cached);
        }
        if is_cancelled(latest_generation, generation) {
            return Err("schema validation was cancelled".to_string());
        }

        let validator = &self.cached.as_ref().expect("cache was populated").validator;
        let mut issues = Vec::new();
        issues
            .try_reserve(jello::MAX_DIAGNOSTICS)
            .map_err(|_| "allocation failed while collecting schema issues".to_string())?;
        for error in validator
            .iter_errors(&instance)
            .take(jello::MAX_DIAGNOSTICS)
        {
            if is_cancelled(latest_generation, generation) {
                return Err("schema validation was cancelled".to_string());
            }
            issues.push(SchemaIssue {
                message: error.to_string(),
                instance_path: error.instance_path().as_str().to_string(),
                schema_path: error.schema_path().as_str().to_string(),
            });
        }

        if issues.is_empty() {
            Ok(SchemaState::Valid)
        } else {
            Ok(SchemaState::Invalid(issues))
        }
    }

    fn compile(
        &self,
        canonical_schema: PathBuf,
        latest_generation: Arc<AtomicU64>,
        generation: u64,
    ) -> Result<CachedSchema, String> {
        let root = canonical_schema
            .parent()
            .ok_or_else(|| "schema path has no parent directory".to_string())?
            .to_path_buf();
        let state = Arc::new(Mutex::new(LoadState {
            root,
            files: HashMap::new(),
            total_bytes: 0,
            limits: self.limits,
            latest_generation,
            generation,
        }));
        let schema = state
            .lock()
            .map_err(|_| "schema loader state is unavailable".to_string())?
            .load_path(&canonical_schema)?;
        let base_uri = Url::from_file_path(&canonical_schema)
            .map_err(|_| {
                format!(
                    "unable to convert schema path to a file URL: {}",
                    canonical_schema.display()
                )
            })?
            .to_string();
        let validator = jsonschema::draft202012::options()
            .with_base_uri(base_uri)
            .with_retriever(LocalSchemaRetriever {
                state: state.clone(),
            })
            .build(&schema)
            .map_err(|error| format!("unable to compile schema: {error}"))?;
        let dependencies = state
            .lock()
            .map_err(|_| "schema loader state is unavailable".to_string())?
            .stamps();
        Ok(CachedSchema {
            canonical_path: canonical_schema,
            dependencies,
            validator,
        })
    }
}

#[cfg(test)]
pub fn validate(schema_path: &Path, instance_json: &str) -> SchemaState {
    let latest_generation = Arc::new(AtomicU64::new(0));
    SchemaEngine::default()
        .validate(schema_path, instance_json, &latest_generation, 0)
        .unwrap_or(SchemaState::NotLoaded)
}

fn dependencies_are_current(
    dependencies: &[FileStamp],
    latest_generation: &Arc<AtomicU64>,
    generation: u64,
) -> bool {
    dependencies.iter().all(|stamp| {
        if is_cancelled(latest_generation, generation) {
            return false;
        }
        let Ok(canonical) = fs::canonicalize(&stamp.path) else {
            return false;
        };
        if canonical != stamp.path {
            return false;
        }
        let Ok(source) = jello::read_utf8_file_stable(&stamp.path) else {
            return false;
        };
        source.len() == stamp.bytes && fingerprint(source.as_bytes()) == stamp.fingerprint
    })
}

fn is_cancelled(latest_generation: &Arc<AtomicU64>, generation: u64) -> bool {
    latest_generation.load(Ordering::Acquire) != generation
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod budget_tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    use super::{SchemaEngine, SchemaLimits, SchemaState};

    fn temp_directory(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("jello-schema-budget-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn schema_file_count_budget_includes_the_root_schema() {
        let directory = temp_directory("files");
        let root = directory.join("root.json");
        fs::write(&root, r#"{"$ref":"child.json"}"#).unwrap();
        fs::write(directory.join("child.json"), r#"{"type":"string"}"#).unwrap();
        let mut engine = SchemaEngine::with_limits(SchemaLimits {
            max_files: 1,
            max_total_bytes: 1024,
        });
        let latest = Arc::new(AtomicU64::new(1));

        let state = engine.validate(&root, r#""value""#, &latest, 1).unwrap();
        fs::remove_dir_all(directory).unwrap();

        let SchemaState::LoadError(message) = state else {
            panic!("the second schema file must exceed the file budget");
        };
        assert!(message.contains("file count limit"), "{message}");
    }

    #[test]
    fn aggregate_schema_byte_budget_is_enforced() {
        let directory = temp_directory("bytes");
        let root = directory.join("root.json");
        fs::write(&root, r#"{"type":"string"}"#).unwrap();
        let mut engine = SchemaEngine::with_limits(SchemaLimits {
            max_files: 2,
            max_total_bytes: 4,
        });
        let latest = Arc::new(AtomicU64::new(1));

        let state = engine.validate(&root, r#""value""#, &latest, 1).unwrap();
        fs::remove_dir_all(directory).unwrap();

        let SchemaState::LoadError(message) = state else {
            panic!("the root schema must exceed the aggregate byte budget");
        };
        assert!(message.contains("total byte limit"), "{message}");
    }

    #[test]
    fn cached_validator_is_recompiled_when_a_dependency_changes() {
        let directory = temp_directory("cache");
        let root = directory.join("root.json");
        fs::write(&root, r#"{"$ref":"child.json"}"#).unwrap();
        let child = directory.join("child.json");
        fs::write(&child, r#"{"type":"string"}"#).unwrap();
        let mut engine = SchemaEngine::default();
        let latest = Arc::new(AtomicU64::new(1));

        assert!(matches!(
            engine.validate(&root, r#""value""#, &latest, 1),
            Some(SchemaState::Valid)
        ));
        fs::write(&child, r#"{"type":"number"}"#).unwrap();
        assert!(matches!(
            engine.validate(&root, r#""value""#, &latest, 1),
            Some(SchemaState::Invalid(_))
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cancelled_schema_work_returns_no_state() {
        let directory = temp_directory("cancelled");
        let root = directory.join("root.json");
        fs::write(&root, r#"{"type":"string"}"#).unwrap();
        let mut engine = SchemaEngine::default();
        let latest = Arc::new(AtomicU64::new(2));

        assert!(engine.validate(&root, r#""value""#, &latest, 1).is_none());
        fs::remove_dir_all(directory).unwrap();
    }
}
