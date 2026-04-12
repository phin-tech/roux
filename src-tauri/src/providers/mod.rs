//! Provider modules — one per first-class agent Roux knows how to light
//! up UI for. A provider does three things:
//!
//! 1. Installs its hooks into the agent's config (phase 2+ tracks this via
//!    `hooks.rs`; per-provider install logic lands in later phases).
//! 2. Normalizes provider-specific hook payloads into the generic
//!    [`crate::status_watcher::StatusUpdate`] shape (phase 2 lives in
//!    [`crate::status_watcher::parse_status_payload`]; richer parsing moves
//!    into per-provider modules alongside Codex support).
//! 3. Contributes one or more built-in [`SpawnProfile`]s via
//!    [`default_profiles`](claude::default_profiles), derived from current
//!    `RouxSettings`. The registry assembled here is the source of truth for
//!    built-in profiles the frontend can offer to users.
//!
//! User-defined profiles are data, not code: they live in
//! `RouxSettings.spawn_profiles` and cannot contribute hook install logic or
//! payload parsing. This is the deliberate line that keeps provider work
//! gated on Rust code review.

pub mod claude;
pub mod codex;

use roux_core::{ProfileSource, SpawnProfile};

use crate::settings::RouxSettings;

/// Assemble the built-in profile registry: one or more profiles from each
/// provider module plus the catch-all "Plain shell" option. Called from the
/// `get_builtin_profiles` Tauri command at frontend startup.
///
/// Ordering matches display order. `Plain shell` goes last so users see
/// named agents first when scanning the picker.
pub fn builtin_profiles(settings: &RouxSettings) -> Vec<SpawnProfile> {
    let mut profiles = Vec::new();
    profiles.extend(claude::default_profiles(settings));
    profiles.extend(codex::default_profiles(settings));
    profiles.push(plain_shell_profile());
    profiles
}

fn plain_shell_profile() -> SpawnProfile {
    SpawnProfile {
        id: "plain-shell".into(),
        name: "Plain shell".into(),
        setup_command: None,
        startup_command: None,
        startup_behavior: None,
        env: None,
        cwd_override: None,
        icon: None,
        provider: None,
        nono_profile: None,
        nono_allow_dirs: None,
        source: ProfileSource::Builtin,
    }
}

/// Shell-quote a single token using POSIX single-quote rules. Safe for use
/// inside a shell string that will be handed to `sh -c` or typed into a
/// shell prompt. Kept provider-module-private because the quoting rules are
/// intentionally minimal: we assume the user's setting is a single argument,
/// not a compound shell expression.
pub(crate) fn shell_quote(token: &str) -> String {
    if token.is_empty() {
        return "''".into();
    }
    if token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '='))
    {
        return token.to_string();
    }
    let mut out = String::with_capacity(token.len() + 2);
    out.push('\'');
    for c in token.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_passes_bare_identifiers() {
        assert_eq!(shell_quote("claude"), "claude");
        assert_eq!(shell_quote("/usr/local/bin/claude"), "/usr/local/bin/claude");
        assert_eq!(shell_quote("opus-4.6"), "opus-4.6");
    }

    #[test]
    fn shell_quote_wraps_spaces_and_special_chars() {
        assert_eq!(shell_quote("has spaces"), "'has spaces'");
        assert_eq!(shell_quote("pipe|redirect"), "'pipe|redirect'");
    }

    #[test]
    fn shell_quote_escapes_embedded_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_quote_empty_becomes_empty_pair() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn builtin_profiles_contains_claude_codex_and_plain_shell() {
        let settings = RouxSettings::default();
        let profiles = builtin_profiles(&settings);
        let ids: Vec<&str> = profiles.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"claude"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"plain-shell"));
        // Plain shell is last so named agents rank above the fallback.
        assert_eq!(ids.last(), Some(&"plain-shell"));
        for profile in &profiles {
            assert_eq!(profile.source, ProfileSource::Builtin);
        }
    }
}
