//! Tauri commands — the bridge between the React frontend and the Rust core.
//!
//! Session state (the current book's DB path, the loaded reference, the running
//! flag / cancel signal) lives in a managed `AppState`. Long translation runs on
//! a dedicated OS thread with its own current-thread runtime — this keeps the
//! non-`Sync` SQLite connection off the async executor — and streams progress to
//! the UI via `progress` / `done` / `job_error` events.

mod util;
mod settings;
mod project;
mod reference;
mod translation;
mod glossary;
mod export_cmd;
mod reader;

pub use crate::session::{cleanup_legacy_data, AppState};
pub use settings::*;
pub use project::*;
pub use reference::*;
pub use translation::*;
pub use glossary::*;
pub use export_cmd::*;
pub use reader::*;
