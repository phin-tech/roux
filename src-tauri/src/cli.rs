use clap::{Parser, Subcommand};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use roux_lib::paths;

mod platform;

#[derive(Parser)]
#[command(name = "roux-cli", about = "Roux terminal manager CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Hook commands. Status variants are called by hooks in ~/.claude/settings.json.
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Show current session statuses
    Status,
    /// Clear all session status files
    Clear,

    // ── Socket commands ──────────────────────────────────────
    /// Open (or focus) a Roux session for a directory, then bring the app to front
    App {
        /// Directory path. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: String,
    },
    /// Split the current pane
    Split {
        /// Direction: horizontal or vertical
        #[arg(short, long, default_value = "horizontal")]
        direction: String,
    },
    /// Session management and introspection
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Open a shell pane
    Shell {
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Focus a pane or session
    Focus {
        /// Pane ID to focus
        #[arg(short, long)]
        pane: Option<String>,
        /// Session ID to focus
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Run a command in a new pane
    Run {
        /// The command to run
        command: String,
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
    },
    /// Multi-scoped notes vault (experimental).
    ///
    /// Read, append, write, and search notes across four scopes
    /// (global / project / repo / session). Session and scope context is
    /// resolved from `$ROUX_SESSION_ID`. See `docs/features/notes.md`.
    Notes {
        #[command(subcommand)]
        action: NotesAction,
    },
    /// Push a notification into Roux's notification service
    Notify {
        /// Notification title (required unless --json is used)
        #[arg(short = 't', long)]
        title: Option<String>,
        /// Notification body
        #[arg(short = 'b', long)]
        body: Option<String>,
        /// Optional subtitle (renders between title and body)
        #[arg(long)]
        subtitle: Option<String>,
        /// Severity: info | success | attention | warning | error
        #[arg(short = 'l', long, default_value = "info")]
        level: String,
        /// Explicit session id; falls back to --cwd, then $ROUX_SESSION_ID, then global
        #[arg(short = 's', long)]
        session: Option<String>,
        /// Working directory used to resolve a session if --session is not given
        #[arg(long)]
        cwd: Option<String>,
        /// Free-form source tag (used as the NotificationSource::Cli provider hint in logs)
        #[arg(long)]
        source: Option<String>,
        /// Read a JSON payload from stdin instead of individual flags.
        /// The JSON should match NotificationRequest: {level, title, body?, subtitle?, sessionId?, source?}
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Create a new session
    Create {
        /// Session name
        #[arg(short, long)]
        name: Option<String>,
        /// Working directory (existing worktree path or repo path). Default: cwd
        #[arg(short, long)]
        working_dir: Option<String>,
        /// Create a new worktree from this branch. Uses --working-dir (or cwd) as the repo to branch from.
        #[arg(long)]
        worktree_branch: Option<String>,
        /// Spawn profile id (e.g. "claude", "plain-shell", "codex", user profile id). Default: claude
        #[arg(short = 'P', long)]
        profile: Option<String>,
        /// Extra flag passed to the agent binary (repeatable; values may begin with --)
        #[arg(short = 'f', long = "flag", allow_hyphen_values = true)]
        flags: Vec<String>,
        /// Nono sandbox profile name
        #[arg(long)]
        nono_profile: Option<String>,
        /// Extra directory to allow under nono (repeatable)
        #[arg(long = "nono-allow-dir")]
        nono_allow_dirs: Vec<String>,
    },
    /// Send text to a session's PTY (appends \r by default; use --no-enter for raw)
    Send {
        /// The text to send
        text: String,
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
        /// Pane id (falls back to $ROUX_PANE_ID)
        #[arg(short, long)]
        pane: Option<String>,
        /// Send raw text without appending \r (Enter). By default Enter is sent.
        #[arg(long = "no-enter")]
        no_enter: bool,
    },
    /// Get the current state of a session (JSON)
    Poll {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// List all sessions (JSON)
    List,
    /// Pane commands for a session
    Panes {
        #[command(subcommand)]
        action: PaneAction,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Claude status: working
    Working,
    /// Claude status: idle
    Idle,
    /// Claude status: attention
    Attention,
    /// Claude status: error
    Error,
    /// Claude status: disconnected
    Disconnected,
    /// Show configured Roux automation hooks
    Show {
        /// Repo path whose project hooks should be included
        #[arg(long)]
        repo_path: Option<String>,
    },
    /// Run a Roux automation hook through the running app
    Run {
        /// Hook event name, e.g. post-watch-success
        event: String,
        #[arg(long)]
        repo_path: Option<String>,
        #[arg(long)]
        worktree_path: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        /// Extra args passed into the hook context. Use `--` before values.
        #[arg(last = true)]
        extra: Vec<String>,
    },
}

#[derive(Subcommand)]
enum PaneAction {
    /// List panes for a session (JSON)
    List {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Create a new pane in a session
    Create {
        /// Session id (falls back to $ROUX_SESSION_ID)
        #[arg(short, long)]
        session: Option<String>,
        /// Spawn profile id (e.g. "plain-shell", "claude", "codex", user profile id). Default: plain-shell
        #[arg(short = 'P', long)]
        profile: Option<String>,
        /// Split direction from the active pane (horizontal|vertical). Default: horizontal
        #[arg(short, long, default_value = "horizontal")]
        direction: String,
        /// Working directory for the new pane. Default: session's worktree_path
        #[arg(short, long)]
        working_dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum NotesAction {
    /// Global scope — your personal catch-all, shared across every session.
    Global {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Project scope — shared by every session tagged with the same project.
    Project {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Repo scope — shared by every session working in the same repo.
    Repo {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Session scope — the current session's personal scratchpad / log.
    Session {
        #[command(subcommand)]
        action: NotesScopeVerb,
    },
    /// Search across the vault by frontmatter tags + inline `#tag` occurrences.
    Search {
        /// Required tag filter. Pass multiple times for AND. Hierarchical prefix
        /// matching is on by default (`--tag api` matches `api/tls`).
        #[arg(long = "tag", action = clap::ArgAction::Append, required = true)]
        tags: Vec<String>,
        /// Restrict to one scope's subtree. Default: whole vault.
        #[arg(long)]
        scope: Option<String>,
        /// Disable hierarchical prefix matching (literal tag names only).
        #[arg(long)]
        tag_exact: bool,
    },
    /// Print the vault root path.
    Root,
}

#[derive(Subcommand)]
enum NotesScopeVerb {
    /// Print the note contents to stdout.
    Show {
        /// Target a topic file within the scope dir instead of `notes.md`.
        #[arg(long)]
        topic: Option<String>,
    },
    /// Append to the note. Reads from stdin if `--content` is not supplied.
    Append {
        #[arg(long)]
        topic: Option<String>,
        /// Content to append. If omitted, stdin is used.
        #[arg(long)]
        content: Option<String>,
        /// Prepend a timestamped heading + block-ref for this entry.
        #[arg(long)]
        timestamp: bool,
        /// Union-merge a tag into the file's frontmatter `tags:` list. Repeatable.
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,
    },
    /// Replace the note body. Reads from stdin if `--content` is not supplied.
    Write {
        #[arg(long)]
        topic: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,
    },
    /// Print the note's absolute filesystem path.
    Path {
        #[arg(long)]
        topic: Option<String>,
        /// Print the scope directory instead of the file.
        #[arg(long)]
        dir: bool,
    },
}

fn status_dir() -> PathBuf {
    platform::status_dir()
}

fn send_socket_command(request: Value) -> Result<Value, String> {
    #[cfg(windows)]
    {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let mut request = request;
        let auth_token = platform::load_socket_auth_token()
            .ok_or_else(|| "Roux command channel token not found".to_string())?;
        if let Some(request_obj) = request.as_object_mut() {
            request_obj.insert("auth_token".to_string(), Value::String(auth_token));
        }

        let endpoint =
            platform::resolve_socket_endpoint().ok_or_else(|| "Roux is not running".to_string())?;
        let mut stream = TcpStream::connect(&endpoint).map_err(|e| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::AddrNotAvailable
            ) {
                "Roux is not running".to_string()
            } else {
                format!("Failed to connect to Roux: {}", e)
            }
        })?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;

        let json = serde_json::to_string(&request).unwrap();
        stream.write_all(json.as_bytes()).map_err(|e| format!("Failed to send command: {}", e))?;
        stream.write_all(b"\n").map_err(|e| format!("Failed to send command: {}", e))?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| format!("Failed to shutdown write: {}", e))?;

        let mut response = String::new();
        let mut reader = std::io::BufReader::new(stream);
        reader
            .read_to_string(&mut response)
            .map_err(|e| format!("Failed to read response: {}", e))?;

        return serde_json::from_str(&response).map_err(|e| format!("Invalid response: {}", e));
    }

    #[cfg(not(windows))]
    {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        let path = platform::socket_path();
        let stream = UnixStream::connect(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::ConnectionRefused
            {
                "Roux is not running".to_string()
            } else {
                format!("Failed to connect to Roux: {}", e)
            }
        })?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("Failed to set timeout: {}", e))?;

        let json = serde_json::to_string(&request).unwrap();
        let mut stream_ref = &stream;
        stream_ref
            .write_all(json.as_bytes())
            .map_err(|e| format!("Failed to send command: {}", e))?;
        stream_ref.write_all(b"\n").map_err(|e| format!("Failed to send command: {}", e))?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| format!("Failed to shutdown write: {}", e))?;

        let mut response = String::new();
        let mut reader = std::io::BufReader::new(&stream);
        reader
            .read_to_string(&mut response)
            .map_err(|e| format!("Failed to read response: {}", e))?;

        serde_json::from_str(&response).map_err(|e| format!("Invalid response: {}", e))
    }
}

fn resolve_path(path: &str) -> String {
    let p = std::path::PathBuf::from(path);
    let absolute =
        if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) };
    absolute.canonicalize().unwrap_or(absolute).to_string_lossy().to_string()
}

fn get_session_id() -> Option<String> {
    std::env::var("ROUX_SESSION_ID").ok()
}

fn get_pane_id() -> Option<String> {
    std::env::var("ROUX_PANE_ID").ok()
}

fn run_socket_command(request: Value) {
    match send_socket_command(request) {
        Ok(response) => {
            let ok = response.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
            if ok {
                if let Some(data) = response.get("data") {
                    println!("{}", serde_json::to_string_pretty(data).unwrap());
                }
            } else {
                let error =
                    response.get("error").and_then(|e| e.as_str()).unwrap_or("unknown error");
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn handle_hook(status: &str) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }

    let data: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return,
    };

    let sid = match data.get("session_id").and_then(|s| s.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return,
    };

    let cwd = data.get("cwd").and_then(|s| s.as_str()).unwrap_or("").to_string();

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let mut out = serde_json::json!({
        "status": status,
        "provider": "claude",
        // Provider-agnostic key; the old `claude_session_id` name was
        // confusing once we picked up non-Claude agents. The watcher still
        // accepts the legacy key so an older roux-cli shim paired with a
        // newer desktop binary keeps working.
        "provider_session_id": sid,
        "cwd": cwd,
        "timestamp": timestamp,
    });

    // Pane-scoped routing fields. Present when the agent was launched inside
    // a Roux-managed PTY (which injects both env vars unconditionally). Absent
    // for legacy or external installs — status_watcher falls back to cwd
    // matching in that case, for notifications only.
    if let Some(roux_sid) = get_session_id() {
        out["roux_session_id"] = Value::String(roux_sid);
    }
    if let Some(pane) = get_pane_id() {
        out["roux_pane_id"] = Value::String(pane);
    }

    if status == "attention" {
        if let Some(tn) = data.get("tool_name") {
            out["tool_name"] = tn.clone();
        }
        if let Some(ti) = data.get("tool_input") {
            out["tool_input"] = ti.clone();
        }
        if let Some(msg) = data.get("message") {
            out["message"] = msg.clone();
        }
    }

    if status == "error" {
        if let Some(et) = data.get("error_type") {
            out["error_type"] = et.clone();
        }
        if let Some(em) = data.get("error_message") {
            out["error_message"] = em.clone();
        }
    }

    let dir = status_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", sid));
    let json = serde_json::to_string(&out).unwrap_or_default();
    let _ = fs::write(path, json);
}

fn handle_hook_action(action: HookAction) {
    match action {
        HookAction::Working => handle_hook("working"),
        HookAction::Idle => handle_hook("idle"),
        HookAction::Attention => handle_hook("attention"),
        HookAction::Error => handle_hook("error"),
        HookAction::Disconnected => handle_hook("disconnected"),
        HookAction::Show { repo_path } => {
            let mut args = serde_json::Map::new();
            if let Some(path) = repo_path {
                args.insert("repo_path".into(), Value::String(path));
            }
            run_socket_command(serde_json::json!({
                "command": "hook-show",
                "args": Value::Object(args),
            }));
        }
        HookAction::Run {
            event,
            repo_path,
            worktree_path,
            branch,
            session,
            project,
            task,
            scope,
            provider,
            extra,
        } => {
            let mut args = serde_json::Map::new();
            args.insert("event".into(), Value::String(event));
            if let Some(path) = repo_path {
                args.insert("repoPath".into(), Value::String(path));
            }
            if let Some(path) = worktree_path {
                args.insert("worktreePath".into(), Value::String(path));
            }
            if let Some(branch) = branch {
                args.insert("branch".into(), Value::String(branch));
            }
            if let Some(session) = session.or_else(get_session_id) {
                args.insert("sessionId".into(), Value::String(session));
            }
            if let Some(project) = project {
                args.insert("projectId".into(), Value::String(project));
            }
            if let Some(task) = task {
                args.insert("taskId".into(), Value::String(task));
            }
            if let Some(scope) = scope {
                args.insert("scope".into(), Value::String(scope));
            }
            if let Some(provider) = provider {
                args.insert("provider".into(), Value::String(provider));
            }
            if !extra.is_empty() {
                args.insert(
                    "args".into(),
                    Value::Array(extra.into_iter().map(Value::String).collect()),
                );
            }
            run_socket_command(serde_json::json!({
                "command": "hook-run",
                "args": Value::Object(args),
            }));
        }
    }
}

fn show_status() {
    let dir = status_dir();
    if !dir.exists() {
        println!("No status files found");
        return;
    }

    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
        .collect();

    if entries.is_empty() {
        println!("No status files found");
        return;
    }

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let content = fs::read_to_string(entry.path()).unwrap_or_default();
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let status = data.get("status").and_then(|s| s.as_str()).unwrap_or("?");
            let cwd = data.get("cwd").and_then(|s| s.as_str()).unwrap_or("?");
            let sid = entry.path().file_stem().unwrap().to_string_lossy().to_string();
            println!("{sid}  status={status}  cwd={cwd}");
        }
    }
}

fn clear_status() {
    let dir = status_dir();
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut count = 0;
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                let _ = fs::remove_file(entry.path());
                count += 1;
            }
        }
        println!("Cleared {} status file(s)", count);
    } else {
        println!("No status directory found");
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Hook { action } => handle_hook_action(action),
        Commands::Status => show_status(),
        Commands::Clear => clear_status(),

        Commands::App { path } => {
            let resolved = resolve_path(&path);
            run_socket_command(serde_json::json!({
                "command": "app-open",
                "args": { "path": resolved },
            }));
        }

        Commands::Split { direction } => {
            run_socket_command(serde_json::json!({
                "command": "split",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": { "direction": direction },
            }));
        }

        Commands::Session { action } => match action {
            SessionAction::Create {
                name,
                working_dir,
                worktree_branch,
                profile,
                flags,
                nono_profile,
                nono_allow_dirs,
            } => {
                // Default working_dir to the current directory when the caller
                // is not already inside a Roux session (which lets the backend
                // inherit repo_root from $ROUX_SESSION_ID). With --worktree-branch
                // the working_dir still becomes the repo to branch off.
                let working_dir = match working_dir {
                    Some(d) => Some(d),
                    None if get_session_id().is_some() => None,
                    None => Some(resolve_path(".")),
                };
                let mut args = serde_json::Map::new();
                if let Some(n) = name {
                    args.insert("name".into(), Value::String(n));
                }
                if let Some(d) = working_dir {
                    args.insert("working_dir".into(), Value::String(d));
                }
                if let Some(b) = worktree_branch {
                    args.insert("worktree_branch".into(), Value::String(b));
                }
                if let Some(p) = profile {
                    args.insert("profile".into(), Value::String(p));
                }
                if !flags.is_empty() {
                    args.insert(
                        "flags".into(),
                        Value::Array(flags.into_iter().map(Value::String).collect()),
                    );
                }
                if let Some(p) = nono_profile {
                    args.insert("nono_profile".into(), Value::String(p));
                }
                if !nono_allow_dirs.is_empty() {
                    args.insert(
                        "nono_allow_dirs".into(),
                        Value::Array(nono_allow_dirs.into_iter().map(Value::String).collect()),
                    );
                }
                run_socket_command(serde_json::json!({
                    "command": "session-create",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": args,
                }));
            }
            SessionAction::Send { text, session, pane, no_enter } => {
                run_socket_command(serde_json::json!({
                    "command": "send",
                    "session_id": session.or_else(get_session_id),
                    "pane_id": pane.or_else(get_pane_id),
                    "args": { "text": text, "enter": !no_enter },
                }));
            }
            SessionAction::Poll { session } => {
                run_socket_command(serde_json::json!({
                    "command": "session-poll",
                    "session_id": session.or_else(get_session_id),
                }));
            }
            SessionAction::List => {
                run_socket_command(serde_json::json!({
                    "command": "session-list",
                }));
            }
            SessionAction::Panes { action } => match action {
                PaneAction::List { session } => {
                    run_socket_command(serde_json::json!({
                        "command": "session-panes-list",
                        "session_id": session.or_else(get_session_id),
                    }));
                }
                PaneAction::Create { session, profile, direction, working_dir } => {
                    let mut args = serde_json::Map::new();
                    args.insert("direction".into(), Value::String(direction));
                    if let Some(p) = profile {
                        args.insert("profile".into(), Value::String(p));
                    }
                    if let Some(d) = working_dir {
                        args.insert("working_dir".into(), Value::String(d));
                    }
                    run_socket_command(serde_json::json!({
                        "command": "session-panes-create",
                        "session_id": session.or_else(get_session_id),
                        "args": args,
                    }));
                }
            },
        },

        Commands::Shell { working_dir } => {
            let mut args = serde_json::Map::new();
            if let Some(d) = working_dir {
                args.insert("working_dir".into(), Value::String(d));
            }
            run_socket_command(serde_json::json!({
                "command": "shell",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": args,
            }));
        }

        Commands::Focus { pane, session } => {
            run_socket_command(serde_json::json!({
                "command": "focus",
                "session_id": session.or_else(get_session_id),
                "pane_id": pane.or_else(get_pane_id),
            }));
        }

        Commands::Run { command, working_dir } => {
            let mut args = serde_json::json!({ "command": command });
            if let Some(d) = working_dir {
                args["working_dir"] = Value::String(d);
            }
            run_socket_command(serde_json::json!({
                "command": "run",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": args,
            }));
        }

        Commands::Notify { title, body, subtitle, level, session, cwd, source, json } => {
            let mut payload = if json {
                let mut input = String::new();
                if std::io::stdin().read_to_string(&mut input).is_err() {
                    eprintln!("Error: failed to read JSON from stdin");
                    std::process::exit(1);
                }
                match serde_json::from_str::<Value>(&input) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("Error: invalid JSON: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                let Some(title) = title else {
                    eprintln!("Error: --title is required unless --json is used");
                    std::process::exit(1);
                };
                let mut obj = serde_json::Map::new();
                obj.insert("title".into(), Value::String(title));
                obj.insert("level".into(), Value::String(level));
                if let Some(b) = body {
                    obj.insert("body".into(), Value::String(b));
                }
                if let Some(s) = subtitle {
                    obj.insert("subtitle".into(), Value::String(s));
                }
                if let Some(s) = source {
                    obj.insert("source".into(), Value::String(s));
                }
                Value::Object(obj)
            };

            // Resolve the session to attach to:
            //   1. explicit --session flag
            //   2. payload already has sessionId (from --json)
            //   3. --cwd flag (server-side cwd → session match)
            //   4. env ROUX_SESSION_ID
            if let Some(s) = session {
                payload["sessionId"] = Value::String(s);
            }
            if payload.get("sessionId").is_none() {
                if let Some(sid) = get_session_id() {
                    payload["sessionId"] = Value::String(sid);
                }
            }

            let mut args = serde_json::Map::new();
            args.insert("payload".into(), payload);
            if let Some(c) = cwd {
                args.insert("cwd".into(), Value::String(c));
            }

            run_socket_command(serde_json::json!({
                "command": "notify",
                "args": Value::Object(args),
            }));
        }

        Commands::Notes { action } => handle_notes(action),
    }
}

