//! Persisted per-user configuration shared by the CLI helper and the desktop
//! application. The only current setting is the UI language, stored as a
//! single `language=en|zh` line under the user's local app data directory.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::diagnostic::Language;

/// Read the persisted language setting. A missing file yields `None`;
/// unexpected content is an error so callers can fall back with a warning.
pub fn load_language_config(path: &Path) -> io::Result<Option<Language>> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match source.trim() {
        "language=en" => Ok(Some(Language::En)),
        "language=zh" => Ok(Some(Language::Zh)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "language config must contain `language=en` or `language=zh`",
        )),
    }
}

/// Atomically replace the language setting, creating parent directories as
/// needed. Falls back through a sibling backup when the target cannot be
/// renamed over directly.
pub fn save_language_config(path: &Path, language: Language) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let (temporary_path, mut temporary) = create_config_temporary(path)?;
    let value = match language {
        Language::En => b"language=en\n".as_slice(),
        Language::Zh => b"language=zh\n".as_slice(),
    };
    let write_result = (|| {
        temporary.write_all(value)?;
        temporary.flush()?;
        temporary.sync_all()
    })();
    drop(temporary);
    if let Err(error) = write_result {
        return Err(remove_config_temporary(&temporary_path, error));
    }

    match fs::rename(&temporary_path, path) {
        Ok(()) => Ok(()),
        Err(first_error) if path.exists() => {
            let backup_path = config_sibling_path(path, "bak");
            if backup_path.exists() {
                fs::remove_file(&temporary_path)?;
                return Err(first_error);
            }
            fs::rename(path, &backup_path)?;
            match fs::rename(&temporary_path, path) {
                Ok(()) => {
                    fs::remove_file(&backup_path).map_err(|cleanup_error| {
                        io::Error::new(
                            cleanup_error.kind(),
                            format!(
                                "language setting was saved, but its backup `{}` could not be removed: {cleanup_error}",
                                backup_path.display()
                            ),
                        )
                    })
                }
                Err(error) => Err(rollback_config_replacement(
                    path,
                    &backup_path,
                    &temporary_path,
                    error,
                )),
            }
        }
        Err(error) => Err(remove_config_temporary(&temporary_path, error)),
    }
}

fn remove_config_temporary(path: &Path, primary_error: io::Error) -> io::Error {
    match fs::remove_file(path) {
        Ok(()) => primary_error,
        Err(cleanup_error) if cleanup_error.kind() == io::ErrorKind::NotFound => primary_error,
        Err(cleanup_error) => io::Error::new(
            primary_error.kind(),
            format!(
                "{primary_error}; temporary language config `{}` could not be removed: {cleanup_error}",
                path.display()
            ),
        ),
    }
}

fn rollback_config_replacement(
    path: &Path,
    backup_path: &Path,
    temporary_path: &Path,
    install_error: io::Error,
) -> io::Error {
    match fs::rename(backup_path, path) {
        Ok(()) => remove_config_temporary(temporary_path, install_error),
        Err(rollback_error) => io::Error::new(
            install_error.kind(),
            format!(
                "failed to install language config: {install_error}; rollback failed: {rollback_error}; the original remains at `{}` and the replacement remains at `{}`",
                backup_path.display(),
                temporary_path.display()
            ),
        ),
    }
}

fn create_config_temporary(path: &Path) -> io::Result<(PathBuf, File)> {
    for attempt in 0..1000 {
        let candidate = config_sibling_path(path, &format!("{attempt}.tmp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a language config temporary file",
    ))
}

fn config_sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".jello-drop.{}.{}", std::process::id(), suffix));
    parent.join(name)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "jello-config-{}-{nonce}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn missing_config_yields_none() {
        let directory = temporary_directory("missing");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("config");

        assert_eq!(load_language_config(&path).unwrap(), None);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn language_config_round_trips_chinese() {
        let directory = temporary_directory("saved-language");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("config");

        save_language_config(&path, Language::Zh).unwrap();

        assert_eq!(load_language_config(&path).unwrap(), Some(Language::Zh));
        assert_eq!(fs::read_to_string(&path).unwrap(), "language=zh\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_language_config_is_reported() {
        let directory = temporary_directory("corrupt-language");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("config");
        fs::write(&path, "language=maybe\n").unwrap();

        let error = load_language_config(&path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let directory = temporary_directory("deep-parent");
        let path = directory.join("Jello").join("config");

        save_language_config(&path, Language::En).unwrap();

        assert_eq!(load_language_config(&path).unwrap(), Some(Language::En));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollback_failure_reports_preserved_config_paths() {
        let directory = temporary_directory("rollback-failure");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("config");
        let backup_path = directory.join("config.backup");
        let temporary_path = directory.join("config.temporary");
        fs::create_dir(&path).unwrap();
        fs::write(&backup_path, "language=en\n").unwrap();
        fs::write(&temporary_path, "language=zh\n").unwrap();

        let error = rollback_config_replacement(
            &path,
            &backup_path,
            &temporary_path,
            io::Error::other("install failed"),
        );
        let message = error.to_string();

        assert!(message.contains("rollback failed"), "{message}");
        assert!(
            message.contains(&backup_path.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains(&temporary_path.display().to_string()),
            "{message}"
        );
        assert!(backup_path.exists());
        assert!(temporary_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
