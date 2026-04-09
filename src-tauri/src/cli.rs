use clap::{Parser, Subcommand};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Parser)]
#[command(name = "roux-cli", about = "Roux terminal manager CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Handle a Claude Code hook event (called by hooks in ~/.claude/settings.json)
    Hook {
        /// Status to set: working, idle, attention, error, disconnected
        status: String,
    },
    /// Show current session statuses
    Status,
    /// Clear all session status files
    Clear,

    // ── Socket commands ──────────────────────────────────────
    /// Split the current pane
    Split {
        /// Direction: horizontal or vertical
        #[arg(short, long, default_value = "horizontal")]
        direction: String,
    },
    /// Create a new Claude session
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
    /// Send text to the active Claude pane
    Send {
        /// The text to send
        text: String,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Create a new session
    Create {
        /// Session name
        #[arg(short, long)]
        name: Option<String>,
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
    },
}

fn status_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("roux").join("status")
}

fn socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("ROUX_SOCKET") {
        return PathBuf::from(path);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("roux").join("roux.sock")
}

#[derive(Debug, Error)]
enum CliError {
    #[error("Roux is not running")]
    RouxNotRunning,
    #[error("Failed to connect to Roux: {source}")]
    Connect {
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to set timeout: {source}")]
    SetTimeout {
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to serialize command: {source}")]
    SerializeCommand {
        #[source]
        source: serde_json::Error,
    },
    #[error("Failed to send command: {source}")]
    SendCommand {
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to shutdown write: {source}")]
    ShutdownWrite {
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to read response: {source}")]
    ReadResponse {
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid response: {source}")]
    InvalidResponse {
        #[source]
        source: serde_json::Error,
    },
}

fn send_socket_command(request: Value) -> Result<Value, CliError> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let path = socket_path();
    let stream = UnixStream::connect(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound
            || e.kind() == std::io::ErrorKind::ConnectionRefused
        {
            CliError::RouxNotRunning
        } else {
            CliError::Connect { source: e }
        }
    })?;

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|source| CliError::SetTimeout { source })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|source| CliError::SetTimeout { source })?;

    let json =
        serde_json::to_string(&request).map_err(|source| CliError::SerializeCommand { source })?;
    let mut stream_ref = &stream;
    stream_ref.write_all(json.as_bytes()).map_err(|source| CliError::SendCommand { source })?;
    stream_ref.write_all(b"\n").map_err(|source| CliError::SendCommand { source })?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|source| CliError::ShutdownWrite { source })?;

    let mut response = String::new();
    let mut reader = std::io::BufReader::new(&stream);
    reader.read_to_string(&mut response).map_err(|source| CliError::ReadResponse { source })?;

    serde_json::from_str(&response).map_err(|source| CliError::InvalidResponse { source })
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
        "claude_session_id": sid,
        "cwd": cwd,
        "timestamp": timestamp,
    });

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
        Commands::Hook { status } => handle_hook(&status),
        Commands::Status => show_status(),
        Commands::Clear => clear_status(),

        Commands::Split { direction } => {
            run_socket_command(serde_json::json!({
                "command": "split",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": { "direction": direction },
            }));
        }

        Commands::Session { action } => match action {
            SessionAction::Create { name, working_dir } => {
                let mut args = serde_json::Map::new();
                if let Some(n) = name {
                    args.insert("name".into(), Value::String(n));
                }
                if let Some(d) = working_dir {
                    args.insert("working_dir".into(), Value::String(d));
                }
                run_socket_command(serde_json::json!({
                    "command": "session-create",
                    "session_id": get_session_id(),
                    "pane_id": get_pane_id(),
                    "args": args,
                }));
            }
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

        Commands::Send { text } => {
            run_socket_command(serde_json::json!({
                "command": "send",
                "session_id": get_session_id(),
                "pane_id": get_pane_id(),
                "args": { "text": text },
            }));
        }
    }
}
