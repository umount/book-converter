//! Serving a project's extracted images to the webview over `bookasset://`.
//!
//! An illustration can be megabytes; sending it through the IPC bridge as a `data:`
//! URL (the way the book cover travels) would JSON-encode every page the reader
//! scrolls past. A URI scheme lets the webview fetch and cache the file itself.
//!
//! Tauri's built-in `asset://` is configured with glob scopes in
//! `tauri.conf.json`, which cannot name this app's data directory portably
//! (`XDG_DATA_HOME/book-converter`, see `paths`). Owning the scheme keeps the
//! same project-id validation the commands use, and nothing outside a project's
//! `assets/` directory is reachable.

pub mod store;

use std::path::{Path, PathBuf};

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext};

use crate::book::source::{image_mime, percent_decode};
use crate::paths::project_dir;

/// The scheme the frontend builds its image URLs with.
pub(crate) const SCHEME: &str = "bookasset";

/// Answer one `bookasset://localhost/<project_id>/<file>` request.
pub(crate) fn respond<R: Runtime>(
    _ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    match resolve(request.uri().path()) {
        Some(path) => match store::AssetStore::read_path(&path) {
            Ok(bytes) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, image_mime(&path.to_string_lossy()))
                .header(header::CONTENT_LENGTH, bytes.len())
                // The file name is a content hash, so a cached copy can never
                // be stale.
                .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
                .body(bytes)
                .unwrap_or_else(|_| not_found()),
            Err(_) => not_found(),
        },
        None => not_found(),
    }
}

/// Map a request path to a file inside a project's `assets/` directory.
///
/// `None` for anything that is not a plain asset of a valid project: the
/// webview's URL is untrusted input, and the whole point of resolving here is
/// that `..` or an absolute path cannot reach the rest of the disk.
fn resolve(path: &str) -> Option<PathBuf> {
    // `convertFileSrc` encodes the whole path as one component, so the
    // separator arrives as `%2F`.
    let decoded = percent_decode(path.trim_start_matches('/'));
    let mut parts = decoded.split('/').filter(|p| !p.is_empty());
    let project_id = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let dir = project_dir(project_id).ok()?;
    resolve_name(&dir, name)
}

fn resolve_name(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        let db = rusqlite::Connection::open_with_flags(
            dir.join("project.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()?;
        let relative: String = db
            .query_row(
                "SELECT relative_path FROM assets WHERE id=?1",
                [name],
                |r| r.get(0),
            )
            .ok()?;
        let filename = relative.strip_prefix("assets/")?;
        if !filename.starts_with(&format!("{name}.")) || !is_asset_name(filename) {
            return None;
        }
        return Some(dir.join(store::DIRECTORY).join(filename));
    }
    if !is_asset_name(name) {
        return None;
    }
    Some(dir.join(store::DIRECTORY).join(name))
}

/// Asset file names are `<content hash>.<ext>` and nothing else — no separators,
/// no dots leading anywhere.
fn is_asset_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let stem = parts.next().unwrap_or("");
    let ext = parts.next().unwrap_or("");
    parts.next().is_none()
        && !stem.is_empty()
        && !ext.is_empty()
        && stem.bytes().all(|b| b.is_ascii_alphanumeric())
        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Vec::new())
        .expect("a 404 with an empty body is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_project_asset() {
        let path = resolve("/p-123%2Fab12cd34.jpg").expect("resolved");
        assert!(path.ends_with("projects/p-123/assets/ab12cd34.jpg"));
        // An unencoded separator works just as well.
        assert_eq!(resolve("/p-123/ab12cd34.jpg"), Some(path));
    }

    #[test]
    fn bare_asset_ids_require_a_registered_safe_filename() {
        let root = std::env::temp_dir().join(format!("asset-resolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let db = rusqlite::Connection::open(root.join("project.db")).unwrap();
        db.execute_batch("CREATE TABLE assets(id TEXT PRIMARY KEY, relative_path TEXT)")
            .unwrap();
        let id = "a".repeat(64);
        assert_eq!(resolve_name(&root, &id), None);
        db.execute(
            "INSERT INTO assets VALUES(?1,?2)",
            [&id, &format!("assets/{id}.png")],
        )
        .unwrap();
        assert_eq!(
            resolve_name(&root, &id),
            Some(root.join(format!("assets/{id}.png")))
        );
        for unsafe_path in [
            format!("assets/{id}.png/../../secret"),
            "../secret.png".into(),
            format!("assets/{}.png", "b".repeat(64)),
        ] {
            db.execute("UPDATE assets SET relative_path=?1", [unsafe_path])
                .unwrap();
            assert_eq!(resolve_name(&root, &id), None);
        }
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The URL comes from the webview, so traversal must not resolve at all.
    #[test]
    fn refuses_anything_outside_a_project_assets_directory() {
        for path in [
            "/",
            "/p-123",
            "/p-123%2F..%2F..%2Fprogress.db",
            "/p-123%2Fsub%2Fa.jpg",
            "/..%2Fab12.jpg",
            "/nested%2Fproject%2Fa.jpg",
            "/p-123%2F.env",
            "/p-123%2Fa%20b.jpg",
            "/p-123%2Fab12.tar.gz",
        ] {
            assert_eq!(resolve(path), None, "{path}");
        }
    }

    /// A request that resolves to a file that is not there must answer, not
    /// panic: the reader asks for every page it scrolls past.
    #[test]
    fn a_missing_file_answers_404() {
        let path = resolve("/p-missing%2Fdeadbeef.jpg").expect("resolved");
        assert!(!path.exists());
        assert_eq!(not_found().status(), StatusCode::NOT_FOUND);
    }
}
