//! Shared filesystem paths for app-wide and per-project data.

use std::path::PathBuf;

/// App data directory (`$XDG_DATA_HOME/book-converter` or `~/.local/share/book-converter`).
pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("book-converter")
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid_project_id")]
pub(crate) struct InvalidProjectId;

/// Accept a single safe project directory component, including obsolete IDs
/// inspected only by the explicit one-shot reset utility.
pub(crate) fn validate_project_id(id: &str) -> Result<(), InvalidProjectId> {
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    valid.then_some(()).ok_or(InvalidProjectId)
}

pub(crate) fn project_dir(id: &str) -> Result<PathBuf, InvalidProjectId> {
    validate_project_id(id)?;
    Ok(app_data_dir().join("projects").join(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_dir_ends_with_book_converter() {
        let p = app_data_dir();
        assert_eq!(
            p.file_name().and_then(|s| s.to_str()),
            Some("book-converter")
        );
    }
    #[test]
    fn project_ids_cannot_escape_the_project_directory() {
        for invalid in ["", ".", "..", "../outside", "a/b", "a\\b", "/tmp", "a%2fb"] {
            assert!(project_dir(invalid).is_err(), "{invalid}");
        }
        assert!(project_dir(&"x".repeat(129)).is_err());
        assert!(project_dir("legacy-test_1")
            .unwrap()
            .ends_with("projects/legacy-test_1"));
    }
}
