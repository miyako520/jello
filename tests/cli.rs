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
        child
            .stdin
            .take()
            .expect("stdin was not piped")
            .write_all(input)
            .expect("failed to write stdin");
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
fn version_reports_package_version() {
    let output = jello()
        .arg("--version")
        .output()
        .expect("failed to run jello");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout was not UTF-8"),
        "jello 0.1.0\n"
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

#[cfg(unix)]
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
