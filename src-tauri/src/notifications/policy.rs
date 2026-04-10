//! Policy layer: decides whether an incoming notification fans out to the OS.
//!
//! All fan-out decisions live here so the rest of the service doesn't have to
//! know about focus, settings, or per-source rules. Inputs:
//!   - `NotificationLevel` — drives the default priority
//!   - `NotificationSource` — lets specific sources force/opt-out
//!   - `window_focused` — if the app is focused, low-signal notifications are
//!     suppressed (you already see the in-app pane); high-signal ones still
//!     fire because the whole point is "get my attention now".
//!   - `settings.notifications_enabled` — global kill switch
//!
//! Rules (v1.1):
//!   1. Kill switch off → never fan out.
//!   2. Severity `Attention | Warning | Error` → always fan out, even when
//!      focused. These are things the user wants to know about immediately,
//!      and macOS handles Do-Not-Disturb at the OS level.
//!   3. Severity `Info | Success` from `Watch` → fan out when unfocused
//!      (preserves the pre-service behavior). Suppress when focused because
//!      the flash animation + badge are already visible.
//!   4. Everything else → in-app only.

use roux_core::{NotificationLevel, NotificationSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyInput<'a> {
    pub level: NotificationLevel,
    pub source: &'a NotificationSource,
    pub window_focused: bool,
    pub notifications_enabled: bool,
}

/// Returns true when the caller should fire an OS notification via
/// `tauri-plugin-notification`.
pub fn should_fan_out_to_os(input: PolicyInput<'_>) -> bool {
    if !input.notifications_enabled {
        return false;
    }

    match input.level {
        NotificationLevel::Attention
        | NotificationLevel::Warning
        | NotificationLevel::Error => true,
        NotificationLevel::Info | NotificationLevel::Success => {
            matches!(input.source, NotificationSource::Watch { .. })
                && !input.window_focused
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        level: NotificationLevel,
        source: NotificationSource,
        focused: bool,
        enabled: bool,
    ) -> bool {
        should_fan_out_to_os(PolicyInput {
            level,
            source: &source,
            window_focused: focused,
            notifications_enabled: enabled,
        })
    }

    #[test]
    fn kill_switch_suppresses_everything() {
        assert!(!input(
            NotificationLevel::Error,
            NotificationSource::Internal,
            false,
            false,
        ));
    }

    #[test]
    fn error_fires_even_when_focused() {
        assert!(input(
            NotificationLevel::Error,
            NotificationSource::Internal,
            true,
            true,
        ));
    }

    #[test]
    fn attention_fires_even_when_focused() {
        assert!(input(
            NotificationLevel::Attention,
            NotificationSource::Hook { provider: "claude".into() },
            true,
            true,
        ));
    }

    #[test]
    fn watch_success_suppressed_when_focused() {
        assert!(!input(
            NotificationLevel::Success,
            NotificationSource::Watch { watch_id: "w1".into() },
            true,
            true,
        ));
    }

    #[test]
    fn attention_fires_when_unfocused() {
        assert!(input(
            NotificationLevel::Attention,
            NotificationSource::Hook { provider: "claude".into() },
            false,
            true,
        ));
    }

    #[test]
    fn error_fires_when_unfocused() {
        assert!(input(
            NotificationLevel::Error,
            NotificationSource::Cli,
            false,
            true,
        ));
    }

    #[test]
    fn info_from_random_source_does_not_fire() {
        assert!(!input(
            NotificationLevel::Info,
            NotificationSource::Cli,
            false,
            true,
        ));
    }

    #[test]
    fn success_from_watch_fires_for_backwards_compat() {
        assert!(input(
            NotificationLevel::Success,
            NotificationSource::Watch { watch_id: "w1".into() },
            false,
            true,
        ));
    }

    #[test]
    fn info_from_osc_does_not_fire() {
        assert!(!input(
            NotificationLevel::Info,
            NotificationSource::Osc { code: 9, sender_id: None },
            false,
            true,
        ));
    }
}
