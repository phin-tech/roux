//! Provider profile registry.
//!
//! The built-in [`SpawnProfile`] definitions (claude / codex / plain-shell)
//! now live in `roux_core::providers` so the daemon can resolve and run them
//! headlessly (work-item dispatch). This module re-exports them so existing
//! desktop callers (`crate::providers::builtin_profiles`) are unchanged, and
//! remains the home for desktop-only provider concerns (hook install, status
//! payload parsing) as those land.
//!
//! User-defined profiles are data, not code: they live in
//! `RouxSettings.spawn_profiles` and cannot contribute hook install logic or
//! payload parsing. This is the deliberate line that keeps provider work
//! gated on Rust code review.

pub use roux_core::providers::builtin_profiles;
