use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::Emitter;

use crate::osc;

struct PtySession {
    master: Box<dyn MasterPty + Send>,
    #[allow(dead_code)]
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn std::io::Write + Send>,
}

pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn spawn(
        &self,
        session_id: &str,
        working_dir: &str,
        model: Option<&str>,
        additional_flags: &[String],
        app: tauri::AppHandle,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let mut cmd = CommandBuilder::new("claude");
        if let Some(m) = model {
            cmd.arg("--model");
            cmd.arg(m);
        }
        for flag in additional_flags {
            cmd.arg(flag);
        }
        cmd.cwd(working_dir);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to get PTY writer: {}", e))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

        let id_for_thread = session_id.to_string();
        let app_for_thread = app.clone();

        // Reader thread: reads PTY output, emits to frontend, parses OSC
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // PTY closed
                        let _ = app_for_thread.emit(
                            &format!("session-exit:{}", id_for_thread),
                            serde_json::json!({"code": null}),
                        );
                        break;
                    }
                    Ok(n) => {
                        let data = &buf[..n];

                        // Emit raw output as base64
                        let b64 = base64::engine::general_purpose::STANDARD.encode(data);
                        let _ = app_for_thread.emit(
                            &format!("pty-output:{}", id_for_thread),
                            b64,
                        );

                        // Parse OSC for status updates
                        if let Some(status) = osc::parse_osc_status(data) {
                            let _ = app_for_thread.emit(
                                &format!("session-status:{}", id_for_thread),
                                &status,
                            );
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let session = PtySession {
            master: pair.master,
            child,
            writer,
        };

        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.to_string(), session);

        Ok(())
    }

    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        use std::io::Write;
        session
            .writer
            .write_all(data)
            .map_err(|e| format!("Write failed: {}", e))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("Flush failed: {}", e))
    }

    pub fn resize(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("Session {} not found", session_id))?;

        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Resize failed: {}", e))
    }

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(mut session) = sessions.remove(session_id) {
            let _ = session.child.kill();
        }
        Ok(())
    }
}