fn scope_name(a: &NotesAction) -> Option<&'static str> {
    match a {
        NotesAction::Global { .. } => Some("global"),
        NotesAction::Project { .. } => Some("project"),
        NotesAction::Repo { .. } => Some("repo"),
        NotesAction::Session { .. } => Some("session"),
        NotesAction::Search { .. } | NotesAction::Root => None,
    }
}

fn build_target(scope: &str, topic: Option<String>) -> serde_json::Value {
    serde_json::json!({
        "scope": scope,
        "sessionId": get_session_id(),
        "topic": topic,
        "overrideSlug": serde_json::Value::Null,
    })
}

fn read_stdin_if_needed(content: Option<String>) -> String {
    match content {
        Some(s) => s,
        None => {
            use std::io::Read;
            let mut buf = String::new();
            let _ = std::io::stdin().read_to_string(&mut buf);
            // Trim one trailing newline — echo's default behavior shouldn't
            // double-space-out every appended entry.
            if buf.ends_with('\n') {
                buf.pop();
            }
            buf
        }
    }
}

fn handle_notes(action: NotesAction) {
    match action {
        NotesAction::Root => {
            run_socket_command(serde_json::json!({
                "command": "notes-vault-root",
                "args": {},
            }));
        }
        NotesAction::Search { tags, scope, tag_exact } => {
            run_socket_command(serde_json::json!({
                "command": "notes-search",
                "args": {
                    "tags": tags,
                    "scope": scope,
                    "exact": tag_exact,
                },
            }));
        }
        _ => {
            let scope = scope_name(&action).expect("scope-aware variants only");
            match action {
                NotesAction::Global { action }
                | NotesAction::Project { action }
                | NotesAction::Repo { action }
                | NotesAction::Session { action } => handle_notes_verb(scope, action),
                _ => unreachable!(),
            }
        }
    }
}

