//! Optional JSON Schema Draft 2020-12 validation (feature `schema`).
//!
//! Relative references are confined to the schema directory and network
//! references are disabled. Validators are cached per schema path and
//! recompiled when any dependency's content changes.

use std::collections::{hash_map::DefaultHasher, HashMap};
use std::error::Error;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use jsonschema::{Retrieve, Uri, Validator};
use serde_json::Value;
use url::Url;

pub const MAX_SCHEMA_FILES: usize = 64;
pub const MAX_SCHEMA_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SCHEMA_NODES: usize = 100_000;
pub const MAX_SCHEMA_INSTANCE_NODES: usize = 1_000_000;

type CancelCheck = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    pub message: String,
    pub instance_path: String,
    pub schema_path: String,
}

#[derive(Debug, Clone, Copy)]
struct SchemaLimits {
    max_files: usize,
    max_total_bytes: usize,
    max_schema_nodes: usize,
    max_instance_nodes: usize,
}

impl Default for SchemaLimits {
    fn default() -> Self {
        Self {
            max_files: MAX_SCHEMA_FILES,
            max_total_bytes: MAX_SCHEMA_TOTAL_BYTES,
            max_schema_nodes: MAX_SCHEMA_NODES,
            max_instance_nodes: MAX_SCHEMA_INSTANCE_NODES,
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

struct LoadState {
    root: PathBuf,
    files: HashMap<PathBuf, LoadedSchema>,
    total_bytes: usize,
    schema_nodes: usize,
    limits: SchemaLimits,
    is_cancelled: CancelCheck,
}

impl std::fmt::Debug for LoadState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadState")
            .field("root", &self.root)
            .field("files", &self.files)
            .field("total_bytes", &self.total_bytes)
            .field("schema_nodes", &self.schema_nodes)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl LoadState {
    fn load_path(&mut self, path: &Path) -> Result<Value, String> {
        if (self.is_cancelled)() {
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

        let source = crate::input::read_utf8_file_stable(path)
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
        if (self.is_cancelled)() {
            return Err("schema validation was cancelled".to_string());
        }

        let value: Value = serde_json::from_str(&source)
            .map_err(|error| format!("schema {} is not valid JSON: {error}", path.display()))?;
        let remaining_nodes = self
            .limits
            .max_schema_nodes
            .saturating_sub(self.schema_nodes);
        let node_count = value_node_count_up_to(&value, remaining_nodes).ok_or_else(|| {
            format!(
                "schema node limit exceeded (maximum {})",
                self.limits.max_schema_nodes
            )
        })?;
        if (self.is_cancelled)() {
            return Err("schema validation was cancelled".to_string());
        }
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
        self.schema_nodes += node_count;
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
        // Component-wise containment: `Path::starts_with` compares path
        // components, so a sibling directory whose name merely begins with
        // the schema directory (for example `schemas-evil`) is rejected.
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
pub struct SchemaValidator {
    cached: Option<CachedSchema>,
    limits: SchemaLimits,
}

impl SchemaValidator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate `instance_json` against the schema at `schema_path`.
    ///
    /// Returns the list of violations, or a load/compile error message.
    pub fn validate(
        &mut self,
        schema_path: &Path,
        instance_json: &str,
    ) -> Result<Vec<SchemaIssue>, String> {
        self.validate_with_cancel(schema_path, instance_json, || false)?
            .ok_or_else(|| "schema validation was unexpectedly cancelled".to_string())
    }

    /// Validate while periodically consulting `is_cancelled`.
    ///
    /// Returns `Ok(None)` when cancellation is observed. The callback is
    /// checked between file reads, cache validation, compilation, and issue
    /// collection so callers can discard superseded work without reporting a
    /// schema error.
    pub fn validate_with_cancel<F>(
        &mut self,
        schema_path: &Path,
        instance_json: &str,
        is_cancelled: F,
    ) -> Result<Option<Vec<SchemaIssue>>, String>
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        let is_cancelled: CancelCheck = Arc::new(is_cancelled);
        if is_cancelled() {
            return Ok(None);
        }
        match self.validate_inner(schema_path, instance_json, &is_cancelled) {
            Ok(_) if is_cancelled() => Ok(None),
            Ok(issues) => Ok(Some(issues)),
            Err(_) if is_cancelled() => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn validate_inner(
        &mut self,
        schema_path: &Path,
        instance_json: &str,
        is_cancelled: &CancelCheck,
    ) -> Result<Vec<SchemaIssue>, String> {
        let canonical_schema = fs::canonicalize(schema_path)
            .map_err(|error| format!("unable to open schema {}: {error}", schema_path.display()))?;
        if is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }
        let instance: Value = serde_json::from_str(instance_json)
            .map_err(|error| format!("formatted preview is not strict JSON: {error}"))?;
        if value_node_count_up_to(&instance, self.limits.max_instance_nodes).is_none() {
            return Err(format!(
                "schema instance node limit exceeded (maximum {})",
                self.limits.max_instance_nodes
            ));
        }
        if is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }

        let cache_is_current = self.cached.as_ref().is_some_and(|cached| {
            cached.canonical_path == canonical_schema
                && dependencies_are_current(&cached.dependencies, is_cancelled)
        });
        if is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }
        if !cache_is_current {
            self.cached = None;
            let cached = self.compile(canonical_schema.clone(), is_cancelled.clone())?;
            self.cached = Some(cached);
        }
        if is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }

        let validator = &self.cached.as_ref().expect("cache was populated").validator;
        let mut issues = Vec::new();
        issues
            .try_reserve(crate::lexer::MAX_DIAGNOSTICS)
            .map_err(|_| "allocation failed while collecting schema issues".to_string())?;
        for error in validator
            .iter_errors(&instance)
            .take(crate::lexer::MAX_DIAGNOSTICS)
        {
            if is_cancelled() {
                return Err("schema validation was cancelled".to_string());
            }
            issues.push(SchemaIssue {
                message: error.to_string(),
                instance_path: error.instance_path().as_str().to_string(),
                schema_path: error.schema_path().as_str().to_string(),
            });
        }
        Ok(issues)
    }

    fn compile(
        &self,
        canonical_schema: PathBuf,
        is_cancelled: CancelCheck,
    ) -> Result<CachedSchema, String> {
        let root = canonical_schema
            .parent()
            .ok_or_else(|| "schema path has no parent directory".to_string())?
            .to_path_buf();
        let state = Arc::new(Mutex::new(LoadState {
            root,
            files: HashMap::new(),
            total_bytes: 0,
            schema_nodes: 0,
            limits: self.limits,
            is_cancelled: is_cancelled.clone(),
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
        if is_cancelled() {
            return Err("schema validation was cancelled".to_string());
        }
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

fn dependencies_are_current(dependencies: &[FileStamp], is_cancelled: &CancelCheck) -> bool {
    dependencies.iter().all(|stamp| {
        if is_cancelled() {
            return false;
        }
        let Ok(canonical) = fs::canonicalize(&stamp.path) else {
            return false;
        };
        if canonical != stamp.path {
            return false;
        }
        let Ok(source) = crate::input::read_utf8_file_stable(&stamp.path) else {
            return false;
        };
        source.len() == stamp.bytes && fingerprint(source.as_bytes()) == stamp.fingerprint
    })
}

fn value_node_count_up_to(value: &Value, limit: usize) -> Option<usize> {
    fn visit(value: &Value, count: &mut usize, limit: usize) -> bool {
        if *count >= limit {
            return false;
        }
        *count += 1;
        match value {
            Value::Array(values) => values.iter().all(|value| visit(value, count, limit)),
            Value::Object(values) => values.values().all(|value| visit(value, count, limit)),
            _ => true,
        }
    }

    let mut count = 0;
    visit(value, &mut count, limit).then_some(count)
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "jello-schema-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn draft_2020_12_reports_instance_and_schema_paths() {
        let directory = TestDirectory::new("invalid-instance");
        let schema = directory.path().join("schema.json");
        fs::write(
            &schema,
            r#"{
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }"#,
        )
        .unwrap();

        let issues = SchemaValidator::new()
            .validate(&schema, r#"{"name": 42}"#)
            .unwrap();

        assert_eq!(issues[0].instance_path, "/name");
        assert_eq!(issues[0].schema_path, "/properties/name/type");
    }

    #[test]
    fn relative_refs_inside_schema_directory_are_supported() {
        let directory = TestDirectory::new("child-ref");
        let child_dir = directory.path().join("defs");
        fs::create_dir(&child_dir).unwrap();
        fs::write(
            child_dir.join("name.json"),
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"string"}"#,
        )
        .unwrap();
        let schema = directory.path().join("schema.json");
        fs::write(
            &schema,
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"defs/name.json"}"#,
        )
        .unwrap();

        let mut validator = SchemaValidator::new();
        assert!(validator.validate(&schema, r#""Ada""#).unwrap().is_empty());
        assert!(!validator.validate(&schema, "42").unwrap().is_empty());
    }

    #[test]
    fn parent_directory_ref_is_rejected() {
        let directory = TestDirectory::new("parent-ref");
        let schema_dir = directory.path().join("schemas");
        fs::create_dir(&schema_dir).unwrap();
        fs::write(
            directory.path().join("outside.json"),
            r#"{"type":"string"}"#,
        )
        .unwrap();
        let schema = schema_dir.join("schema.json");
        fs::write(&schema, r#"{"$ref":"../outside.json"}"#).unwrap();

        let error = SchemaValidator::new()
            .validate(&schema, r#""Ada""#)
            .unwrap_err();
        assert!(error.contains("outside the schema directory"), "{error}");
    }

    #[test]
    fn network_ref_is_rejected() {
        let directory = TestDirectory::new("network-ref");
        let schema = directory.path().join("schema.json");
        fs::write(&schema, r#"{"$ref":"https://example.com/schema.json"}"#).unwrap();

        let error = SchemaValidator::new()
            .validate(&schema, r#""Ada""#)
            .unwrap_err();
        assert!(error.contains("network references are disabled"), "{error}");
    }

    #[test]
    fn sibling_directory_with_a_prefix_name_is_rejected() {
        let directory = TestDirectory::new("prefix-sibling");
        let schema_dir = directory.path().join("schemas");
        fs::create_dir(&schema_dir).unwrap();
        let evil_dir = directory.path().join("schemas-evil");
        fs::create_dir(&evil_dir).unwrap();
        fs::write(evil_dir.join("outside.json"), r#"{"type":"string"}"#).unwrap();
        let schema = schema_dir.join("schema.json");
        fs::write(&schema, r#"{"$ref":"../schemas-evil/outside.json"}"#).unwrap();

        let error = SchemaValidator::new()
            .validate(&schema, r#""Ada""#)
            .unwrap_err();
        assert!(error.contains("outside the schema directory"), "{error}");
    }

    #[test]
    fn cached_validator_is_recompiled_when_a_dependency_changes() {
        let directory = TestDirectory::new("cache");
        let root = directory.path().join("root.json");
        fs::write(&root, r#"{"$ref":"child.json"}"#).unwrap();
        let child = directory.path().join("child.json");
        fs::write(&child, r#"{"type":"string"}"#).unwrap();
        let mut validator = SchemaValidator::new();

        assert!(validator.validate(&root, r#""value""#).unwrap().is_empty());
        fs::write(&child, r#"{"type":"number"}"#).unwrap();
        assert!(!validator.validate(&root, r#""value""#).unwrap().is_empty());
    }

    #[test]
    fn schema_file_count_budget_includes_the_root_schema() {
        let directory = TestDirectory::new("files");
        let root = directory.path().join("root.json");
        fs::write(&root, r#"{"$ref":"child.json"}"#).unwrap();
        fs::write(directory.path().join("child.json"), r#"{"type":"string"}"#).unwrap();
        let mut validator = SchemaValidator {
            cached: None,
            limits: SchemaLimits {
                max_files: 1,
                max_total_bytes: 1024,
                ..SchemaLimits::default()
            },
        };

        let error = validator.validate(&root, r#""value""#).unwrap_err();
        assert!(error.contains("file count limit"), "{error}");
    }

    #[test]
    fn aggregate_schema_byte_budget_is_enforced() {
        let directory = TestDirectory::new("bytes");
        let root = directory.path().join("root.json");
        fs::write(&root, r#"{"type":"string"}"#).unwrap();
        let mut validator = SchemaValidator {
            cached: None,
            limits: SchemaLimits {
                max_files: 2,
                max_total_bytes: 4,
                ..SchemaLimits::default()
            },
        };

        let error = validator.validate(&root, r#""value""#).unwrap_err();
        assert!(error.contains("total byte limit"), "{error}");
    }

    #[test]
    fn cancellable_validation_stops_after_work_has_started() {
        let directory = TestDirectory::new("cancelled-after-start");
        let schema = directory.path().join("schema.json");
        fs::write(&schema, r#"{"type":"string"}"#).unwrap();
        let checks = Arc::new(AtomicUsize::new(0));
        let cancellation_checks = checks.clone();

        let result = SchemaValidator::new()
            .validate_with_cancel(&schema, r#""value""#, move || {
                cancellation_checks.fetch_add(1, Ordering::AcqRel) > 0
            })
            .unwrap();

        assert!(result.is_none());
        assert!(checks.load(Ordering::Acquire) >= 2);
    }

    #[test]
    fn instance_node_budget_is_enforced_before_validation() {
        let directory = TestDirectory::new("instance-nodes");
        let schema = directory.path().join("schema.json");
        fs::write(&schema, "{}").unwrap();
        let mut validator = SchemaValidator {
            cached: None,
            limits: SchemaLimits {
                max_files: 1,
                max_total_bytes: 1024,
                max_schema_nodes: 1024,
                max_instance_nodes: 2,
            },
        };

        let error = validator.validate(&schema, "[1, 2]").unwrap_err();

        assert!(error.contains("instance node limit"), "{error}");
    }

    #[test]
    fn aggregate_schema_node_budget_is_enforced_before_compilation() {
        let directory = TestDirectory::new("schema-nodes");
        let schema = directory.path().join("schema.json");
        fs::write(&schema, r#"{"allOf":[{},{}]}"#).unwrap();
        let mut validator = SchemaValidator {
            cached: None,
            limits: SchemaLimits {
                max_files: 1,
                max_total_bytes: 1024,
                max_schema_nodes: 3,
                max_instance_nodes: 1024,
            },
        };

        let error = validator.validate(&schema, "null").unwrap_err();

        assert!(error.contains("schema node limit"), "{error}");
    }
}
