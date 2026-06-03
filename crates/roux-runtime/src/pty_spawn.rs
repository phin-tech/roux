use std::path::PathBuf;

pub const DEFAULT_PTY_COLS: u16 = 80;
pub const DEFAULT_PTY_ROWS: u16 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyDimensions {
    pub cols: u16,
    pub rows: u16,
}

impl PtyDimensions {
    pub fn as_tuple(self) -> (u16, u16) {
        (self.cols, self.rows)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyCommandPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpawnPlan {
    pub size: PtyDimensions,
    pub command: PtyCommandPlan,
}

pub struct ShellSpawnPlanInputs<'a> {
    pub working_dir: &'a str,
    pub shell: &'a str,
    pub roux_env: &'a [(String, String)],
    pub worktree_path: Option<&'a str>,
    pub initial_size: Option<(u16, u16)>,
}

pub struct TaskSpawnPlanInputs<'a> {
    pub command: &'a str,
    pub working_dir: &'a str,
    pub shell: &'a str,
    pub roux_env: &'a [(String, String)],
    pub worktree_path: Option<&'a str>,
    pub initial_size: Option<(u16, u16)>,
}

pub fn pty_dimensions(initial: Option<(u16, u16)>) -> PtyDimensions {
    let (cols, rows) = initial.unwrap_or((DEFAULT_PTY_COLS, DEFAULT_PTY_ROWS));
    PtyDimensions { cols: cols.max(1), rows: rows.max(1) }
}

pub fn shell_spawn_plan(inputs: ShellSpawnPlanInputs<'_>) -> PtySpawnPlan {
    let mut command = PtyCommandPlan {
        program: PathBuf::from(inputs.shell),
        args: Vec::new(),
        env: inputs.roux_env.to_vec(),
        env_remove: Vec::new(),
        cwd: inputs.working_dir.to_string(),
    };

    append_shell_command_flags(&mut command.args, inputs.shell);

    PtySpawnPlan { size: pty_dimensions(inputs.initial_size), command }
}

pub fn task_spawn_plan(inputs: TaskSpawnPlanInputs<'_>) -> PtySpawnPlan {
    let command = PtyCommandPlan {
        program: PathBuf::from(inputs.shell),
        args: task_command_args(inputs.shell, inputs.command),
        env: inputs.roux_env.to_vec(),
        env_remove: Vec::new(),
        cwd: inputs.working_dir.to_string(),
    };

    PtySpawnPlan { size: pty_dimensions(inputs.initial_size), command }
}

fn append_shell_command_flags(args: &mut Vec<String>, shell: &str) {
    #[cfg(windows)]
    {
        let shell_lower = shell.to_ascii_lowercase();
        if shell_lower.contains("pwsh") || shell_lower.contains("powershell") {
            args.push("-NoLogo".to_string());
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (args, shell);
    }
}

fn task_command_args(shell: &str, command: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn roux_env() -> Vec<(String, String)> {
        vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("ROUX_SOCKET".to_string(), "/tmp/roux.sock".to_string()),
            ("ROUX_CLI".to_string(), "/roux/bin/roux".to_string()),
            ("ROUX_SESSION_ID".to_string(), "session-a".to_string()),
            ("ROUX_AGENT_ALIAS".to_string(), "builder".to_string()),
        ]
    }

    #[test]
    fn pty_dimensions_default_and_zero_safe() {
        assert_eq!(
            pty_dimensions(None),
            PtyDimensions { cols: DEFAULT_PTY_COLS, rows: DEFAULT_PTY_ROWS }
        );
        assert_eq!(pty_dimensions(Some((0, 0))), PtyDimensions { cols: 1, rows: 1 });
    }

    #[test]
    fn shell_plan_uses_configured_shell() {
        let env = roux_env();

        let plan = shell_spawn_plan(ShellSpawnPlanInputs {
            working_dir: "/work/project",
            shell: "/bin/zsh",
            roux_env: &env,
            worktree_path: None,
            initial_size: Some((132, 37)),
        });

        assert_eq!(plan.size, PtyDimensions { cols: 132, rows: 37 });
        assert_eq!(plan.command.program, PathBuf::from("/bin/zsh"));
        assert!(plan.command.args.is_empty());
        assert_eq!(plan.command.env, env);
        assert_eq!(plan.command.cwd, "/work/project");
    }

    #[cfg(not(windows))]
    #[test]
    fn task_plan_uses_shell_command_on_unix() {
        let env = roux_env();

        let plan = task_spawn_plan(TaskSpawnPlanInputs {
            command: "cargo test",
            working_dir: "/work/project",
            shell: "/bin/zsh",
            roux_env: &env,
            worktree_path: None,
            initial_size: Some((90, 20)),
        });

        assert_eq!(plan.size, PtyDimensions { cols: 90, rows: 20 });
        assert_eq!(plan.command.program, PathBuf::from("/bin/zsh"));
        assert_eq!(plan.command.args, vec!["-c", "cargo test"]);
        assert_eq!(plan.command.env, env);
    }
}
