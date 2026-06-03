//! Built-in spawn-profile definitions, shared by the desktop (new-session
//! picker) and the daemon (headless work-item dispatch). Profiles are derived
//! from `RouxSettings`; user-defined profiles live in settings as data.
//!
//! This module is intentionally pure data + string building (no Tauri, no
//! filesystem) so the daemon can resolve `"claude"` → a runnable startup
//! command without depending on the desktop crate.

use crate::{ProfileSource, Provider, RouxSettings, SpawnProfile, StartupBehavior};

const PTY_ENTER: char = '\r';

/// Assemble the built-in profile registry: one or more profiles from each
/// provider plus the catch-all "Plain shell". Ordering matches display order;
/// `Plain shell` goes last so users see named agents first.
pub fn builtin_profiles(settings: &RouxSettings) -> Vec<SpawnProfile> {
    let mut profiles = Vec::new();
    profiles.extend(claude_default_profiles(settings));
    profiles.extend(codex_default_profiles(settings));
    profiles.push(plain_shell_profile());
    profiles
}

/// Resolve a profile id to a concrete [`SpawnProfile`]: user-defined profiles
/// win over built-ins (matching how the frontend registry layers them), then
/// fall back to the built-in set. Returns `None` for an unknown id.
pub fn resolve_profile(id: &str, settings: &RouxSettings) -> Option<SpawnProfile> {
    if let Some(p) = settings.spawn_profiles.iter().find(|p| p.id == id) {
        return Some(p.clone());
    }
    builtin_profiles(settings).into_iter().find(|p| p.id == id)
}

/// Return a copy of `profile` constrained for card planning runs. Planning
/// sessions should inspect, ask questions, and produce a plan without editing
/// files. Execution runs keep the original profile unchanged.
pub fn profile_with_planning_constraints(profile: &SpawnProfile) -> SpawnProfile {
    let mut profile = profile.clone();
    let Some(startup) =
        profile.startup_command.as_deref().map(str::trim).filter(|startup| !startup.is_empty())
    else {
        return profile;
    };
    let extra_args: &[&str] = match profile.provider {
        Some(Provider::Claude) => &["--permission-mode", "plan"],
        Some(Provider::Codex) => &["--sandbox", "read-only", "--ask-for-approval", "never"],
        _ => return profile,
    };

    let mut command = startup.to_string();
    for arg in extra_args {
        command.push(' ');
        command.push_str(&shell_quote(arg));
    }
    profile.startup_command = Some(command);
    profile
}

/// Default profiles contributed by the Claude provider: the stock `claude`
/// command with the user's configured binary path, default model, and
/// additional flags stitched in.
pub fn claude_default_profiles(settings: &RouxSettings) -> Vec<SpawnProfile> {
    vec![SpawnProfile {
        id: "claude".into(),
        name: "Claude".into(),
        setup_command: None,
        startup_command: Some(claude_startup_command(settings)),
        startup_behavior: None,
        env: None,
        before_shell_starts: None,
        cwd_override: None,
        icon: None,
        provider: Some(Provider::Claude),
        source: ProfileSource::Builtin,
    }]
}

