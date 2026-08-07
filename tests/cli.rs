use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn jello() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jello"))
}

fn run(args: &[&str], input: Option<&str>) -> Output {
    run_bytes(args, input.map(str::as_bytes))
}

fn run_bytes(args: &[&str], input: Option<&[u8]>) -> Output {
    let mut command = jello();
    command
        .env_remove("NO_COLOR")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().expect("failed to spawn jello");
    if let Some(input) = input {
        let result = child
            .stdin
            .take()
            .expect("stdin was not piped")
            .write_all(input);
        if let Err(error) = result {
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe,
                "failed to write stdin"
            );
        }
    }
    child.wait_with_output().expect("failed to wait for jello")
}

fn unique_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("jello-{}-{}-{}", std::process::id(), nonce, name))
}

#[test]
fn invalid_utf8_is_invalid_content() {
    let output = run_bytes(&[], Some(&[0xFF]));

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("error[E015]"));
}

#[test]
fn diagnostics_do_not_emit_untrusted_terminal_controls() {
    let output = run(&["--color", "never"], Some("\u{1b}]52;c;VEVTVA==\u{7}{}"));

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(r"\u{001B}"));
    assert!(stderr.contains(r"\u{0007}"));
    assert!(stderr.chars().all(|ch| ch == '\n' || !ch.is_control()));
}

