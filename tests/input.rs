use std::io;

#[test]
fn stable_reader_accepts_utf8_within_the_input_limit() {
    let path = std::env::temp_dir().join(format!(
        "jello-stable-input-{}-valid.json",
        std::process::id()
    ));
    std::fs::write(&path, "{\"name\":\"小林\"}").unwrap();

    let source = jello::read_utf8_file_stable(&path).unwrap();
    std::fs::remove_file(path).unwrap();

    assert_eq!(source, "{\"name\":\"小林\"}");
}

#[test]
fn stable_reader_rejects_invalid_utf8_as_invalid_data() {
    let path = std::env::temp_dir().join(format!(
        "jello-stable-input-{}-invalid.json",
        std::process::id()
    ));
    std::fs::write(&path, [0xff, 0xfe]).unwrap();

    let error = jello::read_utf8_file_stable(&path).unwrap_err();
    std::fs::remove_file(path).unwrap();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
