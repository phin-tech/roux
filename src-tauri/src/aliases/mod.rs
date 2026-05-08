//! Agent alias service — stable, restart-durable identity for sessions.
//!
//! Aliases are human-meaningful labels (e.g. `reviewer`, `frontend`) that
//! resolve to whichever session currently holds them. They survive session
//! restart so cross-session references don't rot when UUIDs change.
//!
//! Mirrors the `notifications` module split: `store` holds the in-memory
//! state and is persistence/Tauri-free; `manager` (added next) owns the
//! `Arc<Mutex<Store>>`, JSON persistence, and event emission.

pub mod manager;
pub mod persistence;
pub mod store;

pub use manager::{AliasManager, ALIAS_EVENT};
pub use persistence::{load_aliases, persistence_path, save_aliases};
pub use store::{AliasStore, BindError, BindRequest, ProjectFilter};
