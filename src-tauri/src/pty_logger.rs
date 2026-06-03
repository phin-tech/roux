//! PTY output logging with ring buffer for replay.
//!
//! Each PTY gets a log file at `~/.config/roux/sessions/{session_id}/{pty_id}/terminal.log`
//! plus an in-memory ring buffer for fast replay on attach.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use crate::paths::roux_config_dir;

const RING_BUFFER_MAX: usize = 256 * 1024; // 256KB

pub struct PtyLogger {
    file: Option<BufWriter<File>>,
    ring: VecDeque<u8>,
    path: PathBuf,
    seq: u64,
}

impl PtyLogger {
    pub fn new(session_id: &str, pty_id: &str) -> Self {
        let dir = roux_config_dir().join("sessions").join(session_id).join(pty_id);

        let path = dir.join("terminal.log");

        let file = fs::create_dir_all(&dir)
            .and_then(|_| OpenOptions::new().create(true).append(true).open(&path))
            .map(BufWriter::new)
            .ok();

        Self { file, ring: VecDeque::with_capacity(RING_BUFFER_MAX), path, seq: 0 }
    }

    /// Write bytes to both file and ring buffer
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        self.seq += 1;

        // Append to file (best effort, don't fail on IO errors)
        if let Some(ref mut file) = self.file {
            let _ = file.write_all(bytes);
        }

        // Push to ring, evicting old bytes if needed
        let overflow = (self.ring.len() + bytes.len()).saturating_sub(RING_BUFFER_MAX);
        if overflow > 0 {
            self.ring.drain(..overflow);
        }
        self.ring.extend(bytes);
    }

    /// Flush file buffer to disk
    pub fn flush(&mut self) {
        if let Some(ref mut file) = self.file {
            let _ = file.flush();
        }
    }

    /// Get recent bytes from ring buffer (fast path for replay)
    pub fn recent(&self, max_bytes: usize) -> Vec<u8> {
        let start = self.ring.len().saturating_sub(max_bytes);
        self.ring.iter().skip(start).copied().collect()
    }

    /// Current sequence number (for sync detection)
    #[allow(dead_code)] // Reserved for future replay sync
    pub fn seq(&self) -> u64 {
        self.seq
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for PtyLogger {
    fn drop(&mut self) {
        self.flush();
    }
}