#[test]
fn version_reports_package_version() {
    let output = jello()
        .arg("--version")
        .output()
        .expect("failed to run jello");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout was not UTF-8"),
        format!("jello {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn formats_stdin_and_reports_invalid_input() {
    let formatted = run(&[], Some(r#"{"a":[1,true]}"#));
    assert!(formatted.status.success());
    assert_eq!(
        String::from_utf8(formatted.stdout).unwrap(),
        "{\n  \"a\": [\n    1,\n    true\n  ]\n}\n"
    );

    let invalid = run(&[], Some(r#"{"a" 1}"#));
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8(invalid.stderr)
        .unwrap()
        .contains("error[E006]"));
}

#[test]
fn check_is_quiet_and_uses_exit_status() {
    let canonical = "{\n  \"a\": 1\n}\n";
    let clean = run(&["--check"], Some(canonical));
    assert!(clean.status.success());
    assert!(clean.stdout.is_empty());

    let dirty = run(&["--check"], Some(r#"{"a":1}"#));
    assert_eq!(dirty.status.code(), Some(1));
    assert!(dirty.stdout.is_empty());
}

#[test]
fn writes_formatted_file_and_rejects_write_without_path() {
    let path = unique_path("write.json");
    fs::write(&path, r#"{"a":1}"#).unwrap();

    let output = run(&["--write", path.to_str().unwrap()], None);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{\n  \"a\": 1\n}\n");
    fs::remove_file(path).unwrap();

    let missing = run(&["--write"], None);
    assert_eq!(missing.status.code(), Some(2));
}

#[test]
fn easy_mode_repairs_prints_and_saves_without_touching_source() {
    let directory = unique_path("easy");
    fs::create_dir(&directory).unwrap();
    let source_path = directory.join("data.json");
    let output_path = directory.join("data.fixed.json");
    let original = "{note:'ok',items:[1,2,]}";
    fs::write(&source_path, original).unwrap();

    let output = jello().arg("easy").arg(&source_path).output().unwrap();

    let expected = "{\n  \"note\": \"ok\",\n  \"items\": [\n    1,\n    2\n  ]\n}\n";
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert_eq!(fs::read_to_string(&source_path).unwrap(), original);
    assert_eq!(fs::read_to_string(&output_path).unwrap(), expected);
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("saved formatted output"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn easy_mode_uses_numbered_output_and_writes_nothing_for_unrepairable_input() {
    let directory = unique_path("easy-collision");
    fs::create_dir(&directory).unwrap();
    let source_path = directory.join("data.json");
    fs::write(&source_path, "{}").unwrap();
    fs::write(directory.join("data.fixed.json"), "keep").unwrap();

    let output = jello().arg("easy").arg(&source_path).output().unwrap();

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(directory.join("data.fixed.json")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(directory.join("data.fixed-2.json")).unwrap(),
        "{}\n"
    );

    let invalid_path = directory.join("invalid.json");
    fs::write(&invalid_path, "{a:}").unwrap();
    let invalid = jello().arg("easy").arg(&invalid_path).output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(!directory.join("invalid.fixed.json").exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn easy_mode_requires_an_input_path() {
    let output = run(&["easy"], None);

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("`easy` requires an input path"));
}

#[test]
fn supports_compact_and_custom_indentation() {
    let compact = run(&["--compact"], Some(r#"{"a":[1]}"#));
    assert!(compact.status.success());
    assert_eq!(String::from_utf8(compact.stdout).unwrap(), "{\"a\":[1]}\n");

    let indented = run(&["--indent", "4"], Some(r#"{"a":[1]}"#));
    assert!(indented.status.success());
    assert_eq!(
        String::from_utf8(indented.stdout).unwrap(),
        "{\n    \"a\": [\n        1\n    ]\n}\n"
    );

    let conflict = run(&["--compact", "--indent", "4"], Some("{}"));
    assert_eq!(conflict.status.code(), Some(2));
}

#[test]
fn double_dash_allows_paths_beginning_with_hyphen() {
    let directory = unique_path("dash-path");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("--data.json"), r#"{"a":1}"#).unwrap();

    let output = jello()
        .current_dir(&directory)
        .args(["--", "--data.json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\n  \"a\": 1\n}\n"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn validates_option_conflicts_and_values() {
    for args in [
        vec!["--check", "--write"],
        vec!["--indent", "17"],
        vec!["--indent"],
    ] {
        let output = run(&args, None);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn reads_non_unicode_path_without_panicking() {
    use std::os::unix::ffi::OsStringExt;

    let directory = unique_path("non-unicode-path");
    fs::create_dir(&directory).unwrap();
    let filename = std::ffi::OsString::from_vec(b"data-\xFF.json".to_vec());
    let path = directory.join(filename);
    fs::write(&path, r#"{"a":1}"#).unwrap();

    let output = jello()
        .arg(&path)
        .output()
        .expect("failed to run jello with a non-Unicode path");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\n  \"a\": 1\n}\n"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn chinese_language_localizes_argument_errors_in_any_order() {
    for args in [["--unknown", "--lang", "zh"], ["--lang", "zh", "--unknown"]] {
        let output = run(&args, None);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("参数错误：未知选项"));
    }
}

#[test]
fn chinese_language_localizes_check_failure() {
    let output = run(&["--lang", "zh", "--check"], Some("{}"));

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("检查：输入尚未格式化"));
}

#[test]
fn chinese_language_localizes_repair_records() {
    let output = run(&["--lang", "zh", "--fix", "--json5"], Some("{'a':1}"));

    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("修复[F001]"));
}

#[test]
fn argument_errors_honor_explicit_color_in_any_order() {
    for args in [
        ["--color", "always", "--unknown"],
        ["--unknown", "--color", "always"],
    ] {
        let output = run(&args, None);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("\u{1b}["));
    }
}

#[test]
fn help_has_order_independent_priority() {
    for args in [
        vec!["--help", "--unknown"],
        vec!["--unknown", "--help"],
        vec!["--check", "--write", "--help"],
    ] {
        let output = run(&args, None);
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("USAGE:"));
    }
}

#[test]
fn fix_requires_json5_for_json5_lexical_syntax() {
    let strict = run(&["--fix"], Some("{a: 0x10}"));
    assert_eq!(strict.status.code(), Some(1));

    let json5 = run(&["--fix", "--json5"], Some("{a:/* comment */0x10}"));
    assert!(json5.status.success());
    let stderr = String::from_utf8(json5.stderr).unwrap();
    assert!(stderr.contains("fix[F002]"));
    assert!(stderr.contains("fix[F005]"));
}

#[cfg(unix)]
#[test]
fn write_refuses_symbolic_links() {
    use std::os::unix::fs::symlink;

    let directory = unique_path("symlink-write");
    fs::create_dir(&directory).unwrap();
    let target = directory.join("target.json");
    let link = directory.join("link.json");
    fs::write(&target, r#"{"a":1}"#).unwrap();
    symlink(&target, &link).unwrap();

    let output = jello().args(["--write"]).arg(&link).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read_to_string(&target).unwrap(), r#"{"a":1}"#);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn diff_mode_prints_a_unified_diff_instead_of_json() {
    let output = run(&["--fix", "--diff", "--json5"], Some("[1,]"));

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("@@ -1,1 +1,3 @@"));
    assert!(stdout.contains("- [1,]"));
    assert!(stdout.contains("+ ["));
    assert!(!stdout.contains("[\n  1\n]"));
}

#[test]
fn diff_mode_requires_fix() {
    let output = run(&["--diff"], Some("{}"));

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("--diff requires --fix"));
}

#[test]
fn diff_mode_rejects_check_and_write_combinations() {
    let path = unique_path("diff-check.json");
    fs::write(&path, "{}").unwrap();
    let output = run(
        &["--fix", "--diff", "--check", path.to_str().unwrap()],
        None,
    );
    fs::remove_file(path).unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn diff_mode_is_supported_by_easy() {
    let path = unique_path("diff-easy.json");
    fs::write(&path, "[1,]").unwrap();
    let output = run(&["easy", "--diff", path.to_str().unwrap()], None);
    fs::remove_file(path).unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("@@ -1,1 +1,3 @@"));
    assert!(stdout.contains("- [1,]"));
    assert!(stdout.contains("+ ["));
}

#[cfg(feature = "schema")]
#[test]
fn schema_mode_validates_the_formatted_output() {
    let schema = unique_path("schema-valid.json");
    fs::write(
        &schema,
        r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#,
    )
    .unwrap();
    let output = run(
        &["--schema", schema.to_str().unwrap()],
        Some(r#"{"name":"Ada"}"#),
    );
    fs::remove_file(schema).unwrap();

    assert_eq!(output.status.code(), Some(0));
}

#[cfg(feature = "schema")]
#[test]
fn schema_violations_exit_one_with_issue_paths() {
    let schema = unique_path("schema-violation.json");
    fs::write(
        &schema,
        r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#,
    )
    .unwrap();
    let output = run(
        &["--schema", schema.to_str().unwrap()],
        Some(r#"{"name":42}"#),
    );
    fs::remove_file(schema).unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("/name"), "{stderr}");
}

#[cfg(feature = "schema")]
#[test]
fn missing_schema_file_exits_two() {
    let output = run(
        &["--schema", "does-not-exist-schema.json"],
        Some(r#"{"name":"Ada"}"#),
    );

    assert_eq!(output.status.code(), Some(2));
}

#[cfg(feature = "schema")]
#[test]
fn schema_mode_validates_fixed_output() {
    let schema = unique_path("schema-fix.json");
    fs::write(&schema, r#"{"type":"array","items":{"type":"integer"}}"#).unwrap();
    let output = run(
        &["--fix", "--schema", schema.to_str().unwrap()],
        Some("[1 2]"),
    );
    fs::remove_file(schema).unwrap();

    assert_eq!(output.status.code(), Some(0));
}

#[cfg(not(feature = "schema"))]
#[test]
fn schema_mode_reports_an_unavailable_build() {
    let output = run(&["--schema", "x.json"], Some("{}"));

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("without the schema feature"));
}
