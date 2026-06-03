use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use roux_core::{
    SpawnProfile, TerminalDefaults, TerminalEnvRule, TerminalEnvRuleMode, TerminalEnvRuleSpec,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalProfileEnvPlan {
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
}

pub struct TerminalProfileEnvInputs<'a> {
    pub base_env: BTreeMap<String, String>,
    pub terminal_defaults: Option<&'a TerminalDefaults>,
    pub roux_env: &'a [(String, String)],
    pub profile: Option<&'a SpawnProfile>,
    pub launch_env: Option<&'a BTreeMap<String, TerminalEnvRule>>,
    pub shell: &'a str,
    pub working_dir: &'a Path,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TerminalProfileEnvError {
    #[error("terminal env command for {name} is empty")]
    EmptyEnvCommand { name: String },
    #[error("terminal env command for {name} failed with status {status}")]
    EnvCommandFailed { name: String, status: String },
    #[error("terminal env command for {name} could not run: {error}")]
    EnvCommandIo { name: String, error: String },
    #[error("terminal env value for {name} is missing")]
    MissingEnvValue { name: String },
    #[error("{stage} command failed with status {status}")]
    PreflightFailed { stage: &'static str, status: String },
    #[error("{stage} command could not run: {error}")]
    PreflightIo { stage: &'static str, error: String },
}

pub fn resolve_terminal_profile_env(
    inputs: TerminalProfileEnvInputs<'_>,
) -> Result<TerminalProfileEnvPlan, TerminalProfileEnvError> {
    let mut env = EnvAccumulator::new(inputs.base_env);

    if let Some(defaults) = inputs.terminal_defaults {
        apply_env_rules(&mut env, defaults.env.as_ref(), inputs.shell, inputs.working_dir)?;
    }

    for (name, value) in inputs.roux_env {
        env.set(name, value.clone());
    }

    if let Some(profile) = inputs.profile {
        apply_env_rules(&mut env, profile.env.as_ref(), inputs.shell, inputs.working_dir)?;
    }

    apply_env_rules(&mut env, inputs.launch_env, inputs.shell, inputs.working_dir)?;

    for (name, value) in inputs.roux_env {
        env.set(name, value.clone());
    }

    if let Some(defaults) = inputs.terminal_defaults {
        run_preflight(
            "global beforeShellStarts",
            defaults.before_shell_starts.as_deref(),
            &env.values,
            inputs.shell,
            inputs.working_dir,
        )?;
    }
    if let Some(profile) = inputs.profile {
        run_preflight(
            "profile beforeShellStarts",
            profile.before_shell_starts.as_deref(),
            &env.values,
            inputs.shell,
            inputs.working_dir,
        )?;
    }

    Ok(env.into_plan())
}

fn apply_env_rules(
    env: &mut EnvAccumulator,
    rules: Option<&BTreeMap<String, TerminalEnvRule>>,
    shell: &str,
    working_dir: &Path,
) -> Result<(), TerminalProfileEnvError> {
    let Some(rules) = rules else {
        return Ok(());
    };
    for (name, rule) in rules {
        if !is_valid_env_name(name) {
            continue;
        }
        apply_env_rule(env, name, rule, shell, working_dir)?;
    }
    Ok(())
}

fn apply_env_rule(
    env: &mut EnvAccumulator,
    name: &str,
    rule: &TerminalEnvRule,
    shell: &str,
    working_dir: &Path,
) -> Result<(), TerminalProfileEnvError> {
    match rule {
        TerminalEnvRule::LegacyValue(value) => env.set(name, value.clone()),
        TerminalEnvRule::Structured(spec) => {
            apply_structured_rule(env, name, spec, shell, working_dir)?
        }
    }
    Ok(())
}

fn apply_structured_rule(
    env: &mut EnvAccumulator,
    name: &str,
    spec: &TerminalEnvRuleSpec,
    shell: &str,
    working_dir: &Path,
) -> Result<(), TerminalProfileEnvError> {
    match spec.mode {
        TerminalEnvRuleMode::Value => {
            let value = spec.value.clone().ok_or_else(|| {
                TerminalProfileEnvError::MissingEnvValue { name: name.to_string() }
            })?;
            env.set(name, value);
        }
        TerminalEnvRuleMode::Inherit => {}
        TerminalEnvRuleMode::Unset => env.unset(name),
        TerminalEnvRuleMode::Command => {
            let command = spec
                .command
                .as_deref()
                .map(str::trim)
                .filter(|command| !command.is_empty())
                .ok_or_else(|| TerminalProfileEnvError::EmptyEnvCommand {
                    name: name.to_string(),
                })?;
            let value = run_env_command(name, command, &env.values, shell, working_dir)?;
            env.set(name, value);
        }
    }
    Ok(())
}

fn run_env_command(
    name: &str,
    command: &str,
    env: &BTreeMap<String, String>,
    shell: &str,
    working_dir: &Path,
) -> Result<String, TerminalProfileEnvError> {
    let output = command_with_env(command, env, shell, working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| TerminalProfileEnvError::EnvCommandIo {
            name: name.to_string(),
            error: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(TerminalProfileEnvError::EnvCommandFailed {
            name: name.to_string(),
            status: status_label(output.status),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_preflight(
    stage: &'static str,
    command: Option<&str>,
    env: &BTreeMap<String, String>,
    shell: &str,
    working_dir: &Path,
) -> Result<(), TerminalProfileEnvError> {
    let Some(command) = command.map(str::trim).filter(|command| !command.is_empty()) else {
        return Ok(());
    };
    let status = command_with_env(command, env, shell, working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| TerminalProfileEnvError::PreflightIo {
            stage,
            error: error.to_string(),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(TerminalProfileEnvError::PreflightFailed { stage, status: status_label(status) })
    }
}

fn command_with_env(
    command: &str,
    env: &BTreeMap<String, String>,
    shell: &str,
    working_dir: &Path,
) -> Command {
    let mut cmd = Command::new(shell);
    for arg in shell_command_args(shell, command) {
        cmd.arg(arg);
    }
    cmd.current_dir(working_dir);
    cmd.env_clear();
    cmd.envs(env);
    cmd
}

fn shell_command_args(shell: &str, command: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let shell_lower = shell.to_ascii_lowercase();
        if shell_lower.contains("pwsh") {
            return vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                command.to_string(),
            ];
        }
        if shell_lower.contains("powershell") {
            return vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                command.to_string(),
            ];
        }
        vec!["/C".to_string(), command.to_string()]
    }

    #[cfg(not(windows))]
    {
        let _ = shell;
        vec!["-c".to_string(), command.to_string()]
    }
}

fn status_label(status: std::process::ExitStatus) -> String {
    status.code().map(|code| code.to_string()).unwrap_or_else(|| "signal".to_string())
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct EnvAccumulator {
    values: BTreeMap<String, String>,
    touched: BTreeSet<String>,
    removed: BTreeSet<String>,
}

impl EnvAccumulator {
    fn new(values: BTreeMap<String, String>) -> Self {
        Self { values, touched: BTreeSet::new(), removed: BTreeSet::new() }
    }

    fn set(&mut self, name: &str, value: String) {
        self.values.insert(name.to_string(), value);
        self.touched.insert(name.to_string());
        self.removed.remove(name);
    }

    fn unset(&mut self, name: &str) {
        self.values.remove(name);
        self.touched.remove(name);
        self.removed.insert(name.to_string());
    }

    fn into_plan(self) -> TerminalProfileEnvPlan {
        let env = self
            .touched
            .into_iter()
            .filter_map(|name| self.values.get(&name).map(|value| (name, value.clone())))
            .collect();
        TerminalProfileEnvPlan { env, env_remove: self.removed.into_iter().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roux_core::{ProfileSource, SplitProfileBehavior, StartupBehavior, TerminalEnvRuleSpec};

    fn base_env() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("BASE_ONLY".to_string(), "base".to_string()),
            ("PATH".to_string(), "/bin".to_string()),
            ("REMOVE_ME".to_string(), "inherited".to_string()),
            ("SHARED".to_string(), "base".to_string()),
        ])
    }

    fn rule(mode: TerminalEnvRuleMode) -> TerminalEnvRule {
        TerminalEnvRule::Structured(TerminalEnvRuleSpec { mode, value: None, command: None })
    }

    fn value(value: &str) -> TerminalEnvRule {
        TerminalEnvRule::value(value)
    }

    fn make_profile(env: BTreeMap<String, TerminalEnvRule>) -> SpawnProfile {
        SpawnProfile {
            id: "profile".to_string(),
            name: "Profile".to_string(),
            setup_command: None,
            startup_command: None,
            startup_behavior: Some(StartupBehavior::AutoRun),
            env: Some(env),
            before_shell_starts: None,
            cwd_override: None,
            icon: None,
            provider: None,
            source: ProfileSource::User,
        }
    }

    fn defaults(env: BTreeMap<String, TerminalEnvRule>) -> TerminalDefaults {
        TerminalDefaults {
            env: Some(env),
            before_shell_starts: None,
            split_profile_behavior: SplitProfileBehavior::PlainShell,
        }
    }

    #[test]
    fn env_precedence_applies_global_roux_profile_and_launch_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = defaults(BTreeMap::from([
            ("GLOBAL".to_string(), value("global")),
            ("SHARED".to_string(), value("global")),
        ]));
        let profile = make_profile(BTreeMap::from([
            ("PROFILE".to_string(), value("profile")),
            ("ROUX_SESSION_ID".to_string(), value("profile-session")),
            ("SHARED".to_string(), value("profile")),
        ]));
        let launch = BTreeMap::from([
            ("ROUX_SESSION_ID".to_string(), value("launch-session")),
            ("SHARED".to_string(), value("launch")),
        ]);

        let plan = resolve_terminal_profile_env(TerminalProfileEnvInputs {
            base_env: base_env(),
            terminal_defaults: Some(&defaults),
            roux_env: &[("ROUX_SESSION_ID".to_string(), "roux-session".to_string())],
            profile: Some(&profile),
            launch_env: Some(&launch),
            shell: "/bin/sh",
            working_dir: dir.path(),
        })
        .unwrap();

        assert_eq!(
            BTreeMap::<_, _>::from_iter(plan.env),
            BTreeMap::from([
                ("GLOBAL".to_string(), "global".to_string()),
                ("PROFILE".to_string(), "profile".to_string()),
                ("ROUX_SESSION_ID".to_string(), "roux-session".to_string()),
                ("SHARED".to_string(), "launch".to_string()),
            ])
        );
    }

    #[test]
    fn inherit_keeps_resolved_value_and_unset_removes_inherited_value() {
        let dir = tempfile::tempdir().unwrap();
        let profile = make_profile(BTreeMap::from([
            ("BASE_ONLY".to_string(), rule(TerminalEnvRuleMode::Inherit)),
            ("REMOVE_ME".to_string(), rule(TerminalEnvRuleMode::Unset)),
        ]));

        let plan = resolve_terminal_profile_env(TerminalProfileEnvInputs {
            base_env: base_env(),
            terminal_defaults: None,
            roux_env: &[],
            profile: Some(&profile),
            launch_env: None,
            shell: "/bin/sh",
            working_dir: dir.path(),
        })
        .unwrap();

        assert!(!plan.env.iter().any(|(name, _)| name == "BASE_ONLY"));
        assert_eq!(plan.env_remove, vec!["REMOVE_ME".to_string()]);
    }

    #[test]
    fn value_mode_requires_a_value() {
        let dir = tempfile::tempdir().unwrap();
        let profile = make_profile(BTreeMap::from([(
            "MISSING".to_string(),
            rule(TerminalEnvRuleMode::Value),
        )]));

        let err = resolve_terminal_profile_env(TerminalProfileEnvInputs {
            base_env: base_env(),
            terminal_defaults: None,
            roux_env: &[],
            profile: Some(&profile),
            launch_env: None,
            shell: "/bin/sh",
            working_dir: dir.path(),
        })
        .unwrap_err();

        assert_eq!(err, TerminalProfileEnvError::MissingEnvValue { name: "MISSING".to_string() });
    }

    #[cfg(not(windows))]
    #[test]
    fn command_mode_captures_trimmed_stdout_without_exposing_value_in_error() {
        let dir = tempfile::tempdir().unwrap();
        let profile = make_profile(BTreeMap::from([(
            "TOKEN".to_string(),
            TerminalEnvRule::command("printf ' secret-value \\n'"),
        )]));

        let plan = resolve_terminal_profile_env(TerminalProfileEnvInputs {
            base_env: base_env(),
            terminal_defaults: None,
            roux_env: &[],
            profile: Some(&profile),
            launch_env: None,
            shell: "/bin/sh",
            working_dir: dir.path(),
        })
        .unwrap();

        assert_eq!(plan.env, vec![("TOKEN".to_string(), "secret-value".to_string())]);

        let failing = make_profile(BTreeMap::from([(
            "TOKEN".to_string(),
            TerminalEnvRule::command("printf secret-value; exit 7"),
        )]));
        let err = resolve_terminal_profile_env(TerminalProfileEnvInputs {
            base_env: base_env(),
            terminal_defaults: None,
            roux_env: &[],
            profile: Some(&failing),
            launch_env: None,
            shell: "/bin/sh",
            working_dir: dir.path(),
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("TOKEN"));
        assert!(!msg.contains("secret-value"));
    }

    #[cfg(not(windows))]
    #[test]
    fn preflight_failure_aborts_without_logging_output() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = TerminalDefaults {
            env: None,
            before_shell_starts: Some("printf sensitive; exit 9".to_string()),
            split_profile_behavior: SplitProfileBehavior::PlainShell,
        };

        let err = resolve_terminal_profile_env(TerminalProfileEnvInputs {
            base_env: base_env(),
            terminal_defaults: Some(&defaults),
            roux_env: &[],
            profile: None,
            launch_env: None,
            shell: "/bin/sh",
            working_dir: dir.path(),
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("global beforeShellStarts"));
        assert!(!msg.contains("sensitive"));
    }
}
