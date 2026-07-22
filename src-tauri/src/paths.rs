//! Shared filesystem paths for app-wide and per-project data.

use std::path::PathBuf;

/// App data directory (`$XDG_DATA_HOME/book-converter` or `~/.local/share/book-converter`).
pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("book-converter")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_dir_ends_with_book_converter() {
        let p = app_data_dir();
        assert_eq!(p.file_name().and_then(|s| s.to_str()), Some("book-converter"));
    }
}
