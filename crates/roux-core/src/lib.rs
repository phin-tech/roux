//! Shared types and models for Roux.
//!
//! This crate contains the data types that cross the IPC boundary between
//! the Rust backend and the TypeScript frontend. It has no dependency on
//! Tauri, so it can be used by the CLI, tests, and future tooling.

pub mod agent_fsm;
pub mod models;
pub mod smolvm;
pub mod worktree;

pub use models::*;
pub use smolvm::{SmolMachine, SmolMachineCreateRequest, SmolvmDetection, SmolvmError};
pub use worktree::{
    create_worktree, create_worktree_with_provider, expand_base_template, fetch_origin,
    list_worktrees, list_worktrees_enriched, preview_worktree_base, remove_worktree,
    remove_worktree_with_provider, WorktreeError,
};
