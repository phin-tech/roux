//! Codex provider module.
//!
//! Phase 3 only contributes a built-in [`SpawnProfile`] so the frontend
//! picker can surface Codex alongside Claude. Hook install and payload
//! normalization land with the parent Codex plan once spawn profiles are
//! wired into pane creation.

use roux_core::{ProfileSource, Provider, SpawnProfile};

use super::shell_quote;
use crate::settings::RouxSettings;

/// Default profiles contributed by this provider. Mirrors the Claude profile
/// shape: a single built-in entry, derived from current settings. Future
/// Codex-specific variants (e.g. `codex-exec`) can be added as additional
/// vec entries without frontend changes.
pub fn default_profiles(_settings: &RouxSettings) -> Vec<SpawnProfile> {
    vec![SpawnProfile {
        id: "codex".into(),
        name: "Codex".into(),
        setup_command: None,
        // Until the Codex-specific binary-path setting lands (see parent
        // plan), we assume `codex` is resolvable on PATH inside the PTY. The
        // Roux PTY path injection makes this reliable for users who have it
        // installed globally or under `~/.local/bin`.
        startup_command: Some(shell_quote("codex")),
        startup_behavior: None,
        env: None,
        cwd_override: None,
        icon: None,
        provider: Some(Provider::Codex),
        source: ProfileSource::Builtin,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profiles_returns_a_codex_profile() {
        let settings = RouxSettings::default();
        let profiles = default_profiles(&settings);
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert_eq!(p.id, "codex");
        assert_eq!(p.provider, Some(Provider::Codex));
        assert_eq!(p.source, ProfileSource::Builtin);
        assert_eq!(p.startup_command.as_deref(), Some("codex"));
    }
}