fn claude_startup_command(settings: &RouxSettings) -> String {
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

/// Default profiles contributed by the Codex provider. Assumes `codex` is
/// resolvable on PATH inside the PTY (Roux's PTY path injection makes this
/// reliable for global / `~/.local/bin` installs).
pub fn codex_default_profiles(_settings: &RouxSettings) -> Vec<SpawnProfile> {
    vec![SpawnProfile {
        id: "codex".into(),
        name: "Codex".into(),
        setup_command: None,
        startup_command: Some(shell_quote("codex")),
        startup_behavior: None,
        env: None,
        before_shell_starts: None,
        cwd_override: None,
        icon: None,
        provider: Some(Provider::Codex),
        source: ProfileSource::Builtin,
    }]
}

/// Build the text to type into a freshly-spawned shell to bring `profile` to
/// life: `cd` override, setup command, then the startup command
/// (with `append_system_prompt` folded in per provider). Returns `None` when
/// the profile has nothing to run (e.g. plain shell), so callers leave the
/// shell at its prompt. Profile environment is applied before PTY spawn by
/// the runtime and is intentionally not exported here.
pub fn profile_startup_input(
    profile: &SpawnProfile,
    append_system_prompt: Option<&str>,
) -> Option<String> {
    let cwd = profile.cwd_override.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let setup = profile.setup_command.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let base_startup = profile.startup_command.as_deref().unwrap_or("");
    let startup = append_agent_system_prompt(
        base_startup,
        profile.provider,
        append_system_prompt.unwrap_or("").trim(),
    );
    let has_startup = !startup.trim().is_empty();

    if cwd.is_none() && setup.is_none() && !has_startup {
        return None;
    }

    let mut out = String::new();
    if let Some(cwd) = cwd {
        out.push_str(&format!("cd {}", shell_single_quote(cwd)));
        out.push(PTY_ENTER);
    }
    if let Some(setup) = setup {
        push_pty_command(&mut out, setup);
    }
    if has_startup {
        out.push_str(&startup);
        // typeOnly leaves the command at the prompt for the user to run; for
        // any other behavior (incl. the default) auto-run it.
        if !matches!(profile.startup_behavior, Some(StartupBehavior::TypeOnly)) {
            out.push(PTY_ENTER);
        }
    }
    Some(out)
}

/// Build startup input and, for known auto-run agent profiles, queue the
/// initial task after the agent command starts. This deliberately avoids
/// writing task text into plain shells or type-only profiles.
pub fn profile_startup_input_with_initial_task(
    profile: &SpawnProfile,
    append_system_prompt: Option<&str>,
    initial_task_prompt: Option<&str>,
) -> Option<String> {
    let mut input = profile_startup_input(profile, append_system_prompt)?;
    let task = initial_task_prompt.unwrap_or("").trim();
    if task.is_empty() {
        return Some(input);
    }
    if matches!(profile.startup_behavior, Some(StartupBehavior::TypeOnly)) {
        return Some(input);
    }
    if !matches!(profile.provider, Some(Provider::Claude) | Some(Provider::Codex)) {
        return Some(input);
    }
    if profile.startup_command.as_deref().map(str::trim).filter(|cmd| !cmd.is_empty()).is_none() {
        return Some(input);
    }

    if input.ends_with('\n') {
        input.pop();
        input.push(PTY_ENTER);
    } else if !input.ends_with(PTY_ENTER) {
        input.push(PTY_ENTER);
    }
    input.push_str(task);
    input.push(PTY_ENTER);
    Some(input)
}

/// Build a non-interactive shell command that starts a supported autonomous
/// agent with `initial_task_prompt` as the initial positional prompt. This is
/// used by daemon-owned work-item runs where there is no real terminal frontend
/// to answer interactive shell capability probes before Roux types input.
pub fn profile_startup_command_with_initial_prompt(
    profile: &SpawnProfile,
    append_system_prompt: Option<&str>,
    initial_task_prompt: &str,
) -> Option<String> {
    let task = initial_task_prompt.trim();
    if task.is_empty() {
        return None;
    }
    if matches!(profile.startup_behavior, Some(StartupBehavior::TypeOnly)) {
        return None;
    }
    if !matches!(profile.provider, Some(Provider::Claude) | Some(Provider::Codex)) {
        return None;
    }

    let base_startup = profile.startup_command.as_deref().unwrap_or("").trim();
    if base_startup.is_empty() {
        return None;
    }
    let startup = append_agent_system_prompt(
        base_startup,
        profile.provider,
        append_system_prompt.unwrap_or("").trim(),
    );
    let startup = format!("{startup} {}", shell_single_quote(task));

    let setup = profile.setup_command.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let mut out = String::new();
    if let Some(setup) = setup {
        push_shell_command(&mut out, setup);
    }
    out.push_str(&startup);
    Some(out)
}

fn push_pty_command(out: &mut String, command: &str) {
    let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
    for (index, line) in normalized.split('\n').enumerate() {
        if index > 0 {
            out.push(PTY_ENTER);
        }
        out.push_str(line);
    }
    out.push(PTY_ENTER);
}

fn push_shell_command(out: &mut String, command: &str) {
    let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
    out.push_str(normalized.trim());
    out.push('\n');
}

/// Splice `prompt` into a startup command using the provider-appropriate flag
/// (`--append-system-prompt 'X'` for Claude, `-c instructions='X'` for Codex).
/// No-op for empty prompt, empty command, or providers without a known flag.
/// Mirrors the frontend `appendAgentSystemPrompt`.
fn append_agent_system_prompt(cmd: &str, provider: Option<Provider>, prompt: &str) -> String {
    if prompt.is_empty() || cmd.trim().is_empty() {
        return cmd.to_string();
    }
    let flag = match provider {
        Some(Provider::Claude) => "--append-system-prompt",
        Some(Provider::Codex) => "-c instructions=",
        _ => return cmd.to_string(),
    };
    let sep = if flag.ends_with('=') { "" } else { " " };
    format!("{cmd} {flag}{sep}{}", shell_single_quote(prompt))
}

/// Always wrap in single quotes (POSIX), escaping embedded quotes via the
/// `'\''` dance. Used for values that may contain anything (env values,
/// system-prompt text) — unlike [`shell_quote`], which leaves bare tokens bare.
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn plain_shell_profile() -> SpawnProfile {
    SpawnProfile {
        id: "plain-shell".into(),
        name: "Plain shell".into(),
        setup_command: None,
        startup_command: None,
        startup_behavior: None,
        env: None,
        before_shell_starts: None,
        cwd_override: None,
        icon: None,
        provider: None,
        source: ProfileSource::Builtin,
    }
}

/// Shell-quote a single token using POSIX single-quote rules. Safe for use
/// inside a shell string handed to `sh -c` or typed into a shell prompt. The
/// quoting rules are intentionally minimal: a single argument, not a compound
/// shell expression.
pub fn shell_quote(token: &str) -> String {
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
    use crate::TerminalEnvRule;

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
        assert_eq!(ids.last(), Some(&"plain-shell"));
        for profile in &profiles {
            assert_eq!(profile.source, ProfileSource::Builtin);
        }
    }

    #[test]
    fn claude_startup_command_uses_default_binary_when_unset() {
        let settings = RouxSettings::default();
        assert_eq!(claude_startup_command(&settings), "claude");
    }

    #[test]
    fn claude_startup_command_uses_custom_binary_and_model() {
        let settings = RouxSettings {
            claude_binary_path: Some("/opt/claude/bin/claude".into()),
            default_model: Some("claude-opus-4-6".into()),
            ..RouxSettings::default()
        };
        assert_eq!(
            claude_startup_command(&settings),
            "/opt/claude/bin/claude --model claude-opus-4-6"
        );
    }

    #[test]
    fn claude_startup_command_appends_additional_flags() {
        let settings = RouxSettings {
            additional_flags: vec!["--verbose".into(), "--trust-workspace".into()],
            ..RouxSettings::default()
        };
        assert_eq!(claude_startup_command(&settings), "claude --verbose --trust-workspace");
    }

    #[test]
    fn resolve_profile_prefers_user_then_builtin_then_none() {
        let settings = RouxSettings::default();
        assert_eq!(resolve_profile("claude", &settings).unwrap().id, "claude");
        assert_eq!(resolve_profile("plain-shell", &settings).unwrap().id, "plain-shell");
        assert!(resolve_profile("does-not-exist", &settings).is_none());
    }

    #[test]
    fn plain_shell_has_no_startup_input() {
        assert!(profile_startup_input(&plain_shell_profile(), None).is_none());
    }

    #[test]
    fn claude_startup_input_auto_runs_the_command() {
        let profile = claude_default_profiles(&RouxSettings::default()).remove(0);
        assert_eq!(profile_startup_input(&profile, None), Some("claude\r".to_string()));
    }

    #[test]
    fn claude_startup_input_folds_in_append_system_prompt() {
        let profile = claude_default_profiles(&RouxSettings::default()).remove(0);
        let input = profile_startup_input(&profile, Some("Be terse")).unwrap();
        assert_eq!(input, "claude --append-system-prompt 'Be terse'\r");
    }

    #[test]
    fn codex_startup_input_uses_instructions_flag() {
        let profile = codex_default_profiles(&RouxSettings::default()).remove(0);
        let input = profile_startup_input(&profile, Some("ship it")).unwrap();
        assert_eq!(input, "codex -c instructions='ship it'\r");
    }

    #[test]
    fn startup_input_emits_env_and_setup_before_startup() {
        let mut env = std::collections::BTreeMap::new();
        env.insert("FOO".to_string(), TerminalEnvRule::value("bar baz"));
        env.insert("not valid".to_string(), TerminalEnvRule::value("skip"));
        let profile = SpawnProfile {
            id: "custom".into(),
            name: "Custom".into(),
            setup_command: Some("npm ci".into()),
            startup_command: Some("run".into()),
            startup_behavior: None,
            env: Some(env),
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: None,
            source: ProfileSource::User,
        };
        let input = profile_startup_input(&profile, None).unwrap();
        assert_eq!(input, "npm ci\rrun\r");
    }

    #[test]
    fn type_only_startup_is_not_auto_run() {
        let profile = SpawnProfile {
            id: "to".into(),
            name: "TypeOnly".into(),
            setup_command: None,
            startup_command: Some("claude".into()),
            startup_behavior: Some(StartupBehavior::TypeOnly),
            env: None,
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: Some(Provider::Claude),
            source: ProfileSource::User,
        };
        assert_eq!(profile_startup_input(&profile, None), Some("claude".to_string()));
    }

    #[test]
    fn initial_task_is_typed_after_auto_run_agent_startup() {
        let profile = claude_default_profiles(&RouxSettings::default()).remove(0);
        let input =
            profile_startup_input_with_initial_task(&profile, Some("Be terse"), Some("Fix it"))
                .unwrap();
        assert_eq!(input, "claude --append-system-prompt 'Be terse'\rFix it\r");
    }

    #[test]
    fn initial_task_is_not_typed_into_plain_shell() {
        assert!(profile_startup_input_with_initial_task(
            &plain_shell_profile(),
            None,
            Some("Fix it"),
        )
        .is_none());
    }

    #[test]
    fn initial_task_is_not_typed_for_type_only_profiles() {
        let profile = SpawnProfile {
            id: "to".into(),
            name: "TypeOnly".into(),
            setup_command: None,
            startup_command: Some("claude".into()),
            startup_behavior: Some(StartupBehavior::TypeOnly),
            env: None,
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: Some(Provider::Claude),
            source: ProfileSource::User,
        };
        assert_eq!(
            profile_startup_input_with_initial_task(&profile, None, Some("Fix it")),
            Some("claude".to_string()),
        );
    }

    #[test]
    fn initial_task_is_not_typed_for_unknown_provider() {
        let profile = SpawnProfile {
            id: "custom".into(),
            name: "Custom".into(),
            setup_command: None,
            startup_command: Some("run-agent".into()),
            startup_behavior: None,
            env: None,
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: None,
            source: ProfileSource::User,
        };
        assert_eq!(
            profile_startup_input_with_initial_task(&profile, None, Some("Fix it")),
            Some("run-agent\r".to_string()),
        );
    }

    #[test]
    fn initial_prompt_command_seeds_claude_positional_prompt() {
        let profile = claude_default_profiles(&RouxSettings::default()).remove(0);
        let command =
            profile_startup_command_with_initial_prompt(&profile, Some("Be terse"), "Fix it")
                .unwrap();
        assert_eq!(command, "claude --append-system-prompt 'Be terse' 'Fix it'");
    }

    #[test]
    fn initial_prompt_command_seeds_codex_positional_prompt() {
        let profile = codex_default_profiles(&RouxSettings::default()).remove(0);
        let command =
            profile_startup_command_with_initial_prompt(&profile, Some("Be terse"), "Fix it")
                .unwrap();
        assert_eq!(command, "codex -c instructions='Be terse' 'Fix it'");
    }

    #[test]
    fn planning_constraints_force_claude_plan_permission_mode() {
        let profile = profile_with_planning_constraints(
            &claude_default_profiles(&RouxSettings::default())[0],
        );
        let command =
            profile_startup_command_with_initial_prompt(&profile, Some("Be terse"), "Plan it")
                .unwrap();
        assert_eq!(
            command,
            "claude --permission-mode plan --append-system-prompt 'Be terse' 'Plan it'",
        );
    }

    #[test]
    fn planning_constraints_force_codex_read_only_without_approval() {
        let profile =
            profile_with_planning_constraints(&codex_default_profiles(&RouxSettings::default())[0]);
        let command =
            profile_startup_command_with_initial_prompt(&profile, Some("Be terse"), "Plan it")
                .unwrap();
        assert_eq!(
            command,
            "codex --sandbox read-only --ask-for-approval never -c instructions='Be terse' 'Plan it'",
        );
    }

    #[test]
    fn initial_prompt_command_quotes_multiline_task_prompt() {
        let profile = claude_default_profiles(&RouxSettings::default()).remove(0);
        let command = profile_startup_command_with_initial_prompt(
            &profile,
            None,
            "Plan this\nit's important",
        )
        .unwrap();
        assert_eq!(command, "claude 'Plan this\nit'\\''s important'");
    }

    #[test]
    fn initial_prompt_command_preserves_env_and_setup_before_agent() {
        let mut env = std::collections::BTreeMap::new();
        env.insert("FOO".to_string(), TerminalEnvRule::value("bar baz"));
        env.insert("not valid".to_string(), TerminalEnvRule::value("skip"));
        let profile = SpawnProfile {
            id: "custom-claude".into(),
            name: "Custom Claude".into(),
            setup_command: Some("echo setup".into()),
            startup_command: Some("claude --dangerously-skip-permissions".into()),
            startup_behavior: None,
            env: Some(env),
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: Some(Provider::Claude),
            source: ProfileSource::User,
        };
        let command =
            profile_startup_command_with_initial_prompt(&profile, Some("Be terse"), "Fix it")
                .unwrap();
        assert_eq!(
            command,
            "echo setup\nclaude --dangerously-skip-permissions --append-system-prompt 'Be terse' 'Fix it'",
        );
    }

    #[test]
    fn initial_prompt_command_rejects_plain_and_type_only_profiles() {
        assert!(profile_startup_command_with_initial_prompt(
            &plain_shell_profile(),
            None,
            "Fix it"
        )
        .is_none());

        let profile = SpawnProfile {
            id: "to".into(),
            name: "TypeOnly".into(),
            setup_command: None,
            startup_command: Some("claude".into()),
            startup_behavior: Some(StartupBehavior::TypeOnly),
            env: None,
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: Some(Provider::Claude),
            source: ProfileSource::User,
        };
        assert!(profile_startup_command_with_initial_prompt(&profile, None, "Fix it").is_none());
    }
}
