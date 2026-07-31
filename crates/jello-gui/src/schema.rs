pub use crate::schema_engine::{SchemaState, validate};

#[cfg(test)]
mod tests {
    use super::{SchemaState, validate};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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

        let state = validate(&schema, r#"{"name": 42}"#);

        let SchemaState::Invalid(issues) = state else {
            panic!("instance should fail schema validation");
        };
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

        assert!(matches!(validate(&schema, r#""Ada""#), SchemaState::Valid));
        assert!(matches!(validate(&schema, "42"), SchemaState::Invalid(_)));
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

        let SchemaState::LoadError(message) = validate(&schema, r#""Ada""#) else {
            panic!("parent reference should be rejected");
        };
        assert!(
            message.contains("outside the schema directory"),
            "{message}"
        );
    }

    #[test]
    fn network_ref_is_rejected() {
        let directory = TestDirectory::new("network-ref");
        let schema = directory.path().join("schema.json");
        fs::write(&schema, r#"{"$ref":"https://example.com/schema.json"}"#).unwrap();

        let SchemaState::LoadError(message) = validate(&schema, r#""Ada""#) else {
            panic!("network reference should be rejected");
        };
        assert!(
            message.contains("network references are disabled"),
            "{message}"
        );
    }
}
