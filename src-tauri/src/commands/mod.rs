//! Typed IPC adapters for application services and app-wide settings.

mod assistant_v1;
mod book_v1;
mod manga_v1;
mod models;
mod project_v1;
mod settings;
mod shared_v1;

// Re-export command macro companions for generate_handler!.
pub use assistant_v1::*;
pub use book_v1::*;
pub use manga_v1::*;
pub use models::*;
pub use project_v1::*;
pub use settings::*;
pub use shared_v1::*;
