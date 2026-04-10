//! Policy layer: decides whether an incoming notification fans out to the OS.
//!
//! All fan-out decisions live here so the rest of the service doesn't have to
//! know about focus, settings, or per-source rules. Inputs:
//!   - `NotificationLevel` — drives the default priority
//!   - `NotificationSource` — lets specific sources force/opt-out
//!   - `window_focused` — if the app is focused, suppress entirely (the user
//!     will see the in-app pane without needing an OS alert)
//!   - `settings.notifications_enabled` — global kill switch
//!
//! Rules (v1):
//!   1. If the kill switch is off → never fan out.
//!   2. If the window is focused → never fan out (in-app is enough).
//!   3. Severity `Attention | Warning | Error` → fan out.
//!   4. Severity `Info | Success` → fan out only for sources that explicitly
//!      want it. Today only `Watch` (watches already had desktop notifications
//!      before the service existed — preserve behavior).

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
    if input.window_focused {
        return false;
    }

    match input.level {
        NotificationLevel::Attention
        | NotificationLevel::Warning
        | NotificationLevel::Error => true,
        NotificationLevel::Info | NotificationLevel::Success => {
            matches!(input.source, NotificationSource::Watch { .. })
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
    fn focused_suppresses_everything() {
        assert!(!input(
            NotificationLevel::Error,
            NotificationSource::Internal,
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
