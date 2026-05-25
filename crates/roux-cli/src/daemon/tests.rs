use super::streams::{
    handle_alias_events_stream, handle_daemon_pty_attach_stream, handle_mailbox_events_stream,
    handle_subscription_events_stream, handle_watch_events_stream,
};
use super::*;
use roux_core::EventBuilder;
use roux_runtime::alias_store::BindRequest;
use tokio::io::AsyncWriteExt;

fn make_session(id: &str) -> roux_core::Session {
    roux_core::Session {
        id: id.to_string(),
        name: format!("Session {id}"),
        repo_root: "/tmp/repo".to_string(),
        worktree_path: "/tmp/repo".to_string(),
        branch: "main".to_string(),
        is_worktree: false,
        status: roux_core::SessionStatus::Disconnected,
        model: None,
        cost: None,
        created_at: 0,
        project_id: None,
        is_git_repo: false,
        name_override: None,
        primary_pty_id: None,
        archived: false,
        ended_at: None,
        blueprint_id: None,
        pinned_pr_url: None,
        smol_machine_name: None,
    }
}

fn make_watch_config() -> roux_core::CreateWatchConfig {
    roux_core::CreateWatchConfig {
        name: "HTTP".to_string(),
        kind: roux_core::WatchKind::HttpHealth {
            url: "http://localhost".to_string(),
            expected_status: 200,
        },
        mode: roux_core::WatchMode::Recurring { interval_secs: 30 },
        scope: roux_core::WatchScope::Global,
        notify: None,
    }
}

#[tokio::test]
async fn daemon_status_is_daemon_only_socket_command() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);

    let response = handle_request(
        Request {
            command: "daemon-status".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::Value::Null,
        },
        &host,
        &DaemonIdentity::new_for_test("/tmp/roux.sock"),
    )
    .await;

    assert!(response.ok);
    let data = response.data.expect("status payload");
    assert_eq!(data["kind"], "roux-daemon");
    assert_eq!(data["socket"], "/tmp/roux.sock");
    assert_eq!(data["logPath"], "/tmp/roux-daemon.log");
    assert_eq!(data["processCount"], 0);
    assert!(data["capabilities"].as_array().unwrap().contains(&serde_json::json!("daemon-status")));
    assert!(data["capabilities"].as_array().unwrap().contains(&serde_json::json!("daemon-stop")));
    assert!(data["capabilities"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("daemon-pty-attach")));
    assert!(data["capabilities"].as_array().unwrap().contains(&serde_json::json!("worktree-list")));
    assert!(data["capabilities"].as_array().unwrap().contains(&serde_json::json!("watch-list")));
    assert!(data["capabilities"].as_array().unwrap().contains(&serde_json::json!("watch-events")));
    assert!(data["capabilities"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("mailbox-events")));
    assert!(data["capabilities"].as_array().unwrap().contains(&serde_json::json!("alias-events")));
    assert!(data["capabilities"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("subscription-events")));

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_watch_commands_mutate_runtime_state() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let watch_runner = WatchRunner::new(
        host.watch_handle.clone(),
        AutomationHookManager::from_config_root(dir.path()),
    );
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create = handle_request_with_watch_runner(
        Request {
            command: "watch-create".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "config": make_watch_config() }),
        },
        &host,
        Some(&watch_runner),
        &identity,
    )
    .await;
    assert!(create.ok, "create failed: {:?}", create.error);
    let created: roux_core::Watch =
        serde_json::from_value(create.data.clone().expect("created watch")).unwrap();
    assert!(matches!(created.runtime_state, roux_core::RuntimeState::Active));

    let list = handle_request(
        Request {
            command: "watch-list".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::Value::Null,
        },
        &host,
        &identity,
    )
    .await;
    assert!(list.ok, "list failed: {:?}", list.error);
    assert_eq!(list.data.as_ref().unwrap().as_array().unwrap().len(), 1);

    let pause = handle_request_with_watch_runner(
        Request {
            command: "watch-pause".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": created.id }),
        },
        &host,
        Some(&watch_runner),
        &identity,
    )
    .await;
    assert!(pause.ok, "pause failed: {:?}", pause.error);
    assert_eq!(pause.data.as_ref().unwrap()["runtimeState"]["type"], "paused");

    let mut replacement: roux_core::Watch =
        serde_json::from_value(pause.data.clone().expect("paused watch")).unwrap();
    replacement.name = "Updated by client".to_string();
    let replace = handle_request_with_watch_runner(
        Request {
            command: "watch-replace".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "watch": replacement }),
        },
        &host,
        Some(&watch_runner),
        &identity,
    )
    .await;
    assert!(replace.ok, "replace failed: {:?}", replace.error);
    assert_eq!(replace.data.as_ref().unwrap()["name"], "Updated by client");

    let resume = handle_request_with_watch_runner(
        Request {
            command: "watch-resume".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": replace.data.as_ref().unwrap()["id"] }),
        },
        &host,
        Some(&watch_runner),
        &identity,
    )
    .await;
    assert!(resume.ok, "resume failed: {:?}", resume.error);
    assert_eq!(resume.data.as_ref().unwrap()["runtimeState"]["type"], "active");

    let remove = handle_request_with_watch_runner(
        Request {
            command: "watch-remove".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": resume.data.as_ref().unwrap()["id"] }),
        },
        &host,
        Some(&watch_runner),
        &identity,
    )
    .await;
    assert!(remove.ok, "remove failed: {:?}", remove.error);
    assert!(host.watch_handle.list().await.unwrap().is_empty());

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_mailbox_events_streams_live_mailbox_events() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
    let (mut server, client) = tokio::io::duplex(4096);
    let identity_for_stream = identity.clone();
    let stream_task = tokio::spawn(async move {
        handle_mailbox_events_stream(
            Request {
                command: "mailbox-events".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::Value::Null,
            },
            &mut server,
            &identity_for_stream,
        )
        .await
    });

    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(ready["type"], "ready");

    identity.mailbox_manager.post(EventBuilder::new("hello").to("auditor")).unwrap();

    line.clear();
    reader.read_line(&mut line).await.unwrap();
    let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(frame["type"], "event");
    assert_eq!(frame["event"]["kind"], "posted");
    assert_eq!(frame["event"]["event"]["body"], "hello");
    assert_eq!(frame["event"]["event"]["to"], "auditor");

    stream_task.abort();
}

