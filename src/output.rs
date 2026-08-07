use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};

#[derive(Debug)]
pub struct SavedOutput {
    pub path: PathBuf,
    pub cleanup_warning: Option<CleanupWarning>,
}

#[derive(Debug)]
pub struct CleanupWarning {
    pub path: PathBuf,
    pub error: io::Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EasyPublish {
    Linked,
    #[cfg(windows)]
    Moved,
}

pub fn save_fixed(source: &Path, contents: &[u8]) -> io::Result<SavedOutput> {
    write_easy_output_with(
        source,
        contents,
        |file, contents| file.write_all(contents),
        |path| fs::remove_file(path),
    )
}

/// Atomically replace `path` with `contents`, refusing to touch the file when
/// its on-disk content no longer matches `expected_contents`.
///
/// Unlike [`save_fixed`], this never creates a new file; callers should only
/// use it for files this program wrote earlier in the same session.
pub fn save_updated(path: &Path, expected_contents: &[u8], contents: &[u8]) -> io::Result<()> {
    verify_file_content_matches(path, expected_contents)?;
    let metadata = fs::metadata(path)?;
    let (temporary_path, mut temporary) = create_temporary_sibling(path, "update")?;
    let write_result = (|| {
        temporary.set_permissions(metadata.permissions())?;
        temporary.write_all(contents)?;
        temporary.flush()?;
        temporary.sync_all()
    })();
    drop(temporary);

    if let Err(error) = write_result {
        return Err(remove_temporary(&temporary_path, error));
    }
    verify_file_content_matches(path, expected_contents)?;
    match fs::rename(&temporary_path, path) {
        Ok(()) => Ok(()),
        Err(error) => Err(remove_temporary(&temporary_path, error)),
    }
}

fn verify_file_content_matches(path: &Path, expected: &[u8]) -> io::Result<()> {
    if fs::read(path)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "the file changed after it was opened; reopen it before saving",
        ));
    }
    Ok(())
}

pub fn save_as_new(source: &Path, destination: &Path, contents: &[u8]) -> io::Result<SavedOutput> {
    write_output_with_publish(
        source,
        destination,
        contents,
        |file, contents| file.write_all(contents),
        |path| fs::remove_file(path),
        publish_easy_candidate,
        |_| destination.to_path_buf(),
        1,
        "destination already exists",
    )
}

pub(crate) fn write_easy_output_with<F, R>(
    path: &Path,
    contents: &[u8],
    write_contents: F,
    remove_published_temporary: R,
) -> io::Result<SavedOutput>
where
    F: FnOnce(&mut File, &[u8]) -> io::Result<()>,
    R: FnOnce(&Path) -> io::Result<()>,
{
    write_easy_output_with_publish(
        path,
        contents,
        write_contents,
        remove_published_temporary,
        publish_easy_candidate,
    )
}

pub(crate) fn write_easy_output_with_publish<F, R, P>(
    path: &Path,
    contents: &[u8],
    write_contents: F,
    remove_published_temporary: R,
    publish_candidate: P,
) -> io::Result<SavedOutput>
where
    F: FnOnce(&mut File, &[u8]) -> io::Result<()>,
    R: FnOnce(&Path) -> io::Result<()>,
    P: FnMut(&Path, &Path) -> io::Result<EasyPublish>,
{
    write_output_with_publish(
        path,
        path,
        contents,
        write_contents,
        remove_published_temporary,
        publish_candidate,
        |attempt| easy_output_path(path, attempt),
        1000,
        "could not reserve an easy-mode output file name",
    )
}

