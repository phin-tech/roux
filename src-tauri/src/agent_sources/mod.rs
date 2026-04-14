//! Event sources that feed the agent FSM registry.
//!
//! Each source owns its own transport (file watcher, session lifecycle
//! observer, future socket listener, etc.) and translates raw events
//! into `AgentInput` values that share a single mpsc channel into the
//! registry worker. Adding a source means writing one module that holds
//! a `Sender<AgentInput>` and calls `.send(...)` — there's no trait to
//! implement because the channel itself is the pluggability boundary.

pub mod file_status;
pub mod humanize;
pub mod notification_sink;
