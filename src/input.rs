use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::MAX_INPUT_BYTES;

/// Read a UTF-8 file without accepting content that changes between two
/// consecutive bounded reads.
pub fn read_utf8_file_stable(path: &Path) -> io::Result<String> {
    read_utf8_file_stable_with(path, || {})
}

fn read_utf8_file_stable_with(path: &Path, between_reads: impl FnOnce()) -> io::Result<String> {
    let first = read_bytes_limited(path, MAX_INPUT_BYTES)?;
    between_reads();
    let second = read_bytes_limited(path, MAX_INPUT_BYTES)?;
    if first != second {
        return Err(io::Error::other(
            "file changed while it was being read; try again",
        ));
    }
    String::from_utf8(first)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "input is not valid UTF-8"))
}

fn read_bytes_limited(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > limit as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input exceeds the {limit} byte limit"),
        ));
    }

    let capacity = limit
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "input limit is too large"))?;
    let read_limit = u64::try_from(capacity)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "input limit is too large"))?;
    let initial_capacity = usize::try_from(metadata.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "input size is too large"))?
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "input size is too large"))?
        .min(capacity);
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(initial_capacity)
        .map_err(|_| io::Error::other("allocation failed while reading input"))?;
    file.take(read_limit).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("input exceeds the {limit} byte limit"),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{read_bytes_limited, read_utf8_file_stable_with};

    #[test]
    fn small_file_does_not_retain_the_full_input_limit_capacity() {
        let path = std::env::temp_dir().join(format!(
            "jello-stable-input-{}-small-capacity.json",
            std::process::id()
        ));
        std::fs::write(&path, "{}").unwrap();

        let bytes = read_bytes_limited(&path, crate::MAX_INPUT_BYTES).unwrap();

        assert_eq!(bytes, b"{}");
        assert!(
            bytes.capacity() < 64 * 1024,
            "capacity was {}",
            bytes.capacity()
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn changing_a_file_between_reads_is_rejected() {
        let path = std::env::temp_dir().join(format!(
            "jello-stable-input-{}-changed.json",
            std::process::id()
        ));
        std::fs::write(&path, "{}").unwrap();

        let error = read_utf8_file_stable_with(&path, || {
            std::fs::write(&path, "[]").unwrap();
        })
        .unwrap_err();
        std::fs::remove_file(path).unwrap();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("changed while"));
    }
}
