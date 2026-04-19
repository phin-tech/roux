//! Claude provider module.
//!
//! Until spawn profiles are wired into pane creation (phase 5), this module
//! only contributes the built-in Claude [`SpawnProfile`] shown in the
//! new-session picker. Hook install and payload parsing still live in
//! `hooks.rs` / `status_watcher.rs`; they will migrate here when the
//! parent Codex plan lands.

use roux_core::{ProfileSource, Provider, SpawnProfile};

use super::shell_quote;
use crate::settings::RouxSettings;

/// Default profiles contributed by this provider. One entry for now — the
/// stock `claude` command with the user's configured binary path, default
/// model, and additional flags stitched in. A future multi-variant world
/// (e.g. `claude-with-mcp`) can return more than one.
pub fn default_profiles(settings: &RouxSettings) -> Vec<SpawnProfile> {
    vec![SpawnProfile {
        id: "claude".into(),
        name: "Claude".into(),
        setup_command: None,
        startup_command: Some(build_startup_command(settings)),
        startup_behavior: None,
        env: None,
        cwd_override: None,
        icon: None,
        provider: Some(Provider::Claude),
        nono_profile: None,
        nono_allow_dirs: None,
        source: ProfileSource::Builtin,
    }]
}

fn build_startup_command(settings: &RouxSettings) -> String {
    let binary =
        settings.claude_binary_path.as_deref().filter(|s| !s.is_empty()).unwrap_or("claude");
    let mut cmd = shell_quote(binary);
    if let Some(model) = settings.default_model.as_deref().filter(|s| !s.is_empty()) {
        cmd.push_str(" --model ");
        cmd.push_str(&shell_quote(model));
    }
    for flag in &settings.additional_flags {
        if flag.is_empty() {
            continue;
        }
        cmd.push(' ');
        cmd.push_str(&shell_quote(flag));
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profiles_returns_a_claude_profile() {
        let settings = RouxSettings::default();
        let profiles = default_profiles(&settings);
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert_eq!(p.id, "claude");
        assert_eq!(p.name, "Claude");
        assert_eq!(p.provider, Some(Provider::Claude));
        assert_eq!(p.source, ProfileSource::Builtin);
    }

    #[test]
    fn startup_command_uses_default_binary_when_unset() {
        let settings = RouxSettings::default();
        let cmd = build_startup_command(&settings);
        assert_eq!(cmd, "claude");
    }

    #[test]
    fn startup_command_uses_custom_binary_and_model() {
        let settings = RouxSettings {
            claude_binary_path: Some("/opt/claude/bin/claude".into()),
            default_model: Some("claude-opus-4-6".into()),
            ..RouxSettings::default()
        };
        let cmd = build_startup_command(&settings);
        assert_eq!(cmd, "/opt/claude/bin/claude --model claude-opus-4-6");
    }

    #[test]
    fn startup_command_quotes_binary_path_with_spaces() {
        let settings = RouxSettings {
            claude_binary_path: Some("/Applications/Claude Code.app/claude".into()),
            ..RouxSettings::default()
        };
        let cmd = build_startup_command(&settings);
        assert_eq!(cmd, "'/Applications/Claude Code.app/claude'");
    }

    #[test]
    fn startup_command_appends_additional_flags() {
        let settings = RouxSettings {
            additional_flags: vec!["--verbose".into(), "--trust-workspace".into()],
            ..RouxSettings::default()
        };
        let cmd = build_startup_command(&settings);
        assert_eq!(cmd, "claude --verbose --trust-workspace");
    }

    #[test]
    fn startup_command_ignores_empty_binary_and_model_strings() {
        let settings = RouxSettings {
            claude_binary_path: Some(String::new()),
            default_model: Some(String::new()),
            ..RouxSettings::default()
        };
        let cmd = build_startup_command(&settings);
        assert_eq!(cmd, "claude");
    }
}