#[tokio::test]
async fn daemon_subscription_events_streams_live_subscription_events() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
    let (mut server, client) = tokio::io::duplex(4096);
    let identity_for_stream = identity.clone();
    let stream_task = tokio::spawn(async move {
        handle_subscription_events_stream(
            Request {
                command: "subscription-events".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::Value::Null,
            },
            &mut server,
            &identity_for_stream,
        )
        .await
    });

    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(ready["type"], "ready");

    identity.subscription_manager.subscribe("auditor", "build.**", None).unwrap();

    line.clear();
    reader.read_line(&mut line).await.unwrap();
    let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(frame["type"], "event");
    assert_eq!(frame["event"]["kind"], "created");
    assert_eq!(frame["event"]["subscription"]["alias"], "auditor");
    assert_eq!(frame["event"]["subscription"]["pattern"], "build.**");

    stream_task.abort();
}

#[tokio::test]
async fn daemon_alias_events_streams_live_alias_events() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
    let (mut server, client) = tokio::io::duplex(4096);
    let identity_for_stream = identity.clone();
    let stream_task = tokio::spawn(async move {
        handle_alias_events_stream(
            Request {
                command: "alias-events".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::Value::Null,
            },
            &mut server,
            &identity_for_stream,
        )
        .await
    });

    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(ready["type"], "ready");

    identity
        .alias_manager
        .bind(
            "reviewer",
            BindRequest { session_id: Some("session-1".into()), ..Default::default() },
        )
        .unwrap();

    line.clear();
    reader.read_line(&mut line).await.unwrap();
    let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(frame["type"], "event");
    assert_eq!(frame["event"]["kind"], "set");
    assert_eq!(frame["event"]["alias"]["alias"], "reviewer");
    assert_eq!(frame["event"]["alias"]["sessionId"], "session-1");

    stream_task.abort();
}

#[tokio::test]
async fn daemon_watch_events_stream_sends_ready_and_backlog() {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let mut watch = roux_core::Watch {
        id: "watch-a".to_string(),
        name: "HTTP".to_string(),
        kind: roux_core::WatchKind::HttpHealth {
            url: "http://localhost".to_string(),
            expected_status: 200,
        },
        mode: roux_core::WatchMode::Recurring { interval_secs: 30 },
        scope: roux_core::WatchScope::Global,
        runtime_state: roux_core::RuntimeState::Paused,
        last_result: None,
        last_checked: None,
        notify: roux_core::NotifyConfig::default(),
        created_at: 0,
    };
    watch.last_checked = Some(1);
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: vec![watch],
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let watch_runner = WatchRunner::new(
        host.watch_handle.clone(),
        AutomationHookManager::from_config_root(dir.path()),
    );
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
    let (mut server, client) = tokio::io::duplex(4096);
    let host_for_stream = host.clone();
    let runner_for_stream = watch_runner.clone();
    let identity_for_stream = identity.clone();
    let stream_task = tokio::spawn(async move {
        handle_watch_events_stream(
            Request {
                command: "watch-events".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "backlog": true }),
            },
            &mut server,
            &host_for_stream,
            &runner_for_stream,
            &identity_for_stream,
        )
        .await
    });

    let mut reader = BufReader::new(client);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let ready: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(ready["type"], "ready");

    line.clear();
    reader.read_line(&mut line).await.unwrap();
    let update: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(update["type"], "update");
    assert_eq!(update["event"]["watch"]["id"], "watch-a");
    assert_eq!(update["event"]["changed"], false);

    stream_task.abort();
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_session_rename_mutates_runtime_state() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: vec![make_session("s1")],
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);

    let response = handle_request(
        Request {
            command: "session-rename".to_string(),
            session_id: Some("s1".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "name": "Daemon owned" }),
        },
        &host,
        &DaemonIdentity::new_for_test("/tmp/roux.sock"),
    )
    .await;

    assert!(response.ok);
    let session = host.session_handle.get("s1").await.unwrap().unwrap();
    assert_eq!(session.name_override.as_deref(), Some("Daemon owned"));

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

