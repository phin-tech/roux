//! Categorical event store with mailbox + bus usage patterns.
//!
//! One append-only `Event` log with multi-axis addressing (`to` for
//! direct mail, `topic` for broadcast, plus `kind`, `correlation_id`,
//! `project_id`, `from`). Per-recipient read/ack state lives in
//! `ReadState` so a single event can serve N consumers without
//! duplicating the payload.
//!
//! - Mailbox view  ⇒ filter by `to=<alias>` + recipient read state
//! - Bus view      ⇒ filter by `topic` (exact match in Phase 2)
//! - Firehose view ⇒ no filter
//!
//! See `/docs/features/mailbox.md` for usage. Mirrors the
//! `notifications` and `aliases` module split: `store` is in-memory and
//! persistence/Tauri-free; `manager` (added next) layers persistence and
//! event emission on top.

pub mod manager;
pub mod persistence;
pub mod store;

pub use manager::{MailboxManager, MAILBOX_EVENT};
pub use persistence::{
    append_event, events_path, load_events, load_read_state, read_state_path, save_read_state,
};
pub use store::{EventStore, PostError};
