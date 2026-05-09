//! Persistent bus subscriptions.
//!
//! An alias subscribes to a topic glob (`repo-a.*`, `**.completed`); the
//! mailbox layer delivers matching events into the subscriber's inbox.
//! Mirrors the `aliases` module split: `store` is in-memory and pure,
//! `manager` owns persistence and Tauri event emission.
//!
//! See `/docs/features/mailbox.md` for the user-facing surface and
//! [#161](https://github.com/phin-tech/roux/issues/161) for the design
//! motivation.

pub mod manager;
pub mod persistence;
pub mod store;

pub use manager::{SubscriptionManager, SUBSCRIPTION_EVENT};
pub use persistence::{load_subscriptions, persistence_path, save_subscriptions};
pub use store::{AddError, SubscriptionStore};
