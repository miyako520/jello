#![cfg(feature = "windows-drop")]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn drop_binary_processes_a_file_and_preserves_the_source() {
    let directory = temporary_directory("binary");
    fs::create_dir(&directory).unwrap();
    let source_path = directory.join("data.json");
    let original = "{name:'Ada',items:[1,2,]}";
    fs::write(&source_path, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_jello-drop"))
        .env("LOCALAPPDATA", &directory)
        .arg(&source_path)
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&source_path).unwrap(), original);
    assert_eq!(
        fs::read_to_string(directory.join("data.fixed.json")).unwrap(),
        "{\n  \"name\": \"Ada\",\n  \"items\": [\n    1,\n    2\n  ]\n}\n"
    );
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Success"));
    fs::remove_dir_all(directory).unwrap();
}

fn temporary_directory(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "jello-drop-cli-{}-{nonce}-{name}",
        std::process::id()
    ))
}
