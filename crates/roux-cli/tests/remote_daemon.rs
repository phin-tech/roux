use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[test]
fn daemon_connect_persists_tcp_endpoint_used_by_normal_cli_commands() {
    let daemon_dir = tempfile::tempdir().unwrap();
    let client_dir = tempfile::tempdir().unwrap();
    let roux = PathBuf::from(env!("CARGO_BIN_EXE_roux"));
    let mut daemon = DaemonProcess::spawn(&roux, daemon_dir.path());

    let socket = wait_for_file_contents(&daemon_dir.path().join("roux-socket-addr"));
    let token = wait_for_file_contents(&daemon_dir.path().join("roux-socket-token"));

    let connect = run_roux(
        &roux,
        client_dir.path(),
        &["daemon", "connect", socket.trim(), "--auth-token", token.trim()],
    );
    assert_success(&connect, "daemon connect");

    let status = run_roux(&roux, client_dir.path(), &["daemon", "status"]);
    assert_success(&status, "daemon status");
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"kind\": \"roux-daemon\""));

    let sessions = run_roux(&roux, client_dir.path(), &["session", "list"]);
    assert_success(&sessions, "session list");
    assert!(String::from_utf8_lossy(&sessions.stdout).trim().starts_with('['));

    daemon.stop_with_client_config(client_dir.path());
}

struct DaemonProcess {
    roux: PathBuf,
    child: Option<Child>,
}

impl DaemonProcess {
    fn spawn(roux: &Path, daemon_base_path: &Path) -> Self {
        let child = Command::new(roux)
            .arg("daemon")
            .env("ROUX_BASE_PATH", daemon_base_path)
            .env("ROUX_DAEMON_BIND", "tcp://127.0.0.1:0")
            .env_remove("ROUX_SOCKET")
            .env_remove("ROUX_AUTH_TOKEN")
            .env_remove("ROUX_DAEMON_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn daemon");

        Self { roux: roux.to_path_buf(), child: Some(child) }
    }

    fn stop_with_client_config(&mut self, client_base_path: &Path) {
        let _ = run_roux(&self.roux, client_base_path, &["daemon", "stop"]);
        self.wait_or_kill();
    }

    fn wait_or_kill(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            match child.try_wait() {
                Ok(Some(_)) => {
                    let _ = child.wait();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }

        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        self.wait_or_kill();
    }
}

fn run_roux(roux: &Path, base_path: &Path, args: &[&str]) -> Output {
    Command::new(roux)
        .args(args)
        .env("ROUX_BASE_PATH", base_path)
        .env_remove("ROUX_SOCKET")
        .env_remove("ROUX_AUTH_TOKEN")
        .env_remove("ROUX_DAEMON_TOKEN")
        .output()
        .expect("run roux command")
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_for_file_contents(path: &Path) -> String {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if let Ok(contents) = std::fs::read_to_string(path) {
            if !contents.trim().is_empty() {
                return contents;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {}", path.display());
}
