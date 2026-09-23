//! Tauri commands — the bridge between the React frontend and the Rust core.
//!
//! Session state (the current book's DB path, the loaded reference, the running
//! flag / cancel signal) lives in a managed `AppState`. Long translation runs on
//! a dedicated OS thread with its own current-thread runtime — this keeps the
//! non-`Sync` SQLite connection off the async executor — and streams progress to
//! the UI via `progress` / `done` / `job_error` events.

mod assistant;
mod export_cmd;
mod glossary;
pub(crate) mod ops;
mod project;
mod project_v1;
mod reader;
mod reference;
mod settings;
mod translation;
mod util;

pub use crate::session::{cleanup_legacy_data, AppState};
// Tauri's command macro generates hidden companion symbols next to each
// function; wildcard re-exports intentionally carry those symbols to the flat
// namespace consumed by `generate_handler!`.
pub use assistant::*;
pub use export_cmd::*;
pub use glossary::*;
pub use project::*;
pub use project_v1::*;
pub use reader::*;
pub use reference::*;
pub use settings::*;
pub use translation::*;