fn handle_notes_verb(scope: &str, verb: NotesScopeVerb) {
    match verb {
        NotesScopeVerb::Show { topic } => {
            run_socket_command(serde_json::json!({
                "command": "notes-read",
                "args": build_target(scope, topic),
            }));
        }
        NotesScopeVerb::Append { topic, content, timestamp, tags } => {
            let body = read_stdin_if_needed(content);
            run_socket_command(serde_json::json!({
                "command": "notes-append",
                "args": {
                    "target": build_target(scope, topic),
                    "content": body,
                    "timestamped": timestamp,
                    "tags": tags,
                },
            }));
        }
        NotesScopeVerb::Write { topic, content, tags } => {
            let body = read_stdin_if_needed(content);
            run_socket_command(serde_json::json!({
                "command": "notes-write",
                "args": {
                    "target": build_target(scope, topic),
                    "content": body,
                    "tags": tags,
                },
            }));
        }
        NotesScopeVerb::Path { topic, dir } => {
            run_socket_command(serde_json::json!({
                "command": "notes-path",
                "args": {
                    "target": build_target(scope, topic),
                    "dir": dir,
                },
            }));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // ── resolve_path ────────────────────────────────────────

    #[test]
    fn resolve_path_keeps_absolute_path_as_is() {
        // "/" always exists on Unix, canonicalize succeeds.
        let got = resolve_path("/");
        assert!(got.starts_with('/'));
    }

    #[test]
    fn resolve_path_turns_relative_into_absolute() {
        let got = resolve_path(".");
        assert!(std::path::Path::new(&got).is_absolute(), "got {}", got);
    }

    #[test]
    fn resolve_path_nonexistent_falls_back_to_uncanonicalized_absolute() {
        // canonicalize() fails on missing paths; resolve_path must still
        // return an absolute string rather than panicking or returning "".
        let got = resolve_path("/definitely/not/a/real/path/roux-test");
        assert!(std::path::Path::new(&got).is_absolute(), "got {}", got);
        assert!(got.contains("roux-test"));
    }

    // ── Cli parsing ─────────────────────────────────────────

    #[test]
    fn cli_parses_app_with_default_path() {
        let cli = Cli::try_parse_from(["roux-cli", "app"]).unwrap();
        match cli.command {
            Commands::App { path } => assert_eq!(path, "."),
            other => panic!("expected App, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn cli_parses_app_with_explicit_path() {
        let cli = Cli::try_parse_from(["roux-cli", "app", "/tmp/foo"]).unwrap();
        match cli.command {
            Commands::App { path } => assert_eq!(path, "/tmp/foo"),
            _ => panic!("expected App"),
        }
    }

    #[test]
    fn cli_parses_session_list() {
        let cli = Cli::try_parse_from(["roux-cli", "session", "list"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::List } => {}
            _ => panic!("expected Session::List"),
        }
    }

    #[test]
    fn cli_parses_hook_status_compat_command() {
        let cli = Cli::try_parse_from(["roux-cli", "hook", "working"]).unwrap();
        match cli.command {
            Commands::Hook { action: HookAction::Working } => {}
            _ => panic!("expected Hook::Working"),
        }
    }

    #[test]
    fn cli_parses_hook_show_with_repo_path() {
        let cli =
            Cli::try_parse_from(["roux-cli", "hook", "show", "--repo-path", "/repo"]).unwrap();
        match cli.command {
            Commands::Hook { action: HookAction::Show { repo_path } } => {
                assert_eq!(repo_path.as_deref(), Some("/repo"));
            }
            _ => panic!("expected Hook::Show"),
        }
    }

    #[test]
    fn cli_parses_hook_run_with_context_and_extra_args() {
        let cli = Cli::try_parse_from([
            "roux-cli",
            "hook",
            "run",
            "post-watch-success",
            "--repo-path",
            "/repo",
            "--worktree-path",
            "/repo/.worktrees/x",
            "--branch",
            "feat/x",
            "--session",
            "sid",
            "--task",
            "tid",
            "--scope",
            "session",
            "--provider",
            "worktrunk",
            "--",
            "--verbose",
        ])
        .unwrap();
        match cli.command {
            Commands::Hook {
                action:
                    HookAction::Run {
                        event,
                        repo_path,
                        worktree_path,
                        branch,
                        session,
                        task,
                        scope,
                        provider,
                        extra,
                        ..
                    },
            } => {
                assert_eq!(event, "post-watch-success");
                assert_eq!(repo_path.as_deref(), Some("/repo"));
                assert_eq!(worktree_path.as_deref(), Some("/repo/.worktrees/x"));
                assert_eq!(branch.as_deref(), Some("feat/x"));
                assert_eq!(session.as_deref(), Some("sid"));
                assert_eq!(task.as_deref(), Some("tid"));
                assert_eq!(scope.as_deref(), Some("session"));
                assert_eq!(provider.as_deref(), Some("worktrunk"));
                assert_eq!(extra, vec!["--verbose"]);
            }
            _ => panic!("expected Hook::Run"),
        }
    }

    #[test]
    fn cli_parses_session_poll_with_session_id() {
        let cli = Cli::try_parse_from(["roux-cli", "session", "poll", "-s", "sid-1"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Poll { session } } => {
                assert_eq!(session.as_deref(), Some("sid-1"));
            }
            _ => panic!("expected Session::Poll"),
        }
    }

    #[test]
    fn cli_parses_session_send_default_has_enter_true() {
        let cli = Cli::try_parse_from(["roux-cli", "session", "send", "hello"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { text, no_enter, .. } } => {
                assert_eq!(text, "hello");
                assert!(!no_enter, "--no-enter not passed, so no_enter must be false");
            }
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_send_with_no_enter_flag() {
        let cli =
            Cli::try_parse_from(["roux-cli", "session", "send", "hello", "--no-enter"]).unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { no_enter, .. } } => assert!(no_enter),
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_send_with_session_and_pane() {
        let cli =
            Cli::try_parse_from(["roux-cli", "session", "send", "hi", "-s", "sid", "-p", "pid"])
                .unwrap();
        match cli.command {
            Commands::Session { action: SessionAction::Send { session, pane, .. } } => {
                assert_eq!(session.as_deref(), Some("sid"));
                assert_eq!(pane.as_deref(), Some("pid"));
            }
            _ => panic!("expected Session::Send"),
        }
    }

    #[test]
    fn cli_parses_session_create_with_full_options() {
        let cli = Cli::try_parse_from([
            "roux-cli",
            "session",
            "create",
            "--name",
            "feat-x",
            "--worktree-branch",
            "feat/x",
            "--profile",
            "claude",
            "-f",
            "--debug",
            "-f",
            "--model=opus",
            "--nono-profile",
            "strict",
            "--nono-allow-dir",
            "~/work",
            "--nono-allow-dir",
            "/tmp",
        ])
        .unwrap();
        match cli.command {
            Commands::Session {
                action:
                    SessionAction::Create {
                        name,
                        worktree_branch,
                        profile,
                        flags,
                        nono_profile,
                        nono_allow_dirs,
                        working_dir,
                    },
            } => {
                assert_eq!(name.as_deref(), Some("feat-x"));
                assert_eq!(worktree_branch.as_deref(), Some("feat/x"));
                assert_eq!(profile.as_deref(), Some("claude"));
                assert_eq!(flags, vec!["--debug", "--model=opus"]);
                assert_eq!(nono_profile.as_deref(), Some("strict"));
                assert_eq!(nono_allow_dirs, vec!["~/work", "/tmp"]);
                assert!(working_dir.is_none());
            }
            _ => panic!("expected Session::Create"),
        }
    }

    #[test]
    fn cli_parses_session_panes_list_with_session() {
        let cli =
            Cli::try_parse_from(["roux-cli", "session", "panes", "list", "-s", "sid"]).unwrap();
        match cli.command {
            Commands::Session {
                action: SessionAction::Panes { action: PaneAction::List { session } },
            } => {
                assert_eq!(session.as_deref(), Some("sid"));
            }
            _ => panic!("expected Session::Panes::List"),
        }
    }

    #[test]
    fn cli_parses_session_panes_create_defaults() {
        let cli =
            Cli::try_parse_from(["roux-cli", "session", "panes", "create", "-s", "sid"]).unwrap();
        match cli.command {
            Commands::Session {
                action:
                    SessionAction::Panes {
                        action: PaneAction::Create { session, profile, direction, working_dir },
                    },
            } => {
                assert_eq!(session.as_deref(), Some("sid"));
                assert!(profile.is_none());
                assert_eq!(direction, "horizontal");
                assert!(working_dir.is_none());
            }
            _ => panic!("expected Session::Panes::Create"),
        }
    }

    #[test]
    fn cli_parses_session_panes_create_with_all_options() {
        let cli = Cli::try_parse_from([
            "roux-cli", "session", "panes", "create", "-s", "sid", "-P", "shell", "-d", "vertical",
            "-w", "/tmp",
        ])
        .unwrap();
        match cli.command {
            Commands::Session {
                action:
                    SessionAction::Panes {
                        action: PaneAction::Create { profile, direction, working_dir, .. },
                    },
            } => {
                assert_eq!(profile.as_deref(), Some("shell"));
                assert_eq!(direction, "vertical");
                assert_eq!(working_dir.as_deref(), Some("/tmp"));
            }
            _ => panic!("expected Session::Panes::Create"),
        }
    }

    #[test]
    fn cli_rejects_removed_top_level_send() {
        // `roux send "x"` used to work; now it must live under `session`.
        let err = Cli::try_parse_from(["roux-cli", "send", "x"]);
        assert!(err.is_err(), "top-level `send` must no longer parse");
    }

    #[test]
    fn cli_split_still_parses_with_direction() {
        let cli = Cli::try_parse_from(["roux-cli", "split", "-d", "vertical"]).unwrap();
        match cli.command {
            Commands::Split { direction } => assert_eq!(direction, "vertical"),
            _ => panic!("expected Split"),
        }
    }
}
