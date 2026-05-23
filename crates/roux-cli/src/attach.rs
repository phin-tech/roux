use std::io::{self, IsTerminal, Read, Write};
use std::sync::{
    atomic::{AtomicBool, AtomicI32, Ordering},
    Arc,
};
use std::thread;

use serde::Deserialize;
use serde_json::Value;

use crate::cli_socket::{send_socket_command, stream_socket_command};

pub(crate) struct AttachOptions {
    pub target: Option<String>,
    pub session: Option<String>,
    pub max_bytes: usize,
    pub no_input: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AttachFrame {
    #[serde(rename = "ready")]
    Ready {
        #[serde(default, rename = "replayBytes")]
        replay_bytes: Vec<u8>,
    },
    #[serde(rename = "output")]
    Output { bytes: Vec<u8> },
    #[serde(rename = "exit")]
    Exit { code: Option<i32> },
    #[serde(rename = "error")]
    Error { error: String },
}

pub(crate) fn run(options: AttachOptions) -> Result<i32, String> {
    let pty_id = resolve_attach_pty_id(options.target, options.session)?;
    if let Some((cols, rows)) = current_terminal_size() {
        if let Err(err) = send_pty_resize(&pty_id, cols, rows) {
            eprintln!("attach: warning: failed to resize daemon PTY: {err}");
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let _raw_mode = if !options.no_input && io::stdin().is_terminal() {
        Some(RawModeGuard::enable()?)
    } else {
        None
    };
    if !options.no_input {
        spawn_input_forwarder(pty_id.clone(), running.clone());
    }

    let exit = stream_attach_output(&pty_id, options.max_bytes);
    running.store(false, Ordering::SeqCst);
    exit
}

fn resolve_attach_pty_id(
    target: Option<String>,
    session: Option<String>,
) -> Result<String, String> {
    if let Some(target) = target.filter(|target| !target.trim().is_empty()) {
        return Ok(target);
    }

    let session_id = session
        .filter(|session| !session.trim().is_empty())
        .or_else(|| std::env::var("ROUX_SESSION_ID").ok())
        .ok_or_else(|| "attach requires a PTY id, --session, or $ROUX_SESSION_ID".to_string())?;

    let response = send_socket_command(serde_json::json!({
        "command": "session-poll",
        "session_id": session_id,
    }))?;
    let data = response_data(response)?;
    if let Some(primary_pty_id) = primary_pty_id_from_session(&data) {
        return Ok(primary_pty_id);
    }

    let response = send_socket_command(serde_json::json!({
        "command": "daemon-pty-list",
    }))?;
    let data = response_data(response)?;
    let ptys =
        data.as_array().ok_or_else(|| "daemon-pty-list returned non-array data".to_string())?;
    select_primary_pty_for_session(ptys, &session_id)
        .ok_or_else(|| format!("no daemon PTY found for session {session_id}"))
}

fn response_data(response: Value) -> Result<Value, String> {
    if response.get("ok").and_then(|ok| ok.as_bool()) == Some(true) {
        return Ok(response.get("data").cloned().unwrap_or(Value::Null));
    }
    let error = response.get("error").and_then(|error| error.as_str()).unwrap_or("unknown error");
    Err(error.to_string())
}

fn primary_pty_id_from_session(session: &Value) -> Option<String> {
    session
        .get("primaryPtyId")
        .and_then(|id| id.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn select_primary_pty_for_session(ptys: &[Value], session_id: &str) -> Option<String> {
    ptys.iter()
        .find(|pty| {
            pty.get("info")
                .and_then(|info| info.get("session_id").or_else(|| info.get("sessionId")))
                .and_then(|id| id.as_str())
                == Some(session_id)
                && pty.get("info").and_then(|info| info.get("role")).and_then(|role| role.as_str())
                    == Some("sessionPrimary")
        })
        .and_then(|pty| pty.get("id").and_then(|id| id.as_str()))
        .map(str::to_string)
}

fn attach_request(id: &str, max_bytes: usize) -> Value {
    serde_json::json!({
        "command": "daemon-pty-attach",
        "args": {
            "id": id,
            "maxBytes": max_bytes,
        },
    })
}

fn pty_write_request(id: &str, data: String) -> Value {
    serde_json::json!({
        "command": "daemon-pty-write",
        "args": {
            "id": id,
            "data": data,
        },
    })
}

fn pty_resize_request(id: &str, cols: u16, rows: u16) -> Value {
    serde_json::json!({
        "command": "daemon-pty-resize",
        "args": {
            "id": id,
            "cols": cols,
            "rows": rows,
        },
    })
}

fn send_pty_resize(id: &str, cols: u16, rows: u16) -> Result<(), String> {
    response_data(send_socket_command(pty_resize_request(id, cols, rows))?).map(|_| ())
}

fn send_pty_write(id: &str, data: String) -> Result<(), String> {
    response_data(send_socket_command(pty_write_request(id, data))?).map(|_| ())
}

fn spawn_input_forwarder(pty_id: String, running: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut buf = [0_u8; 1024];
        while running.load(Ordering::SeqCst) {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if let Err(err) = send_pty_write(&pty_id, data) {
                        eprintln!("attach: failed to write to daemon PTY: {err}");
                        running.store(false, Ordering::SeqCst);
                        break;
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    eprintln!("attach: failed to read stdin: {err}");
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        }
    });
}

fn stream_attach_output(pty_id: &str, max_bytes: usize) -> Result<i32, String> {
    let exit_code = Arc::new(AtomicI32::new(0));
    let exit_for_frames = exit_code.clone();
    let mut stdout = io::stdout().lock();

    stream_socket_command(attach_request(pty_id, max_bytes), move |line| {
        let frame: AttachFrame = match serde_json::from_str(line) {
            Ok(frame) => frame,
            Err(err) => {
                eprintln!("attach: invalid frame from daemon: {err}");
                exit_for_frames.store(1, Ordering::SeqCst);
                return false;
            }
        };

        match frame {
            AttachFrame::Ready { replay_bytes } => {
                write_output(&mut stdout, &replay_bytes, &exit_for_frames)
            }
            AttachFrame::Output { bytes } => write_output(&mut stdout, &bytes, &exit_for_frames),
            AttachFrame::Exit { code } => {
                exit_for_frames.store(code.unwrap_or(0), Ordering::SeqCst);
                false
            }
            AttachFrame::Error { error } => {
                eprintln!("attach: {error}");
                exit_for_frames.store(1, Ordering::SeqCst);
                false
            }
        }
    })?;

    Ok(exit_code.load(Ordering::SeqCst))
}

fn write_output<W: Write>(writer: &mut W, bytes: &[u8], exit_code: &AtomicI32) -> bool {
    if let Err(err) = writer.write_all(bytes).and_then(|_| writer.flush()) {
        eprintln!("attach: failed to write stdout: {err}");
        exit_code.store(1, Ordering::SeqCst);
        return false;
    }
    true
}

#[cfg(unix)]
fn current_terminal_size() -> Option<(u16, u16)> {
    let mut size = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0;
    (ok && size.ws_col > 0 && size.ws_row > 0).then_some((size.ws_col, size.ws_row))
}

#[cfg(not(unix))]
fn current_terminal_size() -> Option<(u16, u16)> {
    None
}

#[cfg(unix)]
struct RawModeGuard {
    original: libc::termios,
}

#[cfg(unix)]
impl RawModeGuard {
    fn enable() -> Result<Self, String> {
        let mut original = unsafe { std::mem::zeroed::<libc::termios>() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return Err(format!("failed to read terminal mode: {}", io::Error::last_os_error()));
        }

        let mut raw = original;
        unsafe {
            libc::cfmakeraw(&mut raw);
        }
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return Err(format!(
                "failed to enable raw terminal mode: {}",
                io::Error::last_os_error()
            ));
        }

        Ok(Self { original })
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original) };
    }
}

#[cfg(not(unix))]
struct RawModeGuard;

#[cfg(not(unix))]
impl RawModeGuard {
    fn enable() -> Result<Self, String> {
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_request_uses_daemon_stream_command() {
        assert_eq!(
            attach_request("pty-1", 4096),
            serde_json::json!({
                "command": "daemon-pty-attach",
                "args": {
                    "id": "pty-1",
                    "maxBytes": 4096,
                },
            })
        );
    }

    #[test]
    fn pty_write_request_uses_daemon_write_command() {
        assert_eq!(
            pty_write_request("pty-1", "hello\r".to_string()),
            serde_json::json!({
                "command": "daemon-pty-write",
                "args": {
                    "id": "pty-1",
                    "data": "hello\r",
                },
            })
        );
    }

    #[test]
    fn selects_session_primary_pty_for_session() {
        let ptys = vec![
            serde_json::json!({
                "id": "secondary",
                "info": { "session_id": "session-a", "role": "secondary" },
            }),
            serde_json::json!({
                "id": "primary",
                "info": { "sessionId": "session-a", "role": "sessionPrimary" },
            }),
        ];

        assert_eq!(select_primary_pty_for_session(&ptys, "session-a").as_deref(), Some("primary"));
    }

    #[test]
    fn reads_primary_pty_id_from_session_snapshot() {
        let session = serde_json::json!({ "id": "session-a", "primaryPtyId": "pty-a" });
        assert_eq!(primary_pty_id_from_session(&session).as_deref(), Some("pty-a"));
    }
}