fn git(repo: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("failed to invoke git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(repo: &std::path::Path) {
    std::fs::create_dir_all(repo).unwrap();
    git(repo, &["init", "-q", "-b", "main"]);
    git(repo, &["config", "user.email", "t@t.test"]);
    git(repo, &["config", "user.name", "Test"]);
    git(repo, &["commit", "--allow-empty", "-m", "init"]);
}

fn shell_quote(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

async fn wait_for_marker(path: &std::path::Path, expected: &str) {
    for _ in 0..200 {
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if content.contains(expected) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("marker {} did not contain {expected:?}", path.display());
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_hook_commands_use_daemon_hook_manager() {
    let dir = tempfile::tempdir().unwrap();
    let hook_root = dir.path().join("hook-root");
    std::fs::create_dir_all(&hook_root).unwrap();
    std::fs::write(hook_root.join("hooks.toml"), r#"post-task-run = "true""#).unwrap();

    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join(".config").join("roux")).unwrap();
    std::fs::write(
        repo.join(".config").join("roux").join("hooks.toml"),
        r#"pre-watch-run = "cat >/dev/null""#,
    )
    .unwrap();

    let hooks = AutomationHookManager::from_config_root(&hook_root);
    let show = handle_hook_show_with_hooks(
        Request {
            command: "hook-show".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "repoPath": repo }),
        },
        hooks.clone(),
    )
    .await;
    assert!(show.ok, "hook-show failed: {:?}", show.error);
    let shown = show.data.as_ref().unwrap().as_array().unwrap();
    assert!(shown.iter().any(|item| item["source"] == "user"));
    assert!(shown.iter().any(|item| item["source"] == "project"));

    let preview_args = serde_json::json!({
        "event": "pre-watch-run",
        "repoPath": repo,
    });
    let preview = handle_hook_preview_with_hooks(
        Request {
            command: "hook-preview".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: preview_args.clone(),
        },
        hooks.clone(),
    )
    .await;
    assert!(preview.ok, "hook-preview failed: {:?}", preview.error);
    let preview_items = preview.data.as_ref().unwrap().as_array().unwrap();
    let approval_id =
        preview_items[0]["approvalId"].as_str().expect("project hook approval id").to_string();
    assert_eq!(preview_items[0]["approved"], false);

    let rejected = handle_hook_run_with_hooks(
        Request {
            command: "hook-run".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: preview_args.clone(),
        },
        hooks.clone(),
    )
    .await;
    assert!(!rejected.ok);
    assert!(rejected.error.as_deref().unwrap_or_default().contains("approval"));

    let approved = handle_hook_approve_with_hooks(
        Request {
            command: "hook-approve".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "approvalId": approval_id }),
        },
        hooks.clone(),
    )
    .await;
    assert!(approved.ok, "hook-approve failed: {:?}", approved.error);

    let run = handle_hook_run_with_hooks(
        Request {
            command: "hook-run".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: preview_args,
        },
        hooks.clone(),
    )
    .await;
    assert!(run.ok, "hook-run failed: {:?}", run.error);
    assert_eq!(run.data.as_ref().unwrap()["event"], "pre-watch-run");
    assert_eq!(run.data.as_ref().unwrap()["ran"], 1);

    let logs = handle_hook_log_list_with_hooks(hooks.clone()).await;
    assert!(logs.ok, "hook-log-list failed: {:?}", logs.error);
    let log_items = logs.data.as_ref().unwrap().as_array().unwrap();
    assert_eq!(log_items.len(), 1);
    let log_path = log_items[0]["path"].as_str().expect("hook log path").to_string();
    let log = handle_hook_log_read_with_hooks(
        Request {
            command: "hook-log-read".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "path": log_path }),
        },
        hooks.clone(),
    )
    .await;
    assert!(log.ok, "hook-log-read failed: {:?}", log.error);
    assert!(log.data.as_ref().unwrap().as_str().unwrap().contains("pre-watch-run"));

    let cleared = handle_hook_clear_approvals_with_hooks(hooks).await;
    assert!(cleared.ok, "hook-clear-approvals failed: {:?}", cleared.error);
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_worktree_commands_mutate_git_worktrees() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init_repo(&repo);
    let worktree_base = dir.path().join("worktrees");
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let before = handle_request(
        Request {
            command: "worktree-list".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "repoPath": repo }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(before.ok, "list failed: {:?}", before.error);
    assert_eq!(before.data.as_ref().unwrap().as_array().unwrap().len(), 1);

    let create = handle_request(
        Request {
            command: "worktree-create".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "repoPath": repo,
                "branch": "feature/daemon-worktree",
                "startPoint": "main",
                "basePath": worktree_base,
                "fetchFirst": false,
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create.ok, "create failed: {:?}", create.error);
    let worktree_path = create.data.as_ref().unwrap()["path"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&worktree_path).exists());

    let branches = handle_request(
        Request {
            command: "worktree-list-branches".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "repoPath": repo }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(branches.ok, "branches failed: {:?}", branches.error);
    assert!(branches
        .data
        .as_ref()
        .unwrap()
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("feature/daemon-worktree")));

    let after_create = handle_request(
        Request {
            command: "worktree-list".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "repoPath": repo }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(after_create.ok, "list after create failed: {:?}", after_create.error);
    assert!(after_create.data.as_ref().unwrap().as_array().unwrap().iter().any(|entry| {
        entry["branch"] == "feature/daemon-worktree" && entry["path"] == worktree_path
    }));

    let remove = handle_request(
        Request {
            command: "worktree-remove".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "repoPath": repo,
                "worktreePath": worktree_path,
                "alsoBranch": true,
                "force": true,
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(remove.ok, "remove failed: {:?}", remove.error);
    assert!(!std::path::Path::new(&worktree_path).exists());

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_worktree_commands_run_hooks_server_side() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init_repo(&repo);
    let worktree_base = dir.path().join("worktrees");
    let hook_root = dir.path().join("hook-root");
    std::fs::create_dir_all(&hook_root).unwrap();
    let marker = dir.path().join("hook-events.txt");
    let marker_arg = shell_quote(&marker);
    let hooks_toml = format!(
        r#"
pre-worktree-create = "{pre_create}"
post-worktree-create = "{post_create}"
pre-worktree-remove = "{pre_remove}"
post-worktree-remove = "{post_remove}"
"#,
        pre_create = toml_escape(&format!("printf pre-create >> {marker_arg}")),
        post_create = toml_escape(&format!("printf post-create >> {marker_arg}")),
        pre_remove = toml_escape(&format!("printf pre-remove >> {marker_arg}")),
        post_remove = toml_escape(&format!("printf post-remove >> {marker_arg}")),
    );
    std::fs::write(hook_root.join("hooks.toml"), hooks_toml).unwrap();
    let hooks = AutomationHookManager::from_config_root(&hook_root);
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);

    let create = handle_worktree_create_with_hooks(
        Request {
            command: "worktree-create".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "repoPath": repo,
                "branch": "feature/hooked-worktree",
                "startPoint": "main",
                "basePath": worktree_base,
            }),
        },
        hooks.clone(),
    )
    .await;
    assert!(create.ok, "create failed: {:?}", create.error);
    let worktree_path = create.data.as_ref().unwrap()["path"].as_str().unwrap().to_string();
    wait_for_marker(&marker, "pre-create").await;
    wait_for_marker(&marker, "post-create").await;

    let remove = handle_worktree_remove_with_hooks(
        Request {
            command: "worktree-remove".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "repoPath": repo,
                "worktreePath": worktree_path,
                "alsoBranch": true,
                "force": true,
            }),
        },
        hooks,
    )
    .await;
    assert!(remove.ok, "remove failed: {:?}", remove.error);
    wait_for_marker(&marker, "pre-remove").await;
    wait_for_marker(&marker, "post-remove").await;

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_session_worktree_target_runs_hooks_server_side() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init_repo(&repo);
    let worktree_base = dir.path().join("worktrees");
    let hook_root = dir.path().join("hook-root");
    std::fs::create_dir_all(&hook_root).unwrap();
    let marker = dir.path().join("session-worktree-hook-events.txt");
    let marker_arg = shell_quote(&marker);
    let hooks_toml = format!(
        r#"
pre-worktree-create = "{pre_create}"
post-worktree-create = "{post_create}"
"#,
        pre_create = toml_escape(&format!("printf pre-create >> {marker_arg}")),
        post_create = toml_escape(&format!("printf post-create >> {marker_arg}")),
    );
    std::fs::write(hook_root.join("hooks.toml"), hooks_toml).unwrap();
    let hooks = AutomationHookManager::from_config_root(&hook_root);
    let settings = roux_core::RouxSettings {
        worktree_base_path: Some(worktree_base.to_string_lossy().into_owned()),
        ..Default::default()
    };

    let (worktree_path, branch, owns_worktree) = resolve_daemon_session_target(
        &repo.to_string_lossy(),
        DaemonSessionTarget::NewWorktree {
            branch: "feature/session-hooked-worktree".to_string(),
            start_point: Some("main".to_string()),
            fetch_first: false,
        },
        &settings,
        hooks,
    )
    .await
    .expect("session target should create worktree");

    assert_eq!(branch, "feature/session-hooked-worktree");
    assert!(owns_worktree);
    assert!(std::path::Path::new(&worktree_path).exists());
    wait_for_marker(&marker, "pre-create").await;
    wait_for_marker(&marker, "post-create").await;
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_session_create_shell_owns_session_and_primary_pty() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let response = handle_request(
        Request {
            command: "session-create-shell".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "session-a",
                "repoPath": dir.path(),
                "name": "Daemon Session",
                "profile": "plain-shell",
                "initialSize": [100, 30]
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(response.ok, "create failed: {:?}", response.error);
    let data = response.data.expect("session payload");
    assert_eq!(data["id"], "session-a");
    assert_eq!(data["name"], "Daemon Session");
    assert_eq!(data["primaryPtyId"], "session-a");

    let session = host.session_handle.get("session-a").await.unwrap().unwrap();
    assert_eq!(session.name, "Daemon Session");
    assert_eq!(session.primary_pty_id.as_deref(), Some("session-a"));

    let ptys = host.pty_handle.list().await.unwrap();
    let pty = ptys.iter().find(|pty| pty.id == "session-a").expect("primary pty");
    assert_eq!(pty.working_dir, dir.path().to_string_lossy());
    assert_eq!(pty.cols, 100);
    assert_eq!(pty.rows, 30);
    assert!(matches!(pty.info.role, roux_core::PtyRole::SessionPrimary));
    assert_eq!(pty.info.session_id.as_deref(), Some("session-a"));

    let _ = host.pty_handle.kill("session-a").await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_session_create_alias_creates_daemon_session() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create = handle_request(
        Request {
            command: "session-create".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "name": "Created from CLI",
                "working_dir": dir.path(),
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create.ok, "session-create alias failed: {:?}", create.error);
    let session_id = create.data.as_ref().unwrap()["session_id"].as_str().unwrap();
    let session = host.session_handle.get(session_id).await.unwrap().expect("session");
    assert_eq!(session.name, "Created from CLI");
    assert_eq!(session.primary_pty_id.as_deref(), Some(session_id));

    let _ = host.pty_handle.kill(session_id).await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_session_create_alias_rejects_prompt_until_attach_queue_exists() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create = handle_request(
        Request {
            command: "session-create".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "working_dir": dir.path(),
                "prompt": "do the thing",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(!create.ok);
    assert!(create.error.as_deref().unwrap_or("").contains("--prompt"));

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_session_panes_create_spawns_secondary_pty_and_list_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create_session = handle_request(
        Request {
            command: "session-create-shell".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "session-panes",
                "repoPath": dir.path(),
                "name": "Pane Session",
                "profile": "plain-shell",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create_session.ok, "session create failed: {:?}", create_session.error);

    let create_pane = handle_request(
        Request {
            command: "session-panes-create".to_string(),
            session_id: Some("session-panes".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "profile": "plain-shell",
                "direction": "vertical",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create_pane.ok, "pane create failed: {:?}", create_pane.error);
    let pane_id = create_pane.data.as_ref().unwrap()["pane_id"].as_str().unwrap().to_string();
    let pty_id = create_pane.data.as_ref().unwrap()["pty_id"].as_str().unwrap().to_string();

    let ptys = host.pty_handle.list().await.unwrap();
    let pty = ptys.iter().find(|pty| pty.id == pty_id).expect("secondary pty");
    assert_eq!(pty.info.session_id.as_deref(), Some("session-panes"));
    assert!(matches!(pty.info.role, roux_core::PtyRole::Secondary));
    assert_eq!(pty.info.profile.as_deref(), Some("plain-shell"));
    assert!(pty_matches_pane(pty, &pane_id));

    let list = handle_request(
        Request {
            command: "session-panes-list".to_string(),
            session_id: Some("session-panes".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(list.ok, "pane list failed: {:?}", list.error);
    let data = list.data.as_ref().unwrap();
    assert_eq!(data["sessionId"], "session-panes");
    assert!(data["layout"].is_null());
    assert!(data["descriptors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|descriptor| descriptor["id"] == pane_id && descriptor["ptyId"] == pty_id));

    let _ = host.pty_handle.kill("session-panes").await;
    let _ = host.pty_handle.kill(&pty_id).await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_top_level_shell_spawns_secondary_pty() {
    let dir = tempfile::tempdir().unwrap();
    let alternate_dir = dir.path().join("alternate");
    std::fs::create_dir_all(&alternate_dir).unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create_session = handle_request(
        Request {
            command: "session-create-shell".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "session-shell",
                "repoPath": dir.path(),
                "name": "Shell Session",
                "profile": "plain-shell",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create_session.ok, "session create failed: {:?}", create_session.error);

    let shell = handle_request(
        Request {
            command: "shell".to_string(),
            session_id: Some("session-shell".to_string()),
            pane_id: Some("session-shell-main".to_string()),
            auth_token: None,
            args: serde_json::json!({ "working_dir": alternate_dir }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(shell.ok, "shell failed: {:?}", shell.error);
    let pty_id = shell.data.as_ref().unwrap()["pty_id"].as_str().unwrap().to_string();

    let pty = host
        .pty_handle
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|pty| pty.id == pty_id)
        .expect("shell pty");
    assert_eq!(pty.info.session_id.as_deref(), Some("session-shell"));
    assert!(matches!(pty.info.role, roux_core::PtyRole::Secondary));
    assert_eq!(pty.info.profile.as_deref(), Some("plain-shell"));
    assert_eq!(pty.working_dir, alternate_dir.to_string_lossy());

    let _ = host.pty_handle.kill("session-shell").await;
    let _ = host.pty_handle.kill(&pty_id).await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_top_level_split_spawns_secondary_pty() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create_session = handle_request(
        Request {
            command: "session-create-shell".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "session-split",
                "repoPath": dir.path(),
                "name": "Split Session",
                "profile": "plain-shell",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create_session.ok, "session create failed: {:?}", create_session.error);

    let split = handle_request(
        Request {
            command: "split".to_string(),
            session_id: Some("session-split".to_string()),
            pane_id: Some("session-split-main".to_string()),
            auth_token: None,
            args: serde_json::json!({ "direction": "vertical" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(split.ok, "split failed: {:?}", split.error);
    let pty_id = split.data.as_ref().unwrap()["pty_id"].as_str().unwrap().to_string();

    let pty = host
        .pty_handle
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|pty| pty.id == pty_id)
        .expect("split pty");
    assert_eq!(pty.info.session_id.as_deref(), Some("session-split"));
    assert!(matches!(pty.info.role, roux_core::PtyRole::Secondary));
    assert_eq!(pty.info.profile.as_deref(), Some("plain-shell"));
    assert_eq!(pty.working_dir, dir.path().to_string_lossy());

    let _ = host.pty_handle.kill("session-split").await;
    let _ = host.pty_handle.kill(&pty_id).await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_notes_commands_use_shared_vault_service() {
    let dir = tempfile::tempdir().unwrap();
    let vault_root = dir.path().join("vault");
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");
    let target = serde_json::json!({
        "scope": "global",
        "sessionId": null,
        "topic": null,
        "overrideSlug": null,
    });

    let append = handle_request(
        Request {
            command: "notes-append".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "vaultRoot": vault_root,
                "target": target,
                "content": "daemon note #daemon",
                "timestamped": false,
                "tags": ["daemon"],
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(append.ok, "append failed: {:?}", append.error);

    let read = handle_request(
        Request {
            command: "notes-read".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "vaultRoot": vault_root,
                "scope": "global",
                "sessionId": null,
                "topic": null,
                "overrideSlug": null,
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(read.ok, "read failed: {:?}", read.error);
    assert!(read.data.as_ref().unwrap()["content"].as_str().unwrap().contains("daemon note"));

    let search = handle_request(
        Request {
            command: "notes-search".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "vaultRoot": vault_root,
                "tags": ["daemon"],
                "scope": "global",
                "exact": false,
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(search.ok, "search failed: {:?}", search.error);
    assert_eq!(search.data.as_ref().unwrap().as_array().unwrap().len(), 1);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_latest_output_alias_reads_daemon_pty_replay() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let start = handle_request(
        Request {
            command: "daemon-pty-spawn-task".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "latest-pty",
                "command": "cat",
                "workingDir": dir.path(),
                "sessionId": "session-latest",
                "paneId": "pane-latest",
                "profile": "task",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(start.ok, "spawn task failed: {:?}", start.error);
    let pty_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let write = handle_request(
        Request {
            command: "daemon-pty-write".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": pty_id, "data": "daemon-latest\n" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(write.ok, "write failed: {:?}", write.error);

    let mut latest = None;
    for _ in 0..50 {
        let response = handle_request(
            Request {
                command: "latest-output".to_string(),
                session_id: None,
                pane_id: Some("pane-latest".to_string()),
                auth_token: None,
                args: serde_json::json!({ "max_bytes": 1024 }),
            },
            &host,
            &identity,
        )
        .await;
        if !response.ok {
            assert!(
                response.error.as_deref().unwrap_or("").contains("daemon PTY not found"),
                "latest-output failed: {:?}",
                response.error
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            continue;
        }
        let data = response.data.unwrap();
        if data["text"].as_str().unwrap_or("").contains("daemon-latest") {
            latest = Some(data);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let latest = latest.expect("latest output should include task output");
    assert_eq!(latest["pty_id"], pty_id);
    assert_eq!(latest["pane_id"], "pane-latest");
    assert!(latest["byte_count"].as_u64().unwrap() >= "daemon-latest".len() as u64);
    assert!(!latest["replay_bytes_base64"].as_str().unwrap().is_empty());

    let _ = host.pty_handle.kill(&pty_id).await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_alias_commands_mutate_daemon_alias_state() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let alias_path = dir.path().join("aliases.json");
    let identity =
        DaemonIdentity::new_for_test_with_alias_path("/tmp/roux.sock", alias_path.clone());

    let set = handle_request(
        Request {
            command: "alias-set".to_string(),
            session_id: Some("session-alias".to_string()),
            pane_id: Some("pane-alias".to_string()),
            auth_token: None,
            args: serde_json::json!({ "alias": "reviewer" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(set.ok, "alias-set failed: {:?}", set.error);
    assert_eq!(set.data.as_ref().unwrap()["alias"], "reviewer");
    assert_eq!(set.data.as_ref().unwrap()["sessionId"], "session-alias");
    assert_eq!(set.data.as_ref().unwrap()["paneId"], "pane-alias");

    let get = handle_request(
        Request {
            command: "alias-get".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "alias": "reviewer" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(get.ok, "alias-get failed: {:?}", get.error);
    assert_eq!(get.data.as_ref().unwrap()["sessionId"], "session-alias");

    let reloaded_identity =
        DaemonIdentity::new_for_test_with_alias_path("/tmp/roux.sock", alias_path);
    let reloaded_get = handle_request(
        Request {
            command: "alias-get".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "alias": "reviewer" }),
        },
        &host,
        &reloaded_identity,
    )
    .await;
    assert!(reloaded_get.ok, "alias reload failed: {:?}", reloaded_get.error);
    assert_eq!(reloaded_get.data.as_ref().unwrap()["sessionId"], "session-alias");

    let whoami = handle_request(
        Request {
            command: "alias-whoami".to_string(),
            session_id: Some("session-alias".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(whoami.ok, "alias-whoami failed: {:?}", whoami.error);
    assert_eq!(whoami.data.as_ref().unwrap().as_array().unwrap().len(), 1);

    let unset = handle_request(
        Request {
            command: "alias-unset".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "alias": "reviewer" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(unset.ok, "alias-unset failed: {:?}", unset.error);
    assert_eq!(unset.data.as_ref().unwrap()["changed"], true);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_mailbox_and_bus_commands_mutate_daemon_state() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let alias_path = dir.path().join("aliases.json");
    let subscription_path = dir.path().join("subscriptions.json");
    let mailbox_events_path = dir.path().join("events.jsonl");
    let mailbox_read_state_path = dir.path().join("read_state.json");
    let identity = DaemonIdentity::new_for_test_with_runtime_paths(
        "/tmp/roux.sock",
        alias_path.clone(),
        subscription_path.clone(),
        mailbox_events_path.clone(),
        mailbox_read_state_path.clone(),
    );

    let invalid_subscribe = handle_request(
        Request {
            command: "bus-subscribe".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "alias": "1auditor",
                "pattern": "build.**",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(!invalid_subscribe.ok);
    assert_eq!(
        invalid_subscribe.error.as_deref(),
        Some(
            "alias name '1auditor' has invalid characters; expected lowercase letters, digits, hyphens, starting with a letter"
        )
    );

    let subscribe = handle_request(
        Request {
            command: "bus-subscribe".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "alias": "auditor",
                "pattern": "build.**",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(subscribe.ok, "bus-subscribe failed: {:?}", subscribe.error);
    let subscription_id = subscribe.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let post = handle_request(
        Request {
            command: "mailbox-post".to_string(),
            session_id: Some("session-sender".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "topic": "build.completed",
                "body": "green",
                "from": "builder",
                "kind": "signal",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(post.ok, "mailbox-post failed: {:?}", post.error);
    let event_id = post.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let reloaded_identity = DaemonIdentity::new_for_test_with_runtime_paths(
        "/tmp/roux.sock",
        alias_path,
        subscription_path,
        mailbox_events_path,
        mailbox_read_state_path,
    );

    let read = handle_request(
        Request {
            command: "mailbox-read".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "alias": "auditor" }),
        },
        &host,
        &reloaded_identity,
    )
    .await;
    assert!(read.ok, "mailbox-read failed: {:?}", read.error);
    let events = read.data.as_ref().unwrap().as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["id"], event_id);

    let count = handle_request(
        Request {
            command: "mailbox-count".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "alias": "auditor" }),
        },
        &host,
        &reloaded_identity,
    )
    .await;
    assert!(count.ok, "mailbox-count failed: {:?}", count.error);
    assert_eq!(count.data.as_ref().unwrap()["unread"], 0);

    let unsubscribe = handle_request(
        Request {
            command: "bus-unsubscribe".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": subscription_id }),
        },
        &host,
        &reloaded_identity,
    )
    .await;
    assert!(unsubscribe.ok, "bus-unsubscribe failed: {:?}", unsubscribe.error);
    assert_eq!(unsubscribe.data.as_ref().unwrap()["removed"], true);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_session_lifecycle_commands_mutate_state_and_ptys() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create = handle_request(
        Request {
            command: "session-create-shell".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "session-life",
                "repoPath": dir.path(),
                "name": "Lifecycle",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create.ok, "create failed: {:?}", create.error);

    let reconnect = handle_request(
        Request {
            command: "session-reconnect-shell".to_string(),
            session_id: Some("session-life".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "initialSize": [100, 30] }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(reconnect.ok, "reconnect failed: {:?}", reconnect.error);
    let pty = host
        .pty_handle
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|pty| pty.id == "session-life")
        .expect("primary pty after reconnect");
    assert_eq!(pty.cols, 100);
    assert_eq!(pty.rows, 30);

    let exists = handle_request(
        Request {
            command: "session-worktree-exists".to_string(),
            session_id: Some("session-life".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(exists.ok, "exists failed: {:?}", exists.error);
    assert_eq!(exists.data.as_ref().unwrap()["exists"], true);

    let archive = handle_request(
        Request {
            command: "session-archive".to_string(),
            session_id: Some("session-life".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(archive.ok, "archive failed: {:?}", archive.error);
    assert_eq!(archive.data.as_ref().unwrap()["archived"], true);
    assert!(host
        .pty_handle
        .list()
        .await
        .unwrap()
        .iter()
        .all(|pty| pty.info.session_id.as_deref() != Some("session-life")));

    let restore = handle_request(
        Request {
            command: "session-restore".to_string(),
            session_id: Some("session-life".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(restore.ok, "restore failed: {:?}", restore.error);
    assert_eq!(restore.data.as_ref().unwrap()["archived"], false);
    assert_eq!(restore.data.as_ref().unwrap()["status"], "disconnected");

    let delete = handle_request(
        Request {
            command: "session-delete".to_string(),
            session_id: Some("session-life".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(delete.ok, "delete failed: {:?}", delete.error);
    assert!(host.session_handle.get("session-life").await.unwrap().is_none());

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_session_refresh_branch_updates_git_status_when_repo_is_initialized() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = make_session("s1");
    session.repo_root = dir.path().to_string_lossy().into_owned();
    session.worktree_path = session.repo_root.clone();
    session.is_git_repo = false;

    let services = RuntimeHostConfig {
        initial_sessions: vec![session],
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let init = Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
    assert!(init.status.success(), "git init failed: {}", String::from_utf8_lossy(&init.stderr));

    let refresh = handle_request(
        Request {
            command: "session-refresh-branch".to_string(),
            session_id: Some("s1".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(refresh.ok, "refresh failed: {:?}", refresh.error);

    let refreshed = host.session_handle.get("s1").await.unwrap().unwrap();
    assert!(refreshed.is_git_repo);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_project_and_session_metadata_commands_mutate_runtime_state() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: vec![make_session("s1")],
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let create = handle_request(
        Request {
            command: "project-create".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "name": "Alpha" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(create.ok, "project-create failed: {:?}", create.error);
    let project_id = create.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let update = handle_request(
        Request {
            command: "project-update".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": project_id,
                "patch": { "name": "Beta", "contextPaths": ["/docs"] },
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(update.ok, "project-update failed: {:?}", update.error);
    assert_eq!(update.data.as_ref().unwrap()["name"], "Beta");
    assert_eq!(update.data.as_ref().unwrap()["contextPaths"][0], "/docs");

    let set_project = handle_request(
        Request {
            command: "session-set-project".to_string(),
            session_id: Some("s1".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "projectId": project_id }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(set_project.ok, "session-set-project failed: {:?}", set_project.error);

    let set_pinned = handle_request(
        Request {
            command: "session-set-pinned-pr-url".to_string(),
            session_id: Some("s1".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "url": "https://github.com/o/r/pull/1" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(set_pinned.ok, "session-set-pinned-pr-url failed: {:?}", set_pinned.error);

    let set_smol = handle_request(
        Request {
            command: "session-set-smol-machine".to_string(),
            session_id: Some("s1".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "machineName": "vm-a" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(set_smol.ok, "session-set-smol-machine failed: {:?}", set_smol.error);

    let session = host.session_handle.get("s1").await.unwrap().unwrap();
    assert_eq!(session.project_id.as_deref(), Some(project_id.as_str()));
    assert_eq!(session.pinned_pr_url.as_deref(), Some("https://github.com/o/r/pull/1"));
    assert_eq!(session.smol_machine_name.as_deref(), Some("vm-a"));

    let remove_project = handle_request(
        Request {
            command: "project-remove".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": project_id }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(remove_project.ok, "project-remove failed: {:?}", remove_project.error);
    let session = host.session_handle.get("s1").await.unwrap().unwrap();
    assert!(session.project_id.is_none());

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[tokio::test]
async fn daemon_session_kill_alias_archives_session() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: vec![make_session("session-kill")],
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let kill = handle_request(
        Request {
            command: "session-kill".to_string(),
            session_id: Some("session-kill".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({}),
        },
        &host,
        &identity,
    )
    .await;
    assert!(kill.ok, "session-kill alias failed: {:?}", kill.error);
    assert_eq!(kill.data.as_ref().unwrap()["archived"], true);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_process_start_and_output_poll_are_daemon_owned() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);

    let start = handle_request(
        Request {
            command: "daemon-process-start".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "command": "printf daemon-owned",
                "workingDir": dir.path(),
            }),
        },
        &host,
        &DaemonIdentity::new_for_test("/tmp/roux.sock"),
    )
    .await;
    assert!(start.ok, "start failed: {:?}", start.error);
    let process_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let mut output = None;
    for _ in 0..500 {
        let poll = handle_request(
            Request {
                command: "daemon-process-output".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": process_id, "maxBytes": 1024 }),
            },
            &host,
            &DaemonIdentity::new_for_test("/tmp/roux.sock"),
        )
        .await;
        assert!(poll.ok, "poll failed: {:?}", poll.error);
        let data = poll.data.unwrap();
        if data["output"].as_str().unwrap_or("").contains("daemon-owned")
            && !data["record"]["running"].as_bool().unwrap_or(true)
        {
            output = Some(data);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let output = output.expect("daemon-owned output should be pollable");
    assert_eq!(output["record"]["id"], process_id);
    assert_eq!(output["record"]["running"], false);
    assert_eq!(output["record"]["exitCode"], 0);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_top_level_run_alias_starts_daemon_process() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let start = handle_request(
        Request {
            command: "run".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "command": "printf daemon-run-alias",
                "working_dir": dir.path(),
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(start.ok, "run alias failed: {:?}", start.error);
    let process_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let mut output = None;
    for _ in 0..500 {
        let poll = handle_request(
            Request {
                command: "daemon-process-output".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": process_id, "maxBytes": 1024 }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(poll.ok, "poll failed: {:?}", poll.error);
        let data = poll.data.unwrap();
        if data["output"].as_str().unwrap_or("").contains("daemon-run-alias")
            && !data["record"]["running"].as_bool().unwrap_or(true)
        {
            output = Some(data);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let output = output.expect("top-level run output should be daemon-owned");
    assert_eq!(output["record"]["exitCode"], 0);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_pty_spawn_task_and_output_poll_are_daemon_owned() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);

    let start = handle_request(
        Request {
            command: "daemon-pty-spawn-task".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "command": "printf daemon-pty-owned",
                "workingDir": dir.path(),
                "initialSize": [80, 24],
                "sessionId": "session-a",
                "paneId": "pane-a",
                "profile": "task",
            }),
        },
        &host,
        &DaemonIdentity::new_for_test("/tmp/roux.sock"),
    )
    .await;
    assert!(start.ok, "start failed: {:?}", start.error);
    let pty_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    let mut output = None;
    for _ in 0..50 {
        let poll = handle_request(
            Request {
                command: "daemon-pty-output".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": pty_id, "maxBytes": 1024 }),
            },
            &host,
            &DaemonIdentity::new_for_test("/tmp/roux.sock"),
        )
        .await;
        assert!(poll.ok, "poll failed: {:?}", poll.error);
        let data = poll.data.unwrap();
        if data["output"].as_str().unwrap_or("").contains("daemon-pty-owned")
            && data["record"]["running"] == false
        {
            output = Some(data);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let output = output.expect("daemon-owned PTY output should be pollable");
    assert_eq!(output["record"]["id"], pty_id);
    assert_eq!(output["record"]["running"], false);
    assert_eq!(output["record"]["exitCode"], 0);
    assert_eq!(output["record"]["info"]["session_id"], "session-a");

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_top_level_send_writes_to_session_primary_pty() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = make_session("session-send");
    session.primary_pty_id = Some("primary-pty".to_string());
    let services = RuntimeHostConfig {
        initial_sessions: vec![session],
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let start = handle_request(
        Request {
            command: "daemon-pty-spawn-task".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "id": "primary-pty",
                "command": "cat",
                "workingDir": dir.path(),
                "sessionId": "session-send",
                "paneId": "pane-send",
                "role": "sessionPrimary",
                "profile": "shell",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(start.ok, "pty start failed: {:?}", start.error);

    let send = handle_request(
        Request {
            command: "send".to_string(),
            session_id: Some("session-send".to_string()),
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "text": "daemon-send-alias", "enter": true }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(send.ok, "send alias failed: {:?}", send.error);
    assert_eq!(send.data.as_ref().unwrap()["id"], "primary-pty");

    let mut output = None;
    for _ in 0..50 {
        let poll = handle_request(
            Request {
                command: "daemon-pty-output".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": "primary-pty", "maxBytes": 2048 }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(poll.ok, "poll failed: {:?}", poll.error);
        let data = poll.data.unwrap();
        if data["output"].as_str().unwrap_or("").contains("daemon-send-alias") {
            output = Some(data);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    output.expect("sent text should appear in daemon PTY output");

    let _ = host.pty_handle.kill("primary-pty").await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_pty_spawn_request_populates_runtime_env() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux-daemon-test.sock");

    let start = handle_request(
        Request {
            command: "daemon-pty-spawn-task".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "command": "printf '%s|%s|%s|%s|%s|%s' \"$ROUX_SESSION_ID\" \"$ROUX_PANE_ID\" \"$ROUX_PROJECT_ID\" \"$ROUX_WORKTREE_PATH\" \"$ROUX_SOCKET\" \"$ROUX_NOTES_ROOT\"",
                "workingDir": dir.path(),
                "id": "pty-env",
                "sessionId": "session-a",
                "paneId": "pane-a",
                "projectId": "project-a",
                "worktreePath": "/worktrees/session-a",
                "notesEnv": {
                    "vaultRoot": "/vault",
                    "sessionSlug": "session-a",
                    "repoSlug": "repo-a"
                }
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(start.ok, "start failed: {:?}", start.error);

    let mut output = None;
    for _ in 0..50 {
        let response = handle_request(
            Request {
                command: "daemon-pty-output".to_string(),
                session_id: None,
                pane_id: None,
                auth_token: None,
                args: serde_json::json!({ "id": "pty-env", "maxBytes": 4096 }),
            },
            &host,
            &identity,
        )
        .await;
        assert!(response.ok, "output failed: {:?}", response.error);
        let data = response.data.expect("output payload");
        if !data["record"]["running"].as_bool().unwrap_or(true) {
            output = data["output"].as_str().map(str::to_string);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        output.as_deref(),
        Some("session-a|pane-a|project-a|/worktrees/session-a|/tmp/roux-daemon-test.sock|/vault")
    );

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_pty_attach_stream_replays_output_and_exit() {
    use tokio::io::AsyncReadExt;

    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let start = handle_request(
        Request {
            command: "daemon-pty-spawn-task".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "command": "printf daemon-pty-stream",
                "workingDir": dir.path(),
                "sessionId": "session-a",
                "paneId": "pane-a",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(start.ok, "start failed: {:?}", start.error);
    let pty_id = start.data.as_ref().unwrap()["id"].as_str().unwrap().to_string();

    for _ in 0..50 {
        let snapshot = host.pty_handle.snapshot(&pty_id, 1024).await.unwrap().unwrap();
        if snapshot.output.contains("daemon-pty-stream") && !snapshot.record.running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let (mut reader, mut writer) = tokio::io::duplex(8192);
    let ok = handle_daemon_pty_attach_stream(
        Request {
            command: "daemon-pty-attach".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": pty_id, "maxBytes": 1024 }),
        },
        &mut writer,
        &host,
        &identity,
    )
    .await;
    assert!(ok);
    drop(writer);

    let mut body = String::new();
    reader.read_to_string(&mut body).await.unwrap();
    let frames: Vec<serde_json::Value> =
        body.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(frames[0]["type"], "ready");
    assert_eq!(frames[0]["id"], pty_id);
    let replay_bytes: Vec<u8> = frames[0]["replayBytes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| byte.as_u64().unwrap() as u8)
        .collect();
    assert!(String::from_utf8_lossy(&replay_bytes).contains("daemon-pty-stream"));
    assert_eq!(frames[1]["type"], "exit");
    assert_eq!(frames[1]["code"], 0);

    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_pty_metadata_commands_mutate_info() {
    let dir = tempfile::tempdir().unwrap();
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let identity = DaemonIdentity::new_for_test("/tmp/roux.sock");

    let start = handle_request(
        Request {
            command: "daemon-pty-spawn-task".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({
                "command": "sleep 1",
                "workingDir": dir.path(),
                "id": "pty-meta",
                "sessionId": "session-a",
                "paneId": "pane-a",
            }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(start.ok, "start failed: {:?}", start.error);

    let detach = handle_request(
        Request {
            command: "daemon-pty-detach".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": "pty-meta" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(detach.ok, "detach failed: {:?}", detach.error);
    assert_eq!(detach.data.as_ref().unwrap()["info"]["status"]["type"], "RunningDetached");

    let attach = handle_request(
        Request {
            command: "daemon-pty-attach-pane".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": "pty-meta", "paneId": "pane-b" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(attach.ok, "attach failed: {:?}", attach.error);
    assert_eq!(attach.data.as_ref().unwrap()["info"]["status"]["pane_id"], "pane-b");

    let rename = handle_request(
        Request {
            command: "daemon-pty-set-name".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": "pty-meta", "name": "Build shell" }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(rename.ok, "rename failed: {:?}", rename.error);
    assert_eq!(rename.data.as_ref().unwrap()["info"]["name"], "Build shell");

    let clear = handle_request(
        Request {
            command: "daemon-pty-set-name".to_string(),
            session_id: None,
            pane_id: None,
            auth_token: None,
            args: serde_json::json!({ "id": "pty-meta", "name": null }),
        },
        &host,
        &identity,
    )
    .await;
    assert!(clear.ok, "clear failed: {:?}", clear.error);
    assert!(clear.data.as_ref().unwrap()["info"]["name"].is_null());

    let _ = host.pty_handle.kill("pty-meta").await;
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_socket_serves_status_request() {
    use tokio::io::AsyncReadExt;

    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("roux.sock");
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let watch_runner = WatchRunner::new(
        host.watch_handle.clone(),
        AutomationHookManager::from_config_root(dir.path()),
    );
    let log_path = dir.path().join("roux-daemon.log");
    let server = start_socket_server(
        host.clone(),
        watch_runner,
        DaemonIdentity::new(
            platform::SocketEndpoint::Unix(socket_path.clone()),
            log_path.clone(),
            None,
        ),
        DaemonLog::new_for_test(log_path.clone()),
    )
    .await
    .unwrap();

    let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    stream.write_all(br#"{"command":"daemon-status"}"#).await.unwrap();
    stream.write_all(b"\n").await.unwrap();
    stream.shutdown().await.unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["kind"], "roux-daemon");
    let expected_log_path = log_path.to_string_lossy().to_string();
    assert_eq!(value["data"]["logPath"], serde_json::Value::String(expected_log_path));

    server.shutdown();
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[cfg(not(windows))]
#[tokio::test]
async fn daemon_socket_stop_requests_shutdown_after_response() {
    use tokio::io::AsyncReadExt;

    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("roux.sock");
    let services = RuntimeHostConfig {
        initial_sessions: Vec::new(),
        session_persist_path: dir.path().join("sessions.json"),
        initial_projects: Vec::new(),
        project_persist_path: dir.path().join("projects.json"),
        initial_watches: Vec::new(),
        watch_persist_path: Some(dir.path().join("watches.json")),
    }
    .build();
    let (host, joins) = services.spawn_with(tokio::spawn);
    let watch_runner = WatchRunner::new(
        host.watch_handle.clone(),
        AutomationHookManager::from_config_root(dir.path()),
    );
    let log_path = dir.path().join("roux-daemon.log");
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let server = start_socket_server(
        host.clone(),
        watch_runner,
        DaemonIdentity::new(
            platform::SocketEndpoint::Unix(socket_path.clone()),
            log_path.clone(),
            None,
        )
        .with_shutdown(shutdown_tx),
        DaemonLog::new_for_test(log_path),
    )
    .await
    .unwrap();

    let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    stream.write_all(br#"{"command":"daemon-stop"}"#).await.unwrap();
    stream.write_all(b"\n").await.unwrap();
    stream.shutdown().await.unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["stopping"], true);

    for _ in 0..10 {
        if *shutdown_rx.borrow_and_update() {
            break;
        }
        let _ = shutdown_rx.changed().await;
    }
    assert!(*shutdown_rx.borrow(), "daemon-stop should signal shutdown");

    server.shutdown();
    host.process_handle.shutdown().await;
    host.pty_handle.shutdown().await;
    host.watch_handle.shutdown().await;
    host.session_handle.shutdown().await;
    host.project_handle.shutdown().await;
    drop(host);
    for join in joins {
        join.await.unwrap();
    }
}

#[test]
fn request_authorized_requires_identity_token() {
    let identity = DaemonIdentity::new_for_test_with_endpoint(
        platform::SocketEndpoint::Tcp("127.0.0.1:7777".to_string()),
        Some("secret-token".to_string()),
    );
    let request = |auth_token: Option<&str>| Request {
        command: "daemon-status".to_string(),
        session_id: None,
        pane_id: None,
        auth_token: auth_token.map(str::to_string),
        args: serde_json::json!({}),
    };

    assert!(!request_authorized(&request(None), &identity));
    assert!(!request_authorized(&request(Some("wrong-token")), &identity));
    assert!(request_authorized(&request(Some("secret-token")), &identity));
}
