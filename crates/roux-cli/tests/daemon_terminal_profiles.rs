#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use roux_core::{
    ProfileSource, RouxSettings, SpawnProfile, SplitProfileBehavior, StartupBehavior,
    TerminalDefaults, TerminalEnvRule, TerminalEnvRuleMode, TerminalEnvRuleSpec,
};
use roux_sdk::{CommandRequest, Pty, PtySnapshot, Roux, SocketEndpoint};
use serde_json::Value;

struct DaemonGuard {
    child: Child,
}

impl DaemonGuard {
    fn new(base_path: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_roux"))
            .arg("daemon")
            .env("ROUX_BASE_PATH", base_path)
            .env_remove("ROUX_SOCKET")
            .env_remove("ROUX_DAEMON_BIND")
            .env_remove("ROUX_DAEMON_TOKEN")
            .env_remove("ROUX_AUTH_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn roux daemon");
        Self { child }
    }

    async fn stop(mut self, client: &Roux) {
        let _: Value = client.command(CommandRequest::new("daemon-stop")).await.unwrap();
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            if self.child.try_wait().expect("poll daemon child").is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("daemon-stop was acknowledged but process did not exit");
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_process_resolves_terminal_defaults_profiles_and_launch_env() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("preflight.log");
    write_settings(dir.path(), &marker);

    let guard = DaemonGuard::new(dir.path());
    let client = Roux::builder()
        .endpoint(SocketEndpoint::Unix(dir.path().join("roux.sock")))
        .connect()
        .unwrap();
    wait_for_daemon(&client, dir.path()).await;

    let mut launch_env = BTreeMap::new();
    launch_env.insert("SHARED".to_string(), TerminalEnvRule::value("launch"));
    launch_env.insert("LAUNCH_ONLY".to_string(), TerminalEnvRule::value("launch"));
    let profile_output = client
        .spawn_task(sh_command(
            "printf 'SHARED=%s\nGLOBAL_ONLY=%s\nPROFILE_ONLY=%s\nCOMMAND_VALUE=%s\nUNSET_ME=%s\nLAUNCH_ONLY=%s\n' \"$SHARED\" \"$GLOBAL_ONLY\" \"$PROFILE_ONLY\" \"$COMMAND_VALUE\" \"${UNSET_ME-unset}\" \"$LAUNCH_ONLY\"",
        ))
        .id("registered-profile-task")
        .working_dir(dir.path().to_string_lossy())
        .profile("prod")
        .env_overrides(launch_env)
        .spawn()
        .await
        .unwrap();
    let profile_snapshot =
        wait_for_pty_exit(&profile_output, "LAUNCH_ONLY=launch", Duration::from_secs(5)).await;

    assert_output_contains(
        &profile_snapshot.output,
        &[
            "SHARED=launch",
            "GLOBAL_ONLY=global",
            "PROFILE_ONLY=profile",
            "COMMAND_VALUE=profile-command",
            "UNSET_ME=unset",
            "LAUNCH_ONLY=launch",
        ],
    );

    let inline_output = client
        .spawn_task(sh_command("printf 'SHARED=%s\nINLINE_ONLY=%s\n' \"$SHARED\" \"$INLINE_ONLY\""))
        .id("inline-profile-task")
        .working_dir(dir.path().to_string_lossy())
        .profile_data(inline_profile(&marker))
        .spawn()
        .await
        .unwrap();
    let inline_snapshot =
        wait_for_pty_exit(&inline_output, "INLINE_ONLY=inline", Duration::from_secs(5)).await;

    assert_output_contains(&inline_snapshot.output, &["SHARED=inline", "INLINE_ONLY=inline"]);

    let preflight_log = std::fs::read_to_string(&marker).unwrap();
    assert_output_contains(
        &preflight_log,
        &["global-preflight", "profile-preflight", "inline-preflight"],
    );

    guard.stop(&client).await;
}

fn write_settings(base_path: &Path, marker: &Path) {
    let settings = RouxSettings {
        shell_binary_path: Some("/bin/sh".to_string()),
        terminal_defaults: TerminalDefaults {
            env: Some(BTreeMap::from([
                ("GLOBAL_ONLY".to_string(), TerminalEnvRule::value("global")),
                ("SHARED".to_string(), TerminalEnvRule::value("global")),
                ("UNSET_ME".to_string(), TerminalEnvRule::value("global")),
            ])),
            before_shell_starts: Some(format!(
                "printf 'global-preflight\n' >> {}",
                shell_quote_path(marker)
            )),
            split_profile_behavior: SplitProfileBehavior::PlainShell,
        },
        spawn_profiles: vec![registered_profile(marker)],
        ..Default::default()
    };

    std::fs::create_dir_all(base_path).unwrap();
    std::fs::write(
        base_path.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
}

fn registered_profile(marker: &Path) -> SpawnProfile {
    SpawnProfile {
        id: "prod".to_string(),
        name: "Prod".to_string(),
        setup_command: None,
        startup_command: None,
        startup_behavior: Some(StartupBehavior::AutoRun),
        env: Some(BTreeMap::from([
            ("PROFILE_ONLY".to_string(), TerminalEnvRule::value("profile")),
            ("SHARED".to_string(), TerminalEnvRule::value("profile")),
            ("COMMAND_VALUE".to_string(), TerminalEnvRule::command("printf profile-command")),
            ("UNSET_ME".to_string(), rule(TerminalEnvRuleMode::Unset)),
        ])),
        before_shell_starts: Some(format!(
            "printf 'profile-preflight\n' >> {}",
            shell_quote_path(marker)
        )),
        cwd_override: None,
        icon: None,
        provider: None,
        source: ProfileSource::User,
    }
}

fn inline_profile(marker: &Path) -> SpawnProfile {
    SpawnProfile {
        id: "inline-test".to_string(),
        name: "Inline Test".to_string(),
        setup_command: None,
        startup_command: None,
        startup_behavior: Some(StartupBehavior::AutoRun),
        env: Some(BTreeMap::from([
            ("INLINE_ONLY".to_string(), TerminalEnvRule::value("inline")),
            ("SHARED".to_string(), TerminalEnvRule::value("inline")),
        ])),
        before_shell_starts: Some(format!(
            "printf 'inline-preflight\n' >> {}",
            shell_quote_path(marker)
        )),
        cwd_override: None,
        icon: None,
        provider: None,
        source: ProfileSource::Inline,
    }
}

fn rule(mode: TerminalEnvRuleMode) -> TerminalEnvRule {
    TerminalEnvRule::Structured(TerminalEnvRuleSpec { mode, value: None, command: None })
}

async fn wait_for_daemon(client: &Roux, base_path: &Path) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        match client.status().await {
            Ok(_) => return,
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    let log = std::fs::read_to_string(base_path.join("logs/roux-daemon.log"))
        .unwrap_or_else(|_| "<missing daemon log>".to_string());
    panic!("daemon did not become ready; log:\n{log}");
}

async fn wait_for_pty_exit(pty: &Pty, needle: &str, timeout: Duration) -> PtySnapshot {
    let started = Instant::now();
    let mut last: Option<PtySnapshot> = None;
    while started.elapsed() < timeout {
        let snapshot = pty.snapshot(65_536).await.unwrap();
        if !snapshot.record.running && snapshot.output.contains(needle) {
            return snapshot;
        }
        last = Some(snapshot);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let output = last.map(|snapshot| snapshot.output).unwrap_or_default();
    panic!("PTY did not exit with expected output {needle:?}; last output:\n{output}");
}

fn assert_output_contains(output: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            output.contains(needle),
            "expected output to contain {needle:?}; output:\n{output}"
        );
    }
}

fn sh_command(script: &str) -> String {
    format!("/bin/sh -c {}", shell_quote_str(script))
}

fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    shell_quote_str(value.as_ref())
}

fn shell_quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