#[allow(clippy::too_many_arguments)]
fn write_output_with_publish<F, R, P, C>(
    source: &Path,
    temporary_anchor: &Path,
    contents: &[u8],
    write_contents: F,
    remove_published_temporary: R,
    mut publish_candidate: P,
    candidate_for_attempt: C,
    max_attempts: usize,
    exhausted_message: &'static str,
) -> io::Result<SavedOutput>
where
    F: FnOnce(&mut File, &[u8]) -> io::Result<()>,
    R: FnOnce(&Path) -> io::Result<()>,
    P: FnMut(&Path, &Path) -> io::Result<EasyPublish>,
    C: Fn(usize) -> PathBuf,
{
    let permissions = fs::metadata(source)?.permissions();
    let (temporary_path, mut temporary) = create_temporary_sibling(temporary_anchor, "easy")?;
    let prepare_result = (|| {
        temporary.set_permissions(permissions)?;
        write_contents(&mut temporary, contents)?;
        temporary.flush()?;
        temporary.sync_all()
    })();
    drop(temporary);

    if let Err(error) = prepare_result {
        return Err(remove_temporary(&temporary_path, error));
    }

    for attempt in 0..max_attempts {
        let candidate = candidate_for_attempt(attempt);
        match publish_candidate(&temporary_path, &candidate) {
            Ok(publish) => {
                let cleanup_warning =
                    match publish {
                        EasyPublish::Linked => remove_published_temporary(&temporary_path)
                            .err()
                            .map(|error| CleanupWarning {
                                path: temporary_path,
                                error,
                            }),
                        #[cfg(windows)]
                        EasyPublish::Moved => None,
                    };
                return Ok(SavedOutput {
                    path: candidate,
                    cleanup_warning,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(remove_temporary(&temporary_path, error)),
        }
    }

    Err(remove_temporary(
        &temporary_path,
        io::Error::new(io::ErrorKind::AlreadyExists, exhausted_message),
    ))
}

fn remove_temporary(path: &Path, error: io::Error) -> io::Error {
    match fs::remove_file(path) {
        Ok(()) => error,
        Err(cleanup_error) => io::Error::new(
            error.kind(),
            format!(
                "{error}; failed to remove temporary file {:?}: {cleanup_error}",
                path
            ),
        ),
    }
}

pub(crate) fn easy_output_path(path: &Path, attempt: usize) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = OsString::from(
        path.file_stem()
            .or_else(|| path.file_name())
            .unwrap_or_else(|| OsStr::new("output")),
    );
    name.push(".fixed");
    if attempt > 0 {
        name.push(format!("-{}", attempt + 1));
    }
    if let Some(extension) = path.extension().filter(|extension| !extension.is_empty()) {
        name.push(".");
        name.push(extension);
    }
    parent.join(name)
}

#[cfg(not(windows))]
fn publish_easy_candidate(temporary: &Path, candidate: &Path) -> io::Result<EasyPublish> {
    fs::hard_link(temporary, candidate)?;
    Ok(EasyPublish::Linked)
}

#[cfg(windows)]
fn publish_easy_candidate(temporary: &Path, candidate: &Path) -> io::Result<EasyPublish> {
    publish_easy_candidate_with(
        temporary,
        candidate,
        |from, to| fs::hard_link(from, to),
        windows_move_no_replace,
    )
}

#[cfg(windows)]
pub(crate) fn publish_easy_candidate_with<H, M>(
    temporary: &Path,
    candidate: &Path,
    hard_link: H,
    move_no_replace: M,
) -> io::Result<EasyPublish>
where
    H: FnOnce(&Path, &Path) -> io::Result<()>,
    M: FnOnce(&Path, &Path) -> io::Result<()>,
{
    match hard_link(temporary, candidate) {
        Ok(()) => Ok(EasyPublish::Linked),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(error),
        Err(link_error) => match move_no_replace(temporary, candidate) {
            Ok(()) => Ok(EasyPublish::Moved),
            Err(move_error) => Err(io::Error::new(
                move_error.kind(),
                format!(
                    "hard-link publish failed: {link_error}; no-replace move failed: {move_error}"
                ),
            )),
        },
    }
}

fn create_temporary_sibling(path: &Path, suffix: &str) -> io::Result<(PathBuf, File)> {
    for attempt in 0..1000 {
        let candidate = sibling_path(path, suffix, attempt);
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
        "could not reserve a temporary file name",
    ))
}

fn sibling_path(path: &Path, suffix: &str, attempt: usize) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_else(|| OsStr::new("input")));
    name.push(format!(
        ".jello.{}.{}.{}",
        std::process::id(),
        attempt,
        suffix
    ));
    parent.join(name)
}

#[cfg(windows)]
fn other_io_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, message)
}

#[cfg(windows)]
fn windows_move_no_replace(temporary: &Path, candidate: &Path) -> io::Result<()> {
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    const ERROR_FILE_EXISTS: i32 = 80;
    const ERROR_ALREADY_EXISTS: i32 = 183;

    let temporary = windows_full_path(temporary)?;
    let candidate_name = candidate.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "output path has no file name")
    })?;
    let candidate = temporary
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "temporary path has no parent"))?
        .join(candidate_name);
    let temporary = windows_verbatim_path(&temporary)?;
    let candidate = windows_verbatim_path(&candidate)?;

    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            candidate.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved != 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    if matches!(
        error.raw_os_error(),
        Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS)
    ) {
        Err(io::Error::new(io::ErrorKind::AlreadyExists, error))
    } else {
        Err(error)
    }
}

#[cfg(windows)]
fn windows_full_path(path: &Path) -> io::Result<PathBuf> {
    let input = windows_null_terminated(path.as_os_str().encode_wide())?;
    let mut required = unsafe {
        GetFullPathNameW(
            input.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if required == 0 {
        return Err(io::Error::last_os_error());
    }

    loop {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(required as usize)
            .map_err(|_| other_io_error("allocation failed while resolving a Windows path"))?;
        buffer.resize(required as usize, 0_u16);
        let length = unsafe {
            GetFullPathNameW(
                input.as_ptr(),
                required,
                buffer.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length < required {
            buffer.truncate(length as usize);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        required = length;
    }
}

#[cfg(windows)]
fn windows_verbatim_path(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let verbatim: Vec<u16> = "\\\\?\\".encode_utf16().collect();
    let device: Vec<u16> = "\\\\.\\".encode_utf16().collect();
    let unc: Vec<u16> = "\\\\".encode_utf16().collect();
    let mut output = Vec::new();
    if wide.starts_with(&verbatim) || wide.starts_with(&device) {
        output
            .try_reserve_exact(wide.len() + 1)
            .map_err(|_| other_io_error("allocation failed while preparing a Windows path"))?;
        output.extend_from_slice(&wide);
    } else if wide.starts_with(&unc) {
        let prefix: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
        output
            .try_reserve_exact(prefix.len() + wide.len() - 2 + 1)
            .map_err(|_| other_io_error("allocation failed while preparing a Windows path"))?;
        output.extend_from_slice(&prefix);
        output.extend_from_slice(&wide[2..]);
    } else {
        output
            .try_reserve_exact(verbatim.len() + wide.len() + 1)
            .map_err(|_| other_io_error("allocation failed while preparing a Windows path"))?;
        output.extend_from_slice(&verbatim);
        output.extend_from_slice(&wide);
    }
    output.push(0);
    Ok(output)
}

#[cfg(windows)]
fn windows_null_terminated<I>(wide: I) -> io::Result<Vec<u16>>
where
    I: IntoIterator<Item = u16>,
{
    let mut wide: Vec<u16> = wide.into_iter().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows path contains a null character",
        ));
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    fn GetFullPathNameW(
        file_name: *const u16,
        buffer_length: u32,
        buffer: *mut u16,
        file_part: *mut *mut u16,
    ) -> u32;
}
