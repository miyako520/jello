use std::fs;
use std::io;
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
            "jello-output-{name}-{}-{nonce}",
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
fn save_as_new_refuses_to_replace_an_existing_file() {
    let directory = TestDirectory::new("save-as-existing");
    let source = directory.path().join("source.json");
    let destination = directory.path().join("chosen.json");
    fs::write(&source, b"{\"source\":true}").unwrap();
    fs::write(&destination, b"keep me").unwrap();

    let error = jello::save_as_new(&source, &destination, b"{\"fixed\":true}\n").unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&destination).unwrap(), b"keep me");
}

#[test]
fn save_fixed_uses_the_next_available_numbered_name() {
    let directory = TestDirectory::new("numbered");
    let source = directory.path().join("data.json");
    let occupied = directory.path().join("data.fixed.json");
    fs::write(&source, b"{\"source\":true}").unwrap();
    fs::write(&occupied, b"occupied").unwrap();

    let saved = jello::save_fixed(&source, b"{\"fixed\":true}\n").unwrap();

    assert_eq!(saved.path, directory.path().join("data.fixed-2.json"));
    assert_eq!(fs::read(saved.path).unwrap(), b"{\"fixed\":true}\n");
    assert_eq!(fs::read(occupied).unwrap(), b"occupied");
}
