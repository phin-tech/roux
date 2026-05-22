use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use crate::paths;

#[derive(Debug, Clone)]
pub struct DaemonLog {
    path: Arc<PathBuf>,
}

impl DaemonLog {
    pub fn init() -> Self {
        let dir = paths::roux_config_dir().join("logs");
        let _ = fs::create_dir_all(&dir);
        rotate_existing_logs(&dir);

        let path = dir.join("roux-daemon.log");
        let log = Self { path: Arc::new(path) };
        log.write("=== Roux daemon started ===");
        log.write(&format!("Log file: {}", log.path.display()));
        log.write(&format!("OS: {} {}", std::env::consts::OS, std::env::consts::ARCH));
        log.write(&format!(
            "SHELL: {}",
            std::env::var("SHELL").unwrap_or_else(|_| "(unset)".into())
        ));
        log
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    #[cfg(test)]
    pub fn new_for_test(path: impl Into<PathBuf>) -> Self {
        Self { path: Arc::new(path.into()) }
    }

    pub fn write(&self, msg: &str) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(self.path.as_ref())
        {
            let _ = writeln!(file, "[{}] {}", timestamp(), msg);
        }
        eprintln!("[roux-daemon] {msg}");
    }
}

fn rotate_existing_logs(dir: &std::path::Path) {
    let path = dir.join("roux-daemon.log");
    for i in (1..5).rev() {
        let old = dir.join(format!("roux-daemon.{i}.log"));
        let new = dir.join(format!("roux-daemon.{}.log", i + 1));
        let _ = fs::rename(old, new);
    }
    if path.exists() {
        let _ = fs::rename(path, dir.join("roux-daemon.1.log"));
    }
}

fn timestamp() -> String {
    let dur =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{hours:02}:{mins:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_hh_mm_ss() {
        assert_eq!(timestamp().len(), 8);
    }
}
